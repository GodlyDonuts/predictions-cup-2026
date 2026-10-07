import test from 'node:test';import assert from 'node:assert/strict';import {selectCohortProfitLockedDepthV1 as select} from './cohort-profit-locked-depth-sweep-v1.mjs';
test('multi-rung sweep chooses greatest safe liquidation quantity without spending below floor',()=>{
 const r=select({remainingLots:[{quantity:3,entryPrice:'0.310000'}],remainingOwnedQuantity:3,realizedProfit:'0.025000',yesAsks:[{price:'0.690000',quantity:1},{price:'0.695000',quantity:2}]});assert.equal(r.selected.quantity,2);assert.equal(r.selected.throughLevel,1);assert.equal(r.selected.worstPartialFillCohortLowerBound,'0.005000');assert.equal(r.executionAuthority,false);
});
test('duplicate or reversed rungs cannot fabricate covered exit capacity',()=>{
 const args={remainingLots:[{quantity:3,entryPrice:'0.310000'}],remainingOwnedQuantity:3,realizedProfit:'0.975000'};
 for(const prices of[['0.690000','0.690000'],['0.695000','0.690000']])assert.throws(()=>select({...args,yesAsks:prices.map(price=>({price,quantity:2}))}));
});
