import test from 'node:test';import assert from 'node:assert/strict';import {aonDepthProceeds} from './aon-depth-proceeds.mjs';
const policy={feeBps:0,feePerShare:'0.000000'};
test('YES proceeds walk depth and skip already-consumed shares',()=>{
 const r=aonDepthProceeds('YES','0.905000',8,{bids:[{price:'0.920000',quantity:10},{price:'0.910000',quantity:10}]},policy,3n);
 assert.equal(r.gross,'7.350000');assert.equal(r.netImprovement,'0.110000');assert.deepEqual(r.segments.map(s=>s.quantity),[7,1]);
});
test('NO proceeds use complements of ascending YES asks',()=>{
 const r=aonDepthProceeds('NO','0.895000',6,{asks:[{price:'0.090000',quantity:4},{price:'0.100000',quantity:10}]},policy);
 assert.equal(r.gross,'5.440000');assert.equal(r.netImprovement,'0.070000');
});
test('insufficient at-limit depth cannot create partial AON proceeds',()=>{
 const r=aonDepthProceeds('YES','0.915000',6,{bids:[{price:'0.920000',quantity:4},{price:'0.910000',quantity:100}]},policy);
 assert.equal(r.canFill,false);assert.equal(r.filledQuantity,0);assert.equal(r.gross,null);assert.deepEqual(r.segments,[]);
});
