import {digest} from '../live-paper/model.mjs';
import {validateProbabilityReceiptBank} from '../live-paper/probability-receipt.mjs';

export const RECEIPT_PRODUCTION_POLICY=Object.freeze({takeProfit:'0.020000',stopLoss:'0.050000',maximumHoldMs:172800000,cooldownMs:900000,orderShares:1});
const need=(ok,code)=>{if(!ok)throw Error('RECEIPT_PRODUCTION_'+code);};
// Produces a reviewable immutable strategy candidate. It does not create an
// activation, handoff, account position, intent, API client or order.
export function prepareReceiptProductionBank({paperBank,nativeBackend,contracts}){
 validateProbabilityReceiptBank(paperBank);
 need(nativeBackend?.schemaVersion===1&&nativeBackend.mode==='native_public_book_prefix_v1'&&['binaryHash','engineHash','runtimeLibraryHash'].every(k=>/^[a-f0-9]{64}$/.test(nativeBackend[k])),'NATIVE_BINDING');
 const candidates=paperBank.research.map(r=>{
  const contract=contracts?.[r.marketId];
  need(r.side==='YES'&&contract?.exchangeId===r.exchangeId&&contract.predicateHash===r.predicateHash&&/^[a-f0-9]{64}$/.test(contract.rootHash)&&Number.isSafeInteger(contract.settlesAtMs),'CONTRACT_BINDING');
  return{id:r.marketId+':YES',marketId:r.marketId,exchangeId:r.exchangeId,side:r.side,predicateHash:r.predicateHash,riskGroups:['all','market_'+r.marketId],settlesAtMs:contract.settlesAtMs,
   research:{id:r.id,predicateHash:r.predicateHash,side:r.side,lower:r.lower,upper:r.upper,recordedAtMs:Date.parse(r.recordedAt),sourceAtMs:Date.parse(r.sourceAt),expiresAtMs:Date.parse(r.expiresAt),method:r.method,settlementReviewed:r.settlementReviewed,flags:structuredClone(r.flags),sources:structuredClone(r.sources)}};
 });
 return{schemaVersion:2,mode:'baal_probability_receipt_production_v1',cutoff:paperBank.cutoff,nativeBackend:structuredClone(nativeBackend),
  paperBinding:{bankHash:digest(paperBank),policyHash:digest(paperBank.policy),configHash:digest(paperBank.config),cutoff:paperBank.cutoff,researchHashes:paperBank.research.map(r=>({id:r.id,hash:digest(r)}))},
  receiptPolicy:{...RECEIPT_PRODUCTION_POLICY},
  policy:{minimumEdge:paperBank.policy.minimumLowerEdge,maximumSpread:paperBank.policy.maximumSpread,maximumBookAgeMs:paperBank.config.maxDataAgeMs,maximumResearchAgeMs:paperBank.policy.maximumResearchAgeMs,fractionalKellyBps:2500,slippageTicks:paperBank.config.slippageTicks,depthFractionBps:paperBank.config.depthFractionBps,maxOrderCost:'1.000000',maxExposure:'10.000000',cashReserve:'90.000000',cumulativeBuyBudget:'25.000000',groupCaps:{all:'10.000000',...Object.fromEntries(candidates.map(c=>['market_'+c.marketId,'1.000000']))}},
  contracts:Object.fromEntries(candidates.map(c=>[c.marketId,{rootHash:contracts[c.marketId].rootHash,exchangeId:c.exchangeId,predicateHash:c.predicateHash}])),candidates};
}
