import test from 'node:test';
import assert from 'node:assert/strict';
import {analyzeSameMarketComplementV1, screenComplementBookV1} from './same-market-complement-arb-v1.mjs';

const epoch = Date.parse('2026-10-13T02:55:00.000Z');
const at = ms => new Date(epoch + ms).toISOString();
function book(ms, {marketId='101', exchangeId='1001', bid=.4, ask=.42, bidSize=100, askSize=100, sequence=Math.floor(ms / 1000) + 1, sourceMs=ms, bids=null, asks=null}={}) {
  return {marketId, exchangeId, availableAt:at(ms), sourceAt:at(sourceMs), nextExpiryAt:at(86400000), sequence:String(sequence),
    bids:bids??[{price:bid.toFixed(3),quantity:bidSize},{price:(bid-.01).toFixed(3),quantity:bidSize}],
    asks:asks??[{price:ask.toFixed(3),quantity:askSize},{price:(ask+.01).toFixed(3),quantity:askSize}]};
}
function run(observations, {breaks=[], ...overrides}={}) {
  return analyzeSameMarketComplementV1({observations,breaks,options:{splitAt:at(0),decisionSpacingMs:60000,
    firstLegLatenciesMs:[0],hedgeLatenciesMs:[0],maxFirstBookDelayMs:5000,maxHedgeBookDelayMs:5000,
    maxBookAgeMs:5000,depthFractionBps:100,...overrides}});
}
const scenario = (report, side='YES') => report.diagnostics.find(x=>x.period==='heldout'&&x.firstSide===side);

test('same-time exact YES+NO complement costs one dollar plus spread at top and through visible depth',()=>{
  const x=screenComplementBookV1(book(0,{bids:[{price:'0.400',quantity:1000},{price:'0.390',quantity:2000}],
    asks:[{price:'0.420',quantity:1000},{price:'0.430',quantity:2000}]}));
  assert.equal(x.topPairAcquisitionCost,'1.020000');
  assert.equal(x.topPairNetIfBothFill,'-0.020000');
  assert.equal(x.participationPairDepth,'30');
  assert.equal(x.participationCostTotal,'30.600000');
  assert.equal(x.participationNetTotal,'-0.600000');
  assert.equal(x.participationNetPerPair,-.02);
});

test('duplicate price levels are rejected instead of being double-counted as depth',()=>{
  assert.throws(()=>screenComplementBookV1(book(0,{bids:[{price:'0.400',quantity:100},{price:'0.400',quantity:100}]})),/COMPLEMENT_BIDS/);
  assert.throws(()=>screenComplementBookV1(book(0,{asks:[{price:'0.420',quantity:100},{price:'0.420',quantity:100}]})),/COMPLEMENT_ASKS/);
});

test('unchanged book completes a sequential pair only after two crossed spreads',()=>{
  const report=run([book(0),book(1000),book(2000)]),yes=scenario(report);
  assert.equal(yes.completedPairs,1);
  assert.equal(yes.firstLegCost.mean,.42);
  assert.ok(Math.abs(yes.pairNetPnlPerShare.mean + .02)<1e-12);
  assert.ok(Math.abs(yes.firstLegUnhedgedLiquidationPnlAtHedge.mean + .02)<1e-12);
  assert.ok(yes.firstLegWorstObservedLiquidationPnlBeforeHedge.mean < 0);
  assert.equal(report.profitabilityEstablished,false);
});

test('favorable movement may make later netting positive but records first-leg exposure separately',()=>{
  const report=run([book(0),book(1000),book(2000,{bid:.46,ask:.48})]),yes=scenario(report);
  assert.equal(yes.completedPairs,1);
  assert.ok(Math.abs(yes.pairNetPnlPerShare.mean-.04)<1e-12);
  assert.ok(Math.abs(yes.firstLegUnhedgedLiquidationPnlAtHedge.mean-.04)<1e-12);
  assert.equal(yes.notActualOrdersOrFills,true);
  assert.equal(report.profitabilityEstablished,false);
});

test('adverse move exposes the unhedged first leg and lowers net pair payout',()=>{
  const report=run([book(0),book(1000),book(2000,{bid:.35,ask:.37})]),yes=scenario(report);
  assert.equal(yes.completedPairs,1);
  assert.ok(Math.abs(yes.pairNetPnlPerShare.mean+.07)<1e-12);
  assert.ok(Math.abs(yes.firstLegUnhedgedLiquidationPnlAtHedge.mean+.07)<1e-12);
});

test('latency selects a strictly later observation, and invalidation between legs censors the pair',()=>{
  const points=[book(0),book(1000),book(2000),book(3000)];
  const delayed=run(points,{firstLegLatenciesMs:[1000],hedgeLatenciesMs:[0]});
  assert.equal(scenario(delayed).firstLegCost.mean,.42,'target-time quote is skipped; first entry is the 2-second observation');
  assert.equal(scenario(delayed).secondLegObservedDelayMs.mean,1000);
  const broken=run(points,{breaks:[{marketId:'101',at:at(2000)}]});
  assert.equal(scenario(broken).completedPairs,0);
  assert.equal(scenario(broken).breakCensored>0,true);
});

test('replay keeps contract identities separate and rejects stale or out-of-order source points',()=>{
  const twoContracts=[book(0,{exchangeId:'1001'}),book(0,{exchangeId:'2001'}),book(1000,{exchangeId:'1001',sequence:2}),
    book(1000,{exchangeId:'2001',sequence:2}),book(2000,{exchangeId:'1001',sequence:3}),book(2000,{exchangeId:'2001',sequence:3})];
  const report=run(twoContracts);
  assert.equal(report.observations.markets,1);
  assert.equal(report.observations.contracts,2);
  assert.equal(scenario(report).completedContracts,2);
  const stale=run([book(0,{sourceMs:-6000}),book(1000),book(2000)]);
  assert.ok(stale.observations.invalidOrDuplicate>0);
  assert.throws(()=>run([book(1000),book(0)]),/COMPLEMENT_INPUT_ORDER/);
});

