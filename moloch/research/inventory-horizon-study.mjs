import fs from 'node:fs';import {createHash} from 'node:crypto';
import {completedTail,entryPolicyTransition,flatEntryTransition} from '../run.mjs';
import {replayVariant} from '../compare.mjs';import {payloadHash} from '../../execution/ledger.mjs';

const output=process.argv[2];if(!output)throw Error('new output required');
const read=p=>JSON.parse(fs.readFileSync(p)),registration=read('data/moloch-fast-registration-20261014-76.json');
const parent=completedTail(registration.parent);if(parent.snapshot.hash!==registration.parentSnapshotHash)throw Error('HORIZON_PARENT');
const directory='runtime/moloch/fast-control-20261014-76',snapshot=read(directory+'/state.json'),{hash,...body}=snapshot,report=read(directory+'/report.json');
if(hash!==payloadHash(body)||body.continuation.parentSnapshotHash!==parent.snapshot.hash||report.mode!=='local_paper_tail'||report.eventJournalHash!==body.lastHash)throw Error('HORIZON_TERMINAL');
const source=fs.readFileSync(body.source.path).subarray(0,body.source.consumedBytes);if(source.length!==body.source.consumedBytes||createHash('sha256').update(source).digest('hex')!==body.source.consumedSha256)throw Error('HORIZON_SOURCE');
const rows=fs.readFileSync(directory+'/events.jsonl','utf8').trim().split(/\r?\n/).map(JSON.parse);let sequence=0,previous='';
for(const row of rows){const{hash:h,...b}=row;if(h!==payloadHash(b)||b.sequence!==++sequence||b.previous!==previous)throw Error('HORIZON_JOURNAL');previous=h;}
if(sequence!==body.sequence||previous!==body.lastHash)throw Error('HORIZON_JOURNAL_END');
const initial=structuredClone(parent.snapshot.state),c=body.continuation;
entryPolicyTransition(initial,c.entryPolicyTransition.entryExchangeIds);
flatEntryTransition(initial,c.flatEntryTransition.inventoryAgeClock==='execution',c.flatEntryTransition.exitPolicy,c.flatEntryTransition.entryPolicy);
const results=[];
for(const maximumInventoryAgeMs of [1800000,30000,60000,120000,300000]){
 const r=replayVariant(initial,rows.map(row=>row.event),{maximumInventoryAgeMs});
 if(maximumInventoryAgeMs===1800000&&payloadHash(r.state)!==payloadHash(body.state))throw Error('HORIZON_BASELINE_PARITY');
 results.push({maximumInventoryAgeMs,cash:r.report.cash,realized:r.report.realized,ownedCost:r.report.ownedCost,unresolved:r.report.unresolvedOrders,newFills:r.newFills.length,closed:r.report.ownedCost==='0.000000'&&r.report.unresolvedOrders===0&&r.report.reservedCash==='0.000000',lastFresh:r.lastFreshLiquidation,worstFresh:r.worstFreshLiquidation,maximumAgeExits:r.newFills.filter(f=>f.inventoryExitReason==='maximumAge').length});
 console.log(JSON.stringify(results.at(-1)));
}
const result={schemaVersion:1,at:new Date().toISOString(),parentSnapshotHash:parent.snapshot.hash,recordedSnapshotHash:hash,sourceSha256:body.source.consumedSha256,eventCount:sequence,baselineFullStateParity:true,inheritedRealized:parent.snapshot.state.realized,initialCash:initial.cash,results,newAccountCapital:'0',contestOrders:0,profitabilityEstablished:false,limitations:['Retrospective policy intervention on an already inspected period, not a new independent holdout.','Same initial wallet, execution clocks, participation, entry deadline, funding, latency and cancel coverage retained for every case.','Public depth and trade-through fills are counterfactual; an exit remains conditional on later full quantity depth.','Open results remain censored; reducing inventory age does not by itself establish positive expected return.']};
fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n',{flag:'wx'});
