import test from 'node:test';
import assert from 'node:assert/strict';
import {validateUniverse} from './breadth.mjs';
import {breadthFeatures,activityReport} from './breadth-features.mjs';
const u=validateUniverse({tournamentId:'T',exchanges:[{exchangeId:'1',marketId:'2',option:'YES'}]},'T');
const rec=(at,bid,ask,missing=false)=>({at,event:{clockAt:at,bulkPrices:{tournamentId:'T',universeHash:u.hash,requestStartedAt:at,receivedAt:at,quotes:missing?[]:[{exchangeId:'1',marketId:'2',option:'YES',latestPrice:.5,bestBid:bid,bestAsk:ask,spread:ask-bid}],missingIds:missing?['1']:[]}}});
test('breadth features are causal and never executable; future values leave earlier features unchanged',()=>{
 const r=[rec('2026-10-02T01:00:00Z',.4,.6),rec('2026-10-02T01:03:00Z',.45,.65),rec('2026-10-02T01:06:00Z',.5,.7)];
 const x=breadthFeatures(r,u);assert.equal(x[0].midChange,null);assert.ok(Math.abs(x[1].midChange-.05)<1e-10);assert.equal(x[1].quoteChanged,true);assert.equal(x[1].executable,false);
 const changed=structuredClone(r);changed[2].event.bulkPrices.quotes[0].bestBid=.6;assert.deepEqual(breadthFeatures(changed,u).slice(0,2),x.slice(0,2));assert.equal(activityReport(x).exchanges[0].quoteChanges,2);
});
test('missing coverage resets history; crossed quotes have no midpoint; future collection rejected',()=>{
 const r=[rec('2026-10-02T01:00:00Z',.4,.6),rec('2026-10-02T01:03:00Z',0,0,true),rec('2026-10-02T01:06:00Z',.7,.6)];
 const x=breadthFeatures(r,u);assert.equal(x[1].mid,null);assert.equal(x[1].quoteChanged,null);
 const bad=rec('2026-10-02T01:00:00Z',.4,.6);bad.event.bulkPrices.receivedAt='2026-10-02T01:00:01Z';assert.throws(()=>breadthFeatures([bad],u),/Future/);
});
