import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {roundTripLabel,targetPolicy,ROUND_TRIP} from './roundtrip-target.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),start=Date.parse('2026-10-01T23:00:00Z'),at=n=>new Date(start+n*1000).toISOString();
const row=(n,overrides={})=>({at:at(n),instrument:'9:YES',segment:0,historyLength:6,book:{marketId:'377',exchangeId:'9',instrument:'9:YES',sequence:String(n),sourceAt:at(n),receivedAt:at(n),requestStartedAt:at(n),versionKey:n+':'+at(n),bids:[{price:'0.400000',quantity:10000}],asks:[{price:'0.410000',quantity:10000}]},...overrides});
test('round-trip target follows delayed entry, post-fill horizon and delayed exit with exact costs',()=>{
 const result=roundTripLabel([0,60,120,180].map(n=>row(n)),0,c,60000).label;
 assert.equal(result.entryAt,at(60));assert.equal(result.labelAt,at(180));assert.equal(result.quantity,10);assert.equal(result.fillExecutions,2);assert.ok(Math.abs(result.y+.02081)<1e-12);assert.ok(Math.abs(result.persistence+.02081)<1e-12);
 assert.equal(roundTripLabel([0,60,120].map(n=>row(n)),0,c,60000).reason,'exit_censored');
});
test('warmup, source segment breaks and never-filled decisions are explicit exclusions',()=>{
 assert.equal(roundTripLabel([row(0,{historyLength:5})],0,c,60000).reason,'decision_warmup');
 assert.equal(roundTripLabel([row(0),row(60,{segment:1})],0,c,60000).reason,'segment_boundary');
 const empty=[0,60,120,180].map(n=>{const r=row(n);r.book.asks[0].quantity=50;return r;});
 assert.equal(roundTripLabel(empty,0,c,60000).reason,'entry_unfilled');assert.throws(()=>targetPolicy('wrong'),/Unsupported/);assert.equal(targetPolicy(ROUND_TRIP),ROUND_TRIP);
});
test('round-trip labels retain partial-depth quantity and stop-loss timing from the execution engine',()=>{
 const partial=[0,60,120,180].map(n=>{const r=row(n);r.book.asks[0].quantity=300;return r;});
 assert.equal(roundTripLabel(partial,0,c,60000).label.quantity,3);
 const stopped=[0,60,120].map(n=>{const r=row(n);if(n)r.book.bids[0].price='0.350000';return r;});
 const label=roundTripLabel(stopped,0,c,600000).label;assert.equal(label.entryAt,at(60));assert.equal(label.labelAt,at(120));assert.ok(label.y<-.06);
});
