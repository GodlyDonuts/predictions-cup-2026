import {amount,money,SCALE} from '../paper/fixed.mjs';
// Refund arithmetic is conditional on refundable cost equalling these original
// acquisition principals. Actual settlement receipts must verify that basis.
export function pairRefundScenariosV1(costs){
 if(!Array.isArray(costs)||costs.length!==2)throw Error('PAIR_REFUND_INPUT');
 const c=costs.map(amount);if(c.some(x=>x<=0n||x>=SCALE))throw Error('PAIR_REFUND_COST');
 const results=['YES','NO','REFUND'],rows=[];const principal=c[0]+c[1];
 for(const a of results)for(const b of results){
  const outcomes=[a,b],payout=outcomes.reduce((n,o,i)=>n+(o==='NO'?SCALE:o==='REFUND'?c[i]:0n),0n);
  const category=outcomes.every(o=>o==='REFUND')?'both_refunded':outcomes.includes('REFUND')?'mixed_refund':a==='YES'&&b==='YES'?'inconsistent_both_parties_win':'ordinary_mutually_exclusive_party_winners';
  rows.push({outcomes,category,payout:money(payout),net:money(payout-principal)});
 }
 const ordinary=rows.filter(r=>r.category==='ordinary_mutually_exclusive_party_winners');
 return{principal:money(principal),rows,ordinaryMinimumNet:money(ordinary.reduce((n,r)=>amount(r.net)<n?amount(r.net):n,SCALE)),refundBasisAssumedOriginalPrincipal:true,actualSettlementProven:false,executionAuthority:false};
}

