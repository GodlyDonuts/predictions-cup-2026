import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {digest} from './model.mjs';
import {validateUniverse} from './breadth.mjs';
import {historyEndpoint,normalizeHistory} from './history.mjs';
import {openLimitEntry,runLimitEntry,limitEntryCodeIdentity} from './limit-entry-watch.mjs';

const base=Date.parse('2026-10-02T10:00:00.000Z'),stamp=ms=>new Date(base+ms).toISOString();
const config=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),tid=config.tournamentId;
const universe=validateUniverse(JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url))),tid);
const exchange=universe.exchanges.find(x=>x.marketId==='377').exchangeId;
function record(sequence,previous,event,ms){const body={sequence,previous,at:stamp(ms),event,state:{fixture:true}};return {...body,hash:digest(body)};}
function book(ms){return {endpoint:'/markets/377/orderbook?tournamentId='+tid+'&depth=20',requestStartedAt:stamp(ms-100),receivedAt:stamp(ms),data:{contexts:[{type:'tournament',tournament:{id:tid},orderbook:{exchanges:[{exchangeId:exchange,option:'YES',asOf:{at:stamp(ms-200),sequence:ms},bids:[{price:.495,quantity:10000}],asks:[{price:.500,quantity:10000}]}]}}]}};}
const cycle=ms=>({type:'cycle',clockAt:stamp(ms),sourceSnapshots:[book(ms-100)],supplementalDepth:[],accountExclusions:[],exclusionObservedAt:stamp(ms-500)});
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-limit-watch-')),workspace=path.join(dir,'workspace');fs.mkdirSync(workspace);const watches=[];
 t.after(()=>{for(const w of watches)w.close();const target=path.resolve(dir);assert.equal(path.dirname(target),path.resolve(os.tmpdir()));assert.ok(path.basename(target).startsWith('sig-limit-watch-'));fs.rmSync(target,{recursive:true,force:true});});
 const policy={...JSON.parse(fs.readFileSync(new URL('./dip-policy.json',import.meta.url))),executionPolicy:'responsive_v2',requireScheduledEntry:true,maxOpenMarkets:3};
 fs.writeFileSync(path.join(dir,'dip-responsive-bank.json'),JSON.stringify({config:{...config,feeBps:0},policy,universe,identities:Object.fromEntries(config.marketIds.map(id=>[id,{state:'FL',office:'Senate'}]))}));
 const source=path.join(dir,'journal.jsonl'),records=[];let now=10000;
 function append(event,ms){const r=record(records.length+1,records.at(-1)?.hash||'',event,ms);fs.appendFileSync(source,JSON.stringify(r)+'\n');records.push(r);return r;}
 const history=normalizeHistory({endpoint:historyEndpoint(exchange,stamp(-100)),requestStartedAt:stamp(-100),receivedAt:stamp(0),data:{marketId:'377',exchangeId:exchange,resolution:'1m',from:stamp(-3600000),to:stamp(-60000),coverage:{complete:true,projectedThroughSequence:1},candles:Array.from({length:8},(_,i)=>({time:stamp(-(45-i*5)*60000),open:.55,high:.55,low:.55,close:.55,vwap:.55,volume:10,tradeCount:1}))}},{marketId:'377',exchangeId:exchange});
 append({type:'market_history',clockAt:stamp(0),history},0);append(cycle(1000),1000);append(cycle(3000),3000);
 return {dir,workspace,source,records,append,setNow(ms){now=ms;},open(cohort='first',initialize=false){const w=openLimitEntry({runtimeDir:dir,cohort,initialize,now:()=>base+now,workspaceDir:workspace});watches.push(w);return w;},run(cohort='first',initialize=false){return runLimitEntry({runtimeDir:dir,cohort,initialize,once:true,now:()=>base+now,workspaceDir:workspace});},cohortDir(cohort='first'){return path.join(dir,'limit-entry-cohorts',cohort);}};
}
const wallet=(w,name='immediate_dip')=>w.sourceState().strategies[name].wallet.strategies.mean_reversion;
const rows=file=>fs.readFileSync(file,'utf8').trimEnd().split('\n').map(JSON.parse);

