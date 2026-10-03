import test from 'node:test';import assert from 'node:assert/strict';
import {createReboundWatch,advanceReboundWatch} from './rebound-entry.mjs';
const base=Date.parse('2026-10-03T00:00:00Z'),at=ms=>new Date(base+ms).toISOString();
const dip={marketId:'354',exchangeId:'1043',availableAt:at(1000),receivedAt:at(990),reference:{anchor:'0.450000'},bid:'0.435000',ask:'0.440000'};
const update=(w,time,bid,ask,request=time-100)=>advanceReboundWatch(w,{marketId:'354',exchangeId:'1043',option:'YES',bestBid:bid,bestAsk:ask,crossed:false},{requestStartedAt:at(request),receivedAt:at(time-10)},at(time));
test('confirmation requires a later independent request and an observed rise from the trough',()=>{
 const w=createReboundWatch(dip);update(w,2000,.44,.445,900);assert.equal(w.signal,null,'in-flight pre-dip request cannot confirm');
 update(w,3000,.43,.435);assert.equal(w.troughBid,'0.430000');assert.equal(w.signal,null);
 update(w,4000,.435,.44);assert.equal(w.status,'confirmed');assert.equal(w.signal.limit,'0.440000');assert.equal(w.signal.troughAvailableAt,at(3000));assert.equal(w.signal.depthVerified,false);
});
test('an expensive rebound cannot consume all the remaining discount; watches expire without forced entries',()=>{
 const w=createReboundWatch(dip);update(w,2000,.445,.45);assert.equal(w.signal,null);update(w,122000,.44,.445);assert.equal(w.status,'confirmation_not_observed');assert.equal(w.signal,null);
});
test('missing coverage, stale receipts and invalid quotes revoke a watch rather than invent confirmation',()=>{
 const gap=createReboundWatch(dip);update(gap,92000,.44,.445);assert.equal(gap.status,'coverage_gap');
 const stale=createReboundWatch(dip);advanceReboundWatch(stale,{marketId:'354',exchangeId:'1043',option:'YES',bestBid:.44,bestAsk:.445,crossed:false},{requestStartedAt:at(1500),receivedAt:at(1600)},at(23000));assert.equal(stale.status,'coverage_gap');
 const invalid=createReboundWatch(dip);update(invalid,2000,.45,.44);assert.equal(invalid.status,'invalid_quote');
 const scope=createReboundWatch(dip);assert.throws(()=>advanceReboundWatch(scope,{marketId:'355',exchangeId:'1043',option:'YES'},{requestStartedAt:at(1900),receivedAt:at(1990)},at(2000)));
});
