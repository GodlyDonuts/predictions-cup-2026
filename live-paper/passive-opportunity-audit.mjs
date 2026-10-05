import {instantNs} from '../feed/contract.mjs';
import {amount,TICK} from '../paper/fixed.mjs';
import {tapeTick} from './canonical-entry-exit-study.mjs';

// These are observed counterparties, never orders, queue priority or fills.
export function auditPassiveOpportunities(quotes,pages){
 const trades=new Map();
 for(const page of pages)for(const t of page.trades){if(!trades.has(t.key))trades.set(t.key,{...t,exchangeId:page.exchangeId,firstAvailableAt:page.receivedAt});}
 return quotes.map(q=>{
  const start=instantNs(q.activationAt),end=instantNs(q.expiresAt),limit=amount(q.yesEquivalentLimit),direction=(q.action==='buy')===(q.side==='YES')?-1n:1n;
  let eligible=0,unusable=0,touchEvents=0,touchShares=0,throughEvents=0,throughShares=0,lateAfterResolution=0;
  for(const t of trades.values()){
   if(t.exchangeId!==q.exchangeId||instantNs(t.createdAt)<start||instantNs(t.createdAt)>=end)continue;
   if(q.resolvedAt&&instantNs(t.firstAvailableAt)>instantNs(q.resolvedAt)){lateAfterResolution++;continue;}
   eligible++;const px=tapeTick(t);if(!px){unusable++;continue;}const delta=(amount(px.price)-limit)*direction;
   if(delta===0n){touchEvents++;touchShares+=t.size;}if(delta>=TICK){throughEvents++;throughShares+=t.size;}
  }
  return{quoteId:q.id,marketId:q.marketId,exchangeId:q.exchangeId,side:q.side,action:q.action,limit:q.limit,createdAt:q.createdAt,activationAt:q.activationAt,expiresAt:q.expiresAt,resolution:q.resolution,
   eligibleUniqueTrades:eligible,unusablePrices:unusable,touchEvents,touchShares,strictThroughEvents:throughEvents,strictThroughShares:throughShares,lateAfterResolution,
   queuePositionProven:false,executionProven:false};
 });
}