test('named host freezes import closure and source prefix without retrospective orders',t=>{
 const f=fixture(t),w=f.open('first',true),dir=f.cohortDir(),bytes=fs.readFileSync(path.join(dir,'bank.json'));
 assert.equal(w.status().counters.developmentBooks,2);assert.equal(w.status().counters.prospectiveBooks,0);
 for(const name of ['immediate_dip','price_protected_hybrid']){assert.deepEqual(wallet(w,name).fills,[]);assert.deepEqual(wallet(w,name).pending,{});}
 assert.ok(limitEntryCodeIdentity()['live-paper/paper-persistence.mjs']);assert.ok(limitEntryCodeIdentity()['live-paper/limit-entry.mjs']);
 assert.throws(()=>f.open(),e=>e.code==='EEXIST');w.close();assert.throws(()=>f.open('first',true),/FROZEN_LIMIT_ENTRY_ALREADY_EXISTS/);
 assert.deepEqual(fs.readFileSync(path.join(dir,'bank.json')),bytes);
 for(const name of ['../escape','Upper','con','a/b'])assert.throws(()=>f.open(name,true),/INVALID_LIMIT_ENTRY_DIRECTORY/);
 const available=rows(path.join(dir,'journal.jsonl'))[0].event.availability;assert.deepEqual(available,[{from:1,through:3,at:stamp(10000)}]);
});

test('immutable first-consumption clocks replay exactly at a later host restart',t=>{
 const f=fixture(t),w=f.open('first',true);f.append(cycle(13000),13000);f.setNow(14000);assert.equal(w.consume(),1);
 assert.equal(Object.keys(wallet(w).pending).length,1);assert.equal(Object.values(wallet(w).pending)[0].createdAt,stamp(14000));
 f.append(cycle(17000),17000);f.setNow(18000);w.consume();assert.equal(wallet(w).fills.length,1);
 const state=w.sourceState(),journal=path.join(f.cohortDir(),'journal.jsonl'),bytes=fs.readFileSync(journal);w.close();f.setNow(120000);
 const restarted=f.open();assert.deepEqual(restarted.sourceState(),state);assert.deepEqual(fs.readFileSync(journal),bytes);assert.equal(restarted.consume(),0);
 assert.deepEqual(rows(journal).slice(1).map(x=>x.event.availability),[[{from:4,through:4,at:stamp(14000)}],[{from:5,through:5,at:stamp(18000)}]]);
});

test('a backlog cannot invent a timely entry, but an in-window captured quote can resolve an existing order',t=>{
 const late=fixture(t),a=late.open('first',true);late.append(cycle(13000),13000);late.setNow(70000);a.consume();assert.equal(Object.keys(wallet(a).pending).length,0);assert.equal(wallet(a).fills.length,0);assert.equal(a.status().counters.staleAtDecisionBooks,1);
 const f=fixture(t),w=f.open('first',true);f.append(cycle(13000),13000);f.setNow(14000);w.consume();assert.equal(Object.keys(wallet(w).pending).length,1);
 f.append({type:'diagnostic',clockAt:stamp(16000)},16000);f.append(cycle(17000),17000);f.setNow(70000);w.consume();assert.equal(wallet(w).fills.length,1);assert.equal(wallet(w).fills[0].at,stamp(16900));
 const saved=w.sourceState();w.close();f.setNow(90000);assert.deepEqual(f.open().sourceState(),saved);
});

test('partial UTF-8 and bounded incremental reads never advance an incomplete row',t=>{
 const f=fixture(t),w=f.open('first',true),r=record(4,f.records.at(-1).hash,{type:'diagnostic',clockAt:stamp(12000),note:'paired α'},12000),bytes=Buffer.from(JSON.stringify(r)+'\n'),split=bytes.indexOf(Buffer.from('α'))+1;
 f.setNow(13000);fs.appendFileSync(f.source,bytes.subarray(0,split));assert.equal(w.consume(),0);assert.equal(w.status().sourceSequence,3);assert.ok(w.status().incompleteTailBytes>0);w.close();
 const restarted=f.open();assert.equal(restarted.consume(),0);fs.appendFileSync(f.source,bytes.subarray(split));assert.equal(restarted.consume(),1);assert.equal(restarted.status().sourceSequence,4);assert.equal(restarted.status().incompleteTailBytes,0);
});

test('rehashed accounting or availability tampering fails deterministic startup replay',async t=>{
 for(const mode of ['state','availability'])await t.test(mode,t=>{
  const f=fixture(t),w=f.open('first',true);f.append(cycle(13000),13000);f.setNow(14000);w.consume();w.close();
  const file=path.join(f.cohortDir(),'journal.jsonl'),saved=rows(file);if(mode==='state')saved.at(-1).state.counters.prospectiveBooks++;else saved.at(-1).event.availability[0].at=stamp(13900);
  let previous='';for(const row of saved){row.previous=previous;const {hash,...body}=row;row.hash=digest(body);previous=row.hash;}fs.writeFileSync(file,saved.map(JSON.stringify).join('\n')+'\n');const bytes=fs.readFileSync(file);
  assert.throws(()=>f.open(),/LIMIT_ENTRY_REPLAY_MISMATCH/);assert.deepEqual(fs.readFileSync(file),bytes);assert.equal(fs.existsSync(path.join(f.cohortDir(),'worker.lock')),false);
 });
});

