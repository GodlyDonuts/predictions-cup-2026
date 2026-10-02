import test from 'node:test';import assert from 'node:assert/strict';
import {workerHealth,baselineView} from './worker-health.mjs';
test('dead worker is stale even when its persisted status says running',()=>{
 const now=Date.parse('2026-10-02T06:10:00Z');
 assert.equal(workerHealth({status:'running',updatedAt:'2026-10-02T06:08:00Z'},now).status,'stale_worker');
 assert.equal(workerHealth({status:'running',reportedAt:'2026-10-02T06:09:50Z'},now).status,'running');
 assert.equal(workerHealth({status:'running',updatedAt:'2026-10-02T06:11:00Z'},now).status,'invalid_heartbeat');
});
test('a current worker with missing or stale books is not current market evidence',()=>{
 const now=Date.parse('2026-10-02T06:10:00Z'),b={config:{feeBps:0,maxDataAgeMs:20000},mode:'prospective_fee_free_paper_baseline',status:'running',reportedAt:'2026-10-02T06:09:59Z',observations:0,lastObservationAt:'2026-10-02T06:00:00Z'},legacy={config:{feeBps:10},strategies:{}};
 assert.equal(baselineView(b,legacy,{},now).baselineDataStatus,'awaiting_first_accepted_book');
 const stale=baselineView({...b,observations:1},legacy,{},now);assert.equal(stale.baselineWorkerStatus,'running');assert.equal(stale.baselineDataStatus,'stale_market_data');
 assert.throws(()=>baselineView({...b,config:{feeBps:10}},legacy,{},now));
});
