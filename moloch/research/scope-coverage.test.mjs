import test from 'node:test';import assert from 'node:assert/strict';import {scopeCoverage} from './scope-coverage.mjs';
test('counterfactual holdings in a retired source scope cannot qualify for ranking',()=>{
 const r=scopeCoverage({positions:{'1051':{YES:[{quantity:27}],NO:[]}},netLiquidationPnl:null,unresolvedOrders:0},[{exchangeId:'970'}]);assert.deepEqual(r.uncoveredInventory,[{exchangeId:'1051',quantity:27}]);assert.equal(r.policyRankingEligible,false);
});
test('empty historical scopes do not block a flat marked bank, but unresolved quotes do',()=>{
 const p={positions:{'1051':{YES:[],NO:[]}},netLiquidationPnl:'0.000000',unresolvedOrders:0};assert.equal(scopeCoverage(p,[]).policyRankingEligible,true);assert.equal(scopeCoverage({...p,unresolvedOrders:1},[]).policyRankingEligible,false);
});
