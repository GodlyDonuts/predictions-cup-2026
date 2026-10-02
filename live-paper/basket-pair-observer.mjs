import {observeBasket} from './basket-observer.mjs';
// A complete paired read is one economic observation. Validate both receipts
// through the original observer, but do not publish/count the first half pair.
export function observeCompleteBasketPair(input,books,bank){
 if(!Array.isArray(books)||books.length!==2||new Set(books.map(b=>b?.exchangeId)).size!==2||bank.legs.some(l=>!books.some(b=>b?.exchangeId===l.exchangeId&&b.marketId===l.marketId)))throw Error('INCOMPLETE_BASKET_PAIR');
 const ordered=[...books].sort((a,b)=>Date.parse(a.receivedAt)-Date.parse(b.receivedAt));
 const first=observeBasket(input,ordered[0],bank);
 if(['before_cutoff','request_before_invalidation'].includes(first.observation.status))throw Error('BASKET_PAIR_INVALIDATED_INPUT');
 const staged=first.state;
 // Restore only derived economics, never clocks, depth, versions or exclusions.
 // A transient stale old peer does not erase the previous complete observation.
 for(const field of ['positiveAnchor','positivePersistenceCount','lastPairHash','lastObservation'])staged[field]=structuredClone(input[field]);
 for(const field of ['cofreshPairs','apparentPositivePairs','independentPositiveConfirmations'])staged.counts[field]=input.counts[field];
 const second=observeBasket(staged,ordered[1],bank);
 if(['before_cutoff','request_before_invalidation'].includes(second.observation.status))throw Error('BASKET_PAIR_INVALIDATED_INPUT');
 return second;
}
