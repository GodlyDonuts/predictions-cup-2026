// Versioned compatibility: source freshness bounds admission, not resting life.
// This module creates no permits, account clients, retries or execution rights.
import {instant} from '../paper/fixed.mjs';import {payloadHash} from './ledger.mjs';
import {planMakerQuoteV1,MAKER_SCHEDULER_POLICY_V1} from './maker-quote-scheduler-v1.mjs';
import {bindMakerBatchV1,inspectMakerBatchOutcomeV1} from './maker-batch-outcome-v1.mjs';
const need=(v,c)=>{if(!v)throw Error('MAKER_ORDER_CLOCKS_'+c);};
const freeze=v=>{if(v!==null&&typeof v==='object'){Object.values(v).forEach(freeze);Object.freeze(v);}return v;};
export const MAKER_ORDER_CLOCK_POLICY_V2=Object.freeze({placementLatencyMs:3000,minimumActiveMs:2500});
function horizon(policy){need(Number.isSafeInteger(policy.placementLatencyMs)&&policy.placementLatencyMs>0&&
  Number.isSafeInteger(policy.minimumActiveMs)&&policy.minimumActiveMs>0,'POLICY');return policy.placementLatencyMs+policy.minimumActiveMs;}
function clocks(q){const admission=instant(q.admissionValidUntil??q.validUntil);need(admission===instant(q.validUntil),'ADMISSION_ALIAS');
  return{admission,expiry:instant(q.orderExpiresAt)};}
export function planMakerQuoteV2(state,input,schedulerPolicy=MAKER_SCHEDULER_POLICY_V1,clockPolicy=MAKER_ORDER_CLOCK_POLICY_V2){
  const now=instant(input.at),required=horizon(clockPolicy);
  const desired=input.desired.filter(q=>clocks(q).expiry-now>=required);
  // V1 already checks the original source/admission deadline. Never replace it
  // with the long server expiry when calculating eligibility or queue retention.
  return planMakerQuoteV1(state,{...input,desired},schedulerPolicy);
}
export function bindMakerBatchV2({decision,tournamentId,idempotencyKey,clockPolicy=MAKER_ORDER_CLOCK_POLICY_V2}){
  const required=horizon(clockPolicy),at=instant(decision.at),{dispatchKey,...sourceBody}=decision;
  need(dispatchKey===payloadHash(sourceBody)&&decision.kind==='submit_batch','DECISION');
  const quotes=decision.quotes.map(q=>{const c=clocks(q);need(c.admission>at&&c.expiry-at>=required,'STALE_OR_INACTIVE');
    return{...q,validUntil:q.orderExpiresAt};});
  const wireBody={...sourceBody,quotes},wireDecision={...wireBody,dispatchKey:payloadHash(wireBody)};
  const wire=bindMakerBatchV1({decision:wireDecision,tournamentId,idempotencyKey});
  return freeze({...wire,schemaVersion:2,decisionKey:dispatchKey,sourceDecision:structuredClone(decision),clockPolicy:structuredClone(clockPolicy)});
}
export function assertMakerBatchBindingV2(request){
  need(request?.schemaVersion===2,'VERSION');
  const expected=bindMakerBatchV2({decision:request.sourceDecision,tournamentId:request.body.orders[0].tournamentId,
    idempotencyKey:request.body.idempotencyKey,clockPolicy:request.clockPolicy});
  need(payloadHash(expected)===payloadHash(request),'BINDING_CHANGED');return request;
}
export function assertMakerBatchFirstDispatchV2(request,at){
  assertMakerBatchBindingV2(request);const now=instant(at),required=horizon(request.clockPolicy);
  need(now>=instant(request.sourceDecision.at),'DISPATCH_CLOCK');
  for(const q of request.sourceDecision.quotes){const c=clocks(q);need(now<c.admission&&c.expiry-now>=required,'FIRST_DISPATCH_STALE');}
  return request;
}
// An already dispatched UNKNOWN request remains owned and immutable even after
// admission expires. Only the durable owner may authorize an identical retry;
// this identity checker never grants retry permission or releases liabilities.
export function inspectMakerBatchOutcomeV2(request,response){
  assertMakerBatchBindingV2(request);return inspectMakerBatchOutcomeV1({...request,schemaVersion:1},response);
}
