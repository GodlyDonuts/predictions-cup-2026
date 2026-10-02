import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {digest} from './model.mjs';
import {validateUniverse} from './breadth.mjs';
import {verifySource} from './ml-shadow.mjs';
import {historyEndpoint,normalizeHistory} from './history.mjs';
import {openEntryComparison,runEntryComparison} from './entry-comparison-watch.mjs';

const base=Date.parse('2026-10-02T10:00:00.000Z'),stamp=ms=>new Date(base+ms).toISOString();
const config=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),tid=config.tournamentId;
const rawUniverse=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url)));
const universe=validateUniverse({tournamentId:tid,exchanges:rawUniverse.exchanges.filter(x=>config.marketIds.includes(x.marketId))},tid);
const exchange=universe.exchanges.find(x=>x.marketId==='377').exchangeId;
function record(sequence,previous,event,ms){const body={sequence,previous,at:stamp(ms),event,state:{fixture:true}};return {...body,hash:digest(body)};}
function book(ms,extra={}){return {endpoint:'/markets/377/orderbook?tournamentId='+tid+'&depth=20',requestStartedAt:stamp(ms-100),receivedAt:stamp(ms),data:{contexts:[{type:'tournament',tournament:{id:tid},orderbook:{exchanges:[{exchangeId:exchange,option:'YES',asOf:{at:stamp(ms-200),sequence:ms},bids:[{price:.495,quantity:10000}],asks:[{price:.5,quantity:10000}]}]}}]},...extra};}
function cycle(ms,extra={}){return {type:'cycle',clockAt:stamp(ms),sourceSnapshots:[book(ms-100)],supplementalDepth:[],accountExclusions:[],exclusionObservedAt:stamp(ms-500),...extra};}
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-entry-watch-test-')),workspace=path.join(dir,'workspace');fs.mkdirSync(workspace);
 t.after(()=>{const target=path.resolve(dir);assert.equal(path.dirname(target),path.resolve(os.tmpdir()));assert.ok(path.basename(target).startsWith('sig-entry-watch-test-'));fs.rmSync(target,{recursive:true,force:true});});
 const policy={...JSON.parse(fs.readFileSync(new URL('./dip-policy.json',import.meta.url))),executionPolicy:'responsive_v2',requireScheduledEntry:true,maxOpenMarkets:3};
 fs.writeFileSync(path.join(dir,'dip-responsive-bank.json'),JSON.stringify({config:{...config,feeBps:0},policy,universe,identities:Object.fromEntries(config.marketIds.map(id=>[id,{state:'FL',office:'Senate'}]))}));
 const source=path.join(dir,'journal.jsonl'),records=[];let now=10000;
 function append(event,ms){const r=record(records.length+1,records.at(-1)?.hash||'',event,ms);fs.appendFileSync(source,JSON.stringify(r)+'\n');records.push(r);return r;}
 const history=normalizeHistory({endpoint:historyEndpoint(exchange,stamp(-100)),requestStartedAt:stamp(-100),receivedAt:stamp(0),data:{marketId:'377',exchangeId:exchange,resolution:'1m',from:stamp(-3600000),to:stamp(-60000),coverage:{complete:true,projectedThroughSequence:1},candles:Array.from({length:8},(_,i)=>({time:stamp(-(45-i*5)*60000),open:.55,high:.55,low:.55,close:.55,vwap:.55,volume:10,tradeCount:1}))}},{marketId:'377',exchangeId:exchange});
 append({type:'market_history',clockAt:stamp(0),history},0);
 append(cycle(1000),1000);append(cycle(3000),3000);
 return {dir,workspace,source,records,append,setNow(ms){now=ms;},open(cohort='first',initialize=false){return openEntryComparison({runtimeDir:dir,cohort,initialize,now:()=>base+now});},run(cohort='first',initialize=false){return runEntryComparison({runtimeDir:dir,cohort,initialize,once:true,now:()=>base+now,workspaceDir:workspace});},cohortDir(cohort='first'){return path.join(dir,'entry-comparisons',cohort);}};
}
const wallet=(watch,name='historical_dip')=>watch.sourceState().strategies[name].wallet.strategies.mean_reversion;

