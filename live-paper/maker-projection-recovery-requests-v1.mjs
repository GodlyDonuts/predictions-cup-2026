// Map the new ledger explicitly into the EXISTING recovery request contract.
// Mapping neither schedules a request nor releases a quote/cash reservation.
import {instant} from '../paper/fixed.mjs';import {instantNs} from '../feed/contract.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {validateQuoteRecoveryRequest} from './quote-projection-recovery.mjs';
const need=(v,c)=>{if(!v)throw Error('MAKER_RECOVERY_MAP_'+c);};
const millis=v=>typeof v==='number'&&Number.isSafeInteger(v)?v:instant(v);
export function makerProjectionRecoveryRequestsV1({quotes,universe,at}) {
  const now=instant(at);need(Array.isArray(quotes)&&Array.isArray(universe?.exchanges)&&typeof universe.tournamentId==='string','INPUT');
  const requests=new Map(),blocked=[];
  for(const q of quotes){
    if(q.closedAt)continue;
    const label={quoteId:String(q.id),marketId:q.marketId,exchangeId:q.exchangeId};
    const p=q.projection,b=p?.barrier;
    if(!b){blocked.push({...label,reason:'post_deadline_engine_barrier_missing'});continue;}
    need(p.scope.tournamentId===universe.tournamentId&&p.scope.marketId===q.marketId&&p.scope.exchangeId===q.exchangeId,'SCOPE_BINDING');
    const activation=millis(q.activationAt),end=Math.min(millis(q.expiresAt),q.cancelEffectiveAt===undefined?Infinity:millis(q.cancelEffectiveAt));
    need(instant(p.activationAt)===activation&&instant(p.endAt)===end,'LIFETIME_BINDING');
    if(end>now){blocked.push({...label,reason:'quote_not_ended'});continue;}
    if(end<=activation){blocked.push({...label,reason:'invalid_active_window'});continue;}
    need(instant(b.requestStartedAt)>=end&&instant(b.receivedAt)>=instant(b.requestStartedAt)&&
      instantNs(b.sourceAt)>=BigInt(end)*1000000n&&typeof b.bookHash==='string'&&/^[a-f0-9]{64}$/.test(b.bookHash),'DEADLINE_BOOK');
    const body={schemaVersion:1,kind:'expired_paper_quote_coverage',marketId:q.marketId,exchangeId:q.exchangeId,
      from:new Date(activation).toISOString(),to:new Date(end).toISOString(),requiredCheckpoint:b.sequence};
    const request={...body,key:payloadHash(body)};
    // Unsupported stale/oversized windows are reported, not silently erased.
    try{validateQuoteRecoveryRequest(request,universe,now);}catch(error){blocked.push({...label,reason:'existing_recovery_contract_rejected',code:error.message});continue;}
    requests.set(request.key,request);
  }
  return {schemaVersion:1,at,requests:[...requests.values()].sort((a,b)=>instant(a.from)-instant(b.from)||a.key.localeCompare(b.key)),blocked,
    truncated:false,networkRequests:0,executionAuthority:false,cashReleaseAuthority:false};
}
