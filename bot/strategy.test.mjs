import test from 'node:test';
import assert from 'node:assert/strict';
import {verifiedPairs,planPair,toMills,formatMills} from './strategy.mjs';
const cfg={minimumEdge:.02,maxSharesPerLeg:250,maxPackageCost:250,cashReserve:80000,tournamentId:'test'};
const pair=[{exchangeId:'1'},{exchangeId:'2'}];
const books={'1':{bids:[{price:.8,quantity:1000}]},'2':{bids:[{price:.3,quantity:1000}]}};
test('NO pair uses executable bids and has a conservative settlement floor',()=>{
 const p=planPair(pair,books,cfg,100000);assert.equal(p.quantity,250);assert.ok(Math.abs(p.minimumProfit-25)<1e-8);
 for(const winner of ['D','R','third'])assert.ok((winner==='third'?2:1)*p.quantity>p.worstCaseCost);
});
test('no edge, missing liquidity, reserve, or existing holdings produces no order',()=>{
 assert.equal(planPair(pair,{'1':books['1'],'2':{bids:[{price:.1,quantity:1000}]}},cfg,100000),null);
 assert.equal(planPair(pair,{},cfg,100000),null);
 assert.equal(planPair(pair,books,cfg,80000),null);
 assert.equal(planPair(pair,books,cfg,100000,[{exchangeId:'1',quantity:1}]),null);
});
test('different races, stages, or duplicate party do not qualify as a hedge',()=>{
 const markets=[{id:'a',status:'open',exchanges:[{id:'1'}]},{id:'b',status:'open',exchanges:[{id:'2'}]}];
 const node=party=>({root:{node_type:'contract',contract_type:'Election Outcome',settled_with:null,settlement_date:'2026-11-04',contract_details:{resolutionType:'Party Winner',raceStage:'General',raceId:'r',stageId:'s',electionDate:'2026-11-03',winnerName:party}}});
 const nodes={a:node('Democratic Party'),b:node('Republican Party')};assert.equal(verifiedPairs(markets,nodes).length,1);
 nodes.b.root.contract_details.stageId='different';assert.equal(verifiedPairs(markets,nodes).length,0);
 nodes.b=node('Democratic Party');assert.equal(verifiedPairs(markets,nodes).length,0);
});
test('exact fixed-point accounting preserves half-cent tick and rejects excess precision',()=>{
 assert.equal(toMills('94401.38'),94401380);assert.equal(toMills('0.005'),5);
 assert.equal(formatMills(12345),'12.345');assert.throws(()=>toMills('0.0001'));
 const p=planPair(pair,{'1':{bids:[{price:.805,quantity:1000}]},'2':{bids:[{price:.3,quantity:1000}]}},cfg,100000);
 assert.equal(p.worstCaseCost,'223.750');assert.equal(p.minimumProfit,'26.250');
});
