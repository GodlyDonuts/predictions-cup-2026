import {amount,money,price} from '../paper/fixed.mjs';
import {sourcePurchaseAllocationV1} from './source-purchase-allocation-v1.mjs';

// Cash-efficient greedy allocation, not an exact integer portfolio optimum.
// Input targets must already include forecast/risk/depth checks. This allocator
// never turns a candidate or paper reference into execution authorization.
export function sharedValueAllocationV1({cashLower,capitalLimit,exposures,candidates,unknownAcceptance,unresolvedIntentIds}){
 const base={actualOrders:0,executionAuthority:false,orders:[],unspentCash:cashLower,method:'descending_net_edge_per_principal_greedy'};
 if(unknownAcceptance!==false||!Array.isArray(unresolvedIntentIds)||unresolvedIntentIds.length)return{...base,reason:'UNRESOLVED_EXECUTION_STATE'};
 if(amount(cashLower)<0n||amount(capitalLimit)<0n)throw Error('SHARED_ALLOCATION_NEGATIVE_LIMIT');
 const exposureIds=new Set();let reserved=0n;
 for(const e of exposures){if(!['owned','reservation','loan'].includes(e.kind)||typeof e.exposureId!=='string'||exposureIds.has(e.exposureId)||amount(e.fullPrincipal)<0n)throw Error('SHARED_ALLOCATION_EXPOSURE');exposureIds.add(e.exposureId);if(e.unknownAcceptance===true)return{...base,reason:'UNKNOWN_RESERVATION_ACCEPTANCE'};if(e.kind==='reservation')reserved+=amount(e.fullPrincipal);}
 if(reserved>amount(cashLower))return{...base,reason:'RESERVATIONS_EXCEED_CASH',unspentCash:'0.000000'};
 const keys=new Set(),candidateIds=new Set(),marketEvents=new Map(),ranked=[];
 for(const c of candidates){
  if(typeof c.id!=='string'||!['yes','no'].includes(c.side)||!Number.isSafeInteger(c.quantityTarget)||c.quantityTarget<0||!Number.isSafeInteger(c.depthQuantity)||c.depthQuantity<0)throw Error('SHARED_ALLOCATION_CANDIDATE');
  if(candidateIds.has(c.id))throw Error('SHARED_ALLOCATION_DUPLICATE_ID');candidateIds.add(c.id);
  if(typeof c.marketId!=='string'||typeof c.exchangeId!=='string'||typeof c.eventId!=='string'||marketEvents.has(c.marketId)&&marketEvents.get(c.marketId)!==c.eventId)throw Error('SHARED_ALLOCATION_SCOPE');marketEvents.set(c.marketId,c.eventId);
  if(amount(c.lowerValue)<0n||amount(c.lowerValue)>1000000n||amount(c.minimumNetEdge)<=0n||amount(c.marketLimit)<0n||amount(c.eventLimit)<0n)throw Error('SHARED_ALLOCATION_VALUE');
  const key=c.marketId+':'+c.exchangeId+':'+c.side;if(keys.has(key))throw Error('SHARED_ALLOCATION_DUPLICATE_INSTRUMENT');keys.add(key);
  if(exposureIds.has('shared-allocation:'+c.id))throw Error('SHARED_ALLOCATION_RESERVATION_ID_COLLISION');
  const unit=price(c.limit),net=amount(c.lowerValue)-unit-amount(c.executionCost);
  if(amount(c.executionCost)!==0n)throw Error('SHARED_ALLOCATION_REQUIRES_ZERO_FEE_COST_IN_LIMIT');
  if(net<amount(c.minimumNetEdge)||net<=0n)continue;
  ranked.push({...c,unit,net});
 }
 ranked.sort((a,b)=>{const cross=a.net*b.unit-b.net*a.unit;return cross>0n?-1:cross<0n?1:a.id.localeCompare(b.id);});
 const census=structuredClone(exposures),orders=[];let reservedNew=0n,surplus=0n;
 for(const c of ranked){
  const allocation=sourcePurchaseAllocationV1({cashLower,capitalLimit,marketLimit:c.marketLimit,eventLimit:c.eventLimit,marketId:c.marketId,eventId:c.eventId,exposures:census,quantityTarget:Math.min(c.quantityTarget,c.depthQuantity),limit:c.limit,unknownAcceptance,unresolvedIntentIds});
  if(!allocation.proposedQuantity)continue;
  const cost=amount(allocation.maximumCost),edge=BigInt(allocation.proposedQuantity)*c.net;
  census.push({kind:'reservation',exposureId:'shared-allocation:'+c.id,marketId:c.marketId,eventId:c.eventId,fullPrincipal:allocation.maximumCost});
  orders.push({id:c.id,marketId:c.marketId,exchangeId:c.exchangeId,eventId:c.eventId,side:c.side,quantity:allocation.proposedQuantity,limit:c.limit,maximumCost:allocation.maximumCost,judgmentNetSurplus:money(edge)});reservedNew+=cost;surplus+=edge;
 }
 const priorReservations=exposures.filter(e=>e.kind==='reservation').reduce((n,e)=>n+amount(e.fullPrincipal),0n);
 return{...base,reason:orders.length?'SHARED_FUNDED_VALUE_CANDIDATES':'NO_SHARED_FUNDED_VALUE',orders,unspentCash:money(amount(cashLower)-priorReservations-reservedNew),newReservedPrincipal:money(reservedNew),judgmentNetSurplus:money(surplus),profitabilityEstablished:false};
}
