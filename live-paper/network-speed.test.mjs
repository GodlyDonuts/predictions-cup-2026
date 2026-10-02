import test from 'node:test';import assert from 'node:assert/strict';
import {ReadClient} from './transport-runtime.mjs';import {validateNetworkBudget,networkWriteInterval} from './transport-network-budget.mjs';
const budget={schemaVersion:2,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,formerWriterStopCommentId:5947535773};
const route='/tournaments/midterm-elections';const ok=()=>({ok:true,status:200,json:async()=>({})});
test('v2 permits only explicit coordinated read/write pacing; old defaults remain',()=>{
 assert.deepEqual(validateNetworkBudget(budget),budget);assert.equal(networkWriteInterval(),6000);assert.equal(networkWriteInterval(6000,budget),2500);
 for(const change of [{intervalMs:999},{writeIntervalMs:2000},{writeIntervalMs:undefined},{schemaVersion:3},{extra:true},{soleAccountClient:false}])assert.throws(()=>validateNetworkBudget({...budget,...change}));
});
for(const pattern of ['reads','writes','alternating','blocks'])test(`every inclusive rolling minute retains headroom: ${pattern}`,async()=>{
 let now=0,active=0,maxActive=0;const calls=[];
 const client=new ReadClient('FAKE',{networkBudget:budget,now:()=>now,sleepFn:async ms=>now+=ms,authorizeDispatch:()=>true,
 fetchFn:async(_url,options)=>{maxActive=Math.max(maxActive,++active);calls.push({at:now,write:options.method!=='GET'});if(calls.length%7===0)now+=7000;active--;return ok();}});
 await Promise.all(Array.from({length:150},(_,i)=>{const write=pattern==='writes'||pattern==='alternating'&&i%2===0||pattern==='blocks'&&i%20<10;return write?client.dispatch({method:i%2?'DELETE':'POST',endpoint:'/orders',beforeDispatch:async()=>{},beforeNetwork:async()=>{}}):client.request(route);}));
 assert.equal(maxActive,1);
 for(const call of calls){const window=calls.filter(x=>x.at>=call.at&&x.at<=call.at+60000);assert.ok(window.filter(x=>!x.write).length<=61);assert.ok(window.filter(x=>x.write).length<=25);}
 for(let i=1;i<calls.length;i++)assert.ok(calls[i].at-calls[i-1].at>=(calls[i-1].write?2500:1000));
});
test('write reservation survives restart, and reads cannot erase a Retry-After cooldown',async()=>{
 let now=0,persisted;const calls=[];const options={networkBudget:budget,now:()=>now,sleepFn:async ms=>now+=ms,onAdmission:n=>persisted=n,authorizeDispatch:()=>true,fetchFn:async(_url,o)=>{calls.push({at:now,method:o.method});return ok();}};
 const first=new ReadClient('FAKE',options);await first.dispatch({method:'POST',endpoint:'/orders',beforeDispatch:async()=>{},beforeNetwork:async()=>{now+=5000;}});assert.equal(persisted,7500);
 const second=new ReadClient('FAKE',options);second.next=persisted;await second.request(route);assert.equal(calls[1].at,7500);
 const limited=new ReadClient('FAKE',{...options,fetchFn:async()=>({ok:false,status:429,headers:new Headers({'Retry-After':'90'}),json:async()=>({})})});await assert.rejects(limited.request(route));const deadline=persisted;
 const resumed=new ReadClient('FAKE',options);resumed.next=deadline;await resumed.request(route);assert.equal(calls.at(-1).at,deadline);
});
test('slow persistence, stale write permits and manual-review failures still prevent dispatch',async()=>{
 let now=0,calls=0,persisted;const client=new ReadClient('FAKE',{networkBudget:budget,now:()=>now,authorizeDispatch:()=>true,onAdmission:n=>{persisted=n;now+=25000;},fetchFn:async()=>{calls++;return ok();}});
 await assert.rejects(client.dispatch({method:'POST',endpoint:'/orders',beforeDispatch:async()=>{},beforeNetwork:async()=>{if(now>10000)throw Error('STALE');}}),/STALE/);assert.equal(calls,0);assert.equal(persisted,2500);
 client.manualReview=true;await assert.rejects(client.request(route),/manual review/);assert.equal(calls,0);
});
test('STOP while queued behind a read prevents the faster client from sending a write',async()=>{
 let now=0,writes=0;const control=new AbortController(),client=new ReadClient('FAKE',{networkBudget:budget,signal:control.signal,now:()=>now,sleepFn:async ms=>{now+=ms;control.abort();},authorizeDispatch:()=>true,fetchFn:async(_url,o)=>{if(o.method!=='GET')writes++;return ok();}});
 await client.request(route);await assert.rejects(client.dispatch({method:'POST',endpoint:'/orders',beforeDispatch:async()=>{},beforeNetwork:async()=>{}}),/Stopped/);assert.equal(writes,0);
});
test('overflowing Retry-After and failed timing publication retain their persistent manual-review fences',async()=>{
 for(const fail of ['retry','timing']){let persisted,calls=0;const client=new ReadClient('FAKE',{networkBudget:budget,now:()=>0,onAdmission:n=>persisted=n,onTiming:()=>{if(fail==='timing')throw Error('PRIVATE_DETAIL');},fetchFn:async()=>{calls++;return fail==='retry'?{ok:false,status:429,headers:new Headers({'Retry-After':'99999999999999999999999999999999999'})}:ok();}});
  await assert.rejects(client.request(route));assert.equal(client.manualReview,true);assert.equal(persisted,null);await assert.rejects(client.request(route));assert.equal(calls,1);
 }
});
