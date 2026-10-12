// Complete requested history is not necessarily caught-up reporting history.
// This pure tracker releases no cash and performs no network or order action.
import {instant} from './fixed.mjs';import {instantNs} from '../feed/contract.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const need=(v,c)=>{if(!v)throw Error('MAKER_PROJECTION_'+c);};
const seq=v=>{need(typeof v==='string'&&/^(0|[1-9]\d*)$/.test(v),'SEQUENCE');return BigInt(v);};
const scopeCheck=(s,x)=>need(x.marketId===s.scope.marketId&&x.exchangeId===s.scope.exchangeId&&x.tournamentId===s.scope.tournamentId,'SCOPE');
export function createMakerProjectionBarrierV1({scope,activationAt,endAt}) {
  need(typeof scope?.tournamentId==='string'&&scope.tournamentId.length>0&&
    ['marketId','exchangeId'].every(k=>typeof scope[k]==='string'&&/^[1-9]\d*$/.test(scope[k])),'SCOPE');
  need(instant(endAt)>instant(activationAt),'ACTIVE_WINDOW');
  return {schemaVersion:1,scope:structuredClone(scope),activationAt,endAt,barrier:null,pending:{},windows:[]};
}
export function observeMakerDeadlineBookV1(state,book) {
  scopeCheck(state,book);const s=structuredClone(state),deadline=instant(s.endAt),sequence=seq(book.sequence);
  need(instant(book.receivedAt)>=instant(book.requestStartedAt),'RECEIPT');
  // A fresh authoritative request AND engine source clock must be after the
  // effective expiry/cancellation deadline. A decision-time version is not it.
  if(instant(book.requestStartedAt)<deadline||instantNs(book.sourceAt)<BigInt(deadline)*1000000n)return s;
  if(s.barrier!==null)need(sequence>=seq(s.barrier.sequence),'BOOK_ROLLBACK');
  if(s.barrier===null)s.barrier={sequence:book.sequence,requestStartedAt:book.requestStartedAt,
    receivedAt:book.receivedAt,sourceAt:book.sourceAt,bookHash:payloadHash(book)};
  return s;
}
export function observeMakerProjectionPageV1(state,page) {
  scopeCheck(state,page);const s=structuredClone(state),from=instant(page.from),to=instant(page.to);
  need(from<to&&instant(page.requestStartedAt)>=to&&instant(page.receivedAt)>=instant(page.requestStartedAt),'PAGE_CLOCK');
  need(typeof page.coverage?.complete==='boolean'&&typeof page.pagination?.hasMore==='boolean','COVERAGE');
  const checkpoint=page.coverage.projectedThroughSequence;
  need(checkpoint===null||Number.isSafeInteger(checkpoint)&&checkpoint>=0,'CHECKPOINT');
  const request=page.requestCursor;need(request===null||typeof request==='string'&&request.length>0,'CURSOR');
  const key=payloadHash([page.from,page.to]);let chain=request===null?{from:page.from,to:page.to,expected:null,
    complete:true,checkpoint:null,seen:[]}:s.pending[key];
  if(!chain||chain.expected!==request){delete s.pending[key];return s;}
  chain.complete=chain.complete&&page.coverage.complete&&checkpoint!==null;
  if(checkpoint!==null)chain.checkpoint=chain.checkpoint===null?String(checkpoint):String(BigInt(chain.checkpoint)<BigInt(checkpoint)?chain.checkpoint:checkpoint);
  if(page.pagination.hasMore){
    const next=page.pagination.nextCursor;need(typeof next==='string'&&next.length>0&&next!==request&&!chain.seen.includes(next),'CURSOR_CYCLE');
    chain.seen.push(next);chain.expected=next;s.pending[key]=chain;need(Object.keys(s.pending).length<=16,'PENDING_LIMIT');
  }else{
    need(page.pagination.nextCursor===null,'FINAL_CURSOR');
    if(chain.complete)s.windows.push({from:chain.from,to:chain.to,checkpoint:chain.checkpoint});delete s.pending[key];
  }
  // Exact duplicate evidence does not grow the tracker forever.
  s.windows=[...new Map(s.windows.map(w=>[payloadHash(w),w])).values()];
  return s;
}
export function makerProjectionRetirementV1(state) {
  if(state.barrier===null)return {canRetire:false,reason:'post_deadline_book_required',cashReleaseAuthority:false};
  let through=instant(state.activationAt);const required=seq(state.barrier.sequence);
  for(const w of state.windows.filter(w=>seq(w.checkpoint)>=required).sort((a,b)=>instant(a.from)-instant(b.from))){
    if(instant(w.from)>through)break;through=Math.max(through,instant(w.to));
    if(through>=instant(state.endAt))return {canRetire:true,reason:'deadline_and_caught_up_window_verified',
      barrierSequence:state.barrier.sequence,cashReleaseAuthority:false};
  }
  return {canRetire:false,reason:'caught_up_contiguous_tape_required',barrierSequence:state.barrier.sequence,cashReleaseAuthority:false};
}
