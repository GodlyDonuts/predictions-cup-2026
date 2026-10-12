import test from 'node:test';import assert from 'node:assert/strict';
import {readBodyFailureV1 as classify} from './read-body-failure-v1.mjs';
import {ReadClient} from './transport-runtime.mjs';
test('only typed read failures recover; writes, aborts and unknown callbacks do not',()=>{
 for(const cause of [new SyntaxError('private body'),Object.assign(Error(),{code:'UND_ERR_BODY_TIMEOUT'})]){
  assert.equal(classify(cause,{method:'GET'}).recoverable,true);
  assert.equal(classify(cause,{method:'POST'}).recoverable,false);
  assert.equal(classify(cause,{method:'GET',aborted:true}).recoverable,false);
 }
 assert.equal(classify(Error('private callback'),{method:'GET'}).recoverable,false);
});
test('durable failures remain fatal even when their type looks transient',()=>{
 const r=classify(Object.assign(new SyntaxError('private'),{durableRecordingFailed:true}),{method:'GET'});
 assert.equal(r.fatal,true);assert.equal(r.durableRecordingFailed,true);assert.notEqual(r.recoverable,true);assert.equal(r.message,'Invalid market API JSON');
});
test('actual transport records body_error and permits a subsequent fresh GET without fabricating a response',async()=>{
 let calls=0,clock=100000;const timings=[];
 const client=new ReadClient('SYNTHETIC',{interval:6000,now:()=>clock,sleepFn:async ms=>{clock+=ms;},fetchFn:async()=>({ok:true,status:200,json:async()=>{if(++calls===1)throw new SyntaxError('private');return {positions:[]};}}),onTiming:e=>timings.push(e)});
 await assert.rejects(client.request('/tournaments/midterm-elections/portfolio/positions'),e=>e.recoverable===true&&e.message==='Invalid market API JSON');
 const r=await client.request('/tournaments/midterm-elections/portfolio/positions');assert.deepEqual(r.data,{positions:[]});assert.equal(calls,2);assert.equal(timings[0].outcome,'body_error');
});

