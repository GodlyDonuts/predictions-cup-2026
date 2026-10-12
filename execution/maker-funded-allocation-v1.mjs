// Converts an existing strategy's desired quotes into funded scheduler inputs.
// cashAvailable MUST come from the shared financial owner AFTER every current
// order and other strategy reservation. This module never infers global cash
// from marks, releases pending cancels, adopts foreign holdings, or sends orders.
import {amount, money, price, instant} from '../paper/fixed.mjs';
import {payloadHash} from './ledger.mjs';
import {MAKER_SCHEDULER_POLICY_V1} from './maker-quote-scheduler-v1.mjs';
const need=(v,c)=>{if(!v)throw Error('MAKER_FUNDING_'+c);};
const slot=q=>`${q.exchangeId}:${q.side}:${q.action}`;
const opposite=side=>side==='yes'?'no':'yes';
const qty=(v,c)=>need(Number.isSafeInteger(v)&&v>=0&&v<=2147483647,c);
const validate=q=>{need(typeof q.exchangeId==='string'&&/^[1-9]\d*$/.test(q.exchangeId)&&['yes','no'].includes(q.side)&&['buy','sell'].includes(q.action),'QUOTE');qty(q.quantity,'QUANTITY');need(q.quantity>0,'QUANTITY');price(q.price);};
export function allocateFundedMakerQuotesV1({at,cashAvailable,maximumCommitted,maximumOrderCost,maximumShares,
  ownedPositions,openOrders,proposals,quotePolicy=MAKER_SCHEDULER_POLICY_V1}) {
  const now=instant(at);qty(maximumShares,'SHARE_CAP');need(maximumShares>0,'SHARE_CAP');
  need(Number.isSafeInteger(quotePolicy.maximumBookAgeMs)&&quotePolicy.maximumBookAgeMs>0&&amount(quotePolicy.minimumNetEntryEdge)>0n,'QUOTE_POLICY');
  let cash=amount(cashAvailable);const cap=amount(maximumCommitted),orderCap=amount(maximumOrderCost);
  need(cash>=0n&&cap>0n&&orderCap>0n,'CAPITAL');
  need(Array.isArray(ownedPositions)&&Array.isArray(openOrders)&&Array.isArray(proposals),'INPUT');
  const positions=new Map(),sellPledged=new Map(),hedgePledged=new Map();
  let basis=0n,reserved=0n;
  for(const p of ownedPositions){
    need(typeof p.exchangeId==='string'&&/^[1-9]\d*$/.test(p.exchangeId)&&!positions.has(p.exchangeId)&&['yes','no'].includes(p.side),'OWNED_POSITION');
    qty(p.quantity,'OWNED_QUANTITY');need(p.quantity>0&&amount(p.cost)>0n,'OWNED_BASIS');
    positions.set(p.exchangeId,p);basis+=amount(p.cost);
  }
  const ids=new Set();
  for(const q of openOrders){
    validate(q);qty(q.remainingQuantity,'REMAINING');need(q.remainingQuantity>0&&q.remainingQuantity<=q.quantity,'REMAINING');
    need(typeof q.orderId==='string'&&/^[1-9]\d*$/.test(q.orderId)&&!ids.has(q.orderId),'ORDER_ID');ids.add(q.orderId);
    need(['open','partial'].includes(q.status),'UNRESOLVED_ORDER');
    const p=positions.get(q.exchangeId);
    if(q.action==='buy'){
      reserved+=price(q.price)*BigInt(q.remainingQuantity);
      if(p&&p.side!==q.side)hedgePledged.set(q.exchangeId,(hedgePledged.get(q.exchangeId)??0)+q.remainingQuantity);
    }else{
      need(p?.side===q.side,'UNOWNED_SELL');sellPledged.set(q.exchangeId,(sellPledged.get(q.exchangeId)??0)+q.remainingQuantity);
    }
  }
  for(const [ex,p] of positions)need((sellPledged.get(ex)??0)+Math.min(p.quantity,hedgePledged.get(ex)??0)<=p.quantity,'EXISTING_DOUBLE_PLEDGE');
  let capacity=cap-basis-reserved;if(capacity<0n)capacity=0n;
  const originalCash=cash,originalCapacity=capacity,quotes=[],declined=[],keys=new Set();
  for(const q of proposals){validate(q);need(!keys.has(slot(q)),'DUPLICATE_PROPOSAL');keys.add(slot(q));amount(q.netEdgePerShare);instant(q.sourceAt);instant(q.validUntil);}
  const ranked=[...proposals].sort((a,b)=>{
    if(a.action!==b.action)return a.action==='sell'?-1:1;
    const d=amount(a.netEdgePerShare)*price(b.price)-amount(b.netEdgePerShare)*price(a.price);
    return d>0n?-1:d<0n?1:slot(a).localeCompare(slot(b));
  });
  for(const q of ranked){
    const source=instant(q.sourceAt);
    if(source>now||now-source>quotePolicy.maximumBookAgeMs||instant(q.validUntil)<=now||
      (q.action==='buy'&&(q.reduceOnly===true||amount(q.netEdgePerShare)<amount(quotePolicy.minimumNetEntryEdge)))) {
      declined.push({slot:slot(q),reason:'stale_expired_or_insufficient_net_edge'});continue;
    }
    const unit=price(q.price),p=positions.get(q.exchangeId);
    const matching=openOrders.filter(r=>slot(r)===slot(q)&&price(r.price)===unit).reduce((n,r)=>n+r.remainingQuantity,0);
    // Retained quantity is already reserved. A different limit receives NO
    // credit until the prior cancellation is confirmed in the owner's account.
    let wanted=Math.min(q.quantity,maximumShares,Number(orderCap/unit));
    if(q.action==='sell'){
      need(q.reduceOnly===true,'REDUCE_ONLY');
      const available=p?.side===q.side?p.quantity-(sellPledged.get(q.exchangeId)??0)-Math.min(p.quantity,hedgePledged.get(q.exchangeId)??0):0;
      wanted=Math.min(wanted,Math.max(0,available)+matching);
      if(wanted>0)sellPledged.set(q.exchangeId,(sellPledged.get(q.exchangeId)??0)+Math.max(0,wanted-matching));
    }else{
      if(p?.side===opposite(q.side)&&(sellPledged.get(q.exchangeId)??0)>0){
        const available=p.quantity-(sellPledged.get(q.exchangeId)??0)-(hedgePledged.get(q.exchangeId)??0);
        wanted=Math.min(wanted,Math.max(0,available)+matching);
      }
      const budget=cash<capacity?cash:capacity;
      wanted=Math.min(wanted,matching+Number(budget/unit));
      const additional=Math.max(0,wanted-matching),cost=unit*BigInt(additional);
      cash-=cost;capacity-=cost;
      if(p&&p.side!==q.side)hedgePledged.set(q.exchangeId,(hedgePledged.get(q.exchangeId)??0)+additional);
    }
    if(wanted>0)quotes.push({...structuredClone(q),quantity:wanted});
    else declined.push({slot:slot(q),reason:q.action==='sell'?'owned_inventory_reserved_or_absent':'shared_cash_risk_or_inventory_reserved'});
  }
  need(cash>=0n&&capacity>=0n,'ALLOCATION_OVERSPEND');
  for(const [ex,p]of positions)need((sellPledged.get(ex)??0)+Math.min(p.quantity,hedgePledged.get(ex)??0)<=p.quantity,'ALLOCATION_DOUBLE_PLEDGE');
  const out={schemaVersion:1,at,quotes,declined,cashAvailableBefore:money(originalCash),cashAvailableAfter:money(cash),
    existingOwnedPrincipal:money(basis),existingBuyReservation:money(reserved),additionalBuyReservation:money(originalCash-cash),
    riskCapacityBefore:money(originalCapacity),riskCapacityAfter:money(capacity),
    sourceBinding:payloadHash({ownedPositions,openOrders,proposals,cashAvailable,maximumCommitted,maximumOrderCost,maximumShares,quotePolicy}),
    executionAuthority:false,networkRequests:0,profitabilityEstablished:false};
  return {...out,allocationHash:payloadHash(out)};
}
