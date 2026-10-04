import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';
import {planMakerPairReads} from './maker-read-plan.mjs';
const review=JSON.parse(fs.readFileSync(new URL('../data/canonical-capacity-bank-20261003-v1.json',import.meta.url))).review;
const now=Date.parse(review.recordedAt)+1000,r=(marketId,reason='discovery')=>({marketId,path:'supplementalDepth',reason});
const plan=reads=>({reads,entryMarketIds:['345']});
test('actual held381 plan keeps its book and other obligations while inserting382 beside it at no extra budget',()=>{
 const base=plan([r('381','held_or_pending'),r('363','bid_anchor_paper_risk'),r('277','comparison'),r('345')]),before=structuredClone(base);
 const next=planMakerPairReads(base,review,{now});assert.deepEqual(next.reads.map(x=>x.marketId),['381','382','363','277']);assert.equal(next.reads[0],base.reads[0]);assert.equal(next.reads[2],base.reads[1]);assert.deepEqual(base,before);assert.equal(next.makerPairFocus.requestsAdded,0);assert.deepEqual(next.entryMarketIds,[]);
});
test('either held pair leg reuses its original read, with protected relative order and no duplicate market',()=>{
 for(const held of ['381','382']){
  const base=plan([r('363','priority_held_or_pending'),r(held,'held_or_pending'),r('297','bid_anchor_paper_risk'),r('345')]);
  const next=planMakerPairReads(base,review,{now});assert.equal(next.reads[0],base.reads[0]);assert.equal(next.reads[1],base.reads[1]);assert.equal(next.reads[3],base.reads[2]);assert.equal(next.reads.length,4);assert.equal(new Set(next.reads.map(r=>r.marketId)).size,4);assert.equal(next.makerPairFocus.reason,'coherent_maker_pair');
 }
});
test('full protected budgets and separated held legs cannot lose or reorder an obligation',()=>{
 for(const reads of [[r('381','held_or_pending'),r('297','held_or_pending'),r('363','held_or_pending'),r('281','eligible_candidate')],[r('381','held_or_pending'),r('297','held_or_pending'),r('382','held_or_pending'),r('345')]]){
  const base=plan(reads),next=planMakerPairReads(base,review,{now});assert.equal(next.reads,base.reads);assert.deepEqual(next.makerPairFocus.marketIds,[]);
 }
});
test('flat maker pairs occupy existing optional slots; expired review, account exposure and other pair obligations block substitution',()=>{
 const base=plan([r('363','held_or_pending'),r('297','held_or_pending'),r('345'),r('346','comparison')]),next=planMakerPairReads(base,review,{now});assert.deepEqual(next.reads.map(r=>r.marketId),['363','297','381','382']);
 for(const args of [{now:Date.parse(review.expiresAt)},{now:Date.parse(review.recordedAt)-1},{now,excludedExchangeIds:['1070']}])assert.equal(planMakerPairReads(base,review,args).reads,base.reads);
 for(const extra of [{comparisonSampling:{}},{basketMeasurement:{marketIds:['381','382']}},{paperPackageFocus:{marketIds:['381','382']}}])assert.equal(planMakerPairReads({...base,...extra},review,{now}).reads,base.reads);
});
