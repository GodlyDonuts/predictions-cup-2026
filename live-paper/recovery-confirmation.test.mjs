import test from 'node:test';import assert from 'node:assert/strict';
import {observeRecovery,recoveryConfirmation} from './recovery-confirmation.mjs';
const base=Date.parse('2026-10-02T17:00:00Z'),at=n=>new Date(base+n).toISOString();
function event(i,n,bid){return {decisionEligible:true,continuity:{ready:true,continuous:true},quoteReceivedAt:at(n),confirmedAt:at(n),availableAt:at(n),
 book:{sequence:String(i),sourceAt:at(n),bids:[{price:bid}],asks:[{price:(Number(bid)+.01).toFixed(6)}]}};}
function trace(prices,times=prices.map((_,i)=>(i+1)*1000)){let points=[],last;prices.forEach((price,i)=>{last=event(i+1,times[i],price);points=observeRecovery(points,last);});return {points,last,at:last.availableAt};}
test('requires a meaningful dip and two subsequent independent recovery points',()=>{
 const t=trace(['0.400000','0.380000','0.385000','0.390000']);const c=recoveryConfirmation(t.points,t.last,t.at);assert.equal(c.admitted,true);assert.equal(c.dropTicks,4);assert.equal(c.reboundTicks,2);
 const falling=trace(['0.400000','0.390000','0.380000','0.370000']);assert.equal(recoveryConfirmation(falling.points,falling.last,falling.at).admitted,false);
 const v=trace(['0.400000','0.390000','0.395000','0.390000']);assert.equal(recoveryConfirmation(v.points,v.last,v.at).reason,'recovery_rebound_too_small');
 const after=trace(['0.410000','0.380000','0.395000','0.390000']);assert.equal(recoveryConfirmation(after.points,after.last,after.at).reason,'recovery_bid_falling');
});
test('duplicates, same source time and receipt bursts cannot manufacture confirmation',()=>{
 const first=event(1,1000,'0.400000'),points=observeRecovery([],first);
 assert.deepEqual(observeRecovery(points,{...first,quoteReceivedAt:at(2000)}),points);
 const sameTime=event(2,2000,'0.390000');sameTime.book.sourceAt=first.book.sourceAt;assert.deepEqual(observeRecovery(points,sameTime),points);
 const burst=trace(['0.400000','0.380000','0.385000','0.390000'],[1000,1100,1200,1300]);assert.equal(recoveryConfirmation(burst.points,burst.last,burst.at).reason,'recovery_insufficient_observations');
});
test('future consumer availability and long observation gaps fail the gate',()=>{
 const t=trace(['0.400000','0.380000','0.385000','0.390000']);t.points[3].availableAt=at(5000);assert.equal(recoveryConfirmation(t.points,t.last,at(4000)).admitted,false);
 const gap=trace(['0.400000','0.380000','0.385000','0.390000'],[1000,92000,94000,96000]);assert.equal(recoveryConfirmation(gap.points,gap.last,gap.at).reason,'recovery_observation_gap');
});
test('invalid observations do not erase valid point history or become evidence',()=>{
 const t=trace(['0.400000','0.380000','0.385000','0.390000']);const invalid={...event(5,5000,'0.500000'),decisionEligible:false};assert.deepEqual(observeRecovery(t.points,invalid),t.points);
 assert.equal(recoveryConfirmation(t.points,event(5,5000,'0.500000'),at(5000)).reason,'recovery_current_not_independent');
});
