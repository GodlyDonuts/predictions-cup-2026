import {payloadHash} from './core.mjs';import {validateTiming} from '../live-paper/read-timing.mjs';
const need=(v,c)=>{if(!v)throw Error('PHYSICAL_BLOCK_'+c);};
// Requires affirmative producer-side rejection evidence, not mere missing fills.
export function auditPhysicalGuardAdmissionV1({state,events,timings,diagnostic,guardFailure,stop,now}){
 const active=state.orders.filter(o=>!o.proof&&o.phase!=='abandoned');need(active.length===1,'ONE_INTENT');const o=active[0];
 need(o.phase==='uncertain'&&o.operation==='place'&&o.orderId===null&&o.ack===null&&o.fences.length===1&&o.fetchStarts.length===0&&o.receipts.length===0&&!o.cancelPending&&o.bodyHash===payloadHash(o.body)&&Date.parse(o.body.expirationDate)<now,'EXPIRED_NO_INVOCATION');
 const tail=events.slice(-3);need(tail[0]?.kind==='admitted'&&tail[1]?.kind==='network_fence'&&tail[2]?.kind==='error'&&tail[2].code==='REQUEST_UNCONFIRMED'&&tail[0].operation==='place'&&tail[1].operation==='place','EXACT_PATH');
 const failed=Date.parse(tail[2].at),fenced=Date.parse(tail[1].at),from=Date.parse(o.createdAt);
 need(guardFailure?.stage==='physical'&&guardFailure.code==='BAAL_SCALED_PHYSICAL_FETCH_FENCE'&&guardFailure.deadlineRemainingMs<0&&Date.parse(guardFailure.at)>=fenced&&Date.parse(guardFailure.at)<=failed,'PHYSICAL_CLOCK_FAILURE');
 const faults=diagnostic.failures.filter(f=>f.method==='WRITE'&&f.code==='REST_RECEIPT_BEFORE_FETCH_GUARD'&&f.mayHaveExecuted===false&&f.durableRecordingFailed===false&&f.at===guardFailure.at);need(faults.length===1,'AFFIRMATIVE_BEFORE_FETCH');
 const rows=timings.filter(t=>t.kind==='orders'&&Date.parse(t.queuedAt)>=from&&Date.parse(t.queuedAt)<=failed);need(rows.length===1,'UNIQUE_TIMING');const t=rows[0];validateTiming(t);need(t.outcome==='connection_error'&&t.statusCode===null&&t.bodyMs===null&&Date.parse(t.admittedAt)>=fenced&&Date.parse(t.finishedAt)<=failed,'NO_HTTP_RESPONSE');
 need(stop.kind==='generic_financial_uncertainty'&&stop.sourceHash===o.bodyHash&&stop.ownerId===state.scope.ownerId&&stop.runId===state.scope.runId&&stop.allWritersStop===true,'EXACT_STOP');
 return{schemaVersion:1,kind:'affirmative_physical_guard_block_audit_v1',intentBodyHash:o.bodyHash,eventHash:payloadHash(tail),timingHash:payloadHash(t),diagnosticHash:payloadHash(faults[0]),guardFailureHash:payloadHash(guardFailure),stopHash:payloadHash(stop),expired:true,recordedPhysicalFetches:0,physicalFetchAffirmativelyRejected:true,executionAuthority:false,clearsStop:false};
}
