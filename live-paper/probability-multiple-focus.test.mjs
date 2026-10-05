import test from 'node:test';import assert from 'node:assert/strict';
import {mergeProbabilityReadFocus} from './probability-read-focus.mjs';import {prioritizeAnchorReads} from './bid-anchor-read-focus.mjs';
test('New research cannot displace existing held inventory or add read slots',()=>{
 const old={bankHash:'a',ownedRiskReads:true,executionAuthority:false,markets:[{marketId:'297',phase:'held'}]};
 const next={bankHash:'b',executionAuthority:false,markets:[{marketId:'297',phase:'depth_required'},{marketId:'356',phase:'depth_required'}]};
 const merged=mergeProbabilityReadFocus([next,null,old]);assert.equal(merged.markets.find(m=>m.marketId==='297').phase,'held');assert.equal(merged.ownedRiskReads,true);assert.equal(merged.executionAuthority,false);
 const plan={reads:['277','305','354','377'].map(marketId=>({marketId,path:'supplementalDepth',reason:'discovery'})),entryMarketIds:['277','305','354','377']},result=prioritizeAnchorReads(plan,merged);
 assert.equal(result.reads.length,plan.reads.length);assert.equal(result.reads[0].marketId,'297');assert(result.reads.some(r=>r.marketId==='356'));assert.equal(result.bidAnchorReadFocus.requestsAdded,0);
});
test('Absent or non-live cohorts cannot create a trading permission',()=>{assert.equal(mergeProbabilityReadFocus([null,null]),null);assert.throws(()=>mergeProbabilityReadFocus([{bankHash:'x',markets:[],executionAuthority:true}]));});
