import test from 'node:test';import assert from 'node:assert/strict';import {boundedLookup} from './realtime/dns-lookup.mjs';
test('only transient DNS lookup falls back once with the original hostname',()=>{
 const evidence=[],host='private-host.invalid';let lookups=0,resolves=0,result;
 const lookup=boundedLookup(e=>evidence.push(e),(h,o,cb)=>{lookups++;assert.equal(h,host);cb({code:'EAI_AGAIN'});},(h,cb)=>{resolves++;assert.equal(h,host);cb(null,['192.0.2.1','192.0.2.2']);});
 lookup(host,{all:true},(...args)=>result=args);assert.equal(lookups,1);assert.equal(resolves,1);assert.deepEqual(result,[null,[{address:'192.0.2.1',family:4},{address:'192.0.2.2',family:4}]]);assert.doesNotMatch(JSON.stringify(evidence),/private|192/);
});
test('normal resolution and permanent lookup failures never invoke the fallback',()=>{
 for(const error of [null,{code:'ENOTFOUND'},{code:'ECONNREFUSED'}]){let result;const lookup=boundedLookup(()=>assert.fail(),(h,o,cb)=>cb(error,'192.0.2.1',4),()=>assert.fail());lookup('private',{},(...a)=>result=a);assert.equal(result[0],error);}
 const ipv6=boundedLookup(()=>assert.fail(),(h,o,cb)=>cb({code:'EAI_AGAIN'}),()=>assert.fail());ipv6('private',{family:6},error=>assert.equal(error.code,'EAI_AGAIN'));
});
test('failed or empty fallback terminates without a loop or an invalid address',()=>{
 for(const addresses of [null,[],['PRIVATE_TOKEN'],['::1']]){let attempts=0,result;const lookup=boundedLookup(()=>{},(h,o,cb)=>cb({code:'EAI_AGAIN'}),(h,cb)=>{attempts++;cb(null,addresses);});lookup('private',{},(...a)=>result=a);assert.equal(attempts,1);assert.equal(result[0].code,'EAI_AGAIN');}
});
