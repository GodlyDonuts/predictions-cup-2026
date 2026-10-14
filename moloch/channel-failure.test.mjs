import test from 'node:test';import assert from 'node:assert/strict';import {channelFailure} from './realtime/capture.mjs';
test('channel failures classify diagnosis without retaining secret-bearing messages',()=>{
 const r=channelFailure('CHANNEL_ERROR',{message:'JWT expired: PRIVATE_BEARER_VALUE'});assert.equal(r.category,'channel_authorization');assert.equal(JSON.stringify(r).includes('PRIVATE_BEARER_VALUE'),false);
 assert.equal(channelFailure('CHANNEL_ERROR',{message:'too many channels'}).category,'channel_capacity');assert.equal(channelFailure('TIMED_OUT').category,'join_timeout');assert.equal(channelFailure('CHANNEL_ERROR',{code:'ECONNRESET'}).category,'channel_network');assert.equal(channelFailure('CHANNEL_ERROR',{message:'PRIVATE_BEARER_VALUE'}).category,'unclassified');
});
test('wrapped socket and aggregate causes expose only fixed diagnostic codes',()=>{
 const error=new Error('PRIVATE_BEARER_VALUE',{cause:{name:'ErrorEvent',error:new AggregateError([{code:'ENOTFOUND',message:'PRIVATE_HOST_AND_TOKEN'}],'PRIVATE_URL')}});
 const r=channelFailure('CHANNEL_ERROR',error);assert.equal(r.category,'channel_network');assert.deepEqual(r.networkCodes,['ENOTFOUND']);assert.equal(r.errorShape,'Error');assert.doesNotMatch(JSON.stringify(r),/PRIVATE/);
 const cyclic={message:'PRIVATE_VALUE'};cyclic.cause=cyclic;const c=channelFailure('CHANNEL_ERROR',cyclic);assert.equal(c.inspectedErrorNodes,1);assert.equal(c.category,'unclassified');assert.doesNotMatch(JSON.stringify(c),/PRIVATE/);
});
test('unknown arbitrary codes and cause chains are bounded and never persisted',()=>{
 let e={code:'PRIVATE_TOKEN',message:'PRIVATE_VALUE'};for(let i=0;i<100;i++)e={cause:e};const r=channelFailure('CHANNEL_ERROR',e);assert.equal(r.inspectedErrorNodes,12);assert.equal(r.networkCodes,undefined);assert.doesNotMatch(JSON.stringify(r),/PRIVATE/);
 const a=channelFailure('CHANNEL_ERROR',{cause:{reason:'authorization failed PRIVATE_BEARER'}});assert.equal(a.category,'channel_authorization');assert.doesNotMatch(JSON.stringify(a),/PRIVATE/);
});
