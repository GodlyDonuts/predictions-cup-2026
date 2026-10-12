import {price,money,TICK} from '../paper/fixed.mjs';
export function marketableAllocationCostV1({limit,quantity,allocations,adverseTicks=1}){
 const ceiling=price(limit);if(!Number.isSafeInteger(quantity)||quantity<1||!Array.isArray(allocations)||!Number.isSafeInteger(adverseTicks)||adverseTicks<0||adverseTicks>100)throw Error('ALLOCATION_COST_INPUT');
 let used=0,cost=0n;const seen=new Set(),rows=[];
 for(const a of allocations){
  if(typeof a.key!=='string'||!a.key||seen.has(a.key)||!Number.isSafeInteger(a.quantity)||a.quantity<1||used+a.quantity>quantity)throw Error('ALLOCATION_COST_DEPTH');seen.add(a.key);
  const execution=price(a.observedRawPrice)+BigInt(adverseTicks)*TICK;
  if(execution>ceiling)throw Error('ALLOCATION_COST_LIMIT');
  used+=a.quantity;cost+=execution*BigInt(a.quantity);rows.push({key:a.key,quantity:a.quantity,price:money(execution)});
 }
 if(used!==quantity)throw Error('ALLOCATION_COST_INCOMPLETE');
 const reservation=ceiling*BigInt(quantity);
 return{quantity,modeledPrincipal:money(cost),worstLimitReservation:money(reservation),reservationPremium:money(reservation-cost),rows,actualFillProven:false,priceImprovementIsNotProfit:true};
}
