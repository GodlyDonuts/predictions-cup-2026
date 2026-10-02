import test from 'node:test';import assert from 'node:assert/strict';
import {ReadClient} from './runtime.mjs';
test('execution dispatch stays disabled unless explicitly wired and never bypasses queued STOP',async()=>{
 let now=0,calls=0,stopped=false;const starts=[],admissions=[];
 const client=new ReadClient('synthetic',{now:()=>now,sleepFn:async ms=>{now+=ms;stopped=true;},fetchFn:async(_url,opts)=>{calls++;starts.push({now,method:opts.method});return {ok:true,status:200,json:async()=>({})};},authorizeDispatch:s=>s.endpoint==='/orders'&&s.method==='POST',onAdmission:n=>admissions.push(n)});
 assert.throws(()=>new ReadClient('synthetic').dispatch({method:'POST',endpoint:'/orders',body:{}}));
 await client.request('/tournaments/midterm-elections');
 await assert.rejects(client.dispatch({method:'POST',endpoint:'/orders',body:{idempotencyKey:'frozen'},beforeDispatch:()=>{if(stopped)throw Error('Stopped');}}),/Stopped/);
 assert.equal(calls,1);assert.equal(admissions.length,1);assert.equal(now,6000);
});
test('GET and order writes serialize in the same persisted budget and body is frozen at enqueue',async()=>{
 let now=0;const sent=[],admissions=[];
 const client=new ReadClient('synthetic',{now:()=>now,sleepFn:async ms=>{now+=ms;},authorizeDispatch:s=>s.endpoint==='/orders'&&s.method==='POST',onAdmission:n=>admissions.push(n),fetchFn:async(_url,o)=>{sent.push({at:now,body:o.body});return {ok:true,status:201,json:async()=>({orderId:7})};}});
 const body={idempotencyKey:'same',quantity:1},write=client.dispatch({method:'POST',endpoint:'/orders',body,beforeDispatch:()=>{}});body.quantity=999;
 const read=client.request('/tournaments/midterm-elections');await Promise.all([write,read]);
 assert.deepEqual(sent.map(x=>x.at),[0,6000]);assert.equal(JSON.parse(sent[0].body).quantity,1);assert.deepEqual(admissions,[6000,12000]);
});
test('ambiguous official error exposes only known code and same-request delay',async()=>{
 const client=new ReadClient('synthetic',{authorizeDispatch:()=>true,fetchFn:async()=>({ok:false,status:409,headers:{get:()=>null},json:async()=>({code:'REQUEST_IN_FLIGHT',privateField:'never-surface'})})});
 await assert.rejects(client.dispatch({method:'POST',endpoint:'/orders',body:{},beforeDispatch:()=>{}}),e=>e.apiCode==='REQUEST_IN_FLIGHT'&&e.retryAfterMs===90000&&!JSON.stringify(e).includes('never-surface'));
});
test('a STOP during admission persistence is caught by the final network fence',async()=>{
 let stopped=false,sent=0;
 const client=new ReadClient('synthetic',{authorizeDispatch:()=>true,onAdmission:()=>{stopped=true;},fetchFn:async()=>{sent++;throw Error('should not send');}});
 await assert.rejects(client.dispatch({method:'POST',endpoint:'/orders',body:{},beforeDispatch:()=>{},beforeNetwork:()=>{if(stopped)throw Error('Stopped');}}),/Stopped/);
 assert.equal(sent,0);
});
