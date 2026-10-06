// Pure engineering-pause predicate. Caller verifies/writes the boundary and retains
// the original frozen bank, ledger and source before using this result.
import {payloadHash} from '../execution/ledger.mjs';
import {amount,instant} from '../paper/fixed.mjs';
const hex=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const need=(x,c)=>{if(!x)throw Error('PAPER_CONSUMER_PAUSE_'+c);};
const rows=v=>Array.isArray(v)?v:Object.values(v||{});
export function paperConsumerPauseDecision({pauseRecord=null,consumerKind,cohort,bankHash,report,journalAnchor,sourceAnchor,accountingVerified=false,now}){
 const normal={schemaVersion:1,mode:'paper_consumer_engineering_pause_predicate_v1',paused:false,consumeEnabled:true,focusEnabled:true,executionAuthority:false,officialReconciliationAffected:false,marketCaptureAffected:false,accountingReset:false};
 if(pauseRecord===null)return{...normal,reason:'NO_EXPLICIT_PAPER_PAUSE'};
 try{
  need(consumerKind==='counterfactual_paper','COUNTERFACTUAL_ONLY');
  const p=pauseRecord;need(p.schemaVersion===1&&p.owner==='Codex'&&p.cohort===cohort&&p.bankHash===bankHash&&hex(bankHash)&&typeof p.reason==='string'&&p.reason.length>0&&p.reason.length<=256&&['retire_verified_flat','pause_retain_inventory'].includes(p.policy),'OWNER_SCOPE');
  need(instant(p.createdAt)<=instant(now)&&p.bankPreserved===true&&p.journalPreserved===true&&p.sourcePreserved===true&&p.resumeRequiresFullOriginalReconciliation===true,'PRESERVATION');
  need(accountingVerified===true&&report?.bankHash===bankHash&&hex(p.reportHash)&&payloadHash(report)===p.reportHash&&report.executionAuthority===false&&report.unknownAppendAcceptance!==true,'VERIFIED_ACCOUNTING');
  need(journalAnchor&&sourceAnchor&&payloadHash(journalAnchor)===payloadHash(p.journalAnchor)&&payloadHash(sourceAnchor)===payloadHash(p.sourceAnchor)&&Number.isSafeInteger(journalAnchor.sequence)&&journalAnchor.sequence>0&&hex(journalAnchor.hash)&&Number.isSafeInteger(sourceAnchor.bytes)&&sourceAnchor.bytes>0&&hex(sourceAnchor.prefixHash),'ORIGINAL_FRONTIERS');
  need(report.wallets&&Object.keys(report.wallets).length>0,'WALLETS');
  const retainedRisk=Object.entries(report.wallets).map(([caseId,w])=>{
   need(w&&w.positions!==undefined&&w.pending!==undefined,'ACCOUNTING_FIELDS');
   const positions=rows(w.positions),pending=rows(w.pending);need(positions.every(v=>v&&Number.isSafeInteger(v.quantity)&&v.quantity>0)&&pending.every(v=>v&&typeof v==='object'),'RISK_SCHEMA');
   const reserved=w.reservedCost??w.reserved??null;need(reserved!==null&&amount(reserved)>=0n,'RESERVATION');
   return{caseId,positionCount:positions.length,pendingCount:pending.length,reservedCost:reserved,positionsHash:payloadHash(w.positions),pendingHash:payloadHash(w.pending),walletHash:payloadHash(w)};
  });
  if(p.policy==='retire_verified_flat')need(retainedRisk.every(w=>!w.positionCount&&!w.pendingCount&&amount(w.reservedCost)===0n),'FLAT_REQUIRED');
  return{...normal,paused:true,consumeEnabled:false,focusEnabled:false,reason:p.reason,policy:p.policy,pauseHash:payloadHash(p),bankHash,reportHash:p.reportHash,journalAnchor:structuredClone(journalAnchor),sourceAnchor:structuredClone(sourceAnchor),retainedRisk,valuationStatus:'frozen_paper_snapshot_not_current',riskRecordsRetained:true,resumeRequiresFullOriginalReconciliation:true};
 }catch(e){return{...normal,reason:/^PAPER_CONSUMER_PAUSE_[A-Z_]+$/.test(e.message)?e.message:'PAPER_CONSUMER_PAUSE_INVALID',pauseRejected:true};}
}
