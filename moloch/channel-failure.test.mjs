import test from 'node:test';import assert from 'node:assert/strict';import {channelFailure} from './realtime/capture.mjs';
test('channel failures classify diagnosis without retaining secret-bearing messages',()=>{
 const r=channelFailure('CHANNEL_ERROR',{message:'JWT expired: PRIVATE_BEARER_VALUE'});assert.equal(r.category,'channel_authorization');assert.equal(JSON.stringify(r).includes('PRIVATE_BEARER_VALUE'),false);
 assert.equal(channelFailure('CHANNEL_ERROR',{message:'too many channels'}).category,'channel_capacity');assert.equal(channelFailure('TIMED_OUT').category,'join_timeout');assert.equal(channelFailure('CHANNEL_ERROR',{code:'ECONNRESET'}).category,'channel_network');assert.equal(channelFailure('CHANNEL_ERROR',{message:'PRIVATE_BEARER_VALUE'}).category,'unclassified');
});
