import test from 'node:test';import assert from 'node:assert/strict';import {futureActivity} from './research/activity-persistence.mjs';
const event=(received,executed,id='t')=>({event:{kind:'broadcast',event:'market_batch',receivedAt:new Date(received).toISOString(),payload:{books:[{exchangeId:'1'}],trades:[{exchangeId:'1',id,sequence:1,quantity:100,executedAt:new Date(executed).toISOString()}]},duplicate:false,gap:false}});
test('future activity excludes late arrivals executed before activation and deduplicates prints',()=>{
 const rows=[event(2000,500,'old'),event(3000,2000),event(4000,2000),event(4500,4000,'other')];const r=futureActivity(rows,['1'],1000,4000);assert.equal(r.observedFutureTrades,1);assert.equal(r.observedFutureShares,'100');
 assert.throws(()=>futureActivity([event(2000,500),event(3000,2000)],['1'],1000,4000),/TRADE_CONFLICT/);
});
test('selected-market gaps invalidate the label even when no trade accompanies the gap',()=>{
 const r=event(2000,1500);r.event.gap=true;r.event.payload.trades=[];assert.equal(futureActivity([r],['1'],1000,4000).futureGapDetected,true);assert.equal(futureActivity([r],['2'],1000,4000).futureGapDetected,false);
});
test('market-scoped dirty events before activation invalidate a selected label without a book payload',()=>{
 const dirty={event:{kind:'broadcast',event:'book_dirty',marketId:'101',receivedAt:new Date(900).toISOString(),payload:{}}};
 assert.equal(futureActivity([dirty],['1'],1000,4000,[{marketId:'101',exchangeId:'1'}]).futureGapDetected,true);
});
