// Official contract observed 2026-10-12:
// https://sig.thesuper.market/api/v1/docs POST /orders/batch (max 50).
// This adapter has no dispatcher, credentials, timers, leases or authority.
// A successful placement envelope never proves fills/cash reconciliation.
import {price, instant} from '../paper/fixed.mjs';
import {payloadHash, canonicalJson} from './ledger.mjs';
const need=(ok,code)=>{if(!ok)throw Error('MAKER_BATCH_'+code);};
const object=v=>v!==null&&typeof v==='object'&&!Array.isArray(v);
const id=v=>typeof v==='string'&&/^[1-9]\d*$/.test(v)||Number.isSafeInteger(v)&&v>0;
const freeze=v=>{if(object(v)||Array.isArray(v)){Object.values(v).forEach(freeze);Object.freeze(v);}return v;};

export function bindMakerBatchV1({decision,tournamentId,idempotencyKey}) {
  need(decision?.kind==='submit_batch'&&Array.isArray(decision.quotes)&&decision.quotes.length>=2&&decision.quotes.length<=50,'DECISION');
  const {dispatchKey,...decisionBody}=decision;
  need(dispatchKey===payloadHash(decisionBody),'DECISION_CHANGED');
  need(typeof tournamentId==='string'&&/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(tournamentId),'TOURNAMENT');
  need(typeof idempotencyKey==='string'&&idempotencyKey.length>=1&&idempotencyKey.length<=255&&!/[\x00-\x1f\x7f]/.test(idempotencyKey),'KEY');
  const slots=new Set();
  const orders=decision.quotes.map(q=>{
    const slot=`${q.exchangeId}:${q.side}:${q.action}`;
    need(typeof q.exchangeId==='string'&&/^[1-9]\d*$/.test(q.exchangeId)&&!slots.has(slot),'EXCHANGE_OR_DUPLICATE');slots.add(slot);
    need(['yes','no'].includes(q.side)&&['buy','sell'].includes(q.action)&&Number.isSafeInteger(q.quantity)&&q.quantity>0&&q.quantity<=2147483647,'ORDER');
    need(q.action!=='sell'||q.reduceOnly===true,'COVERED_SELL_REQUIRED');
    need(instant(q.validUntil)>instant(decision.at),'EXPIRED');
    return {exchangeId:q.exchangeId,side:q.side,action:q.action,quantity:q.quantity,
      price:Number(price(q.price))/1000000,expirationDate:q.validUntil,tournamentId};
  });
  const body={idempotencyKey,orders},bodyJson=canonicalJson(body);
  return freeze({schemaVersion:1,method:'POST',endpoint:'/orders/batch',decisionKey:dispatchKey,
    body,bodyJson,requestHash:payloadHash(body),writeAdmissionsRequired:1,atomicAcceptance:false});
}

export function assertMakerBatchBindingV1(request) {
  need(request?.schemaVersion===1&&request.method==='POST'&&request.endpoint==='/orders/batch'&&
    request.bodyJson===canonicalJson(request.body)&&request.requestHash===payloadHash(request.body),'REQUEST_CHANGED');
  need(Array.isArray(request.body.orders)&&request.body.orders.length>=2&&request.body.orders.length<=50,'ORDER_COUNT');
  return request;
}

// Final 200/207/422 envelopes replay exactly; incomplete 429/5xx batches must
// retain their ORIGINAL key and ordered body. Missing indexes are unresolved,
// not failed entries that may safely be resubmitted under a fresh key.
export function inspectMakerBatchOutcomeV1(request,response) {
  assertMakerBatchBindingV1(request);
  const n=request.body.orders.length,status=response?.status;
  need(Number.isInteger(status)&&status>=100&&status<=599,'HTTP_STATUS');
  const rows=response?.data?.results,items=Array.from({length:n},(_,index)=>({index,kind:'unobserved',orderId:null,reason:'missing_result'}));
  const seen=new Set(),knownOrderIds=new Set();let malformed=false;
  if(Array.isArray(rows))for(const row of rows){
    // Retain identities even in duplicate/foreign/malformed response indexes.
    if(id(row?.data?.orderId)) {
      const key=String(row.data.orderId);if(knownOrderIds.has(key))malformed=true;
      knownOrderIds.add(key);
    }
    if(!Number.isSafeInteger(row?.index)||row.index<0||row.index>=n||seen.has(row.index)) {malformed=true;continue;}
    seen.add(row.index);const i=row.index,q=request.body.orders[i],data=row.data;
    if(typeof row.ok!=='boolean'||!Number.isInteger(row.status)||!object(data)){malformed=true;continue;}
    const orderId=id(data.orderId)?String(data.orderId):null;
    if(row.ok===true&&row.status>=200&&row.status<300){
      const scoped=data.exchangeId===q.exchangeId;
      const canonical=data.action===q.action&&data.side===q.side&&data.price===q.price&&data.quantity===q.quantity;
      items[i]={index:i,kind:orderId&&scoped&&canonical?'accepted_needs_reconciliation':'unknown',orderId,
        reason:!orderId?'missing_order_identity':!scoped?'exchange_mismatch':!canonical?'canonical_action_mismatch':'placement_only'};
    }else{
      const code=typeof data.code==='string'?data.code:typeof data.error?.code==='string'?data.error.code:null;
      const definite=row.ok===false&&row.status>=400&&row.status<500&&row.status!==429&&
        (row.status!==409||code==='IDEMPOTENCY_KEY_EXPIRED')&&code!=='ORDER_STATUS_UNKNOWN'&&orderId===null;
      items[i]={index:i,kind:definite?'rejected':row.ok===false&&row.status===429&&code==='RATE_LIMITED'?'transient':'unknown',
        orderId,reason:code??'unclassified_item_error'};
    }
  }
  // These documented outer failures reject before placement. A conflict does
  // not prove a previously bound request never executed; 409 stays uncertain.
  const definitiveOuterRejection=!Array.isArray(rows)&&[400,401,403,404].includes(status);
  if(definitiveOuterRejection)for(let i=0;i<n;i++)items[i]={index:i,kind:'rejected',orderId:null,reason:'outer_preplacement_rejection'};
  const accepted=items.filter(i=>i.kind==='accepted_needs_reconciliation'),rejected=items.filter(i=>i.kind==='rejected');
  const allObserved=seen.size===n&&!malformed;
  const finalShape=allObserved&&((status===200&&accepted.length===n)||
    (status===207&&accepted.length>0&&rejected.length>0&&accepted.length+rejected.length===n)||
    (status===422&&rejected.length===n));
  const unresolved=items.filter(i=>['unobserved','unknown','transient'].includes(i.kind)).map(i=>i.index);
  const completeRejection=definitiveOuterRejection||finalShape&&rejected.length===n;
  return freeze({schemaVersion:1,requestHash:request.requestHash,responseHash:payloadHash(response),httpStatus:status,
    items,knownOrderIds:[...knownOrderIds],unresolvedIndexes:unresolved,
    malformedEnvelope:malformed,finalEnvelope:finalShape||definitiveOuterRejection,
    disposition:completeRejection?'definitive_rejection':finalShape?'reconcile_accepted_orders':'reconcile_or_retry_identical_batch',
    retrySameKeyAndOrderedBodyOnly:!completeRejection&&!finalShape,
    // Caller resolves known fills/cancel races and complete account exposure.
    // Never call finishMakerDispatchV1(confirmed) from this report alone.
    accountReconciliationRequired:!completeRejection,profitabilityEstablished:false});
}
