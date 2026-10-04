import {amount,money,TICK,SCALE} from '../paper/fixed.mjs';
import {evaluateCanonicalNoCapacity as baseCapacity,validateCapacityReview as baseReview,CANONICAL_NO_POLICY as defaultPolicy} from './canonical-no-capacity.mjs';
export {CANONICAL_NO_POLICY} from './canonical-no-capacity.mjs';
const need=(ok,c)=>{if(!ok)throw Error('CANONICAL_REFUND_'+c);};
export function validateCapacityReview(review){
 baseReview(review);need(review.refundPolicy?.schemaVersion===1&&review.refundPolicy.representation==='held_purchase_principal_not_face_value'&&review.refundPolicy.feeRefundAssumed===false&&/^[a-f0-9]{64}$/.test(review.refundPolicy.sourceHash),'POLICY');
 need(Array.isArray(review.refundScenarios)&&review.refundScenarios.length>=3&&review.refundScenarios.every(s=>typeof s.id==='string'&&s.id.length>0&&Array.isArray(s.outcomes)&&s.outcomes.length===2&&s.outcomes.some(v=>v==='refund')&&s.outcomes.every(v=>v===0||v===1||v==='refund'))&&new Set(review.refundScenarios.map(s=>s.id)).size===review.refundScenarios.length,'SCENARIOS');
 for(const pair of [['refund','refund'],['refund',0],[0,'refund']])need(review.refundScenarios.some(s=>s.outcomes.every((v,i)=>v===pair[i])),'ASYMMETRIC_STRESS');return review;
}
export function evaluateCanonicalNoCapacity(options){
 validateCapacityReview(options.review);const result=baseCapacity(options),p=options.policy||defaultPolicy,quantity=BigInt(result.matchedQuantity),principal=[0n,0n],fees=[0n,0n];
 for(const segment of result.segments)for(let i=0;i<2;i++){const n=BigInt(segment.quantity);principal[i]+=(amount(segment.noPrices[i])+BigInt(p.adverseTicksPerLeg)*TICK)*n;fees[i]+=amount(p.feePerShare)*n;}
 if(quantity)for(let i=0;i<2;i++)fees[i]+=amount(p.fixedFeePerLeg);
 need(principal[0]+principal[1]+fees[0]+fees[1]===amount(result.acquisitionCost),'COST_RECONCILIATION');
 const scenarios=options.review.refundScenarios.map(s=>{const proceeds=s.outcomes.map((v,i)=>v==='refund'?principal[i]:BigInt(v)*SCALE*quantity).reduce((a,b)=>a+b,0n);return{...s,proceeds:money(proceeds),profit:money(proceeds-amount(result.acquisitionCost))};});
 return{...result,mode:'canonical_no_conditional_capacity_refund_v2',purchasePrincipalByLeg:principal.map(money),nonrefundableModeledFeesByLeg:fees.map(money),refundRepresentation:options.review.refundPolicy.representation,cancellationScenarios:scenarios,
  limitations:[...(result.limitations||[]),'Cancellation refunds modeled held purchase principal, never face value; separate fees are not assumed refundable. Actual platform refundable-cost allocation remains unverified.','Partial cancellation can lose the other leg principal. Both legs refunded produce no principal gain. No paired cancellation or scenario completeness is guaranteed.']};
}
