
import test from 'node:test';
import assert from 'node:assert/strict';
import {classifyOrderBatchResultV1,planOrderBatchV1,retryOrderBatchV1} from './order-batch-plan-v1.mjs';

const tournamentId='550e8400-e29b-41d4-a716-446655440000';
const intents=[
 {intentId:'entry-a',independentlyEligible:true,order:{exchangeId:'36',side:'yes',action:'buy',quantity:10,price:.42}},
 {intentId:'exit-b',independentlyEligible:true,order:{exchangeId:'37',side:'no',action:'sell',quantity:4,price:.3,tournamentId}},
];
const plan=()=>planOrderBatchV1({batchId:'batch-0001',tournamentId,intents});
const item=(index,status,ok,data={orderId:String(100+index)})=>({index,status,ok,data});

test('planner preserves eligible item order and pins explicit tournament scope',()=>{
 const p=plan();assert.deepEqual(p.intentIds,['entry-a','exit-b']);assert.deepEqual(p.body.orders.map(x=>x.exchangeId),['36','37']);assert(p.body.orders.every(x=>x.tournamentId===tournamentId));assert.equal(p.orderCount,2);assert(Object.isFrozen(p)&&Object.isFrozen(p.body)&&Object.isFrozen(p.body.orders));
 const expiry=new Date(Date.now()+24*60*60*1000).toISOString();const expiring=planOrderBatchV1({batchId:'expires',tournamentId,intents:[{...intents[0],order:{...intents[0].order,expirationDate:expiry}}]});assert.equal(expiring.body.orders[0].expirationDate,expiry);
});

test('logical operation produces stable key and exact byte-identical retry body',()=>{
 const a=plan(),b=plan();assert.equal(a.idempotencyKey,b.idempotencyKey);assert.equal(a.payloadHash,b.payloadHash);assert.deepEqual(retryOrderBatchV1(a),retryOrderBatchV1(b));assert.equal(JSON.stringify(retryOrderBatchV1(a)),a.bodyJson);
 const reordered=planOrderBatchV1({batchId:'batch-0001',tournamentId,intents:[...intents].reverse()});assert.notEqual(reordered.idempotencyKey,a.idempotencyKey);assert.notEqual(reordered.payloadHash,a.payloadHash);
 const nextOperation=planOrderBatchV1({batchId:'batch-0002',tournamentId,intents});assert.notEqual(nextOperation.idempotencyKey,a.idempotencyKey);
});

test('rejects invalid count, duplicate identity, ineligible or invalid orders',()=>{
 for(const input of [
  {batchId:'b',tournamentId,intents:[]},
  {batchId:'b',tournamentId,intents:Array.from({length:51},(_,i)=>({...intents[0],intentId:`i${i}`}))},
  {batchId:'b',tournamentId,intents:[intents[0],intents[0]]},
  {batchId:'b',tournamentId,intents:[{...intents[0],independentlyEligible:false}]},
  {batchId:'b',tournamentId,intents:[{...intents[0],order:{...intents[0].order,quantity:0}}]},
  {batchId:'b',tournamentId,intents:[{...intents[0],order:{...intents[0].order,price:.421}}]},
  {batchId:'b',tournamentId,intents:[{...intents[0],order:{exchangeId:'36',side:'yes',action:'buy',quantity:1,expirationDate:'2026-10-14T00:00:00Z'}}]},
  {batchId:'b',tournamentId,intents:[{...intents[0],order:{...intents[0].order,expirationDate:'2026-10-14'}}]},
  {batchId:'b',tournamentId,intents:[{...intents[0],order:{...intents[0].order,expirationDate:'2026-02-30T00:00:00Z'}}]},
  {batchId:'b',tournamentId,intents:[{...intents[0],order:{...intents[0].order,expirationDate:'2000-01-01T00:00:00Z'}}]},
  {batchId:'b',tournamentId,intents:[{...intents[0],order:{...intents[0].order,tournamentId:'other'}}]},
 ])assert.throws(()=>planOrderBatchV1(input));
});

test('accepts both one-order and maximum fifty-order batches',()=>{
 const one=planOrderBatchV1({batchId:'one',tournamentId,intents:[intents[0]]});assert.equal(one.orderCount,1);
 const fifty=Array.from({length:50},(_,i)=>({...intents[0],intentId:`intent-${i}`,order:{...intents[0].order,exchangeId:String(1000+i)}}));
 assert.equal(planOrderBatchV1({batchId:'fifty',tournamentId,intents:fifty}).orderCount,50);
});

