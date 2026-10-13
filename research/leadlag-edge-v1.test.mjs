import test from 'node:test';
import assert from 'node:assert/strict';
import {analyzeLeadLagV1, leadLagMarketGraphV1} from './leadlag-edge-v1.mjs';

const epoch = Date.parse('2026-10-13T00:00:00.000Z');
const at = ms => new Date(epoch + ms).toISOString();

function fixtureGraph() {
  const pair = (raceId, raceName, a, b) => ({raceId, raceName, legs:[a,b]});
  return leadLagMarketGraphV1({candidatePairs:[
    pair('race-al','Governor of Alabama',{marketId:'101',exchangeId:'1001',party:'Democratic Party'},{marketId:'102',exchangeId:'1002',party:'Republican Party'}),
    pair('race-ak','Governor of Alaska',{marketId:'201',exchangeId:'2001',party:'Democratic Party'},{marketId:'202',exchangeId:'2002',party:'Republican Party'}),
  ]});
}

function point(exchangeId, marketId, ms, mid, {spread=.01, bidSize=10, askSize=10, sequence=1, sourceMs=ms, expiryMs=null}={}) {
  return {exchangeId,marketId,availableAt:at(ms),sourceAt:at(sourceMs),nextExpiryAt:expiryMs===null?null:at(expiryMs),
    bid:mid-spread/2,ask:mid+spread/2,bidSize,askSize,sequence:String(sequence)};
}

function pairTape({base, prefix, heldout=false, sameTimeMove=false, shallowEntry=false}) {
  const source='2002',target='2001',sourceMarket='202',targetMarket='201';
  const t=ms=>base+ms;
  const sourcePoints=[point(source,sourceMarket,t(0),.5,{sequence:1}),point(source,sourceMarket,t(2000),.5,{sequence:2}),
    point(source,sourceMarket,t(3000),sameTimeMove?.5:.47,{sequence:3})];
  const decision=point(target,targetMarket,t(4000),.30,{spread:.01,sequence:1});
  const entry=point(target,targetMarket,t(5000),.305,{spread:.01,askSize:shallowEntry?1:10,sequence:2});
  const exit=point(target,targetMarket,t(7000),.355,{spread:.01,sequence:3});
  return [...sourcePoints,decision,entry,exit];
}

function run(observations, {breaks=[], heldoutStateGroups=['state:Alaska'], minDepth=1}={}) {
  return analyzeLeadLagV1({observations,breaks,graph:fixtureGraph(),options:{
    splitAt:at(50000),heldoutStateGroups,lookbacksMs:[1500],latenciesMs:[0,1000],horizonsMs:[1000],
    maxPeerAgeMs:20000,maxDataAgeMs:5000,maxEntryDelayMs:10000,maxExitDelayMs:15000,
    minDecisionSpacingMs:60000,minDepth,minTrainingLabels:1,
  }});
}

function completeSignalTape({heldoutBase=60000,sameTime=false,shallow=false}={}) {
  const training=[point('1002','102',0,.5,{sequence:1}),point('1002','102',2000,.5,{sequence:2}),point('1002','102',3000,.47,{sequence:3}),
    point('1001','101',4000,.30,{spread:.01,sequence:1}),point('1001','101',5000,.305,{spread:.01,sequence:2}),point('1001','101',7000,.355,{spread:.01,sequence:3})];
  const h=heldoutBase, source= sameTime
    ? [point('2002','202',h,.5,{sequence:1}),point('2002','202',h+2000,.5,{sequence:2}),point('2002','202',h+4000,.47,{sequence:3})]
    : [point('2002','202',h,.5,{sequence:1}),point('2002','202',h+2000,.5,{sequence:2}),point('2002','202',h+3000,.47,{sequence:3})];
  const decisionAt=sameTime?h+4000:h+4000;
  const heldout=[...source,point('2001','201',decisionAt,.30,{spread:.01,sequence:1}),
    point('2001','201',decisionAt+1000,.305,{spread:.01,askSize:shallow?1:10,sequence:2}),
    point('2001','201',decisionAt+3000,.355,{spread:.01,sequence:3})];
  return {observations:[...training,...heldout].sort((a,b)=>Date.parse(a.availableAt)-Date.parse(b.availableAt)||a.exchangeId.localeCompare(b.exchangeId)),decisionAt};
}

