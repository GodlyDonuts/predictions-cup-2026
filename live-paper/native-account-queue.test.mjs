import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {NativeAccountQueue,nativeQueueIntentBinding,verifyNativeAccountQueueBinding} from './native-account-queue.mjs';
import {ReadClient} from './transport-runtime.mjs';

const sha=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const executable=path.resolve('backend-rust/account-queue/target/release/baal-account-queue.exe');
const dll=path.join(path.dirname(executable),'libunwind.dll');
const binding={executable,binaryHash:sha(executable),dependencies:fs.existsSync(dll)?[{path:dll,sha256:sha(dll)}]:[]};
const authority={lease:{owner:'Codex'},permit:{runId:'fixture',intentId:'one'}};
const order=(id='one')=>({runId:'fixture',intentId:id,idempotencyKey:'fixture-'+id,method:'POST',endpoint:'/orders',body:{idempotencyKey:'fixture-'+id,quantity:1,price:0.25}});
function fixture(t,overrides={}){
 const directory=fs.mkdtempSync(path.join(os.tmpdir(),'baal-native-account-adapter-'));let now=1000,owner=true,operate=true;let queue;
 const callbacks={canOperate:()=>operate,canWrite:()=>owner,validateLease:()=>true,validatePermit:()=>true,validateNetworkPermit:()=>true,classifyResult:()=>({outcome:'success'}),...overrides};
 const options={binding,directory,scope:'offline-fixture',now:()=>now,sleep:async ms=>{now+=ms;},...callbacks};queue=new NativeAccountQueue(options);
 t.after(async()=>{await queue.close({force:queue.status.failed});fs.rmSync(directory,{recursive:true,force:true});});
 return {queue,directory,options,get now(){return now;},advance:n=>{now+=n;},owner:v=>{owner=v;},operate:v=>{operate=v;}};
}
test('binary and dependency pins are verified before a process starts',()=>{
 assert.equal(verifyNativeAccountQueueBinding(binding),executable);assert.throws(()=>verifyNativeAccountQueueBinding({...binding,binaryHash:'a'.repeat(64)}),/HASH_MISMATCH/);
});
test('intent binds exact scoped method, endpoint, bytes and key',()=>{
 const spec=order(),b=nativeQueueIntentBinding('offline-fixture',spec);assert.notEqual(b.payload_hash,nativeQueueIntentBinding('different-scope',spec).payload_hash);
 assert.notEqual(b.payload_hash,nativeQueueIntentBinding('offline-fixture',{...spec,endpoint:'/other'}).payload_hash);
 assert.throws(()=>nativeQueueIntentBinding('offline-fixture',{...spec,bodyJson:'{"quantity":9}'}),/BODY_ENCODING/);
 assert.throws(()=>nativeQueueIntentBinding('offline-fixture',{...spec,idempotencyKey:'wrong'}),/KEY_MISMATCH/);
});
test('public authorization booleans cannot replace trusted callback fences',async t=>{
 const f=fixture(t,{canWrite:()=>false});let calls=0;
 await assert.rejects(f.queue.write({...order(),authorized:true,permit_valid:true,lease_valid:true},authority,{perform:async()=>{calls++;}}),/WRITE_STOPPED_OR_OWNER_LOST/);assert.equal(calls,0);
});
test('shared long-lived process serializes mixed operations with completion pacing',async t=>{
 const f=fixture(t);const starts=[];let active=0,peak=0;
 const perform=async spec=>{peak=Math.max(peak,++active);starts.push({at:f.now,write:spec.method!=='GET'});f.advance(359);active--;return{};};
 await Promise.all([f.queue.read({endpoint:'/fixture',perform}),f.queue.write(order(),authority,{perform}),f.queue.read({endpoint:'/fixture',perform})]);
 assert.equal(peak,1);assert.deepEqual(starts,[{at:1000,write:false},{at:2359,write:true},{at:5218,write:false}]);
 const state=(await f.queue.inspect()).state;assert.equal(state.next,6577);assert.equal(state.active,null);
});
test('STOP or lease loss after durable admission prevents the operation callback',async t=>{
 for(const reason of ['stop','lease']){
  let valid=true;const f=fixture(t,{validateLease:()=>valid});let calls=0;
  await assert.rejects(f.queue.write(order(),authority,{beforeNetwork:async()=>{if(reason==='stop')f.operate(false);else valid=false;},perform:async()=>{calls++;}}),reason==='stop'?/STOPPED/:/INVALID_EXECUTION_LEASE/);
  assert.equal(calls,0);f.operate(true);const st=(await f.queue.inspect()).state;assert.equal(st.active,null);assert.equal(Object.keys(st.unknown).length,0);assert.equal(st.writes.length,1);
 }
});
test('owner is checked again after an awaited validator',async t=>{
 let f;f=fixture(t,{validatePermit:async()=>{f.owner(false);return true;}});let calls=0;
 await assert.rejects(f.queue.write(order(),authority,{perform:async()=>{calls++;}}),/OWNER_LOST/);assert.equal(calls,0);
});
test('STOP is checked again after an awaited owner/activation callback',async t=>{
 let f;f=fixture(t,{canWrite:async()=>{f.operate(false);return true;}});let calls=0;
 await assert.rejects(f.queue.write(order(),authority,{perform:async()=>{calls++;return{};}}),/STOPPED/);assert.equal(calls,0);
});
test('write acknowledgment is unknown without a trusted result classifier',async t=>{
 const f=fixture(t,{classifyResult:undefined});let calls=0;
 await assert.rejects(f.queue.write(order(),authority,{perform:async()=>{calls++;return{status:200};}}),/UNKNOWN_ACCEPTANCE/);
 await assert.rejects(f.queue.write(order('two'),authority,{perform:async()=>{calls++;}}),/UNKNOWN_ACCEPTANCE/);assert.equal(calls,1);
 const st=(await f.queue.inspect()).state;assert.equal(Object.keys(st.unknown).length,1);assert.equal(Object.keys(st.intents).length,1);
});
test('malformed semantic result cannot silently authorize a replacement',async t=>{
 const f=fixture(t,{classifyResult:()=>{throw Error('untrusted response detail');}});let calls=0;
 await assert.rejects(f.queue.write(order(),authority,{perform:async()=>{calls++;return{};}}),/COMPLETION_DURABILITY_UNCERTAIN/);assert.equal(calls,1);assert.equal(f.queue.status.failed,true);
 await f.queue.close({force:true});const lock=path.join(f.directory,'offline-account-queue.lock');if(fs.existsSync(lock))fs.unlinkSync(lock); // Fixture child is confirmed exited, never production.
 const restored=new NativeAccountQueue(f.options);try{await assert.rejects(restored.write(order('two'),authority,{perform:async()=>{calls++;}}),/UNKNOWN_ACCEPTANCE/);assert.equal(calls,1);}finally{await restored.close();}
});
test('explicit compaction and trusted reconciliation preserve original intent binding',async t=>{
 const f=fixture(t,{classifyResult:()=>({outcome:'unknown'}),validateReconciliation:(_binding,evidence)=>evidence?.verified===true});
 await assert.rejects(f.queue.write(order(),authority,{perform:async()=>({})}),/UNKNOWN_ACCEPTANCE/);
 const r=await f.queue.compact();assert.equal(r.compacted,true);assert.equal(r.unknown_acceptances,1);
 await assert.rejects(f.queue.reconcile(order(),{verified:false}),/SCOPED_RECONCILIATION/);
 assert.equal((await f.queue.reconcile(order(),{verified:true})).retry_permitted,false);
 await assert.rejects(f.queue.write(order(),authority,{perform:async()=>({})}),/KNOWN_INTENT_RECONCILE_FIRST/);
});
test('process death during an operation aborts callback and retains uncertain write',async t=>{
 const f=fixture(t);let entered;const started=new Promise(r=>{entered=r;});
 const operation=f.queue.write(order(),authority,{perform:async(_spec,signal)=>{entered();return new Promise((resolve,reject)=>{signal.addEventListener('abort',()=>reject(Error('callback aborted')),{once:true});});}});
 await started;await f.queue.close({force:true});await assert.rejects(operation,error=>error.reconciliationRequired===true&&/PROCESS_LOST/.test(error.message));
});
test('process death rejects promptly even when the operation ignores abort',async t=>{
 const f=fixture(t);let entered;const started=new Promise(r=>{entered=r;});
 const operation=f.queue.write(order(),authority,{perform:async()=>{entered();return new Promise(()=>{});}});await started;
 await f.queue.close({force:true});await assert.rejects(operation,error=>error.reconciliationRequired===true);assert.equal(f.queue.status.failed,true);
});
test('an operation timeout durably retains unknown write acceptance',async t=>{
 const f=fixture(t,{operationTimeoutMs:100});let calls=0;
 await assert.rejects(f.queue.write(order(),authority,{perform:async()=>{calls++;return new Promise(()=>{});}}),/UNKNOWN_ACCEPTANCE/);
 await assert.rejects(f.queue.write(order('replacement'),authority,{perform:async()=>{calls++;return{};}}),/UNKNOWN_ACCEPTANCE/);assert.equal(calls,1);
 assert.equal(Object.keys((await f.queue.inspect()).state.unknown).length,1);
});
test('immutable operation bytes cannot be changed by an awaited trusted callback',async t=>{
 const f=fixture(t,{validatePermit:async(_permit,spec)=>{assert.throws(()=>{spec.body.quantity=999;},TypeError);return true;}});const input=order();
 const work=f.queue.write(input,authority,{perform:async spec=>{assert.equal(spec.body.quantity,1);return{};}});input.body.quantity=999;await work;
});
test('STOP while queued prevents a later write from reaching durable admission',async t=>{
 const f=fixture(t);let released,entered;const started=new Promise(r=>{entered=r;});let writes=0;
 const read=f.queue.read({endpoint:'/fixture',perform:async()=>{entered();await new Promise(r=>{released=r;});return{};}});
 const write=f.queue.write(order(),authority,{perform:async()=>{writes++;return{};}});await started;f.operate(false);released();await read;await assert.rejects(write,/STOPPED/);assert.equal(writes,0);f.operate(true);assert.equal((await f.queue.inspect()).state.writes.length,0);
});
test('live-style adapter latency includes JSONL IPC and actual durable fsync',async t=>{
 const f=fixture(t),samples=[];for(let i=0;i<12;i++){const start=performance.now();await f.queue.read({endpoint:'/fixture',perform:async()=>({})});samples.push(performance.now()-start);}
 samples.sort((a,b)=>a-b);t.diagnostic(JSON.stringify({mode:'offline_local_IPC_fsync_measurement',requests:12,medianWallMs:samples[6],p95WallMs:samples[11],networkRequests:0,limitations:'Includes startup in one sample and three synced state transitions per request; fake pacing clock. Excludes actual API/network delay and does not claim a live speedup.'}));assert.equal(samples.length,12);
});
for(const scenario of ['mixed','slow_guard','retry429','retry503'])test(`actual Node completion/cooldown parity: ${scenario}`,async t=>{
 const nativeTimes=[],nodeTimes=[];const delays=[359,500,7],statuses=[scenario==='retry429'?429:scenario==='retry503'?503:200,200,200];
 const f=fixture(t,{classifyError:error=>({outcome:'rejected',status:error.status,retryAfter:error.retryAfter}),classifyResult:()=>({outcome:'success'})});
 let nodeNow=1000,nodePersisted=0,index=0;const budget={schemaVersion:2,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,formerWriterStopCommentId:5947535773};
 const node=new ReadClient('OFFLINE_PLACEHOLDER',{networkBudget:budget,now:()=>nodeNow,sleepFn:async ms=>{nodeNow+=ms;},authorizeDispatch:()=>true,onAdmission:n=>{nodePersisted=n;},fetchFn:async()=>{const i=index++;nodeTimes.push(nodeNow);nodeNow+=delays[i];return{ok:statuses[i]===200,status:statuses[i],headers:new Headers({'Retry-After':'90'}),json:async()=>({})};}});
 try{for(let i=0;i<3;i++){
  const write=scenario==='mixed'&&i===1||scenario==='slow_guard'&&i===0;
  const beforeNetwork=async()=>{if(scenario==='slow_guard'&&i===0)f.advance(5000);};
  const perform=async()=>{nativeTimes.push(f.now);f.advance(delays[i]);if(statuses[i]!==200){const e=Error('fixture HTTP');e.status=statuses[i];e.retryAfter='90';throw e;}return{};};
  try{if(write)await f.queue.write(order('operation'+i),authority,{perform,beforeNetwork});else await f.queue.read({endpoint:'/fixture',perform});}catch(e){assert.equal(e.status,statuses[i]);}
  try{if(write)await node.dispatch({method:'POST',endpoint:'/orders',beforeDispatch:async()=>{},beforeNetwork:async()=>{if(scenario==='slow_guard'&&i===0)nodeNow+=5000;}});else await node.request('/tournaments/midterm-elections');}catch(e){assert.equal(e.status,statuses[i]);}
 }}finally{node.close();}
 assert.deepEqual(nativeTimes,nodeTimes);const state=(await f.queue.inspect()).state;assert.equal(state.next,nodePersisted);assert.equal(state.cooldown,node.cooldown);
});
