import test from 'node:test';import assert from 'node:assert/strict';import {sizeCohortProfitLockedNoExitV1 as size} from './cohort-profit-locked-no-exit-v1.mjs';
const args=(lots,realizedProfit,ask)=>({remainingLots:lots,remainingOwnedQuantity:lots.reduce((n,l)=>n+l.quantity,0),realizedProfit,yesAskLevel:{price:ask,quantity:1000}});
test('realized gains can close a losing position while every partial fill remains profitable',()=>{
 const p=size(args([{quantity:37,entryPrice:'0.310000'}],'0.975000','0.695000'));assert.equal(p.quantity,37);assert.equal(p.protectedLimit,'0.300000');assert.equal(p.protectedTradePnlIfFilled,'-0.370000');assert.equal(p.worstPartialFillCohortLowerBound,'0.605000');assert.equal(p.executionAuthority,false);
});
test('cheap later lots cannot hide an unsafe expensive first partial fill',()=>{
 const p=size(args([{quantity:1,entryPrice:'0.800000'},{quantity:9,entryPrice:'0.100000'}],'0.400000','0.695000'));assert.equal(p.quantity,0);
 const q=size(args([{quantity:1,entryPrice:'0.100000'},{quantity:2,entryPrice:'0.800000'}],'0.400000','0.695000'));assert.equal(q.quantity,2);assert.equal(q.worstPartialFillCohortLowerBound,'0.100000');
});
test('profit floor, observed depth and order notional remain binding',()=>{
 const x=args([{quantity:100,entryPrice:'0.310000'}],'0.025000','0.695000');assert.equal(size(x).quantity,2);assert.equal(size({...x,realizedProfit:'0.004000'}).quantity,0);assert.equal(size({...args([{quantity:100,entryPrice:'0.900000'}],'10.000000','0.095000'),protectiveTicks:0}).quantity,27);assert.throws(()=>size({...x,feesPerShare:'0.001000'}));
});

