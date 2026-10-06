import {amount,price,money,SCALE,TICK,instant} from '../paper/fixed.mjs';
import {normalizeOfficialFillTick} from './api-tick-price.mjs';

// Bulk quotes choose which owned book to read first. They never authorize an
// order or prove depth, a fill, settlement value, or realized profit.
export function rankBaalOwnedExitReadHintsV1({scope,universeHash,owned,hints,now,
 maximumSharesPerOrder=100,maximumOrderCost='25.000000'}) {
 const fallback=()=>({ranked:[],orderEligible:false,executionAuthority:false});
 try {
  if(!scope||hints?.tournamentId!==scope.tournamentId||hints.universeHash!==universeHash
   ||!Array.isArray(hints.quoteCaptures)||!Array.isArray(owned))return fallback();
  const cap=amount(maximumOrderCost),ranked=[];
  if(!Number.isSafeInteger(maximumSharesPerOrder)||maximumSharesPerOrder<1||maximumSharesPerOrder>100
   ||cap<=0n||cap>25000000n)return fallback();
  for(const slot of owned) {
   const p=slot.proof;
   if(!p?.remainingOwnedQuantity||!Array.isArray(p.remainingLots))continue;
   const quotes=hints.quoteCaptures.filter(q=>q.exchangeId===p.exchangeId&&q.marketId===p.marketId&&q.option==='YES');
   if(quotes.length!==1)continue;
   const q=quotes[0];
   if(q.tournamentId!==undefined&&q.tournamentId!==scope.tournamentId||q.crossed!==false
    ||!/^[a-f0-9]{64}$/.test(q.sourceHash??''))continue;
   const start=instant(q.requestStartedAt),end=instant(q.receivedAt);
   const known=q.knownAt===undefined?end:instant(q.knownAt);
   if(start>end||end>known||known>now||now-start>20000)continue;
   const bid=SCALE-price(normalizeOfficialFillTick(q.bestAsk).price);
   if(bid<TICK||bid>SCALE-TICK)continue;
   let remaining=Math.min(p.remainingOwnedQuantity,maximumSharesPerOrder,Number(cap/bid));
   let quantity=0,principal=0n;
   for(const lot of p.remainingLots) {
    if(!remaining)break;
    if(!Number.isSafeInteger(lot.quantity)||lot.quantity<=0){quantity=0;break;}
    const cost=price(lot.entryPrice);
    if(bid<cost+TICK)break;
    const take=Math.min(remaining,lot.quantity);quantity+=take;remaining-=take;
    principal+=cost*BigInt(take);
   }
   if(!quantity)continue;
   ranked.push({exchangeId:p.exchangeId,marketId:p.marketId,maximumHypotheticalQuantity:quantity,
    quotedNoBid:money(bid),upperBoundProfitWithoutDepth:money(bid*BigInt(quantity)-principal),
    quoteReceivedAt:q.receivedAt,sourceHash:q.sourceHash});
  }
  ranked.sort((a,b)=>amount(a.upperBoundProfitWithoutDepth)>amount(b.upperBoundProfitWithoutDepth)?-1:
   amount(a.upperBoundProfitWithoutDepth)<amount(b.upperBoundProfitWithoutDepth)?1:a.exchangeId.localeCompare(b.exchangeId));
  return {ranked,priority:'largest_quoted_fifo_gain_first',depthVerified:false,actualProfitClaim:false,
   orderEligible:false,executionAuthority:false};
 }catch{return fallback();}
}
