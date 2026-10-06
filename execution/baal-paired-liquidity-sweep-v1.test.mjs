import test from 'node:test';import assert from 'node:assert/strict';import{createHash}from'node:crypto';
import{planBaalPairedLiquiditySweep,rankDisjointBaalSweeps}from'./baal-paired-liquidity-sweep-v1.mjs';
const T='bda92870-621e-47b0-bc3c-3602c5c26f55',H='a'.repeat(64),at='2026-10-06T18:00:00.000Z';
function fixture({a=[[.875,200]],b=[[.145,5000]],first='11',second='12',eventId=H}={}){
 const binding={ownerId:'Codex',runId:'quant-read-comparison',tournamentId:T,eventId,reviewHash:H};
 const legs=[a,b].map((bids,i)=>{const marketId=i?second:first,exchangeId=String(100+Number(marketId));const bodyText=JSON.stringify({marketId,contexts:[{type:'tournament',tournament:{id:T},orderbook:{exchanges:[{exchangeId,option:'YES',bids:bids.map(([price,quantity])=>({price,quantity}))}]}}]});
  return{marketId,exchangeId,receipt:{method:'GET',status:200,endpoint:`/markets/${marketId}/orderbook?tournamentId=${T}&depth=20`,requestStartedAt:'2026-10-06T17:59:58.000Z',receivedAt:'2026-10-06T17:59:59.000Z',bodyText,sourceBodyHash:createHash('sha256').update(bodyText).digest('hex')}};});
 return{binding,legs,availableNominalCapital:'1000.000000',now:at};
}
test('price/capital bounded sweep consumes29 compared with2 under a one-percent rule; counterfactual only',()=>{
 const r=planBaalPairedLiquiditySweep(fixture());assert.equal(r.selected.quantity,29);assert.equal(r.selected.maximumNominalCost,'28.420000');assert.equal(r.selected.conditionalOrdinaryMarginLowerBound,'0.580000');assert.equal(r.onePercentBestRungControl.quantity,2);assert.equal(r.onePercentBestRungControl.conditionalOrdinaryMarginLowerBound,'0.040000');assert.equal(r.orderEligible,false);assert.equal(r.actualFills,0);assert.equal(r.profitabilityEstablished,false);
});
test('multiple exact book rungs use worst limit for funding and depth VWAP only as descriptive data',()=>{
 const r=planBaalPairedLiquiditySweep(fixture({a:[[.90,4],[.89,5]],b:[[.16,9]]}));assert.equal(r.selected.quantity,9);assert.deepEqual(r.selected.limits,['0.110000','0.840000']);assert.equal(r.selected.maximumNominalCost,'8.550000');assert.equal(r.selected.displayedDepthNotional,'8.510000');assert.equal(r.selected.conditionalOrdinaryMarginLowerBound,'0.450000');
});
test('planner maximizes protected dollar margin rather than quantity at a worse price tier',()=>{
 const r=planBaalPairedLiquiditySweep(fixture({a:[[.90,20],[.51,100]],b:[[.60,100]]}));assert.equal(r.selected.quantity,20);assert.deepEqual(r.selected.limits,['0.100000','0.400000']);assert.equal(r.selected.conditionalOrdinaryMarginLowerBound,'10.000000');assert.ok(r.candidates.some(c=>c.quantity>20));assert.ok(r.candidates.filter(c=>c.quantity>20).every(c=>Number(c.conditionalOrdinaryMarginLowerBound)<10));
});
test('quantity100, capital, event and individual leg caps independently constrain output',()=>{
 let f=fixture({a:[[.90,10000]],b:[[.60,10000]]}),r=planBaalPairedLiquiditySweep(f);assert.equal(r.selected.quantity,62);r=planBaalPairedLiquiditySweep({...f,maximumLegCost:'10.000000'});assert.equal(r.selected.quantity,25);r=planBaalPairedLiquiditySweep({...f,availableNominalCapital:'5.000000'});assert.equal(r.selected.quantity,10);r=planBaalPairedLiquiditySweep({...f,maximumEventCost:'2.500000'});assert.equal(r.selected.quantity,5);r=planBaalPairedLiquiditySweep({...f,maximumQuantity:2});assert.equal(r.selected.quantity,2);
});
test('hash edits, stale/future/wrong scope, crossed ordering and fractional depth refuse evidence',()=>{
 for(const edit of [f=>f.legs[0].receipt.sourceBodyHash='f'.repeat(64),f=>f.legs[0].receipt.requestStartedAt='2026-10-06T17:59:30.000Z',f=>f.legs[0].receipt.receivedAt='2026-10-06T18:00:01.000Z',f=>f.legs[0].receipt.endpoint+='/bad']){const f=fixture();edit(f);assert.throws(()=>planBaalPairedLiquiditySweep(f),/PAIRED_SWEEP_/);}
 assert.throws(()=>planBaalPairedLiquiditySweep(fixture({a:[[.8,200],[.9,200]]})),/SORTED_TICKS/);assert.throws(()=>planBaalPairedLiquiditySweep(fixture({a:[[.875,200.5]]})),/WHOLE_DEPTH/);assert.throws(()=>planBaalPairedLiquiditySweep({...fixture(),maximumQuantity:101}),/QUANTITY_CAP/);
});
test('disjoint ranking prioritizes total dollar margin within explicit capital/market slots, never permits an order',()=>{
 const small=planBaalPairedLiquiditySweep(fixture({a:[[.90,1]],b:[[.40,1]],first:'21',second:'22',eventId:'b'.repeat(64)}));
 const larger=planBaalPairedLiquiditySweep(fixture());const r=rankDisjointBaalSweeps({plans:[small,larger],availableNominalCapital:'1000.000000',maximumNewMarkets:2});assert.equal(r.selected.length,1);assert.equal(r.selected[0].proposalHash,larger.proposalHash);assert.equal(r.orderEligible,false);assert.equal(r.globallyOptimal,false);
 const duplicated=rankDisjointBaalSweeps({plans:[larger,larger],availableNominalCapital:'1000.000000'});assert.equal(duplicated.selected.length,1);
});
test('exact raw decimal tails cannot round onto a profitable tick; enumerated actual IEEE representation remains supported',()=>{
 const f=fixture(),r=f.legs[0].receipt;r.bodyText=r.bodyText.replace('"price":0.875','"price":0.8750000000000000001');r.sourceBodyHash=createHash('sha256').update(r.bodyText).digest('hex');assert.throws(()=>planBaalPairedLiquiditySweep(f),/EXACT_PRICE_SOURCE/);
 const g=fixture({a:[[1-(1-.075),200]],b:[[.95,200]]});assert.ok(planBaalPairedLiquiditySweep(g).selected);
 const alias=g.legs[0].receipt;alias.bodyText=alias.bodyText.replace('0.07499999999999996','0.074999999999999960000');alias.sourceBodyHash=createHash('sha256').update(alias.bodyText).digest('hex');assert.ok(planBaalPairedLiquiditySweep(g).selected);
 const scientific=fixture();scientific.legs[0].receipt.bodyText=scientific.legs[0].receipt.bodyText.replace('0.875','8.7500e-1');scientific.legs[0].receipt.sourceBodyHash=createHash('sha256').update(scientific.legs[0].receipt.bodyText).digest('hex');assert.ok(planBaalPairedLiquiditySweep(scientific).selected);
});
test('co-received slow firstbook does not conceal a nineteen-second paired read window',()=>{const f=fixture();f.legs[0].receipt.requestStartedAt='2026-10-06T17:59:40.000Z';assert.throws(()=>planBaalPairedLiquiditySweep(f),/SOURCE_SKEW/);});
test('different market aliases cannot spend the same physical exchange depth twice; scopeIDs remain exact safe integers',()=>{const first=planBaalPairedLiquiditySweep(fixture());const x=fixture({first:'21',second:'22',eventId:'b'.repeat(64)});x.legs[0].exchangeId=first.exchangeIds[0];const r=x.legs[0].receipt,d=JSON.parse(r.bodyText);d.contexts[0].orderbook.exchanges[0].exchangeId=x.legs[0].exchangeId;r.bodyText=JSON.stringify(d);r.sourceBodyHash=createHash('sha256').update(r.bodyText).digest('hex');const alias=planBaalPairedLiquiditySweep(x);assert.equal(rankDisjointBaalSweeps({plans:[first,alias],availableNominalCapital:'1000.000000'}).selected.length,1);const large=fixture();large.legs[0].marketId='9007199254740992';assert.throws(()=>planBaalPairedLiquiditySweep(large),/LEG_SCOPE/);});
