import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createDip,advanceDip} from './dip-shadow.mjs';
import {verifySource} from './ml-shadow.mjs';
import {digest} from './model.mjs';
import {atomicPaperJson} from './paper-persistence.mjs';
import {openDipWorker} from './dip-worker.mjs';
const config=JSON.parse(fs.readFileSync(new URL('./fee-free-config.json',import.meta.url))),policy=JSON.parse(fs.readFileSync(new URL('./dip-policy.json',import.meta.url)));
const at=ms=>new Date(Date.parse('2026-10-02T08:00:00.000Z')+ms).toISOString();
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'dip-worker-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const rows=[];
 const append=event=>{const body={sequence:rows.length+1,previous:rows.at(-1)?.hash||'',at:event.clockAt,event,state:{}},row={...body,hash:digest(body)};rows.push(row);fs.appendFileSync(path.join(dir,'journal.jsonl'),JSON.stringify(row)+'\n');return row;};
 append({type:'market_history',clockAt:at(0),history:{marketId:'377',exchangeId:'1066',tournamentId:config.tournamentId,receivedAt:at(0),coverage:{complete:true},candles:Array.from({length:8},(_,i)=>({time:at(-2400000+i*300000),vwap:.6}))}});
 const cfg={...config,marketIds:['377'],strategyUniverse:'diversified-dip-v1'},universe={tournamentId:config.tournamentId,exchanges:[{marketId:'377',exchangeId:'1066',option:'YES'}]};
 const {bank,state}=createDip(fs.readFileSync(path.join(dir,'journal.jsonl'),'utf8'),cfg,universe,policy,{377:{state:'AK',office:'Federal'}});
 for(const [name,value]of [['bank',bank],['state',state]])fs.writeFileSync(path.join(dir,'dip-scale-'+name+'.json'),JSON.stringify(value,null,2));
 const cycle=ms=>append({type:'cycle',clockAt:at(ms),accountExclusions:[],sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+config.tournamentId+'&depth=20',requestStartedAt:at(ms-100),receivedAt:at(ms),data:{contexts:[{type:'tournament',tournament:{id:config.tournamentId},orderbook:{exchanges:[{exchangeId:'1066',option:'YES',asOf:{at:at(ms),sequence:ms},bids:[{price:.545,quantity:10000}],asks:[{price:.550,quantity:10000}]}]}}]}}]});
 const read=name=>JSON.parse(fs.readFileSync(path.join(dir,'dip-scale-'+name+'.json'))),expected=()=>advanceDip(state,verifySource(fs.readFileSync(path.join(dir,'journal.jsonl'),'utf8')),bank);
 const options={runtimeDir:dir,workspaceDir:dir,cohort:'dip-scale',now:()=>Date.parse(at(10000)),onFault:()=>{}};
 return {dir,bank,state,cycle,read,expected,options};
}

test('new host reproduces frozen strategy accounting and resumes without resetting bank or cutoff',async t=>{
 const f=fixture(t),bankBytes=fs.readFileSync(path.join(f.dir,'dip-scale-bank.json'));f.cycle(1000);f.cycle(3000);
 const w=openDipWorker(f.options);t.after(w.close);assert.equal((await w.poll()).status,'running');const saved=f.read('state');assert.deepEqual(saved,f.expected());assert.equal(saved.wallet.strategies.mean_reversion.fills.length,1);w.close();
 const resumed=openDipWorker(f.options);t.after(resumed.close);assert.equal((await resumed.poll()).status,'running');assert.deepEqual(f.read('state'),saved);assert.deepEqual(fs.readFileSync(path.join(f.dir,'dip-scale-bank.json')),bankBytes);assert.equal(f.read('stats').cutoff,f.bank.cutoff);
});

test('failed state replacement retains durable accounting in failed stats and archives the complete candidate',async t=>{
 const f=fixture(t),faults=[];f.cycle(1000);f.cycle(3000);const before=fs.readFileSync(path.join(f.dir,'dip-scale-state.json'));
 const writeJson=(file,value,options)=>atomicPaperJson(file,value,{...options,platform:'win32',sleep:async()=>{},io:{...fs,renameSync(tmp,to){if(to.endsWith('-state.json'))throw Object.assign(Error('SECRET account/path'),{code:'EPERM'});fs.renameSync(tmp,to);}}});
 const w=openDipWorker({...f.options,writeJson,onFault:fault=>faults.push(fault)});t.after(w.close);const result=await w.poll();assert.equal(result.status,'failed');assert.equal(result.sourceSequence,1);assert.deepEqual(fs.readFileSync(path.join(f.dir,'dip-scale-state.json')),before);
 const report=f.read('stats');assert.equal(report.status,'failed');assert.equal(report.sourceSequence,1);assert.equal(report.fault.attemptedSourceSequence,3);assert.equal(report.fault.durableSourceSequence,1);assert.equal(report.fault.attempts,4);assert.equal(JSON.stringify(report).includes('SECRET'),false);
 assert.deepEqual(JSON.parse(fs.readFileSync(path.join(f.dir,report.fault.temporary))),f.expected());assert.equal(fs.readdirSync(path.join(f.dir,'paper-faults')).length,1);assert.equal(faults.length,1);w.close();
 const resumed=openDipWorker(f.options);t.after(resumed.close);await resumed.poll();assert.deepEqual(f.read('state'),f.expected());assert.equal(f.read('state').wallet.strategies.mean_reversion.fills.length,1);
});

