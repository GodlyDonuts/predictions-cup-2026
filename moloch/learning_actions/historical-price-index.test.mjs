import test from 'node:test';
import assert from 'node:assert/strict';
import {encode,sampleAtSourceTime,RECORD_BYTES} from './historical-price-index.mjs';
test('exact original IEEE sample and receipt survive encoding',()=>{
 const p=.0600000000000001,b=encode([{timeMs:1000,price:p,receipt:3},{timeMs:2000,price:.5,receipt:4}]);
 assert.equal(b.length,2*RECORD_BYTES);assert.equal(sampleAtSourceTime(b,999),null);
 const v=sampleAtSourceTime(b,1999);assert.equal(v.price,p);assert.equal(v.receipt,3);assert.equal(v.sourceTimeMs,1000);assert.equal(v.originalDeliveryVerified,false);
 assert.equal(sampleAtSourceTime(b,2000).receipt,4);
});
test('unordered duplicates, invalid prices and bad shape cannot enter index',()=>{
 assert.throws(()=>encode([{timeMs:1,price:.2,receipt:0},{timeMs:1,price:.3,receipt:0}]));
 assert.throws(()=>encode([{timeMs:1,price:1.00000001,receipt:0}]));
 assert.throws(()=>encode([{timeMs:1,price:.5,receipt:-1}]));
 assert.throws(()=>sampleAtSourceTime(Buffer.alloc(23),1));
});
