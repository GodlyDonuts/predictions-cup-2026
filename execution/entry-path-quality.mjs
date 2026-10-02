import {amount,TICK,instant} from '../paper/fixed.mjs';import {instantNs} from '../feed/contract.mjs';
export const ENTRY_PATH_WINDOW_MS=600000;
// A causal description of observed bids, never a probability of recovery.
export function entryPathQuality({books,invalidations=[],entry,at,windowMs=ENTRY_PATH_WINDOW_MS}){
 const now=instant(at),stop=Math.max(now-windowMs,...invalidations.filter(x=>x.marketId===null||x.marketId===entry.marketId).map(x=>instant(x.at)).filter(t=>t<=now));
 const candidates=books.filter(b=>b.marketId===entry.marketId&&b.exchangeId===entry.exchangeId&&instant(b.receivedAt)<=now&&instant(b.receivedAt)>stop
  &&instant(b.requestStartedAt)<=instant(b.receivedAt)).sort((a,b)=>instant(a.receivedAt)-instant(b.receivedAt));
 const path=[];for(const b of candidates){const prior=path.at(-1);if(prior&&(b.sequence<=prior.sequence||instantNs(b.sourceAt)<=instantNs(prior.sourceAt)||instant(b.receivedAt)<=instant(prior.receivedAt)))continue;
  amount(b.bids[0].price);amount(b.asks[0].price);path.push(b);}
 const current=path.find(b=>b.versionKey===entry.bookVersion&&b.receivedAt===entry.bookReceivedAt);
 if(!current)return {available:false,reason:'exact_entry_book_not_in_verified_window',observations:path.length,profitabilityEstablished:false};
 const a=path.filter(b=>instant(b.receivedAt)<=instant(current.receivedAt)),last=a.at(-1),previous=a.at(-2),bid=amount(last.bids[0].price),ask=amount(last.asks[0].price),max=a.reduce((p,b)=>amount(b.bids[0].price)>p?amount(b.bids[0].price):p,bid);
 const gaps=a.slice(1).map((b,i)=>instant(b.receivedAt)-instant(a[i].receivedAt));
 return {available:true,reason:'discrete_verified_pre_entry_books',observations:a.length,spanMs:instant(last.receivedAt)-instant(a[0].receivedAt),maximumObservedGapMs:gaps.length?Math.max(...gaps):null,
  latestIntervalMs:previous?instant(last.receivedAt)-instant(previous.receivedAt):null,latestBidFalling:previous?bid<amount(previous.bids[0].price):null,
  latestBidChangeTicks:previous?Number((bid-amount(previous.bids[0].price))/TICK):null,drawdownFromObservedPeakTicks:Number((max-bid)/TICK),spreadTicks:Number((ask-bid)/TICK),
  firstBid:a[0].bids[0].price,latestBid:last.bids[0].price,latestAsk:last.asks[0].price,firstReceiptAt:a[0].receivedAt,lastReceiptAt:last.receivedAt,
  quoteAgeAtPreparationMs:now-instant(last.receivedAt),contextInterrupted:stop>now-windowMs,wellSampledContext:a.length>=4&&gaps.every(x=>x>=1000&&x<=180000),
  path:a.map(b=>({receivedAt:b.receivedAt,requestStartedAt:b.requestStartedAt,versionKey:b.versionKey,bid:b.bids[0].price,ask:b.asks[0].price})),profitabilityEstablished:false};
}