test('named cohorts preserve their bank and accounting; frozen prefix never creates retrospective fills',t=>{
 const f=fixture(t),w=f.open('first',true),dir=f.cohortDir(),bank=fs.readFileSync(path.join(dir,'bank.json')),journal=fs.readFileSync(path.join(dir,'journal.jsonl'));
 assert.equal(w.status().counters.developmentBooks,2);assert.equal(w.status().counters.prospectiveBooks,0);
 for(const name of ['historical_dip','shock_rebound']){assert.equal(wallet(w,name).cash,config.initialCash);assert.deepEqual(wallet(w,name).fills,[]);assert.deepEqual(wallet(w,name).pending,{});}
 assert.throws(()=>f.open('first',true),/FROZEN_COMPARISON_ALREADY_EXISTS/);
 f.setNow(11000);const second=f.open('second',true);assert.notEqual(second.bank.cutoff,w.bank.cutoff);
 assert.deepEqual(fs.readFileSync(path.join(dir,'bank.json')),bank);assert.deepEqual(fs.readFileSync(path.join(dir,'journal.jsonl')),journal);
 for(const bad of ['../escape','Upper','con','first/other'])assert.throws(()=>f.open(bad,true),/INVALID_COMPARISON_DIRECTORY/);
});

test('post-cutoff independent fills and accounting match deterministic restart replay exactly',t=>{
 const f=fixture(t),w=f.open('first',true);
 f.append(cycle(11000,{sourceSnapshots:[book(10900,{requestStartedAt:stamp(9900)})]}),11000);f.setNow(12000);assert.equal(w.consume(),1);
 assert.deepEqual(wallet(w).pending,{});assert.equal(wallet(w).fills.length,0,'a request begun before cutoff cannot create an entry');
 f.append(cycle(13000),13000);f.setNow(14000);assert.equal(w.consume(),1);assert.equal(wallet(w).fills.length,0);assert.equal(Object.keys(wallet(w).pending).length,1);
 f.append(cycle(16000),16000);f.setNow(17000);assert.equal(w.consume(),1);assert.equal(wallet(w).fills.length,1);assert.equal(wallet(w).fills[0].action,'buy');assert.notEqual(wallet(w).cash,config.initialCash);
 const state=w.sourceState(),report=w.status(),restarted=f.open();assert.deepEqual(restarted.sourceState(),state);assert.deepEqual(restarted.status(),report);assert.equal(restarted.consume(),0);
 assert.deepEqual(restarted.sourceState(),state,'unchanged source cannot duplicate the hypothetical fill');
});

test('rehashed derived-state tampering and changed frozen source prefix fail before journal writes',async t=>{
 for(const mode of ['derived','source','code'])await t.test(mode,t=>{
  const f=fixture(t);f.open('first',true);const file=path.join(f.cohortDir(),'journal.jsonl');
  if(mode==='derived'){
   const rows=verifySource(fs.readFileSync(file,'utf8'));rows[0].state.counters.developmentBooks++;
   const {hash,...body}=rows[0];fs.writeFileSync(file,JSON.stringify({...body,hash:digest(body)})+'\n');
  }else if(mode==='source'){
   let prior='';const changed=f.records.map((r,i)=>{const {hash,...body}=r;if(i===0)body.event={...body.event,note:'changed original source'};body.previous=prior;const next={...body,hash:digest(body)};prior=next.hash;return next;});
   fs.writeFileSync(f.source,changed.map(JSON.stringify).join('\n')+'\n');
  }else{
   const file=path.join(f.cohortDir(),'bank.json'),bank=JSON.parse(fs.readFileSync(file));bank.codeIdentity['live-paper/model.mjs']='0'.repeat(64);fs.writeFileSync(file,JSON.stringify(bank));
  }
  const before=fs.readFileSync(file);assert.throws(()=>f.open(),mode==='derived'?/COMPARISON_REPLAY_MISMATCH/:mode==='source'?/FROZEN_SOURCE_PREFIX_CHANGED/:/FROZEN_COMPARISON_CODE_CHANGED/);assert.deepEqual(fs.readFileSync(file),before);
 });
});