test('source replacement, truncation, changed prefix or accounting crash tail fail closed',async t=>{
 for(const mode of ['replace','truncate','mutate','journal'])await t.test(mode,t=>{
  const f=fixture(t),w=f.open('first',true),journal=path.join(f.cohortDir(),'journal.jsonl'),before=fs.readFileSync(journal);
  if(mode==='journal'){w.close();fs.appendFileSync(journal,'{"sequence":');assert.throws(()=>f.open(),/JOURNAL_INCOMPLETE/);return;}
  if(mode==='replace'){fs.renameSync(f.source,f.source+'.old');fs.copyFileSync(f.source+'.old',f.source);}else if(mode==='truncate')fs.truncateSync(f.source,4);else{const b=fs.readFileSync(f.source);b[20]=b[20]===65?66:65;fs.writeFileSync(f.source,b);}
  assert.throws(()=>w.consume(),/SOURCE_(REPLACED_OR_TRUNCATED|CHANGED)/);assert.throws(()=>w.consume(),/LIMIT_ENTRY_FAILED_CLOSED/);assert.deepEqual(fs.readFileSync(journal),before);
 });
});

test('STOP cannot commit source advancement and runner never reads keys or calls network',async t=>{
 const f=fixture(t),w=f.open('first',true),journal=path.join(f.cohortDir(),'journal.jsonl'),before=fs.readFileSync(journal);f.append(cycle(13000),13000);f.setNow(14000);
 fs.writeFileSync(path.join(f.dir,'LIMIT_ENTRY_STOP'),'engineering fixture');assert.throws(()=>w.consume(),/LIMIT_ENTRY_STOPPED/);assert.deepEqual(fs.readFileSync(journal),before);w.close();fs.unlinkSync(path.join(f.dir,'LIMIT_ENTRY_STOP'));
 let requests=0;t.mock.method(globalThis,'fetch',()=>{requests++;throw Error('NETWORK_FORBIDDEN');});const actualRead=fs.readFileSync,reads=[];
 t.mock.method(fs,'readFileSync',function(file,...rest){reads.push(String(file));assert.ok(!/api-key|credentials/i.test(String(file)));return actualRead.call(this,file,...rest);});
 await f.run();assert.equal(requests,0);const stats=JSON.parse(fs.readFileSync(path.join(f.cohortDir(),'stats.json')));assert.equal(stats.status,'running');assert.equal(stats.networkRequests,0);assert.equal(stats.competitionOrders,0);assert.equal(fs.existsSync(path.join(f.cohortDir(),'worker.lock')),false);
 for(const stop of [path.join(f.workspace,'STOP'),path.join(f.dir,'STOP'),path.join(f.dir,'LIMIT_ENTRY_STOP'),path.join(f.cohortDir(),'STOP')]){fs.writeFileSync(stop,'');await assert.rejects(()=>f.run(),/LIMIT_ENTRY_FAILED_CLOSED/);fs.unlinkSync(stop);}
});

test('CLI startup errors reveal no source payload and release the local owner lock',t=>{
 const f=fixture(t);fs.writeFileSync(f.source,'SECRET private invalid JSON\n');
 const r=spawnSync(process.execPath,['live-paper/limit-entry-watch.mjs','--name','first','--init','--once'],{cwd:path.resolve('.'),env:{...process.env,SIG_PAPER_DIR:f.dir},encoding:'utf8',windowsHide:true});
 assert.equal(r.status,1);assert.equal(r.stdout,'');assert.equal(r.stderr.includes('SECRET'),false);assert.equal(fs.existsSync(path.join(f.cohortDir(),'worker.lock')),false);
});

test('failure flushing a newly created owner lock removes only that uncommitted lock',t=>{
 const f=fixture(t),before=fs.readFileSync(f.source),original=fs.fsyncSync;let failed=false;
 t.mock.method(fs,'fsyncSync',function(fd){if(!failed){failed=true;throw Object.assign(Error('fixture write fault'),{code:'EIO'});}return original.call(this,fd);});
 assert.throws(()=>f.open('first',true),e=>e.code==='EIO');assert.equal(fs.existsSync(path.join(f.cohortDir(),'worker.lock')),false);assert.equal(fs.existsSync(path.join(f.cohortDir(),'bank.json')),false);assert.deepEqual(fs.readFileSync(f.source),before);
 const w=f.open('first',true);assert.equal(w.status().sourceSequence,3);
});