test('200 success is classified per item and is not treated as a partial fill',()=>{
 const result=classifyOrderBatchResultV1(plan(),{status:200,body:{results:[item(0,201,true),item(1,201,true)]}});assert.equal(result.classification,'completed_success');assert.deepEqual(result.retryIndexes,[]);assert(result.results.every(x=>x.outcome==='success'));
});

test('207 mixed terminal outcome is completed partial; only failed item is reported',()=>{
 const result=classifyOrderBatchResultV1(plan(),{status:207,body:{results:[item(0,201,true),item(1,422,false,{error:'rejected'})]}});assert.equal(result.classification,'completed_partial');assert.equal(result.retryMode,'none');assert.deepEqual(result.results.map(x=>x.outcome),['success','terminal_failure']);assert.equal(result.results.length,2);
});

test('207 transient per-item results request exact same batch retry for only transient indexes',()=>{
 const result=classifyOrderBatchResultV1(plan(),{status:207,body:{results:[item(0,201,true),item(1,429,false,{code:'RATE_LIMITED',error:'wait'})]}});assert.equal(result.classification,'incomplete_retryable');assert.equal(result.retryMode,'same_batch_same_key');assert.deepEqual(result.retryIndexes,[1]);assert.equal(result.idempotencyKey,plan().idempotencyKey);assert.equal(result.payloadHash,plan().payloadHash);
});

test('503 budget stop retries same immutable body and identifies omitted tail indexes',()=>{
 const p=plan(),result=classifyOrderBatchResultV1(p,{status:503,body:{error:{code:'SERVICE_UNAVAILABLE',message:'budget exhausted'}}});assert.equal(result.classification,'request_retryable');assert.equal(result.retryMode,'same_batch_same_key');assert.deepEqual(result.retryIndexes,[0,1]);assert.deepEqual(result.missingIndexes,[0,1]);assert.deepEqual(retryOrderBatchV1(p),retryOrderBatchV1(p));
});

test('503 with partial results resumes transient and unissued indexes while preserving terminal/success indexes',()=>{
 const result=classifyOrderBatchResultV1(plan(),{status:503,headers:{'Retry-After':'7'},body:{results:[item(0,201,true)]}});assert.equal(result.classification,'incomplete_retryable');assert.deepEqual(result.retryIndexes,[1]);assert.deepEqual(result.missingIndexes,[1]);assert.equal(result.retryAfterSeconds,7);assert.equal(result.retryAfterSource,'response-header');assert.equal(result.retryBackoffRequired,true);
});

test('429 and 502 all-item outcomes are retryable with the identical batch identity',()=>{
 for(const [status,data] of [[429,{code:'RATE_LIMITED',error:'throttled'}],[502,{code:'ORDER_STATUS_UNKNOWN',error:'unknown'}]]){
  const result=classifyOrderBatchResultV1(plan(),{status,body:{results:[item(0,status,false,data),item(1,status,false,data)]}});assert.equal(result.classification,'incomplete_retryable');assert.deepEqual(result.retryIndexes,[0,1]);assert.equal(result.retryMode,'same_batch_same_key');
 }
});

test('422 all terminal failures are not retried',()=>{
 const result=classifyOrderBatchResultV1(plan(),{status:422,body:{results:[item(0,422,false,{error:'bad'}),item(1,403,false,{error:{code:'FORBIDDEN'}})]}});assert.equal(result.classification,'completed_terminal_failure');assert.equal(result.retryMode,'none');assert.deepEqual(result.retryIndexes,[]);
});

test('missing items are unresolved only for documented incomplete 503; otherwise malformed',()=>{
 const partial={results:[item(0,201,true)]};const safe=classifyOrderBatchResultV1(plan(),{status:503,body:partial});assert.equal(safe.classification,'incomplete_retryable');assert.deepEqual(safe.missingIndexes,[1]);assert.deepEqual(safe.retryIndexes,[1]);
 const unsafe=classifyOrderBatchResultV1(plan(),{status:207,body:partial});assert.equal(unsafe.classification,'malformed_response');assert.equal(unsafe.retryMode,'blocked');
});

test('duplicate, out-of-range, malformed and contract-inconsistent results are blocked',()=>{
 const cases=[
  {status:207,body:{results:[item(0,201,true),item(0,422,false)]}},
  {status:207,body:{results:[item(0,201,true),item(2,422,false)]}},
  {status:200,body:{results:[item(0,201,true),{index:1,ok:false,status:503}]}},
  {status:200,body:{results:[item(0,201,true),item(1,422,false)]}},
  {status:207,body:{results:[item(0,201,true),item(1,201,true)]}},
  {status:200,body:{results:[item(0,201,true),{...item(1,201,true),data:null}]}},
  {status:200,body:{results:[item(0,201,true),item(1,302,false)]}},
 ];for(const response of cases){const result=classifyOrderBatchResultV1(plan(),response);assert.equal(result.classification,'malformed_response');assert.equal(result.retryMode,'blocked');assert.deepEqual(result.retryIndexes,[]);}
});

