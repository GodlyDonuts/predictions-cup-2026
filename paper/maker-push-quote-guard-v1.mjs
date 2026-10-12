// Consume ONLY a view produced by the existing verified RealtimeBooks bridge.
// Capture readiness is not account or execution authority. Withdrawals are
// requests for the host's normal cancel path, not evidence of cancellation.
import {instantNs} from '../feed/contract.mjs';
import {instant,price} from './fixed.mjs';
import {payloadHash} from '../execution/ledger.mjs';
export function makerPushQuoteGuardV1({view,scope=null,at,openQuotes,maximumBookAgeMs=10000}) {
  const need=(v,c)=>{if(!v)throw Error('MAKER_PUSH_GUARD_'+c);};
  const now=instant(at),nowNs=BigInt(now)*1000000n;
  const unavailable=view===null;
  if(unavailable){
    need(typeof scope?.marketId==='string'&&/^[1-9]\d*$/.test(scope.marketId)&&
      typeof scope.exchangeId==='string'&&/^[1-9]\d*$/.test(scope.exchangeId),'UNAVAILABLE_SCOPE');
    // RealtimeHost.view returns null while stopped. This is explicitly missing
    // evidence, not a fabricated ready snapshot or an invented REST receipt.
    view={mode:'read_only_market_capture',executionAuthorized:false,...scope,books:[],reasons:['host_unavailable'],
      connectionId:null,epoch:0,revision:null,ready:false,state:'unavailable'};
  }
  need(Number.isSafeInteger(maximumBookAgeMs)&&maximumBookAgeMs>0&&maximumBookAgeMs<=20000,'POLICY');
  need(view?.mode==='read_only_market_capture'&&view.executionAuthorized===false&&
    typeof view.marketId==='string'&&/^[1-9]\d*$/.test(view.marketId)&&
    typeof view.exchangeId==='string'&&/^[1-9]\d*$/.test(view.exchangeId)&&Array.isArray(view.books)&&
    Array.isArray(view.reasons)&&Array.isArray(openQuotes),'VIEW');
  const affected=openQuotes.filter(q=>q.marketId===view.marketId);
  const ids=new Set();for(const q of affected){need(typeof q.quoteId==='string'&&q.quoteId.length>0&&!ids.has(q.quoteId),'QUOTE_ID');ids.add(q.quoteId);}
  const reasons=[];let book=null;
  if(view.ready!==true||view.state!=='ready'||typeof view.connectionId!=='string'||!view.connectionId||
    !Number.isSafeInteger(view.epoch)||view.epoch<1)reasons.push('stream_not_ready',...view.reasons);
  else {
    const candidate=view.books.find(b=>b.exchangeId===view.exchangeId);
    if(!candidate)reasons.push('missing_book');
    else {
      const received=instant(candidate.receivedAt),version=candidate.asOf;
      if(!['rest','websocket'].includes(candidate.source)||received>now||now-received>=maximumBookAgeMs)reasons.push('stale_or_unknown_observation');
      else if(!version||!Number.isSafeInteger(version.sequence)||version.sequence<0||instantNs(version.at)>nowNs+1000000000n)reasons.push('invalid_source_version');
      else if(!candidate.bids?.length||!candidate.asks?.length)reasons.push('no_executable_depth');
      else {
        const bid=price(String(candidate.bids[0].price)),ask=price(String(candidate.asks[0].price));
        if(bid>=ask)reasons.push('crossed_book');
        else if(view.nextKnownExpiryAt!==null&&instantNs(view.nextKnownExpiryAt)<=nowNs)reasons.push('known_source_expiry');
        else book=structuredClone(candidate);
      }
    }
  }
  const withdrawQuoteIds=affected.filter(q=>{
    if(!book)return true;
    const stream=q.sourceBinding?.stream;
    return stream!==undefined&&(stream.connectionId!==view.connectionId||stream.epoch!==view.epoch);
  }).map(q=>q.quoteId);
  const out={schemaVersion:1,at,marketId:view.marketId,exchangeId:view.exchangeId,paperProposalsAllowed:book!==null,
    book,viewAvailable:!unavailable,
    streamBinding:{connectionId:view.connectionId,epoch:view.epoch,revision:view.revision,viewHash:unavailable?null:payloadHash(view)},
    withdrawQuoteIds,reasons:[...new Set(reasons)],
    // No requestStartedAt is invented: pushed observations have receivedAt.
    cashReservationReleaseAllowed:false,cancellationConfirmed:false,accountReconciliationRequired:true,
    executionAuthority:false,networkRequests:0,profitabilityEstablished:false};
  return {...out,guardHash:payloadHash(out)};
}
