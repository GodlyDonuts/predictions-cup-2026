import test from 'node:test';import assert from 'node:assert/strict';
import {workerHealth,baselineView,marketDataHealth} from './worker-health.mjs';
test('dead worker is stale even when its persisted status says running',()=>{
 const now=Date.parse('2026-10-02T06:10:00Z');
 assert.equal(workerHealth({status:'running',updatedAt:'2026-10-02T06:08:00Z'},now).status,'stale_worker');
 assert.equal(workerHealth({status:'running',reportedAt:'2026-10-02T06:09:50Z'},now).status,'running');
 assert.equal(workerHealth({status:'running',updatedAt:'2026-10-02T06:11:00Z'},now).status,'invalid_heartbeat');
});

test('baseline coverage counts currently fresh required books, not the newest receipt or ever-observed markets',()=>{
 const now=Date.parse('2026-10-02T06:10:00Z'),config={feeBps:0,maxDataAgeMs:20000,marketIds:['a','b']};
 const wallet={books:{a:{marketId:'a',receivedAt:'2026-10-02T06:09:59Z'},b:{marketId:'b',receivedAt:'2026-10-02T06:09:20Z'}},strategies:{test:{positions:{b:{quantity:1}}}}};
 const dataHealth=marketDataHealth(wallet,config,now);assert.equal(dataHealth.coveredMarkets,1);assert.equal(dataHealth.everObservedMarkets,2);assert.equal(dataHealth.staleHeldInstruments,1);
 const baseline={mode:'prospective_fee_free_paper_baseline',config,status:'running',reportedAt:'2026-10-02T06:09:59Z',lastObservationAt:'2026-10-02T06:09:59Z',observations:10,dataHealth};
 const legacy={config:{feeBps:10},strategies:{}};
 assert.equal(baselineView(baseline,legacy,{},now).baselineDataStatus,'stale_market_data');assert.equal(baselineView(baseline,legacy,{},now).baselineBookAgeMs,40000);
 wallet.books.b.receivedAt=wallet.books.a.receivedAt;baseline.dataHealth=marketDataHealth(wallet,config,now);
 assert.equal(baselineView(baseline,legacy,{},now).baselineDataStatus,'fresh_market_data');
 const aged=baselineView(baseline,legacy,{},now+25000);assert.equal(aged.dataHealth.coveredMarkets,0);assert.equal(aged.baselineDataStatus,'stale_market_data');
});

test('required invalidations, missing held books and future clocks cannot appear fresh',()=>{
 const now=Date.parse('2026-10-02T06:10:00Z'),config={feeBps:0,maxDataAgeMs:20000,marketIds:['a']};
 const wallet={books:{a:{marketId:'a',receivedAt:'2026-10-02T06:09:59Z'}},invalidated:{a:{reason:'outage'}},strategies:{test:{positions:{missing:{quantity:1}}}}};
 const h=marketDataHealth(wallet,config,now);assert.equal(h.coveredMarkets,0);assert.equal(h.invalidated,1);assert.equal(h.missingHeldInstruments,1);
 wallet.invalidated={};wallet.books.a.receivedAt='2026-10-02T06:11:00Z';const future=marketDataHealth(wallet,config,now);assert.equal(future.invalidClocks,1);assert.equal(future.coveredMarkets,0);
});
test('a current worker with missing or stale books is not current market evidence',()=>{
 const now=Date.parse('2026-10-02T06:10:00Z'),b={config:{feeBps:0,maxDataAgeMs:20000},mode:'prospective_fee_free_paper_baseline',status:'running',reportedAt:'2026-10-02T06:09:59Z',observations:0,lastObservationAt:'2026-10-02T06:00:00Z'},legacy={config:{feeBps:10},strategies:{}};
 assert.equal(baselineView(b,legacy,{},now).baselineDataStatus,'awaiting_first_accepted_book');
 const stale=baselineView({...b,observations:1},legacy,{},now);assert.equal(stale.baselineWorkerStatus,'running');assert.equal(stale.baselineDataStatus,'stale_market_data');
 const fresh={...b,observations:1,lastObservationAt:'2026-10-02T06:09:59Z'};
 assert.equal(baselineView({...fresh,dataHealth:{invalidated:1,coveredMarkets:1,expectedMarkets:1}},legacy,{},now).baselineDataStatus,'invalidated_market_data');
 assert.equal(baselineView({...fresh,dataHealth:{invalidated:0,coveredMarkets:1,expectedMarkets:4}},legacy,{},now).baselineDataStatus,'partial_market_coverage');
 assert.equal(baselineView({...fresh,lastObservationAt:undefined},legacy,{},now).baselineDataStatus,'invalid_market_clock');
 assert.equal(baselineView({...fresh,lastObservationAt:'2026-10-02T06:11:00Z'},legacy,{},now).baselineDataStatus,'invalid_market_clock');
 assert.throws(()=>baselineView({...b,config:{feeBps:10}},legacy,{},now));
});