test('market graph unions repeated multi-party pair rows but keeps races and state holdouts explicit',()=>{
  const structural={candidatePairs:[
    {raceId:'r',raceName:'U.S. Senate Montana',legs:[{marketId:'1',exchangeId:'11',party:'Democratic Party'},{marketId:'2',exchangeId:'12',party:'Republican Party'}]},
    {raceId:'r',raceName:'U.S. Senate Montana',legs:[{marketId:'1',exchangeId:'11',party:'Democratic Party'},{marketId:'3',exchangeId:'13',party:'Independent'}]},
  ]};
  const graph=leadLagMarketGraphV1(structural);
  assert.equal(graph.raceCount,1); assert.equal(graph.markets.size,3); assert.equal(graph.exchanges.get('13').stateGroup,'state:Montana');
  assert.throws(()=>leadLagMarketGraphV1({candidatePairs:[{...structural.candidatePairs[0],raceName:'Unscoped market'}]}),/LEADLAG_RACE_TYPE/);
});

test('causal opponent movement selects only after development fit and realizes ask-to-later-bid net',()=>{
  const tape=completeSignalTape(),report=run(tape.observations);
  const row=report.results.find(x=>x.feature==='raceOpponent:1500'&&x.latencyMs===0&&x.horizonMs===1000);
  assert.equal(report.holdout.groups.length,1); assert.deepEqual(report.holdout.groups,['state:Alaska']);
  assert.equal(row.developmentLabels,1); assert.equal(row.heldoutLabels,1); assert.equal(row.selectedTrades,1);
  assert.ok(Math.abs(row.selectedNetOneShareSum-.04)<1e-12); assert.ok(Math.abs(row.selectedMeanNet-.04)<1e-12);
  assert.equal(row.positiveDescriptiveScreen,false,'one heldout trade is not evidence of an edge');
  const delayed=report.results.find(x=>x.feature==='raceOpponent:1500'&&x.latencyMs===1000&&x.horizonMs===1000);
  assert.equal(delayed.selectedTrades,0,'latency takes the next observation strictly after the latency threshold');
  assert.equal(report.assumptions.fees,'0.000000');
  assert.equal(report.profitabilityEstablished,false);
});

test('same-time peer changes are unavailable to the decision and later source data cannot leak backward',()=>{
  const tape=completeSignalTape({sameTime:true});
  const withFuture=[...tape.observations,point('2002','202',80000,.2,{sequence:4})].sort((a,b)=>Date.parse(a.availableAt)-Date.parse(b.availableAt)||a.exchangeId.localeCompare(b.exchangeId));
  const result=run(withFuture).results.find(x=>x.feature==='raceOpponent:1500'&&x.latencyMs===0&&x.horizonMs===1000);
  assert.equal(result.heldoutLabels,1); assert.equal(result.selectedTrades,0,'the peer update in the decision timestamp group is excluded');
});

test('same-time invalidation clears peer history before any grouped decision can read it',()=>{
  const tape=completeSignalTape();
  const result=run(tape.observations,{breaks:[{marketId:'202',at:at(tape.decisionAt)}]})
    .results.find(x=>x.feature==='raceOpponent:1500'&&x.latencyMs===0&&x.horizonMs===1000);
  assert.equal(result.heldoutFeatureSamples,0);
  assert.equal(result.heldoutLabels,0);
});

test('depth shortfall or an invalidation between entry and exit censors the quote outcome',()=>{
  const shallow=completeSignalTape({shallow:true});
  const depthReport=run(shallow.observations,{minDepth:2});
  const feature='raceOpponent:1500';
  const depthRow=depthReport.results.find(x=>x.feature===feature&&x.latencyMs===0&&x.horizonMs===1000);
  assert.equal(depthRow.heldoutLabels,0); assert.equal(depthReport.diagnostics.censoredDepth>0,true);

  const normal=completeSignalTape();
  const breakAt=at(normal.decisionAt+2000);
  const broken=run(normal.observations,{breaks:[{marketId:'201',at:breakAt}]});
  const brokenRow=broken.results.find(x=>x.feature===feature&&x.latencyMs===0&&x.horizonMs===1000);
  assert.equal(brokenRow.heldoutLabels,0); assert.equal(broken.diagnostics.censoredBreak>0,true);
});

test('stale books, nonmonotone time, and version rollback fail closed',()=>{
  const tape=completeSignalTape();
  const stalePoints=tape.observations.map(x=>({...x}));
  stalePoints[0].sourceAt=at(-6000);
  const staleReport=run(stalePoints);
  assert.equal(staleReport.diagnostics.invalidPoints,0);
  assert.equal(staleReport.results.some(x=>x.heldoutLabels>0),true);
  assert.throws(()=>run([...tape.observations].reverse()),/LEADLAG_POINT_ORDER/);
  const rollback=tape.observations.map(x=>({...x}));
  rollback.find(x=>x.exchangeId==='1002'&&x.sequence==='3').sequence='1';
  assert.throws(()=>run(rollback),/LEADLAG_VERSION_ORDER/);
});
