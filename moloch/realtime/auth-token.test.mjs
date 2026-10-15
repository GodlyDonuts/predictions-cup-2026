import test from 'node:test';import assert from 'node:assert/strict';import {EventEmitter} from 'node:events';
import {authToken} from './auth-token.mjs';import {httpsOnce} from './read-get.mjs';
const input={headers:{Authorization:'Bearer SENTINEL'}};
test('one original POST uses the fixed auth route, TLS verification and no body',async()=>{
 let requests=0;const result=await authToken(input,{request:(url,options,callback)=>{requests++;assert.equal(url.href,'https://sig.thesuper.market/api/v1/realtime/token');assert.equal(options.method,'POST');assert.equal(options.rejectUnauthorized,true);assert.equal(options.headers.Authorization,'Bearer SENTINEL');const req=new EventEmitter();req.destroy=()=>{};req.end=(body)=>{assert.equal(body,undefined);queueMicrotask(()=>{const res=new EventEmitter();Object.assign(res,{statusCode:200,headers:{},complete:true});callback(res);res.emit('data',Buffer.from('{"token":"PRIVATE_PIPE_ONLY"}'));res.emit('end');});};return req;}});
 assert.equal(requests,1);assert.equal(result.status,200);assert.equal(JSON.parse(result.content).token,'PRIVATE_PIPE_ONLY');
});
test('auth failure does not retry or echo error credentials',async()=>{
 let requests=0;const result=await authToken(input,{request:()=>{requests++;const req=new EventEmitter();req.destroy=()=>{};req.end=()=>queueMicrotask(()=>req.emit('error',{code:'ENOTFOUND',message:'SENTINEL'}));return req;}});assert.equal(requests,1);assert.equal(result.status,0);assert.ok(!JSON.stringify(result).includes('SENTINEL'));
});
test('auth accepts no endpoint, body, extra header, order route or alternate host',()=>{
 assert.throws(()=>authToken({...input,endpoint:'/orders'}),/AUTH_INPUT/);assert.throws(()=>authToken({headers:{...input.headers,Host:'evil.test'}}),/AUTH_INVALID/);
 assert.throws(()=>httpsOnce(new URL('https://sig.thesuper.market/api/v1/orders'),input.headers,{method:'POST'}),/METHOD_INVALID/);
 assert.throws(()=>httpsOnce(new URL('https://evil.test/api/v1/realtime/token'),input.headers,{method:'POST'}),/DESTINATION_INVALID/);
});
