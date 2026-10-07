import {payloadHash} from './core.mjs';import {inspectScopedRestNoAccount} from './no-rest-sell-reconciliation-v2.mjs';
import {auditBlockedAdmissionV2} from './blocked-admission-audit-v2.mjs';
const need=(v,c)=>{if(!v)throw Error('BLOCKED_RECONCILIATION_'+c);};
const ordered=rows=>[...rows].sort((a,b)=>a.id-b.id);
// No API, writes, lock changes or STOP removal. A successor may import this
// closure only together with sealed original actor and reconciliation receipts.
export function reconcileBlockedAdmissionV2({state,events,timings,after,at}){
 const now=Date.parse(at);need(Number.isFinite(now),'CLOCK');const audit=auditBlockedAdmissionV2({state,events,timings,now}),o=state.orders.find(x=>!x.proof&&x.phase!=='abandoned'),member=state.scope.exchanges.find(x=>x.exchangeId===o.body.exchangeId);need(member,'SCOPE');
 const binding={...state.scope,...member},before=inspectScopedRestNoAccount({account:o.before,binding,readAt:o.createdAt}),current=inspectScopedRestNoAccount({account:after,binding,readAt:at});
 const receipts=[after.cash,after.positions,...after.openOrdersPages,...after.scopedOrdersPages];need(receipts.every(r=>Date.parse(r.requestStartedAt)>Date.parse(o.body.expirationDate)&&now-Date.parse(r.requestStartedAt)<=20000),'FRESH_AFTER_EXPIRY');
 const target=a=>a.positions.filter(p=>p.exchangeId===member.exchangeId);need(payloadHash(target(before))===payloadHash(target(current)),'TARGET_POSITION_CHANGED');
 need(payloadHash(ordered(before.scopedOrders))===payloadHash(ordered(current.scopedOrders)),'SCOPED_HISTORY_CHANGED');
 const next=structuredClone(state),closed=next.orders.find(x=>x.intentId===o.intentId);closed.phase='abandoned';closed.abandonedAt=at;closed.abandonedReason='expired_blocked_before_physical_fetch_reconciled';closed.blockedAdmissionAudit=audit;
 return{state:next,proof:{schemaVersion:1,kind:'blocked_admission_scoped_reconciliation_v2',originalEconomicStateHash:payloadHash(state),closedEconomicStateHash:payloadHash(next),auditHash:payloadHash(audit),afterHash:payloadHash(after),at,executionAuthority:false,clearsStop:false}};
}

