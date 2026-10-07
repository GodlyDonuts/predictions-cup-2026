import test from 'node:test';import assert from 'node:assert/strict';import {comparePairedEntryLegRiskV1 as compare} from './paired-entry-leg-risk-v1.mjs';
test('cheapest leg need not minimize failed-hedge liquidation loss',()=>{
 const r=compare({quantity:20,legs:[{exchangeId:'1',noBuyLimit:'0.100000',noSellLimit:'0.070000'},{exchangeId:'2',noBuyLimit:'0.880000',noSellLimit:'0.875000'}]});assert.equal(r.preferLowerUnhedgedLossExchangeId,'2');assert.ok(r.scenarios.every(s=>s.fullFunding==='19.600000'));assert.equal(r.scenarios[0].conditionalOrdinarySurplusIfBothFill,'0.400000');assert.equal(r.scenarios[0].stressedFirstLegLiquidationPnlIfHedgeFails,'-0.700000');assert.equal(r.scenarios[1].stressedFirstLegLiquidationPnlIfHedgeFails,'-0.200000');assert.equal(r.scenarios[0].probabilityEstimated,false);
});
test('full funding, order cap and missing positive settlement surplus remain explicit',()=>{
 assert.throws(()=>compare({quantity:100,legs:[{exchangeId:'1',noBuyLimit:'0.100000',noSellLimit:'0.090000'},{exchangeId:'2',noBuyLimit:'0.880000',noSellLimit:'0.870000'}]}),/FUNDED/);
 const r=compare({quantity:1,legs:[{exchangeId:'1',noBuyLimit:'0.500000',noSellLimit:'0.490000'},{exchangeId:'2',noBuyLimit:'0.510000',noSellLimit:'0.500000'}]});assert.equal(r.scenarios[0].minimumHedgeCompletionProbabilityForScenarioBreakEven,null);
});

