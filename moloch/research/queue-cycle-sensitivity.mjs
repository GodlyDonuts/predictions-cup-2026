import fs from 'node:fs';import path from 'node:path';
import {completedTail,entryPolicyTransition,drainPolicyTransition,flatEntryTransition} from '../run.mjs';
import {advanceMoloch} from '../engine.mjs';import {replayVariant} from '../compare.mjs';import {payloadHash} from '../../execution/ledger.mjs';
const output=process.argv[2];if(!output)throw Error('new output required');
const mode=process.argv[3]??'queue';if(!['queue','entryPrice'].includes(mode))throw Error('cycle mode queue or entryPrice required');
const read=file=>JSON.parse(fs.readFileSync(file)),registration=read('data/moloch-fast-registration-20261014-67.json'),parent=completedTail(registration.parent);
if(parent.snapshot.hash!==registration.parentSnapshotHash)throw Error('QUEUE_STUDY_PARENT');
let baseline=structuredClone(parent.snapshot.state),previousParentHash=parent.snapshot.hash;const episodes=[];
for(let phase=67;phase<=70;phase++){
 const directory=`runtime/moloch/fast-candidate-20261014-${phase}`,snapshot=read(directory+'/state.json'),{hash,...body}=snapshot,report=read(directory+'/report.json');
 if(hash!==payloadHash(body)||body.continuation.parentSnapshotHash!==previousParentHash||report.eventJournalHash!==body.lastHash||report.source.sha256!==body.source.consumedSha256)throw Error('QUEUE_STUDY_CHAIN');
 const c=body.continuation;
 if(c.entryPolicyTransition)entryPolicyTransition(baseline,c.entryPolicyTransition.entryExchangeIds);
 if(c.flatEntryTransition)flatEntryTransition(baseline,c.flatEntryTransition.inventoryAgeClock==='execution',c.flatEntryTransition.exitPolicy??null,c.flatEntryTransition.entryPolicy??null);
 if(c.drainPolicyTransition)drainPolicyTransition(baseline);
 const source=fs.readFileSync(body.source.path).subarray(0,body.source.consumedBytes);
 const {createHash}=await import('node:crypto');if(source.length!==body.source.consumedBytes||createHash('sha256').update(source).digest('hex')!==body.source.consumedSha256)throw Error('QUEUE_STUDY_SOURCE');
 const rows=fs.readFileSync(directory+'/events.jsonl','utf8').trim().split(/\r?\n/).filter(Boolean).map(JSON.parse);let sequence=0,previous='';
 for(const row of rows){const{hash:h,...b}=row;if(h!==payloadHash(b)||b.sequence!==++sequence||b.previous!==previous)throw Error('QUEUE_STUDY_JOURNAL');advanceMoloch(baseline,b.event);if(payloadHash(baseline)!==b.stateHash)throw Error('QUEUE_STUDY_BASELINE_STATE');previous=h;}
 if(sequence!==body.sequence||previous!==body.lastHash||payloadHash(baseline)!==payloadHash(snapshot.state))throw Error('QUEUE_STUDY_TERMINAL');
 episodes.push({phase,snapshotHash:hash,policy:snapshot.state.policy,events:rows.map(r=>r.event)});previousParentHash=hash;
}
const entryStopAt=new Date(Date.parse(registration.registeredAt)+240000).toISOString();
const cases=mode==='entryPrice'?[['recorded',{}],['joinFast',{insideTicks:0,maximumInventoryAgeMs:60000}],['protectedFast',{insideTicks:0,entryBackoffTicks:4,maximumInventoryAgeMs:60000}],['protectedFastDeadline',{insideTicks:0,entryBackoffTicks:4,maximumInventoryAgeMs:60000,entryStopAt}]]:[['strictActual',{}],['queue25',{fillModel:'conditionalFifo',touchAttributionBps:2500}],['queue50',{fillModel:'conditionalFifo',touchAttributionBps:5000}],['queue100',{fillModel:'conditionalFifo',touchAttributionBps:10000}],['deadlineStrict',{entryStopAt}],['deadlineQueue50',{entryStopAt,fillModel:'conditionalFifo',touchAttributionBps:5000}]],results=[];
for(const [name,overrides]of cases){
 let state=structuredClone(parent.snapshot.state);const stages=[];
 for(const episode of episodes){const result=replayVariant(state,episode.events,{...episode.policy,...overrides});state=result.state;stages.push({phase:episode.phase,realized:result.report.realized,cash:result.report.cash,ownedCost:result.report.ownedCost,unresolved:result.report.unresolvedOrders,lastFreshLiquidation:result.lastFreshLiquidation,worstFreshLiquidation:result.worstFreshLiquidation});}
 const result=replayVariant(state,[]),r=result.report;results.push({name,overrides,closed:r.ownedCost==='0.000000'&&r.unresolvedOrders===0,report:r,newFills:state.fills.slice(parent.snapshot.state.fills.length),stages});
 console.log(JSON.stringify({name,cash:r.cash,realized:r.realized,owned:r.ownedCost,unresolved:r.unresolvedOrders,newFills:state.fills.length-parent.snapshot.state.fills.length,eligible:r.profitSignalEligible}));
}
fs.writeFileSync(output,JSON.stringify({schemaVersion:1,at:new Date().toISOString(),baselineStateParity:true,parentSnapshotHash:parent.snapshot.hash,sourceSnapshots:episodes.map(e=>({phase:e.phase,hash:e.snapshotHash,events:e.events.length})),results,contestOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:['Retrospective assumption sensitivity on an already inspected losing cycle; not untouched validation.','At-touch attribution fractions assume aggressor direction and FIFO queue visibility; they do not certify actual fills.','Unchanged1% participation, latency, funding, fees, write caps and complete-window reservation rules remain enforced.','Each scenario retains its own continuous financial state; outcomes cannot be summed or copied into the original bank.','Finite source clocks and outages remain; an open terminal bank is censored, not a profitable closed cycle.']},null,2)+'\n',{flag:'wx'});
