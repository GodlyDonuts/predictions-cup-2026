import test from 'node:test';import assert from 'node:assert/strict';import {dipPerformance} from './dip-performance.mjs';
test('partial exits and losing open inventory cannot become completed small wins',()=>{
 const at='2026-10-02T07:00:00Z',f=(action,quantity,price)=>({instrument:'1:YES',action,quantity,price,fee:'0.000000',at});
 const partial=dipPerformance([f('buy',10,'0.500000'),f('sell',4,'0.510000')],[{enteredAt:'2026-10-02T06:00:00Z'}],at);assert.equal(partial.closedRoundTrips,0);assert.equal(partial.realizedPnl,'0.040000');assert.equal(partial.openCostBasis,'3.000000');assert.equal(partial.oldestInventoryAgeMs,3600000);
 const closed=dipPerformance([f('buy',10,'0.500000'),f('sell',4,'0.510000'),f('sell',6,'0.470000')],[],at);assert.equal(closed.closedRoundTrips,1);assert.equal(closed.losses,1);assert.equal(closed.wins,0);assert.equal(closed.realizedPnl,'-0.140000');assert.ok(Math.abs(closed.fillWeightedRealizedReturn+.028)<1e-12);
 assert.throws(()=>dipPerformance([f('sell',1,'0.500000')],[],at),/oversell/);
});