test('outer request rate limit preserves Retry-After and terminal validation failure is distinguished',()=>{
 const rate=classifyOrderBatchResultV1(plan(),{status:429,headers:{'Retry-After':'60'},body:{error:{code:'RATE_LIMITED'}}});assert.equal(rate.classification,'request_retryable');assert.equal(rate.retryMode,'same_batch_same_key');assert.equal(rate.retryAfterSeconds,60);assert.equal(rate.retryBackoffRequired,true);
 const omittedHeader=classifyOrderBatchResultV1(plan(),{status:429,body:{error:{code:'RATE_LIMITED'}}});assert.equal(omittedHeader.retryAfterSeconds,60);assert.equal(omittedHeader.retryAfterSource,'documented-minimum');
 const shortHeader=classifyOrderBatchResultV1(plan(),{status:429,headers:{'Retry-After':'30'},body:{error:{code:'RATE_LIMITED'}}});assert.equal(shortHeader.retryAfterSeconds,60);
 const invalid=classifyOrderBatchResultV1(plan(),{status:400,body:{error:{code:'VALIDATION_ERROR'}}});assert.equal(invalid.classification,'request_terminal_failure');assert.equal(invalid.retryMode,'none');
});

test('REQUEST_IN_FLIGHT stays unresolved and carries the documented minimum wait',()=>{
 const result=classifyOrderBatchResultV1(plan(),{status:409,headers:{'Retry-After':'120'},body:{error:{code:'REQUEST_IN_FLIGHT'}}});assert.equal(result.classification,'request_retryable');assert.equal(result.retryMode,'same_batch_same_key');assert.deepEqual(result.retryIndexes,[0,1]);assert.equal(result.retryAfterSeconds,120);assert.equal(result.retryAfterSource,'response-header');
 const minimum=classifyOrderBatchResultV1(plan(),{status:409,body:{error:{code:'REQUEST_IN_FLIGHT'}}});assert.equal(minimum.retryAfterSeconds,90);assert.equal(minimum.retryAfterSource,'documented-minimum');
});

test('503 cannot report a completed result envelope as success or terminal completion',()=>{
 for(const results of [
  [item(0,201,true),item(1,201,true)],
  [item(0,403,false,{error:'denied'}),item(1,422,false,{error:'invalid'})],
  [item(0,429,false,{code:'RATE_LIMITED'}),item(1,429,false,{code:'RATE_LIMITED'})],
  [item(0,502,false,{code:'ORDER_STATUS_UNKNOWN'}),item(1,502,false,{code:'ORDER_STATUS_UNKNOWN'})],
 ]){
  const result=classifyOrderBatchResultV1(plan(),{status:503,body:{results}});assert.equal(result.classification,'malformed_response');assert.equal(result.retryMode,'blocked');
 }
});

test('unsupported retry envelopes and item error codes are blocked',()=>{
 for(const response of [
  {status:502,body:{error:{code:'INTERNAL_ERROR'}}},
  {status:503,body:{error:{code:'UNKNOWN_TRANSIENT'}}},
  {status:200,body:{results:[item(0,429,false,{code:'NOT_RATE_LIMITED'}),item(1,201,true)]}},
  {status:502,body:{results:[item(0,502,false,{code:'NOT_ORDER_STATUS_UNKNOWN'}),item(1,502,false,{code:'ORDER_STATUS_UNKNOWN'})]}},
 ]){const result=classifyOrderBatchResultV1(plan(),response);assert.equal(result.classification,'malformed_response');assert.equal(result.retryMode,'blocked');}
});

test('tampered plan and malformed response cannot produce retry instructions',()=>{
 const p=plan(),changed={...p,payloadHash:'0'.repeat(64)},misattributed={...p,intentIds:['wrong-a','wrong-b']},bodyViewMismatch={...p,body:{...p.body,orders:[]}};
 assert.throws(()=>retryOrderBatchV1(changed));assert.equal(classifyOrderBatchResultV1(changed,{status:503,body:{}}).classification,'malformed_response');
 assert.equal(classifyOrderBatchResultV1(misattributed,{status:200,body:{results:[item(0,201,true),item(1,201,true)]}}).classification,'malformed_response');
 assert.equal(classifyOrderBatchResultV1(bodyViewMismatch,{status:200,body:{results:[item(0,201,true),item(1,201,true)]}}).classification,'malformed_response');
 assert.equal(classifyOrderBatchResultV1(p,{status:503,body:null}).classification,'malformed_response');
});

