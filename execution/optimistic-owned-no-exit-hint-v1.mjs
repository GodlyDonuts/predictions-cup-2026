import {payloadHash} from './core.mjs';import {partyWinnerQuoteHash} from '../live-paper/party-winner-no-screen-v1.mjs';import {normalizeOfficialFillTick} from './api-tick-price.mjs';import {amount,money,price,instant,SCALE,TICK} from '../paper/fixed.mjs';
// An advisory rejection bound only. It never grants execution or assumes depth.
export function optimisticOwnedNoExitHintV1({scope,universeHash,hints,slot,realizedProfit,now}){
 const base={defer:false,executionAuthority:false,networkRequests:0,requiresFreshDepthBeforeOrders:true};
 try{
  const at=instant(now),p=slot?.proof,{verificationHash,...proof}=p??{};if(p?.scopedTradeVerified!==true||verificationHash!==payloadHash(proof)||p.ownerId!==scope.ownerId||p.runId!==scope.runId||p.tournamentId!==scope.tournamentId||!Number.isSafeInteger(p.remainingOwnedQuantity)||p.remainingOwnedQuantity<1||!Array.isArray(p.remainingLots)||!p.remainingLots.length)throw Error('OWN_PROOF');
  if(hints?.tournamentId!==scope.tournamentId||hints.universeHash!==universeHash||!Array.isArray(hints.quoteCaptures))throw Error('ENVELOPE');const rows=hints.quoteCaptures.filter(q=>q.exchangeId===p.exchangeId);if(rows.length!==1)throw Error('UNIQUE_QUOTE');const q=rows[0];if(q.marketId!==p.marketId||q.tournamentId!==scope.tournamentId||q.option!=='YES'||q.crossed===true||!(/^[a-f0-9]{64}$/.test(q.sourceHash??''))||q.quoteHash!==partyWinnerQuoteHash(q))throw Error('QUOTE_SCOPE_HASH');
  const start=instant(q.requestStartedAt),received=instant(q.receivedAt),known=instant(q.knownAt);if(start>received||received>known||known>at||at-start>20000)throw Error('QUOTE_CLOCK');const yesBid=amount(normalizeOfficialFillTick(q.bestBid).price),yesAsk=amount(normalizeOfficialFillTick(q.bestAsk).price);if(yesBid>=yesAsk)throw Error('CROSSED');
  const optimisticNoBid=SCALE-yesAsk,firstCost=price(p.remainingLots[0].entryPrice),upper=amount(realizedProfit)+optimisticNoBid-firstCost;
  return{...base,defer:upper<TICK,reason:upper<TICK?'FIRST_FIFO_SHARE_FAILS_EVEN_ZERO_SLIPPAGE_BOUND':'DEPTH_CHECK_STILL_REQUIRED',ageMs:at-start,optimisticNoBid:money(optimisticNoBid),firstFifoCost:money(firstCost),optimisticCohortAfterOneShare:money(upper),minimumCohortProfit:money(TICK),quoteHash:q.quoteHash,ownedProofHash:verificationHash};
 }catch{return{...base,reason:'HINT_UNAVAILABLE_OR_UNTRUSTWORTHY'};}
}
