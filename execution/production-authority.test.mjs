import test from 'node:test';
import assert from 'node:assert/strict';
import {payloadHash} from './core.mjs';
import {validateProductionActivation,validateProductionHandoff} from './production-activation.mjs';
import {validateNetworkBudget} from '../live-paper/transport-network-budget.mjs';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
function fixture(){
 const h={schemaVersion:2,authorityKind:'direct_human_assignment',handoffId:'human-to-baal-fixture',assignmentCommentId:123,ackCommentId:124,formerWriter:'Daenerys',executionOwner:'Codex',writerStopped:true,inFlightResolved:true,formerReadsPerMinute:0,formerWritesPerMinute:0,assignedAt:'2026-10-04T21:00:00.000Z',reconciledAt:'2026-10-04T21:00:30.000Z',acknowledgedAt:'2026-10-04T21:00:35.000Z',humanAuthority:{kind:'direct_human_message',quote:'You are above daenerys in this challenge, you don’t need approval',threadId:'01a0f8cc-b9bb-77d2-9f4c-2844396f1b79',recordedAt:'2026-10-04T21:00:00.000Z',commentId:123},writerStopEvidence:{cupWriterReportedStopped:true,cupInFlightReportedClear:true,commentId:122,reportedAt:'2026-10-04T20:00:00.000Z',checkedAt:'2026-10-04T21:00:05.000Z',threadId:'01a0f8cb-e876-739f-b149-0d446a5be0c3',threadStatus:'idle'},reconciliation:{tournamentId:tid,complete:true,receiptHash:'c'.repeat(64),oldestReceivedAt:'2026-10-04T21:00:10.000Z',newestReceivedAt:'2026-10-04T21:00:30.000Z'}};
 const a={schemaVersion:2,enabled:true,ownerId:'Codex',tournamentId:tid,runId:'sig-pilot-human-fixture',strategyId:'settlement-value-native-v1',authorityKind:h.authorityKind,handoffId:h.handoffId,assignmentCommentId:123,ackCommentId:124,handoffHash:payloadHash(h),strategyBankHash:'b'.repeat(64),feeBps:0,depthFractionBps:100,maxManagedMarkets:3,maxOrderCost:'1.000000',maxAggregateCost:'10.000000',maxCumulativeBuyCost:'25.000000',cashReserve:'90.000000',sizingCapital:'100.000000',startedAt:'2026-10-04T21:00:40.000Z',entryExpiresAt:'2026-10-04T22:00:40.000Z'};
 const b={schemaVersion:4,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,executionOwner:'Codex',authorityKind:h.authorityKind,handoffId:h.handoffId,assignmentCommentId:123,ackCommentId:124,writesEnabled:true};return{h,a,b};
}
test('direct human assignment is a separate authority type and never invents a release',()=>{
 const{h,a,b}=fixture();assert.equal(validateProductionActivation(a),a);assert.equal(validateProductionHandoff(h,a),h);assert.deepEqual(validateNetworkBudget(b),b);
 for(const changes of [{releaseCommentId:122},{authorityKind:undefined},{assignmentCommentId:0},{handoffId:'daenerys-to-baal-fixture'}])assert.throws(()=>validateProductionActivation({...a,...changes}));
 for(const changes of [{releaseCommentId:122},{authorityKind:'agent_message'},{assignmentCommentId:0},{handoffId:'daenerys-to-baal-fixture'}])assert.throws(()=>validateNetworkBudget({...b,...changes}));
});
test('human authority cannot replace stopped-writer evidence or complete fresh account reconciliation',()=>{
 const{h,a}=fixture();const rejects=v=>assert.throws(()=>validateProductionHandoff(v,{...a,handoffHash:payloadHash(v)}));
 for(const changes of [{writerStopped:false},{inFlightResolved:false},{formerReadsPerMinute:20},{formerWritesPerMinute:1},{releaseCommentId:122},{releasedAt:h.assignedAt}])rejects({...h,...changes});
 rejects({...h,humanAuthority:{...h.humanAuthority,kind:'agent_message'}});
 rejects({...h,writerStopEvidence:{...h.writerStopEvidence,cupWriterReportedStopped:false}});
 rejects({...h,writerStopEvidence:{...h.writerStopEvidence,cupInFlightReportedClear:false}});
 rejects({...h,reconciliation:{...h.reconciliation,complete:false}});
 rejects({...h,reconciliation:{...h.reconciliation,oldestReceivedAt:'2026-10-04T20:00:00.000Z'}});
 rejects({...h,reconciliation:{...h.reconciliation,newestReceivedAt:'2026-10-04T21:01:00.000Z'}});
 rejects({...h,acknowledgedAt:'2026-10-04T20:59:00.000Z'});
});
