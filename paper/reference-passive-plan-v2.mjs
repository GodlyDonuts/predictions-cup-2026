import {amount, price, money, SCALE, TICK} from './fixed.mjs';
import {referenceKellySizingV1} from './reference-kelly-sizing-v1.mjs';

// Joining a one-tick book is possible; improving it would cross the ask.
// This is a quote proposal, never queue-priority or fill evidence.
export function referencePassivePlanV2({book, side, sizing, minimumGap, reentryMaximumLimit=null}) {
  if (!['YES','NO'].includes(side) || !book.bids?.length || !book.asks?.length || typeof book.versionKey !== 'string') throw Error('REFERENCE_PASSIVE_SCOPE');
  const bid=side==='YES'?price(book.bids[0].price):SCALE-price(book.asks[0].price);
  const ask=side==='YES'?price(book.asks[0].price):SCALE-price(book.bids[0].price);
  if (bid>=ask) return {quantity:0,reason:'CROSSED_OR_LOCKED_BOOK',actualOrders:0};
  const limit=bid+TICK<ask?bid+TICK:bid;
  if (limit<TICK || limit>SCALE-TICK || amount(sizing.referenceValue)-limit<amount(minimumGap) || reentryMaximumLimit!==null && limit>amount(reentryMaximumLimit)) return {quantity:0,reason:'NO_POST_ONLY_REFERENCE_GAP',actualOrders:0};
  const levels=side==='YES'?book.bids:book.asks;
  const depth=levels.reduce((n,l)=>{
    if (!Number.isSafeInteger(l.quantity)||l.quantity<1) throw Error('REFERENCE_PASSIVE_DEPTH');
    const next=n+Math.floor(l.quantity/100);
    if (!Number.isSafeInteger(next)) throw Error('REFERENCE_PASSIVE_DEPTH_OVERFLOW');
    return next;
  },0);
  const target=referenceKellySizingV1({...sizing,protectedPrice:money(limit),depthQuantity:depth});
  return {...target,limit:money(limit),availableDepth:depth,quotePlacement:limit===bid?'join_bid':'improve_bid',postOnlyAtObservation:true,queuePriorityKnown:false,quoteSizeDepthIsNotFillEvidence:true,judgmentExpectedSurplus:money(BigInt(target.quantity)*(amount(sizing.referenceValue)-limit)),actualOrders:0};
}
