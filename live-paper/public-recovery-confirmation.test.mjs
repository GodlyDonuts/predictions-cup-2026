import test from 'node:test';import assert from 'node:assert/strict';
import {observeRecovery,recoveryConfirmation} from './public-recovery-confirmation.mjs';
const base=Date.parse('2026-10-02T17:00:00Z'),at=n=>new Date(base+n).toISOString();
const event=(i,n,bid)=>({decisionEligible:true,continuity:{ready:true,continuous:true},quoteReceivedAt:at(n),confirmedAt:at(n),availableAt:at(n),book:{sequence:String(i),sourceAt:at(n),bids:[{price:Number(bid).toFixed(6)}],asks:[{price:(Number(bid)+.01).toFixed(6)}]}});
function trace(prices,times){let points=[],last;prices.forEach((p,i)=>{last=event(i+1,times[i],p);points=observeRecovery(points,last);});return {points,last};}
test('measured minute-spaced quotes can confirm recovery without a continuous-tape claim',()=>{
 const t=trace(['.400000','.380000','.385000','.390000'],[0,65000,130000,195000]);assert.equal(recoveryConfirmation(t.points,t.last,at(195000)).admitted,true);
 const falling=trace(['.400000','.390000','.380000','.370000'],[0,65000,130000,195000]);assert.equal(recoveryConfirmation(falling.points,falling.last,at(195000)).admitted,false);
});
test('a long blind gap starts new recovery context rather than linking a stale trough',()=>{
 const short=trace(['.4','.38','.39','.395'],[0,200000,210000,220000]);assert.equal(recoveryConfirmation(short.points,short.last,at(220000)).admitted,false);
 const t=trace(['.5','.4','.38','.385','.39'],[0,200000,210000,220000,230000]);assert.equal(recoveryConfirmation(t.points,t.last,at(230000)).admitted,true);
});
test('duplicate source, bursts, unavailable evidence and a new falling bid cannot qualify',()=>{
 const t=trace(['.4','.38','.385','.39'],[0,100,200,300]);assert.equal(recoveryConfirmation(t.points,t.last,at(300)).admitted,false);
 const a=trace(['.41','.38','.395','.39'],[0,1000,2000,3000]);assert.equal(recoveryConfirmation(a.points,a.last,at(3000)).reason,'recovery_bid_falling');
 const b=trace(['.4','.38','.385','.39'],[0,1000,2000,3000]);b.points.at(-1).availableAt=at(5000);assert.equal(recoveryConfirmation(b.points,b.last,at(3000)).admitted,false);
 assert.deepEqual(observeRecovery(b.points,{...b.last,quoteReceivedAt:at(6000)}),b.points);
});
