import test from 'node:test';
import assert from 'node:assert/strict';
import {metadataPairs,planPair,toMills,formatMills} from './strategy.mjs';
const cfg={minimumEdge:.02,maxSharesPerLeg:250,maxPackageCost:250,cashReserve:80000,tournamentId:'test'};
const pair=[{exchangeId:'1'},{exchangeId:'2'}];
const books={'1':{bids:[{price:.8,quantity:1000}]},'2':{bids:[{price:.3,quantity:1000}]}};
const markets=[{id:'a',status:'open',exchanges:[{id:'1'}]},{id:'b',status:'open',exchanges:[{id:'2'}]}];
const node=party=>({root:{node_type:'contract',contract_type:'Election Outcome',settled_with:null,settlement_date:'2026-11-04',contract_details:{resolutionType:'Party Winner',raceStage:'General',raceId:'r',stageId:'s',electionDate:'2026-11-03',winnerName:party}}});

test('metadata pair cannot claim a payout floor: both YES loses the entire package cost',()=>{
 const nodes={a:node('Democratic Party'),b:node('Republican Party')};
 const p=planPair(metadataPairs(markets,nodes)[0],books,cfg,100000);
 assert.equal(p.quantity,250);assert.equal(p.worstCaseCost,'225.000');
 assert.equal(p.classification,'unverified_metadata_hypothesis');
 assert.equal(p.settlementVerified,false);assert.equal(p.eligibleForExecution,false);
 assert.equal('minimumPayout' in p,false);assert.equal('minimumProfit' in p,false);
 // Fusion/joint-endorsement rules could make both YES: metadata cannot rule it out.
 const bothYesPayout=0;assert.equal(bothYesPayout-toMills(p.worstCaseCost)/1000,-225);
});
test('no quote difference, missing liquidity, reserve, or existing holdings excludes candidate',()=>{
 assert.equal(planPair(pair,{'1':books['1'],'2':{bids:[{price:.1,quantity:1000}]}},cfg,100000),null);
 assert.equal(planPair(pair,{},cfg,100000),null);
 assert.equal(planPair(pair,books,cfg,80000),null);
 assert.equal(planPair(pair,books,cfg,100000,[{exchangeId:'1',quantity:1}]),null);
});
test('own resting YES bid cannot create a NO liquidity candidate',()=>{
 const ownBid={exchangeId:1,side:'yes',action:'buy',price:.8,quantity:1000};
 assert.equal(planPair(pair,books,cfg,100000,[],[ownBid]),null);
 assert.equal(planPair(pair,books,cfg,100000,[],[{exchangeId:'2',side:'no',action:'sell',price:.1}]),null);
 assert.equal(planPair(pair,books,cfg,100000,[],[{}]),null);
 assert.ok(planPair(pair,books,cfg,100000,[],[{exchangeId:3}]));
});
test('missing settlement, different race/stage/date, or duplicate party cannot match',()=>{
 const nodes={a:node('Democratic Party'),b:node('Republican Party')};
 assert.equal(metadataPairs(markets,nodes).length,1);
 delete nodes.a.root.settlement_date;delete nodes.b.root.settlement_date;
 assert.equal(metadataPairs(markets,nodes).length,0);
 for(const field of ['raceId','stageId','electionDate']){
  const other={a:node('Democratic Party'),b:node('Republican Party')};
  other.b.root.contract_details[field]='different';assert.equal(metadataPairs(markets,other).length,0);
 }
 nodes.a=node('Democratic Party');nodes.b=node('Democratic Party');
 assert.equal(metadataPairs(markets,nodes).length,0);
});
test('exact fixed-point cost accounting preserves half-cent tick and rejects excess precision',()=>{
 assert.equal(toMills('94401.38'),94401380);assert.equal(toMills('0.005'),5);
 assert.equal(formatMills(12345),'12.345');assert.throws(()=>toMills('0.0001'));
 const p=planPair(pair,{'1':{bids:[{price:.805,quantity:1000}]},'2':{bids:[{price:.3,quantity:1000}]}},cfg,100000);
 assert.equal(p.worstCaseCost,'223.750');assert.equal(p.quoteDifference,'0.105');
});
