import test from 'node:test';import assert from 'node:assert/strict';import {EventEmitter} from 'node:events';
import {readGet,validateRead} from './read-get.mjs';
const input={endpoint:'/markets/233/orderbook?tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55&depth=200',headers:{Authorization:'Bearer SECRET_SENTINEL'}};
function fake({status=200,body='{}',error,complete=true,hang=false,headers={}}={}){
 return (url,options,callback)=>{assert.equal(url.hostname,'sig.thesuper.market');assert.equal(options.method,'GET');assert.equal(options.rejectUnauthorized,true);assert.equal(options.headers.Authorization,input.headers.Authorization);
  const req=new EventEmitter();req.destroy=()=>{};req.end=()=>{if(hang)return;queueMicrotask(()=>{if(error){req.emit('error',error);return;}const res=new EventEmitter();Object.assign(res,{statusCode:status,headers,complete,destroy(){}});callback(res);res.emit('data',Buffer.from(body));res.emit('end');});};return req;
 };
}
test('GET accepts scoped books and rejects alternate destinations, writes and extra headers',()=>{
 assert.equal(validateRead(input).hostname,'sig.thesuper.market');
 for(const endpoint of ['https://evil.test','//evil.test/x','/orders/1','/markets/1/orderbook?depth=200&depth=20','/markets/1/../orders/1?depth=200',input.endpoint+'&unknown=1'])assert.throws(()=>validateRead({...input,endpoint}));
 assert.throws(()=>validateRead({...input,method:'POST'}));assert.throws(()=>validateRead({...input,headers:{...input.headers,Host:'evil.test'}}));
});
test('success preserves exact body and performs one request',async()=>{let count=0;const impl=fake({body:'{"price":0.5}'});const result=await readGet(input,{request:(...args)=>{count++;return impl(...args);}});assert.equal(count,1);assert.equal(result.content,'{"price":0.5}');assert.equal(result.status,200);});
test('HTTP errors omit reflected secrets and preserve Retry-After without redirect',async()=>{for(const status of [302,401,429]){const result=await readGet(input,{request:fake({status,body:'SECRET_SENTINEL',headers:{'retry-after':'120'}})});assert.equal(result.status,status);assert.equal(result.retryAfterSeconds,120);assert.equal(result.content,null);assert.ok(!JSON.stringify(result).includes('SECRET_SENTINEL'));}});
test('network errors are allowlisted and redact arbitrary messages',async()=>{const result=await readGet(input,{request:fake({error:{code:'EAI_AGAIN',message:'SECRET_SENTINEL'}})});assert.equal(result.transport.category,'transport_dns');assert.ok(!JSON.stringify(result).includes('SECRET_SENTINEL'));});
test('whole-operation timeout, body cap and incomplete response fail closed',async()=>{
 assert.equal((await readGet(input,{request:fake({hang:true}),timeoutMs:5})).transport.transportCode,'READ_TIMEOUT');
 assert.equal((await readGet(input,{request:fake({body:'12345'}),maxBytes:4})).transport.transportCode,'READ_BODY_LIMIT');
 assert.equal((await readGet(input,{request:fake({complete:false})})).transport.transportCode,'READ_BODY_ABORTED');
});
