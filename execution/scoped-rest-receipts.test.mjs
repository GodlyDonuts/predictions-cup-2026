import test from 'node:test';
import assert from 'node:assert/strict';
import {createScopedRestReceipts} from './scoped-rest-receipts.mjs';
import {exactMoneyFetch} from './exact-money-receipts.mjs';
import {payloadHash} from './core.mjs';

const scope={tournamentId:'test-tournament',tournamentSlug:'midterm-elections'},base=Date.parse('2026-10-06T11:00:00.000Z');
const cash='/tournaments/midterm-elections',url=p=>'https://sig.thesuper.market/api/v1'+p,iso=n=>new Date(n).toISOString();
const cashBody='{"id":"test-tournament","isPendingEnrolment":false,"myBalance":100.23}';
function fixture({body=cashBody,capacity,maximumBytes,fetchFn,onFetchStart,constant=false}={}){
 let clock=base,calls=0;const seen=[];
 const now=()=>constant?clock:++clock;
 const producer=createScopedRestReceipts({scope,now,capacity,maximumBytes,fetchFn:fetchFn||(()=>{calls++;return Promise.resolve(new Response(body,{status:200}));}),onFetchStart:async e=>{seen.push(e);await onFetchStart?.(e);}});
 const call=async(endpoint=cash,options={})=>{const start=now(),r=await producer.fetch(url(endpoint),options),data=await r.json(),end=now();return{endpoint,status:r.status,requestStartedAt:iso(start),receivedAt:iso(end),data};};
 return{producer,call,seen,now,calls:()=>calls};
}
test('records an actual existing fetch invocation and binds a raw receipt without inspecting credentials',async()=>{
 const f=fixture();const options={};Object.defineProperty(options,'headers',{get(){throw Error('must not inspect auth');}});
 const response=await f.call(cash,options),r=f.producer.receiptOf(response);
 assert.equal(f.calls(),1);assert.equal(f.seen.length,1);assert.equal(r.bodyText,cashBody);assert.equal(r.method,'GET');assert.equal(r.requestBodyHash,null);assert(Object.isFrozen(r));
 assert.equal(f.producer.stats().additionalClients,0);assert.equal(f.producer.stats().executionAuthority,false);
 assert(!JSON.stringify(f.seen).includes('headers'));assert(!JSON.stringify(f.producer.stats()).includes('myBalance'));
});
test('composes underneath the existing exact-money decoder without losing its receipt or original numeric body',async()=>{
 const f=fixture(),money=[];const fetch=exactMoneyFetch(f.producer.fetch,{...scope,now:f.now,onReceipt:r=>money.push(r)});
 const start=f.now(),response=await fetch(url(cash)),data=await response.json(),end=f.now();
 const receipt=f.producer.receiptOf({endpoint:cash,status:200,requestStartedAt:iso(start),receivedAt:iso(end),data});
 assert.equal(money.length,1);assert.equal(money[0].fields.myBalance.decimal,'100.230000');assert.equal(receipt.bodyText,cashBody);assert.equal(receipt.sourceBodyHash,money[0].sourceBodyHash);
});
test('POST receipts bind exact request body hashes and callback occurs after supplied fetch was called',async()=>{
 const request={tournamentId:scope.tournamentId,exchangeId:'1066',side:'no',action:'buy',quantity:1,price:0.33,idempotencyKey:'fixed-key'},responseBody='{"orderId":7,"exchangeId":"1066","totalCost":0.33}';let invoked=false;
 const f=fixture({fetchFn:()=>{invoked=true;return Promise.resolve(new Response(responseBody));},onFetchStart:e=>{assert(invoked);assert.equal(e.requestBodyHash,payloadHash(request));}});
 const response=await f.call('/orders',{method:'POST',body:JSON.stringify(request)}),r=f.producer.receiptOf(response,{method:'POST',requestBodyHash:payloadHash(request)});
 assert.equal(r.endpoint,'/orders');assert.equal(r.bodyText,responseBody);assert.equal(r.fetchInvocationId,f.seen[0].fetchInvocationId);
 assert.throws(()=>f.producer.receiptOf(response,{method:'POST',requestBodyHash:'wrong'}),/UNBOUND/);
});
test('mutated data, method, endpoint and clock windows cannot bind a different receipt',async()=>{
 const f=fixture(),response=await f.call();
 for(const changed of [{...response,data:{...response.data,myBalance:0}},{...response,endpoint:'/orders'},{...response,requestStartedAt:response.receivedAt},{...response,receivedAt:response.requestStartedAt}])assert.throws(()=>f.producer.receiptOf(changed),/UNBOUND|CLOCK/);
 assert.throws(()=>f.producer.receiptOf(response,{method:'POST'}),/UNBOUND/);
});
test('bounded retention evicts old raw bodies instead of silently returning the latest matching value',async()=>{
 const f=fixture({capacity:1}),first=await f.call(),second=await f.call();assert.throws(()=>f.producer.receiptOf(first),/UNBOUND/);assert.equal(f.producer.receiptOf(second).bodyText,cashBody);assert.equal(f.producer.stats().records,1);
});
test('a body exceeding the configured retained-byte budget cannot become source evidence',async()=>{
 const f=fixture({maximumBytes:10});await assert.rejects(f.call(),/BODY_BOUND/);assert.equal(f.producer.stats().records,0);
});
test('a failed invoked POST retains start identity without exposing a sensitive underlying error',async()=>{
 const f=fixture({fetchFn:()=>Promise.reject(Error('secret-header-value'))});
 await assert.rejects(f.call('/orders',{method:'POST',body:JSON.stringify({tournamentId:scope.tournamentId})}),e=>e.code==='REST_RECEIPT_FETCH_UNAVAILABLE'&&e.mayHaveExecuted===true&&typeof e.requestStartedAt==='string'&&!JSON.stringify(e).includes('secret-header-value')&&!e.message.includes('secret'));
 assert.equal(f.seen.length,1);assert.equal(f.producer.stats().records,0);
});
test('failure to durably observe an invoked fetch never claims no submission or yields an ACK',async()=>{
 const f=fixture({onFetchStart:()=>{throw Error('private journal failure');}});
 await assert.rejects(f.call('/orders',{method:'POST',body:JSON.stringify({tournamentId:scope.tournamentId})}),e=>e.code==='REST_RECEIPT_START_RECORDING_FAILED'&&e.mayHaveExecuted===true&&e.fatal&&e.durableRecordingFailed);
 assert.equal(f.calls(),1);assert.equal(f.producer.stats().records,0);
});
test('identical bodies with ambiguous timing are rejected rather than attributed by latest response',async()=>{
 const f=fixture({constant:true}),a=await f.call();await f.call();assert.throws(()=>f.producer.receiptOf(a),/UNBOUND/);
});
test('duplicate JSON keys cannot alter economic source interpretation',async()=>{
 const f=fixture({body:'{"myBalance":1,"myBalance":2}'});await assert.rejects(f.call(),/DUPLICATE_KEY/);assert.equal(f.producer.stats().records,0);
});
test('out-of-scope public fetch remains unchanged and is not recorded',async()=>{
 const f=fixture(),r=await f.producer.fetch('https://sig.thesuper.market/docs');assert.equal(await r.text(),cashBody);assert.equal(f.seen.length,0);assert.equal(f.producer.stats().records,0);
});
test('text decoder is captured once and does not permit repeated body consumption',async()=>{
 const f=fixture(),r=await f.producer.fetch(url(cash));assert.equal(await r.text(),cashBody);await assert.rejects(r.json(),/BODY_USED/);assert.equal(f.producer.stats().records,1);
});
test('closing clears retained private bodies and prevents further fetches or late captures',async()=>{
 const f=fixture(),r=await f.producer.fetch(url(cash));f.producer.close();await assert.rejects(r.json(),/CLOSED/);await assert.rejects(f.call(),/CLOSED/);assert.equal(f.producer.stats().bytes,0);assert.equal(f.producer.stats().records,0);
});
test('overflow cancels the stream before materializing an arbitrary complete response text',async()=>{
 let reads=0,cancelled=false;const response={status:200,text(){throw Error('unbounded text must not be called');},body:new ReadableStream({pull(controller){reads++;controller.enqueue(new Uint8Array(9));},cancel(){cancelled=true;}},{highWaterMark:0})};
 const f=fixture({maximumBytes:16,fetchFn:()=>Promise.resolve(response)});
 await assert.rejects(f.call(),/BODY_BOUND/);assert.equal(reads,2);assert(cancelled);assert.equal(f.producer.stats().records,0);
});
test('a text-only response cannot masquerade as a bounded raw producer',async()=>{
 let textCalls=0;const f=fixture({fetchFn:()=>Promise.resolve({status:200,text(){textCalls++;return cashBody;},json(){return JSON.parse(cashBody);}})});
 await assert.rejects(f.call(),/STREAM_REQUIRED/);assert.equal(textCalls,0);assert.equal(f.producer.stats().records,0);
});
test('closing before decode does not pull a single private response byte',async()=>{
 let reads=0;const f=fixture({fetchFn:()=>Promise.resolve({status:200,body:new ReadableStream({pull(c){reads++;c.close();}},{highWaterMark:0})})});const r=await f.producer.fetch(url(cash));f.producer.close();await assert.rejects(r.json(),/CLOSED/);assert.equal(reads,0);
});
test('streamed UTF8 response text matches native decoding across split code points',async()=>{
 const body='{"message":"café","myBalance":100.23}',raw=new TextEncoder().encode(body);let i=0;const f=fixture({body,fetchFn:()=>Promise.resolve(new Response(new ReadableStream({pull(c){if(i===raw.length)c.close();else{const start=i++;c.enqueue(raw.slice(start,start+1));}}})))});
 const response=await f.call(),r=f.producer.receiptOf(response);assert.equal(r.bodyText,body);assert.equal(response.data.message,'café');
});
test('only explicitly scoped depth20 market books gain source receipts',async()=>{
 let clock=base;const source=createScopedRestReceipts({scope:{...scope,marketIds:['354']},now:()=>++clock,fetchFn:()=>Promise.resolve(new Response('{"book":true}'))});
 const endpoint='/markets/354/orderbook?tournamentId=test-tournament&depth=20',start=++clock,r=await source.fetch(url(endpoint)),data=await r.json(),end=++clock;assert.equal(source.receiptOf({endpoint,status:200,requestStartedAt:iso(start),receivedAt:iso(end),data}).endpoint,endpoint);
 for(const other of ['/markets/355/orderbook?tournamentId=test-tournament&depth=20','/markets/354/orderbook?tournamentId=test-tournament&depth=200','/markets/354/orderbook?tournamentId=foreign&depth=20','/markets/354/orderbook?tournamentId=test-tournament&depth=20&depth=20'])await(await source.fetch(url(other))).json();
 assert.equal(source.stats().captured,1);
});
