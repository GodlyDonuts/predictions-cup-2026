import {validateCapacityReview} from './canonical-no-capacity.mjs';
import {instant} from '../paper/fixed.mjs';
const optional=new Set(['discovery','comparison','edge_observation','conditional_capacity_pair']);
const need=(ok,c)=>{if(!ok)throw Error('MAKER_READ_PLAN_'+c);};
// External collection policy: keep the frozen evaluator and maker banks intact.
export function planMakerPairReads(plan,review,{now,excludedExchangeIds=[]}={}){
 validateCapacityReview(review);need(Number.isSafeInteger(now)&&Array.isArray(plan.reads)&&plan.reads.length<=4,'INPUT');
 const legs=review.legs,ids=legs.map(l=>l.marketId);need(ids.length===2&&new Set(plan.reads.map(r=>r.marketId)).size===plan.reads.length,'SCOPE');
 const result=(reads,reason,paired=false)=>({...plan,reads,entryMarketIds:(plan.entryMarketIds||[]).filter(id=>reads.some(r=>r.marketId===id)&&(!paired||!ids.includes(id))),
  makerPairFocus:{marketIds:paired?ids:[],reason,requestsAdded:0,executionAuthority:false}});
 if(now<instant(review.recordedAt)||now>=instant(review.expiresAt))return result(plan.reads,'review_not_current');
 if(legs.some(l=>excludedExchangeIds.includes(l.exchangeId)))return result(plan.reads,'own_account_exposure');
 if(plan.comparisonSampling||plan.basketMeasurement?.marketIds?.length||plan.paperPackageFocus?.marketIds?.length)return result(plan.reads,'other_pair_obligation');
 const protectedReads=plan.reads.filter(r=>!optional.has(r.reason));
 const protectedIds=new Set(protectedReads.map(r=>r.marketId));
 if(new Set([...protectedIds,...ids]).size>plan.reads.length)return result(plan.reads,'protected_budget_full');
 const protectedLegs=protectedReads.filter(r=>ids.includes(r.marketId));
 if(protectedLegs.length===2&&Math.abs(protectedReads.indexOf(protectedLegs[0])-protectedReads.indexOf(protectedLegs[1]))!==1)return result(plan.reads,'separated_protected_legs');
 const spec=id=>{const leg=legs.find(l=>l.marketId===id);return {marketId:id,exchangeId:leg.exchangeId,option:'YES',path:'supplementalDepth',reason:'maker_pair_partner'};};
 const reads=[];
 for(const r of protectedReads){reads.push(r);if(protectedLegs.length===1&&r.marketId===protectedLegs[0].marketId)reads.push(spec(ids.find(id=>id!==r.marketId)));}
 if(!protectedLegs.length)reads.push(...ids.map(spec));
 for(const r of plan.reads)if(reads.length<plan.reads.length&&!reads.some(x=>x.marketId===r.marketId))reads.push(r);
 need(reads.length===plan.reads.length&&new Set(reads.map(r=>r.marketId)).size===reads.length,'BUDGET');
 need(protectedReads.every((r,i)=>reads.filter(x=>protectedIds.has(x.marketId))[i]===r),'PROTECTED_ORDER');
 need(Math.abs(reads.findIndex(r=>r.marketId===ids[0])-reads.findIndex(r=>r.marketId===ids[1]))===1,'PAIR_ADJACENCY');
 return result(reads,'coherent_maker_pair',true);
}
