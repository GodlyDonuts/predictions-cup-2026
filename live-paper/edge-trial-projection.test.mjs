import test from 'node:test';import assert from 'node:assert/strict';import {digest} from './model.mjs';import {projectEdgeTrials,prioritizeEdgeTrials} from './edge-trial-projection.mjs';
const at=n=>new Date(n).toISOString(),universe={exchanges:['1','2','3','4','5','6'].map(marketId=>({marketId}))},plan={entryMarketIds:['3','4'],reads:[{marketId:'1',reason:'priority_held_or_pending'},{marketId:'2',reason:'comparison'},{marketId:'3',reason:'edge_observation'},{marketId:'4',reason:'edge_observation'}]};
test('projection exposes exact current pending deadlines tied to checkpoint and no wallet/account fields',()=>{
 const bank={universe,config:{latencyMs:1000},policy:{trialLifetimeMs:30000,horizonMs:300000,labelLatenessMs:60000}},state={bankHash:digest(bank),trials:{'3':{preparedAt:at(1000)},'4':{preparedAt:at(1000),filledAt:at(2500)}}},stats={bankHash:state.bankHash,stateHash:digest(state),updatedAt:at(3000),sourceSequence:7,sourceHash:'source',journalSequence:3,journalHash:'journal',sourceClosed:false};
 const r=projectEdgeTrials(state,bank,stats,{pid:process.pid,lockId:'owner',wrapperHash:'a'.repeat(64)}),{projectionHash,...body}=r;assert.equal(digest(body),projectionHash);assert.deepEqual(r.trials.map(t=>[t.marketId,t.phase,t.dueAt,t.expiresAt]),[['3','awaiting_fill',at(2000),at(31000)],['4','awaiting_outcome',at(302500),at(362500)]]);assert.equal(r.ordersSubmitted,0);assert.equal(r.networkRequests,0);assert.equal(r.wallet,undefined);assert.throws(()=>projectEdgeTrials(state,bank,{...stats,stateHash:'other'},{pid:process.pid,lockId:'owner',wrapperHash:'a'.repeat(64)}),/INVALID_EDGE/);
});
test('late filled trial survives block rotation; near maturity replaces paper slots and never official risk',()=>{
 const trial={marketId:'5',phase:'awaiting_outcome',dueAt:at(300000),expiresAt:at(360000)};
 assert.equal(prioritizeEdgeTrials(plan,{trials:[trial]},universe,{now:200000}),plan);
 const next=prioritizeEdgeTrials(plan,{trials:[trial,{marketId:'6',phase:'awaiting_fill',dueAt:at(269000),expiresAt:at(299000)}]},universe,{now:270000});assert.equal(next.reads.length,4);assert.equal(next.reads[0].marketId,'1');assert.ok(next.reads.some(x=>x.marketId==='5'));assert.ok(next.reads.some(x=>x.marketId==='6'));assert.equal(next.edgeTrialObservation.requestsAdded,0);assert.equal(next.edgeTrialObservation.executionAuthority,false);assert.deepEqual(plan.reads.map(x=>x.marketId),['1','2','3','4']);
});
test('expired unfilled probes are not chased; expired filled probes get censor observation after live deadlines',()=>{
 const expired={marketId:'5',phase:'awaiting_fill',dueAt:at(1000),expiresAt:at(30000)};assert.equal(prioritizeEdgeTrials(plan,{trials:[expired]},universe,{now:30001}),plan);
 const p=prioritizeEdgeTrials(plan,{trials:[{...expired,phase:'awaiting_outcome'}]},universe,{now:30001});assert.ok(p.reads.some(x=>x.marketId==='5'));
 const protectedPlan={...plan,reads:plan.reads.map(x=>({...x,reason:'priority_held_or_pending'}))};assert.equal(prioritizeEdgeTrials(protectedPlan,{trials:[{...expired,phase:'awaiting_outcome'}]},universe,{now:30001}),protectedPlan);
 assert.equal(prioritizeEdgeTrials(plan,{trials:[{...expired,phase:'awaiting_outcome'}]},universe,{now:30001,excludedMarketIds:['5']}),plan);
});
