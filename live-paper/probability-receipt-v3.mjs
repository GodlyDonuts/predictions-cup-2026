import {instant} from '../paper/fixed.mjs';
import {digest,invalidate,assertState} from './model.mjs';
import {validateProbabilityReceiptBank as validateV2,PROBABILITY_RECEIPT_POLICY} from './probability-receipt-v2.mjs';
export {PROBABILITY_RECEIPT_POLICY,probabilityAccountEligibility,probabilityAccountGate,receiptProbabilityGate,advanceReceiptProbability,receiptProbabilityReport} from './probability-receipt-v2.mjs';
const need=(ok,code)=>{if(!ok)throw Error('PROBABILITY_NATIVE_'+code);},hash=v=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
// This revision changes startup transport only. Economics, research, exposure,
// account projections and the original cash/basis/fill history are unchanged.
export function validateProbabilityReceiptBank(bank){
 need(bank.schemaVersion===4&&bank.mode==='source_probability_native_continuation_v3','BANK');
 validateV2({...bank,schemaVersion:3,mode:'source_probability_owned_risk_continuation_v2'});
 need(bank.nativeBackend?.schemaVersion===1&&bank.nativeBackend.mode==='native_public_book_prefix_v1'&&['binaryHash','engineHash','runtimeLibraryHash'].every(k=>hash(bank.nativeBackend[k])),'NATIVE_IDENTITY');
 return bank;
}
export function validateProbabilityOpening(opening,bank){
 const c=bank.accountingContinuation;
 need(opening?.schemaVersion===1&&digest(opening.priorBank)===c.priorBankHash&&opening.priorBankFileHash===c.priorBankFileHash&&opening.priorBank.name===c.priorCohort&&opening.state?.bankHash===c.priorBankHash&&digest(opening.state)===c.priorStateHash,'OPENING_IDENTITY');validateV2(opening.priorBank);
 need(opening.sourceBytes===bank.sourcePrefixBytes&&opening.sourceSequence===bank.sourcePrefixSequence&&opening.sourceHash===bank.sourcePrefixJournalHash&&opening.sourcePrefixHash===bank.sourcePrefixHash&&opening.journalHash===c.priorJournalHash&&opening.journalFileHash===c.priorJournalFileHash&&opening.journalBytes===c.priorJournalBytes,'OPENING_FRONTIER');
 need(['config','research','identities','universe','policy'].every(k=>digest(opening.priorBank[k])===digest(bank[k]))&&instant(opening.state.lastAvailableAt)<=instant(bank.cutoff),'OPENING_SCOPE');
 need(Object.keys(opening.state.wallets).sort().join('|')==='1000|3000','OPENING_WALLETS');
 for(const [latency,w] of Object.entries(opening.state.wallets))assertState(w.state,{...bank.config,latencyMs:Number(latency)});
 return opening;
}
export function createProbabilityReceiptState(bank,opening){
 validateProbabilityReceiptBank(bank);validateProbabilityOpening(opening,bank);
 const state=structuredClone(opening.state);state.bankHash=digest(bank);state.lastAvailableAt=bank.cutoff;state.barriers={global:bank.cutoff,markets:{}};
 for(const w of Object.values(state.wallets)){invalidate(w.state,null,bank.cutoff,'native_transport_continuation_boundary');for(const st of Object.values(w.state.strategies)){st.expiredIntents+=Object.keys(st.pending).length;st.pending={};}}
 return state;
}
