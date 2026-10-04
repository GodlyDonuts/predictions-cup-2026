import fs from 'node:fs';
import path from 'node:path';
import {decimalMicros,payloadHash} from './core.mjs';

const TID='bda92870-621e-47b0-bc3c-3602c5c26f55';
const need=(ok,code)=>{if(!ok)throw Error('PRODUCTION_'+code);};
const hash=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const stamp=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;
const comment=x=>Number.isSafeInteger(x)&&x>0;
export function productionAuthorityFields(a){
 if(a?.authorityKind==='direct_human_assignment'){
  need(/^human-to-baal-[a-z0-9-]{1,70}$/.test(a.handoffId)&&comment(a.assignmentCommentId)&&a.releaseCommentId===undefined,'HUMAN_ASSIGNMENT');
  return ['handoffId','authorityKind','assignmentCommentId','ackCommentId'];
 }
 need(a?.authorityKind===undefined&&/^daenerys-to-baal-[a-z0-9-]{1,70}$/.test(a?.handoffId)&&comment(a.releaseCommentId)&&a.assignmentCommentId===undefined,'AGENT_RELEASE');
 return ['handoffId','releaseCommentId','ackCommentId'];
}
export function validateProductionActivation(a){
 need(a?.schemaVersion===2&&a.enabled===true&&a.ownerId==='Codex'&&a.tournamentId===TID&&/^sig-pilot-[a-z0-9-]{1,64}$/.test(a.runId)&&a.strategyId==='settlement-value-native-v1','ACTIVATION');
 need(hash(a.handoffHash)&&hash(a.strategyBankHash)&&typeof a.handoffId==='string'&&comment(a.ackCommentId),'HANDOFF');
 productionAuthorityFields(a);
 need(a.feeBps===0&&a.depthFractionBps===100&&a.maxManagedMarkets===3,'POLICY');
 const order=decimalMicros(a.maxOrderCost),total=decimalMicros(a.maxAggregateCost),buys=decimalMicros(a.maxCumulativeBuyCost),reserve=decimalMicros(a.cashReserve),capital=decimalMicros(a.sizingCapital);
 need(order>0n&&order<=1000000n&&total>=order&&total<=10000000n&&buys>=order&&buys<=25000000n&&reserve>=0n&&capital>reserve&&capital<=100000000000n,'LIMITS');
 need(stamp(a.startedAt)&&stamp(a.entryExpiresAt)&&Date.parse(a.entryExpiresAt)>Date.parse(a.startedAt)&&Date.parse(a.entryExpiresAt)-Date.parse(a.startedAt)<=21600000,'WINDOW');
 return a;
}
export function validateProductionHandoff(h,a){
 validateProductionActivation(a);
 need(payloadHash(h)===a.handoffHash&&productionAuthorityFields(a).every(k=>h[k]===a[k])&&h.formerWriter==='Daenerys'&&h.executionOwner==='Codex'&&h.writerStopped===true&&h.inFlightResolved===true&&h.formerReadsPerMinute===0&&h.formerWritesPerMinute===0,'HANDOFF_RECORD');
 if(a.authorityKind==='direct_human_assignment'){
  const u=h.humanAuthority,s=h.writerStopEvidence,r=h.reconciliation;
  need(h.schemaVersion===2&&h.releaseCommentId===undefined&&h.releasedAt===undefined&&u?.kind==='direct_human_message'&&u.commentId===a.assignmentCommentId&&u.threadId==='01a0f8cc-b9bb-77d2-9f4c-2844396f1b79'&&typeof u.quote==='string'&&u.quote.length>0&&u.quote.length<=2000&&stamp(u.recordedAt),'HUMAN_AUTHORITY');
  // Chat activity can be unrelated human work. Record the Cup writer's stop
  // evidence explicitly; an idle chat alone is not a process-stop proof.
  need(comment(s?.commentId)&&s.cupWriterReportedStopped===true&&s.cupInFlightReportedClear===true&&stamp(s.reportedAt)&&stamp(s.checkedAt)&&s.threadId==='01a0f8cb-e876-739f-b149-0d446a5be0c3'&&['idle','active'].includes(s.threadStatus)&&Date.parse(s.reportedAt)<=Date.parse(s.checkedAt)&&Date.parse(s.checkedAt)>=Date.parse(u.recordedAt),'WRITER_STOP_EVIDENCE');
  need(r?.complete===true&&r.tournamentId===TID&&hash(r.receiptHash)&&stamp(r.oldestReceivedAt)&&stamp(r.newestReceivedAt)&&Date.parse(r.oldestReceivedAt)<=Date.parse(r.newestReceivedAt)&&Date.parse(r.newestReceivedAt)<=Date.parse(h.reconciledAt)&&Date.parse(a.startedAt)-Date.parse(r.oldestReceivedAt)<=120000,'RECONCILIATION');
  need(stamp(h.assignedAt)&&h.assignedAt===u.recordedAt&&Date.parse(h.assignedAt)<=Date.parse(h.reconciledAt)&&Date.parse(s.checkedAt)<=Date.parse(h.reconciledAt),'ASSIGNMENT_CLOCK');
 }else need(h.schemaVersion===1&&stamp(h.releasedAt)&&Date.parse(h.releasedAt)<=Date.parse(h.reconciledAt),'RELEASE_CLOCK');
 need(stamp(h.reconciledAt)&&stamp(h.acknowledgedAt)&&Date.parse(h.reconciledAt)<=Date.parse(h.acknowledgedAt)&&Date.parse(h.acknowledgedAt)<=Date.parse(a.startedAt),'HANDOFF_CLOCK');
 return h;
}
export function productionHandoffCurrent(runtimeDir,a){
 try{return !!validateProductionHandoff(JSON.parse(fs.readFileSync(path.join(runtimeDir,'production-handoff.json'),'utf8')),a);}catch{return false;}
}
