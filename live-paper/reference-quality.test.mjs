import test from 'node:test';import assert from 'node:assert/strict';
import {historicalReferenceQuality} from './reference-quality.mjs';
const now=Date.parse('2026-10-02T12:00:00Z'), at=new Date(now).toISOString();
const p={minimumSamples:8,minimumSpanMs:1800000,historyCacheAgeMs:3600000,lastHistoricalObservationAgeMs:2700000,lookbackMs:21600000};
const book=(minutes,mid)=>({at:new Date(now-minutes*60000).toISOString(),receivedAt:new Date(now-minutes*60000+1).toISOString(),mid});
test('repeated quotes expose concentration without changing legacy reference',()=>{
 const books=[...Array.from({length:8},(_,i)=>book(60-i*5,.5)),...Array.from({length:100},()=>book(1,.4))];
 const r=historicalReferenceQuality(null,books,at,p);assert.equal(r.reference.center,.4);assert.equal(r.quality.distinctMinutes,9);assert.equal(r.quality.minuteBalancedCenter,.5);assert.equal(r.quality.largestMinuteWeight,100/108);
});
test('pre-entry decline and future information remain distinct',()=>{
 const books=Array.from({length:12},(_,i)=>book(60-i*5,.7-i*.01));const r=historicalReferenceQuality(null,books,at,p);
 assert.ok(r.quality.recentMinusOlder<0);assert.ok(r.quality.referenceMinusRecent>0);
 assert.deepEqual(historicalReferenceQuality(null,[...books,book(-1,.1),{...book(2,.1),receivedAt:new Date(now+1).toISOString()}],at,p),r);
});
test('complete candles select the exact original reference; incomplete history falls back',()=>{
 const books=Array.from({length:12},(_,i)=>book(60-i*5,.6));
 const history={receivedAt:new Date(now-1).toISOString(),coverage:{complete:true},candles:books.map(b=>({time:b.at,vwap:.5}))};
 const r=historicalReferenceQuality(history,books,at,p);assert.equal(r.reference.center,.5);assert.equal(r.quality.minuteBalancedCenter,.5);
 history.coverage.complete=false;assert.equal(historicalReferenceQuality(history,books,at,p).reference.center,.6);
});
test('unready history has no invented quality scores',()=>{
 assert.equal(historicalReferenceQuality(null,[],at,p).quality,null);
 assert.throws(()=>historicalReferenceQuality(null,Array.from({length:12},(_,i)=>book(60-i*5,NaN)),at,p),/INVALID_REFERENCE_POINTS/);
});
