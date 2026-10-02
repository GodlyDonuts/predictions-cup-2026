import test from 'node:test';import assert from 'node:assert/strict';
import {accountPositions,accountOrders,accountFills,accountPnl,accountView} from './account.mjs';import {ReadClient} from './runtime.mjs';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55',at='2026-10-02T07:00:00Z';
test('official display preserves signed NO holdings, valuation marks and unknown trade action without leaking raw fields',()=>{
 const positions=accountPositions({receivedAt:at,data:{positions:[{exchangeId:'12',marketId:'2',marketTitle:'Example',quantity:-10,avgCost:.3,currentPrice:.4,marketValue:4,costBasis:3,unrealizedPnl:1,privateField:'must-not-export'}]}});
 const fills=accountFills({receivedAt:at,data:{data:[{id:5,orderId:7,exchangeId:'12',marketId:'2',side:'no',quantity:-2,price:.3,filledAt:at}],pagination:{hasMore:true},coverage:{complete:true}}});
 const cache={positions,fills};let v=accountView(cache,'running',at);assert.equal(v.positions[0].side,'no');assert.equal(v.positions[0].quantity,10);assert.equal(v.positions[0].valuationValue,4);assert.equal(v.trades[0].action,null);assert.equal(v.fillHistory.hasMore,true);assert.ok(!JSON.stringify(v).includes('must-not-export'));
 assert.equal(accountView({fills},'degraded',at).available,true);assert.equal(accountView({fills},'degraded',at).sourceTimes.positions,null);
 cache.orderHistory=accountOrders([{id:7,exchangeId:'12',side:'no',action:'sell',quantity:2,priceLimit:.3,open:false,createdAt:at,tournamentId:tid}],at,{openOnly:false});v=accountView(cache,'degraded','2026-10-02T07:10:00Z');assert.equal(v.trades[0].action,'sell');assert.equal(v.ages.positions,600000);assert.equal(v.collectorStatus,'degraded');assert.equal(v.metrics,null);
 assert.throws(()=>accountOrders([{id:7,exchangeId:'12',side:'no',action:'sell',quantity:2,priceLimit:.3,open:true,createdAt:at,tournamentId:'different'}],at));
 assert.throws(()=>accountFills({receivedAt:at,data:{data:[{id:5,orderId:7,exchangeId:'12',marketId:'2',side:'no',quantity:2,price:.3,filledAt:at}],pagination:{hasMore:false},coverage:{complete:true}}}));
});
test('official read endpoints remain fixed scope GETs and share serialized admission',async()=>{
 let now=0;const calls=[];const api=new ReadClient('invented-test-key',{now:()=>now,sleepFn:async ms=>{now+=ms;},fetchFn:async(url,options)=>{calls.push({url,method:options.method});return {ok:true,status:200,json:async()=>({})};}});
 await Promise.all([api.request('/tournaments/midterm-elections/portfolio/fills?limit=100'),api.request('/tournaments/midterm-elections/portfolio/pnl?period=day')]);assert.ok(now>=6000);assert.ok(calls.every(x=>x.method==='GET'));await assert.rejects(api.request('/tournaments/other/portfolio/fills?limit=100'),/allowlist/);await assert.rejects(api.request('/orders/cancel-all'),/allowlist/);
});