test('an incomplete UTF-8 source tail survives restart and is committed exactly once after completion',t=>{
 const f=fixture(t),w=f.open('first',true),r=record(4,f.records.at(-1).hash,{type:'diagnostic',clockAt:stamp(12000),note:'paired \u03b1 evidence'},12000),bytes=Buffer.from(JSON.stringify(r)+'\n'),split=bytes.indexOf(Buffer.from('\u03b1'))+1;
 f.setNow(13000);fs.appendFileSync(f.source,bytes.subarray(0,split));assert.equal(w.consume(),0);assert.equal(w.status().sourceSequence,3);assert.ok(w.status().incompleteTailBytes>0);
 const restarted=f.open();assert.equal(restarted.sourceState().sourceSequence,3);assert.equal(restarted.consume(),0);
 fs.appendFileSync(f.source,bytes.subarray(split));assert.equal(restarted.consume(),1);assert.equal(restarted.status().incompleteTailBytes,0);assert.equal(restarted.status().sourceSequence,4);
 assert.equal(restarted.consume(),0);assert.deepEqual(f.open().sourceState(),restarted.sourceState(),'a later restart verifies the single durable accounting append');
});

test('source truncation, replacement and prefix mutation permanently close the live consumer',async t=>{
 for(const mode of ['truncate','replace','mutate'])await t.test(mode,t=>{
  const f=fixture(t),w=f.open('first',true),file=path.join(f.cohortDir(),'journal.jsonl'),before=fs.readFileSync(file);
  if(mode==='truncate')fs.truncateSync(f.source,4);
  else if(mode==='replace'){fs.renameSync(f.source,f.source+'.old');fs.copyFileSync(f.source+'.old',f.source);}
  else{const bytes=fs.readFileSync(f.source);bytes[20]=bytes[20]===65?66:65;fs.writeFileSync(f.source,bytes);}
  assert.throws(()=>w.consume(),/SOURCE_(REPLACED_OR_TRUNCATED|CHANGED)/);assert.throws(()=>w.consume(),/COMPARISON_FAILED_CLOSED/);assert.deepEqual(fs.readFileSync(file),before);
 });
});

test('an incomplete accounting tail is rejected rather than silently replacing the named cohort',t=>{
 const f=fixture(t);f.open('first',true);const file=path.join(f.cohortDir(),'journal.jsonl');fs.appendFileSync(file,'{"sequence":');const before=fs.readFileSync(file);
 assert.throws(()=>f.open(),/COMPARISON_JOURNAL_INCOMPLETE/);assert.deepEqual(fs.readFileSync(file),before);
});

test('runner respects STOP and cohort lock, writes only its cohort, and never reads credentials or calls fetch',async t=>{
 const f=fixture(t),other=path.join(f.dir,'unrelated-wallet.json'),key=path.join(f.dir,'api-key.txt');fs.writeFileSync(other,'{"untouched":true}');fs.writeFileSync(key,'fixture-do-not-read');
 let requests=0;const reads=[],readFileSync=fs.readFileSync;
 t.mock.method(globalThis,'fetch',async()=>{requests++;throw Error('NETWORK_FORBIDDEN');});
 t.mock.method(fs,'readFileSync',function(file,...rest){reads.push(String(file));assert.notEqual(path.resolve(String(file)),key,'research worker must not read a credential file');return readFileSync.call(this,file,...rest);});
 const dir=f.cohortDir();fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'worker.lock'),'12345');await assert.rejects(()=>f.run('first',true),/EEXIST/);fs.unlinkSync(path.join(dir,'worker.lock'));
 for(const stop of [path.join(f.workspace,'STOP'),path.join(f.dir,'STOP'),path.join(f.dir,'ENTRY_COMPARISON_STOP'),path.join(dir,'STOP')]){fs.writeFileSync(stop,'');await assert.rejects(()=>f.run('first',true),/STOP_EXISTS/);fs.unlinkSync(stop);}
 const report=await f.run('first',true);assert.equal(report.networkRequests,0);assert.equal(report.competitionOrders,0);assert.equal(requests,0);assert.equal(fs.existsSync(path.join(dir,'worker.lock')),false);
 const status=JSON.parse(fs.readFileSync(path.join(dir,'stats.json')));assert.equal(status.status,'running');assert.equal(status.cohort,'first');assert.equal(fs.readFileSync(other,'utf8'),'{"untouched":true}');assert.ok(!reads.some(p=>/api-key|credentials/i.test(p)));
});
