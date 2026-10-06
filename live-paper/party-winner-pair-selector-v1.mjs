// Read REQUEST proposal only. Caller owns persistence, source plan and collector.
import {payloadHash} from '../execution/ledger.mjs';
import {amount,instant} from '../paper/fixed.mjs';
export const PARTY_PAIR_SELECTOR_POLICY=Object.freeze({negativeCooldownMs:300000,requestLifetimeMs:900000,maximumPlanLifetimeMs:1800000,quoteMaximumAgeMs:20000,maximumSkewMs:10000,accountMaximumAgeMs:20000,maximumProtectiveCost:'0.990000',protectedBookSlots:4,maximumAuxiliarySlots:2,maximumRetainedAttempts:512});
const need=(x,c)=>{if(!x)throw Error('PARTY_PAIR_SELECTOR_'+c);};
const hex=s=>typeof s==='string'&&/^[a-f0-9]{64}$/.test(s),id=s=>typeof s==='string'&&/^[1-9]\d*$/.test(s)&&Number.isSafeInteger(Number(s));
const name=s=>typeof s==='string'&&/^[a-z][a-z0-9-]{1,63}$/.test(s);
const pairKey=ids=>{need(Array.isArray(ids)&&ids.length===2&&new Set(ids).size===2&&ids.every(id),'PAIR');return [...ids].sort((a,b)=>Number(a)-Number(b)).join(':');};
const terminal=phase=>['complete','failed','interrupted'].includes(phase);
export function createPartyPairSelectorState(universeHash){need(hex(universeHash),'UNIVERSE_HASH');return{schemaVersion:1,universeHash,policyHash:payloadHash(PARTY_PAIR_SELECTOR_POLICY),nextSequence:0,attempts:{},handledTerminals:{},cooldowns:{},handledSources:{}};}
function validateState(s,u){need(s?.schemaVersion===1&&s.universeHash===u&&s.policyHash===payloadHash(PARTY_PAIR_SELECTOR_POLICY)&&Number.isSafeInteger(s.nextSequence)&&s.nextSequence>=0&&[s.attempts,s.handledTerminals,s.cooldowns,s.handledSources].every(x=>x&&typeof x==='object'&&!Array.isArray(x)),'STATE');return structuredClone(s);}
function planCurrent(p,universe,now){try{need(p?.schemaVersion===1&&p.owner==='Codex'&&name(p.id)&&p.mode==='allowed_universe_pair_read_candidates'&&p.universeHash===universe.hash&&p.protectedBookSlots===4&&p.maximumAuxiliarySlots===2,'SOURCE_PLAN');const start=instant(p.createdAt),end=instant(p.expiresAt);need(start<=now&&now<end&&end-start>0&&end-start<=PARTY_PAIR_SELECTOR_POLICY.maximumPlanLifetimeMs,'SOURCE_PLAN_WINDOW');return true;}catch{return false;}}
function currentAccount(a,tid,now){try{need(a?.complete===true&&a.tournamentId===tid&&a.unknownAcceptance===false&&Array.isArray(a.excludedExchangeIds)&&a.excludedExchangeIds.every(id)&&hex(a.sourceHash),'ACCOUNT');const known=instant(a.availableAt);return known<=now&&[a.positionsAt,a.ordersAt].every(t=>instant(t)<=known&&now-instant(t)<=20000);}catch{return false;}}
function freshPrices(p,now){try{need(p.indicativeMathValid&&Array.isArray(p.quoteClocks)&&p.quoteClocks.length===2,'QUOTES');const starts=p.quoteClocks.map(q=>instant(q.requestStartedAt)),ends=p.quoteClocks.map(q=>instant(q.receivedAt));return p.quoteClocks.every((q,i)=>starts[i]<=ends[i]&&ends[i]<=instant(q.knownAt)&&instant(q.knownAt)<=now&&now-starts[i]<=20000)&&Math.max(...ends)-Math.min(...starts)<=10000;}catch{return false;}}
export function selectPartyWinnerPairCapture({universe,screen,account,state,sourcePlan=null,currentRequest=null,capture=null,terminalRecords=[],boundPlanHash=null,collectorCanRead=true,now}){
 const at=instant(now),s=validateState(state||createPartyPairSelectorState(universe.hash),universe.hash);
 need(screen?.schemaVersion===1&&screen.mode==='source_bound_party_winner_conditional_no_read_ranking'&&Array.isArray(screen.pairs)&&instant(screen.evaluatedAt)<=at,'SCREEN');
 // A terminal source ID can be edited; compare digest as well as update time.
 for(const t of terminalRecords){need(typeof t.sourceId==='string'&&t.sourceId.length>0&&t.sourceId.length<=200&&name(t.name)&&terminal(t.phase)&&instant(t.updatedAt)<=at,'TERMINAL_SOURCE');const key=pairKey(t.marketIds),digest=payloadHash(t),terminalKey=payloadHash({sourceId:t.sourceId});if(s.handledTerminals[terminalKey]===digest)continue;s.handledTerminals[terminalKey]=digest;const negative=t.phase!=='complete'||t.positiveConditionalScreen!==true;if(negative)s.cooldowns[key]=Math.max(s.cooldowns[key]||0,instant(t.updatedAt)+300000);if(Object.hasOwn(s.attempts,t.name))Object.assign(s.attempts[t.name],{phase:t.phase,terminalSourceId:t.sourceId,terminalDigest:digest,updatedAt:t.updatedAt});}
 const result=(reason,request=null,selected=null)=>({schemaVersion:1,mode:'party_pair_read_request_proposal_only',reason,request,selected,nextState:s,sourcePlanHash:sourcePlan?payloadHash(sourcePlan):null,protectedBookSlots:4,maximumAuxiliarySlots:2,competitionOrders:0,networkRequests:0,executionAuthority:false});
 if(boundPlanHash!==null){need(hex(boundPlanHash),'BOUND_PLAN_HASH');return result('rest_lifecycle_plan_bound');}
 if(capture&&capture.phase!=='idle'&&!terminal(capture.phase))return result('capture_active_no_flip');
 if(currentRequest&&instant(currentRequest.expiresAt)>at&&(!capture||capture.name!==currentRequest.name))return result('pending_request_no_flip');
 if(Object.entries(s.attempts).some(([name,a])=>a.phase==='proposed'&&instant(a.expiresAt)>at&&!(capture?.name===name&&terminal(capture.phase))))return result('persisted_nomination_pending_no_flip');
 if(capture?.phase==='complete'&&capture.screen?.positiveConditionalScreen===true&&capture.review&&instant(capture.review.expiresAt)>at)return result('positive_capture_awaiting_owner');
 if(!collectorCanRead)return result('collector_unavailable_or_backoff');
 if(!currentAccount(account,universe.tournamentId,at))return result('current_account_unavailable');
 const excluded=new Set(account.excludedExchangeIds),scope=new Map(universe.exchanges.map(e=>[e.marketId,e.exchangeId]));
 const candidates=screen.pairs.filter(p=>p.ordinaryResolutionOnly===true&&p.orderEligible===false&&p.executionAuthority===false&&p.depthKnown===false&&p.marketIds.length===2&&p.exchangeIds.every((x,i)=>scope.get(p.marketIds[i])===x&&!excluded.has(x))&&freshPrices(p,at)&&p.protectiveTickCost!==null&&amount(p.protectiveTickCost)>0n&&amount(p.protectiveTickCost)<=990000n).sort((a,b)=>amount(a.protectiveTickCost)<amount(b.protectiveTickCost)?-1:amount(a.protectiveTickCost)>amount(b.protectiveTickCost)?1:a.id.localeCompare(b.id));
 for(const p of candidates){const key=pairKey(p.marketIds);if((s.cooldowns[key]||0)>at)continue;
  const fingerprint=payloadHash({eventKey:p.eventKey,rootHashes:p.rootHashes,quoteClocks:p.quoteClocks.map(q=>({requestStartedAt:q.requestStartedAt,receivedAt:q.receivedAt})),rawNoAsks:p.rawNoAsks,protectiveTickCost:p.protectiveTickCost});
  if(s.handledSources[fingerprint])continue;
  const selected={pairId:p.id,marketIds:[...p.marketIds],exchangeIds:[...p.exchangeIds],protectiveTickCost:p.protectiveTickCost,conditionalOrdinarySurplus:p.conditionalProtectiveSurplus,rootsCurrent:p.rootsCurrent,metadataRefreshRequired:!p.rootsCurrent,depthUnknown:true,sourceFingerprint:fingerprint,accountSourceHash:account.sourceHash,accountAfterCachedBulkAllowedForReadHintOnly:true,accountBeforeFutureBookPacketRequired:true,orderEligible:false};
  if(!planCurrent(sourcePlan,universe,at))return result('bounded_source_plan_required_or_expired',null,selected);
  const sequence=++s.nextSequence,requestName='paired-screen-'+payloadHash({sourcePlanHash:payloadHash(sourcePlan),fingerprint,sequence}).slice(0,40),expires=Math.min(at+900000,instant(sourcePlan.expiresAt));
  const request={schemaVersion:1,owner:'Codex',name:requestName,marketIds:[...p.marketIds],createdAt:now,expiresAt:new Date(expires).toISOString()};
  s.attempts[requestName]={sequence,pairKey:key,sourceFingerprint:fingerprint,sourcePlanHash:payloadHash(sourcePlan),createdAt:now,expiresAt:request.expiresAt,phase:'proposed'};s.handledSources[fingerprint]=requestName;
  const names=Object.keys(s.attempts).sort((a,b)=>s.attempts[a].sequence-s.attempts[b].sequence);for(const old of names.slice(0,Math.max(0,names.length-512))){delete s.handledSources[s.attempts[old].sourceFingerprint];delete s.attempts[old];}
  return result('propose_one_metadata_then_depth_capture',request,selected);
 }
 return result(candidates.length?'positive_candidates_cooling_down_or_deduplicated':'no_current_unowned_positive_pair');
}
