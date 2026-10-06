// Pure economic selector: one book, one protected limit, contiguous owned FIFO.
// No receipt validation, account adoption, activation or order authority.
import {price,amount,money} from '../paper/fixed.mjs';
import {payloadHash} from './core.mjs';
import {sizeProfitableOwnedNoExitPrefixV3} from './baal-scaled-profitable-fifo-prefix-v3.mjs';
export function selectBaalProfitableExitDepthSweepV1({yesAsks,...sizing}){
 const base={mode:'baal_profitable_exit_depth_sweep_v1',executionAuthority:false,orderEligible:false,actualFillOrRealizedProfit:false};
 if(!Array.isArray(yesAsks)||!yesAsks.length||yesAsks.length>20)return {...base,reason:'INVALID_BOOK_DEPTH',selected:null};
 let prior=-1n,depth=0n;const candidates=[];
 for(const [index,level] of yesAsks.entries()){
  let px;try{px=price(level.price);}catch{return {...base,reason:'INVALID_BOOK_PRICE',selected:null};}
  if(px<=prior||!Number.isSafeInteger(level.quantity)||level.quantity<=0)return {...base,reason:'UNORDERED_DUPLICATE_OR_INVALID_DEPTH',selected:null};
  prior=px;depth+=BigInt(level.quantity);
  // Only 100 shares can enter this policy. Clamp the numeric adapter, retain
  // the full exact aggregate separately, and never fabricate extra liquidity.
  const result=sizeProfitableOwnedNoExitPrefixV3({...sizing,yesAskLevel:{price:level.price,quantity:Number(depth>100n?100n:depth)}});
  if(result.quantity>0)candidates.push({throughLevel:index,displayedAggregateQuantity:depth.toString(),protectedLimit:result.protectedLimit,quantity:result.quantity,orderNotional:result.orderNotional,removedPrincipalIfFilled:result.removedPrincipalIfFilled,protectedNetProfitIfFilled:result.protectedNetProfitIfFilled,consumedPrefixLots:result.consumedPrefixLots});
 }
 candidates.sort((a,b)=>{const delta=amount(b.protectedNetProfitIfFilled)-amount(a.protectedNetProfitIfFilled);return delta>0n?1:delta<0n?-1:b.quantity-a.quantity||Number(price(b.protectedLimit)-price(a.protectedLimit));});
 const selected=candidates[0]??null;
 return {...base,reason:selected?'MAXIMUM_PROTECTED_DOLLAR_GAIN':'NO_PROFITABLE_FIFO_PREFIX',selected,candidates,sourceBookDepthHash:payloadHash(yesAsks),sourceLotsHash:payloadHash(sizing.remainingLots),fees:money(0n),depthFromSingleReceipt:true};
}
