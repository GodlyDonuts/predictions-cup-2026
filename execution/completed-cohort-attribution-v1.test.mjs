import test from 'node:test';import assert from 'node:assert/strict';import {attributeCompletedCohortV1 as run} from './completed-cohort-attribution-v1.mjs';
const o=(id,action,q,time,cost,cash,pnl)=>({intentId:id,marketId:'1',createdAt:time,body:{exchangeId:'2',action},proof:{scopedTradeVerified:true,fee:'0.000000',scopedPrincipalCost:cost,scopedProceeds:cash,realizedPnl:pnl,fills:[{quantity:q,filledAt:time}]}});
test('FIFO holding time and all realized gains/losses reconcile without winner selection',()=>{
 const buy=o('b','buy',2,'2026-10-07T00:00:00Z','0.200000','0.000000','0.000000'),sell=o('s','sell',-2,'2026-10-07T02:00:00Z','0.000000','0.180000','-0.020000'),r=run([buy,sell]);assert.equal(r.realized,'-0.020000');assert.equal(r.markets[0].weightedHoldingHours,2);assert.equal(r.returnOnDeployedPrincipalPercent,-10);assert.throws(()=>run([buy,sell,sell]),/DUPLICATE/);assert.throws(()=>run([buy]),/NOT_FLAT/);
});

