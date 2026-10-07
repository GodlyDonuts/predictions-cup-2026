import test from 'node:test';import assert from 'node:assert/strict';
import {genericReadPacketMayRetry} from './generic-read-packet-retry-v1.mjs';
test('only a stale read with no financial intent may retry collection',()=>{
 const e={code:'REST_NO_RECEIPT_TIME',fatal:true},s={phase:'ready',intent:null,unknown:false};
 assert.equal(genericReadPacketMayRetry(e,s),true);
 for(const extra of[{durableRecordingFailed:true},{mayHaveExecuted:true},{code:'CORRUPT_JOURNAL'}])assert.equal(genericReadPacketMayRetry({...e,...extra},s),false);
 for(const extra of[{phase:'prepared'},{intent:{actualFetchCount:0}},{phase:'uncertain'},{unknown:true}])assert.equal(genericReadPacketMayRetry(e,{...s,...extra}),false);
});
