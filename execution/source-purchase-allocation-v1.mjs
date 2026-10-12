import {amount,money,price} from '../paper/fixed.mjs';
const minimum=(...v)=>v.reduce((a,b)=>a<b?a:b);
export function sourcePurchaseAllocationV1({cashLower,capitalLimit,marketLimit,eventLimit,marketId,eventId,exposures,quantityTarget,limit,unknownAcceptance,unresolvedIntentIds}) {
 const base={proposedQuantity:0,maximumCost:'0.000000',executionAuthority:false,actualOrders:0};
 if(unknownAcceptance!==false||!Array.isArray(unresolvedIntentIds)||unresolvedIntentIds.length)return{...base,reason:'UNRESOLVED_EXECUTION_STATE'};
 if(!Array.isArray(exposures)||!Number.isSafeInteger(quantityTarget)||quantityTarget<0||quantityTarget>2147483647||typeof marketId!=='string'||typeof eventId!=='string')throw Error('ALLOCATION_INPUT');
 const cash=amount(cashLower),capital=amount(capitalLimit),marketCap=amount(marketLimit),eventCap=amount(eventLimit),unit=price(limit);if([cash,capital,marketCap,eventCap].some(v=>v<0n))throw Error('ALLOCATION_NEGATIVE_LIMIT');
 let gross=0n,market=0n,event=0n,reserved=0n;const seen=new Set();
 for(const e of exposures){if(!['owned','reservation','loan'].includes(e.kind)||typeof e.exposureId!=='string'||seen.has(e.exposureId)||typeof e.marketId!=='string'||typeof e.eventId!=='string'||e.marketId===marketId&&e.eventId!==eventId)throw Error('ALLOCATION_EXPOSURE_IDENTITY');seen.add(e.exposureId);if(e.unknownAcceptance===true)return{...base,reason:'UNKNOWN_RESERVATION_ACCEPTANCE'};const cost=amount(e.fullPrincipal);if(cost<0n)throw Error('ALLOCATION_NEGATIVE_EXPOSURE');gross+=cost;if(e.marketId===marketId)market+=cost;if(e.eventId===eventId)event+=cost;if(e.kind==='reservation')reserved+=cost;}
 const remaining={cash:cash-reserved,capital:capital-gross,market:marketCap-market,event:eventCap-event},capacity=minimum(...Object.values(remaining));
 if(capacity<=0n)return{...base,reason:'NO_FUNDED_RISK_CAPACITY',remaining:Object.fromEntries(Object.entries(remaining).map(([k,v])=>[k,money(v)]))};
 const q=minimum(BigInt(quantityTarget),capacity/unit);return{...base,proposedQuantity:Number(q),maximumCost:money(q*unit),reason:q?'FUNDED_FULL_PRINCIPAL_ALLOCATION':'BELOW_ONE_SHARE_CAPACITY',remaining:Object.fromEntries(Object.entries(remaining).map(([k,v])=>[k,money(v)])),loansDiscounted:false,reservationsIncluded:true,saleProceedsAssumed:false};
}
