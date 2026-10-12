import {price,money,TICK,instant} from './fixed.mjs';

export function roleAwareFillCostV1({intent,observation,allocations,now,latencyMs=3000,adverseTicks=0}){
 const at=instant(now),started=instant(observation.requestStartedAt),received=instant(observation.receivedAt),created=instant(intent.at),limit=price(intent.limit);
 if(!['buy','sell'].includes(intent.action)||!['maker','marketable'].includes(intent.role)||!Number.isSafeInteger(intent.quantity)||intent.quantity<1||!Number.isSafeInteger(adverseTicks)||adverseTicks<0||adverseTicks>100||!Number.isSafeInteger(latencyMs)||latencyMs<3000||!Array.isArray(allocations))throw Error('ROLE_FILL_INPUT');
 const base={actualOrders:0,actualFillProven:false,executionAuthority:false,role:intent.role,reservedCash:intent.action==='buy'?money(limit*BigInt(intent.quantity)):'0.000000'};
 if(started<created+latencyMs||started>received||received>at||at-started>20000)return {...base,eligible:false,reason:'NO_FRESH_INDEPENDENT_LATER_OBSERVATION',modeledQuantity:0};
 let quantity=0,allocatedQuantity=0,value=0n;const seen=new Set(),rows=[];
 for(const d of allocations){
  if(typeof d.key!=='string'||!d.key||seen.has(d.key)||!Number.isSafeInteger(d.quantity)||d.quantity<1||allocatedQuantity+d.quantity>intent.quantity)throw Error('ROLE_FILL_ALLOCATION');seen.add(d.key);allocatedQuantity+=d.quantity;
  const raw=price(d.observedRawPrice),stress=BigInt(intent.role==='maker'?1:adverseTicks)*TICK,stressed=raw+(intent.action==='buy'?stress:-stress);
  if(intent.action==='buy'?stressed>limit:stressed<limit)continue;
  const modeled=intent.role==='maker'?limit:stressed;
  if(modeled<TICK)continue;quantity+=d.quantity;value+=modeled*BigInt(d.quantity);rows.push({key:d.key,quantity:d.quantity,modeledPrice:money(modeled)});
 }
 const posted=limit*BigInt(quantity),improvement=intent.action==='buy'?posted-value:value-posted;
 return {...base,eligible:quantity>0,reason:quantity?'COUNTERFACTUAL_ROLE_PRICE_SCENARIO':'NO_SUPPORTED_LIMIT_CROSSING',modeledQuantity:quantity,modeledNotional:money(value),postedLimitNotionalForModeledFills:money(posted),modeledPriceImprovementAgainstLimit:money(improvement),remainingCashReservation:intent.action==='buy'?money(limit*BigInt(intent.quantity-quantity)):'0.000000',remainingOrderQuantity:intent.quantity-quantity,rows,liquidityMustBeConsumedByCaller:true,queuePriorityKnown:false,priceImprovementIsNotProfit:true};
}
