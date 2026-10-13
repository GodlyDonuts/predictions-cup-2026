import {payloadHash} from '../execution/core.mjs';import {validateQuoteRecoveryRequest} from './quote-projection-recovery.mjs';
// Read-only focus hints from a separately frozen local counterfactual wallet.
// Never adopt account inventory, grant writes, or add account traffic slots.
export function prospectiveQuoteTapeFocusCoreV2({bank,result,universe,now=Date.now()}){
 const {bankHash,...body}=bank??{};
 if(bankHash!==payloadHash(body)||bank.executionAuthority!==false||result?.bankHash!==bankHash||result.cutoff!==bank.cutoff||!Array.isArray(result.cohorts)||result.competitionOrders!==0)throw Error('PROSPECTIVE_TAPE_BANK');
 const requests=new Map(),risk=new Set();
 for(const cohort of result.cohorts){if(!bank.policies.some(p=>payloadHash(p)===payloadHash(cohort.policy))||!Array.isArray(cohort.unresolvedQuotes))throw Error('PROSPECTIVE_TAPE_COHORT');
  for(const q of cohort.unresolvedQuotes){if(q.closedAt||!universe.exchanges.some(x=>x.marketId===q.marketId&&x.exchangeId===q.exchangeId)||!Number.isSafeInteger(q.activationAt)||!Number.isSafeInteger(q.expiresAt)||q.activationAt>=q.expiresAt)throw Error('PROSPECTIVE_TAPE_QUOTE');
   if(q.coveredThrough!==undefined&&(!Number.isSafeInteger(q.coveredThrough)||q.coveredThrough<q.activationAt))throw Error('PROSPECTIVE_TAPE_FRONTIER');risk.add(q.marketId);const finalEnd=Math.min(q.expiresAt,q.cancelEffectiveAt??Infinity),from=Math.max(q.activationAt,q.coveredThrough??q.activationAt),end=Math.min(finalEnd,from+120000);if(end>now||end<=from)continue;
   const r={schemaVersion:1,kind:'expired_paper_quote_coverage',marketId:q.marketId,exchangeId:q.exchangeId,from:new Date(from).toISOString(),to:new Date(end).toISOString(),requiredCheckpoint:'0'};
   const request={...r,key:payloadHash(r)};validateQuoteRecoveryRequest(request,universe,now);requests.set(request.key,request);
  }
 }
 return{riskMarketIds:[...risk],recoveryWindows:[...requests.values()].sort((a,b)=>Date.parse(a.from)-Date.parse(b.from)||Number(a.marketId)-Number(b.marketId)).slice(0,4),bankHash,executionAuthority:false,extraReadSlots:0,requiredCheckpointBasis:'anonymous_coverage_only_no_common_book_engine_sequence_claimed'};
}
