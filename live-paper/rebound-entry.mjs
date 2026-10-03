import {amount,money,TICK,instant} from '../paper/fixed.mjs';
const need=ok=>{if(!ok)throw Error('REBOUND_ENTRY_INVALID');};
export const REBOUND_ENTRY_POLICY=Object.freeze({minimumInitialDiscount:'0.010000',minimumRemainingDiscount:'0.005000',confirmationTicks:1,confirmationLifetimeMs:120000,maximumQuoteGapMs:90000,maximumReceiptLagMs:20000,entryLifetimeMs:60000});
const price=value=>money(BigInt(Math.round(value*1e6)));
export function createReboundWatch(dip){
 need(typeof dip.marketId==='string'&&typeof dip.exchangeId==='string');
 const anchor=amount(dip.reference.anchor),bid=amount(dip.bid),ask=amount(dip.ask);
 need(bid>0n&&ask>bid&&anchor-ask>=amount(REBOUND_ENTRY_POLICY.minimumInitialDiscount));
 instant(dip.availableAt);instant(dip.receivedAt);
 return {marketId:dip.marketId,exchangeId:dip.exchangeId,anchor:money(anchor),status:'watching',troughBid:money(bid),troughAvailableAt:dip.availableAt,
  lastAvailableAt:dip.availableAt,lastReceivedAt:dip.receivedAt,expiresAt:new Date(instant(dip.availableAt)+REBOUND_ENTRY_POLICY.confirmationLifetimeMs).toISOString(),signal:null};
}
export function advanceReboundWatch(w,q,snapshot,availableAt){
 if(w.status!=='watching')return w;
 need(q.marketId===w.marketId&&q.exchangeId===w.exchangeId&&q.option==='YES');
 const at=instant(availableAt),received=instant(snapshot.receivedAt),request=instant(snapshot.requestStartedAt);
 need(request<=received&&received<=at&&at>=instant(w.lastAvailableAt));
 if(at>instant(w.expiresAt)){w.status='confirmation_not_observed';return w;}
 if(received-instant(w.lastReceivedAt)>REBOUND_ENTRY_POLICY.maximumQuoteGapMs||at-received>REBOUND_ENTRY_POLICY.maximumReceiptLagMs){w.status='coverage_gap';return w;}
 if(q.crossed||typeof q.bestBid!=='number'||typeof q.bestAsk!=='number'||q.bestBid<=0||q.bestAsk>=1||q.bestAsk<=q.bestBid){w.status='invalid_quote';return w;}
 if(request<=instant(w.troughAvailableAt))return w;
 w.lastAvailableAt=availableAt;w.lastReceivedAt=snapshot.receivedAt;
 const bid=amount(price(q.bestBid)),ask=amount(price(q.bestAsk));
 if(bid<amount(w.troughBid)){w.troughBid=money(bid);w.troughAvailableAt=availableAt;return w;}
 if(bid<amount(w.troughBid)+BigInt(REBOUND_ENTRY_POLICY.confirmationTicks)*TICK||amount(w.anchor)-ask<amount(REBOUND_ENTRY_POLICY.minimumRemainingDiscount))return w;
 w.status='confirmed';w.signal={marketId:w.marketId,exchangeId:w.exchangeId,availableAt,receivedAt:snapshot.receivedAt,requestStartedAt:snapshot.requestStartedAt,
  troughAvailableAt:w.troughAvailableAt,troughBid:w.troughBid,anchor:w.anchor,limit:money(ask),remainingDiscount:money(amount(w.anchor)-ask),depthVerified:false};
 return w;
}
