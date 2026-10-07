import {amount,money,price,TICK} from '../paper/fixed.mjs';
// A dated depth-based counterfactual value, never a fill, payout or order permit.
export function valueNoLiquidationDepthV1({quantity,remainingCost,yesAsks,slippageTicks=0}){
 if(!Number.isSafeInteger(quantity)||quantity<1||quantity>100||!Number.isSafeInteger(slippageTicks)||slippageTicks<0||slippageTicks>10||!Array.isArray(yesAsks)||yesAsks.length<1||yesAsks.length>20)throw Error('NO_DEPTH_VALUE_INPUT');
 const cost=amount(remainingCost);if(cost<0n)throw Error('NO_DEPTH_VALUE_COST');let previous=-1n,left=quantity,proceeds=0n;const consumed=[];
 for(const level of yesAsks){const ask=price(level.price);if(ask<=previous||ask<5000n||ask>995000n||!Number.isSafeInteger(level.quantity)||level.quantity<1)throw Error('NO_DEPTH_VALUE_LEVEL');previous=ask;
  const noBid=1000000n-ask-BigInt(slippageTicks)*TICK;if(left&&noBid>=5000n){const used=Math.min(left,level.quantity);proceeds+=BigInt(used)*noBid;left-=used;consumed.push({quantity:used,noPrice:money(noBid)});}
 }
 const valuedQuantity=quantity-left,complete=left===0;
 return{schemaVersion:1,kind:'observed_NO_depth_counterfactual_value_v1',quantity,valuedQuantity,unvaluedQuantity:left,complete,proceeds:money(proceeds),remainingCost,unrealizedPnl:complete?money(proceeds-cost):null,fees:'0.000000',slippageTicks,consumed,executionAuthority:false,actualFill:false};
}

