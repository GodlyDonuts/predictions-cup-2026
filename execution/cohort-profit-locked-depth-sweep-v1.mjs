import {sizeCohortProfitLockedNoExitV1} from './cohort-profit-locked-no-exit-v1.mjs';import {amount,price} from '../paper/fixed.mjs';
export function selectCohortProfitLockedDepthV1({yesAsks,requestedQuantity=null,...args}){
 if(!Array.isArray(yesAsks)||yesAsks.length<1||yesAsks.length>20)throw Error('COHORT_DEPTH_INPUT');let depth=0,previous=-1n;const candidates=[];
 for(const [throughLevel,l]of yesAsks.entries()){const p=price(l.price);if(p<=previous||!Number.isSafeInteger(l.quantity)||l.quantity<1)throw Error('COHORT_DEPTH_LEVEL');previous=p;depth=Math.min(100,depth+l.quantity);
  const proposal=sizeCohortProfitLockedNoExitV1({...args,maximumSharesPerOrder:Math.min(args.maximumSharesPerOrder??100,requestedQuantity??100),yesAskLevel:{price:l.price,quantity:depth}});if(proposal.quantity)candidates.push({...proposal,throughLevel});
 }
 candidates.sort((a,b)=>b.quantity-a.quantity||(amount(b.removedPrincipalIfFilled)>amount(a.removedPrincipalIfFilled)?1:amount(b.removedPrincipalIfFilled)<amount(a.removedPrincipalIfFilled)?-1:0)||Number(price(b.protectedLimit)-price(a.protectedLimit)));
 return{selected:candidates[0]??null,candidates,executionAuthority:false};
}
