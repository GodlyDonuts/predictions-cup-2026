import {valueNoLiquidationDepthV1} from '../execution/no-liquidation-depth-value-v1.mjs';
// A future dated depth value for an actual purchase; never a simulated fill.
export function entryHorizonObservationV1({entry,book,horizonMinutes,maximumDelayMs=300000}){
 const target=Date.parse(entry.fillAt)+horizonMinutes*60000,start=Date.parse(book.requestStartedAt),end=Date.parse(book.receivedAt);
 if(book.exchangeId!==entry.exchangeId||book.marketId!==entry.marketId||start<target||end<start||start-target>maximumDelayMs)return null;
 return{horizonMinutes,requestStartedAt:book.requestStartedAt,receivedAt:book.receivedAt,delayAfterHorizonMs:start-target,exactObservedDepth:valueNoLiquidationDepthV1({quantity:entry.quantity,remainingCost:entry.cost,yesAsks:book.asks,slippageTicks:0}),oneTickStress:valueNoLiquidationDepthV1({quantity:entry.quantity,remainingCost:entry.cost,yesAsks:book.asks,slippageTicks:1}),actualSale:false};
}