test('stats failure reports the new durable checkpoint and secondary publication failure remains diagnosed',async t=>{
 const f=fixture(t),faults=[];f.cycle(1000);const w=openDipWorker({...f.options,onFault:x=>faults.push(x),writeJson:(file,value,options)=>atomicPaperJson(file,value,{...options,io:{...fs,renameSync(tmp,to){if(to.endsWith('-stats.json'))throw Object.assign(Error('SECRET'),{code:'EACCES'});fs.renameSync(tmp,to);}}})});t.after(w.close);
 const result=await w.poll();assert.equal(result.status,'failed');assert.equal(result.sourceSequence,2);assert.deepEqual(f.read('state'),f.expected());assert.equal(faults.length,2);assert.ok(faults.every(x=>x.durableSourceSequence===2&&x.osCode==='EACCES'));assert.equal(fs.readdirSync(path.join(f.dir,'paper-faults')).length,2);
});

test('STOP during stats retry cannot publish a fresh running heartbeat after the checkpoint commits',async t=>{
 const f=fixture(t),published=[];f.cycle(1000);let failedOnce=false;
 const writeJson=(file,value,options)=>atomicPaperJson(file,value,{...options,platform:'win32',sleep:async()=>fs.writeFileSync(path.join(f.dir,'DIP_STOP'),'stop'),io:{...fs,renameSync(tmp,to){
  if(to.endsWith('-stats.json')){const status=JSON.parse(fs.readFileSync(tmp)).status;if(!failedOnce){failedOnce=true;throw Object.assign(Error('blocked'),{code:'EPERM'});}published.push(status);}
  fs.renameSync(tmp,to);
 }}});
 const w=openDipWorker({...f.options,writeJson});t.after(w.close);const result=await w.poll();
 assert.equal(result.status,'failed');assert.deepEqual(published,['failed']);assert.equal(f.read('stats').fault.osCode,'COMMIT_BLOCKED');
 assert.equal(f.read('stats').sourceSequence,2);assert.deepEqual(f.read('state'),f.expected());
});

test('STOP, duplicate lock and source corruption cannot publish unverified running progress',async t=>{
 const f=fixture(t),w=openDipWorker(f.options);t.after(w.close);assert.throws(()=>openDipWorker(f.options),e=>e.code==='EEXIST');
 fs.writeFileSync(path.join(f.dir,'DIP_STOP'),'private stop text');f.cycle(1000);assert.equal((await w.poll()).status,'stopped');assert.deepEqual(f.read('state'),f.state);fs.unlinkSync(path.join(f.dir,'DIP_STOP'));
 fs.appendFileSync(path.join(f.dir,'journal.jsonl'),'{}\n');assert.equal((await w.poll()).status,'failed');assert.equal(f.read('stats').sourceSequence,1);assert.deepEqual(f.read('state'),f.state);
});

test('bounded incomplete source waits preserve state until a verified complete source arrives',async t=>{
 const f=fixture(t),w=openDipWorker(f.options);t.after(w.close);const file=path.join(f.dir,'journal.jsonl'),bytes=fs.readFileSync(file);fs.writeFileSync(file,bytes.subarray(0,-1));
 assert.equal((await w.poll()).status,'awaiting_capture');assert.deepEqual(f.read('state'),f.state);fs.appendFileSync(file,'\n');assert.equal((await w.poll()).status,'running');
});

test('CLI startup faults print only redacted JSON and leave no stale worker lock',t=>{
 const f=fixture(t);fs.writeFileSync(path.join(f.dir,'dip-scale-bank.json'),'SECRET invalid JSON');
 const result=spawnSync(process.execPath,['live-paper/dip-worker.mjs','--name','dip-scale','--once'],{cwd:path.resolve('.'),env:{...process.env,SIG_PAPER_DIR:f.dir},encoding:'utf8',windowsHide:true});
 assert.equal(result.status,1);assert.equal(result.stdout,'');assert.equal(result.stderr.includes('SECRET'),false);assert.equal(JSON.parse(result.stderr.trim()).type,'paper_worker_fault');assert.equal(fs.existsSync(path.join(f.dir,'dip-scale-shadow.lock')),false);
});
