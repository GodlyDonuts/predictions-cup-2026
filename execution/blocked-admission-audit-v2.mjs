import {payloadHash} from './core.mjs';import {validateTiming} from '../live-paper/read-timing.mjs';
const need=(v,c)=>{if(!v)throw Error('BLOCKED_ADMISSION_'+c);};
// Historical evidence only. This does not close the intent or clear a STOP.
export function auditBlockedAdmissionV2({state,events,timings,now}){
 const active=state.orders.filter(o=>!o.proof&&o.phase!=='abandoned');need(active.length===1,'ONE_INTENT');const o=active[0];
 need(o.phase==='uncertain'&&o.operation==='place'&&o.orderId===null&&o.ack===null&&!o.proof&&!o.cancelPending&&o.fetchStarts.length===0&&o.fences.length===1&&o.receipts.length===0,'NO_PHYSICAL_OR_ACCEPTANCE_EVIDENCE');
 need(o.bodyHash===payloadHash(o.body)&&Date.parse(o.body.expirationDate)<now,'EXACT_EXPIRED_BODY');
 const tail=events.slice(-3);need(tail.length===3&&tail[0].kind==='admitted'&&tail[0].operation==='place'&&tail[1].kind==='network_fence'&&tail[1].operation==='place'&&tail[2].kind==='error'&&tail[2].code==='EXECUTION_AUTHORITY_UNAVAILABLE','EXACT_TERMINAL_GATE_PATH');
 const from=Date.parse(o.createdAt),admitted=Date.parse(tail[0].at),fenced=Date.parse(tail[1].at),failed=Date.parse(tail[2].at);need(from<=admitted&&admitted<=fenced&&fenced<=failed,'CLOCKS');
 const rows=timings.filter(t=>t.kind==='orders'&&Date.parse(t.queuedAt)>=from&&Date.parse(t.queuedAt)<=failed);need(rows.length===1,'UNIQUE_ORDER_TIMING');const t=rows[0];validateTiming(t);
 need(t.outcome==='blocked'&&t.admittedAt===null&&t.serviceMs===null&&t.statusCode===null&&Date.parse(t.finishedAt)>=fenced&&Date.parse(t.finishedAt)<=failed,'BEFORE_PHYSICAL_CLIENT_FETCH');
 return{schemaVersion:1,kind:'historical_blocked_admission_audit_v2',intentBodyHash:o.bodyHash,eventHash:payloadHash(tail),timingHash:payloadHash(t),expired:true,recordedPhysicalFetches:0,recordedNetworkFences:1,executionAuthority:false,clearsStop:false};
}

