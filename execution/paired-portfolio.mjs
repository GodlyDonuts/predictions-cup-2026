import {decimalMicros,formatMicros,payloadHash,canonicalPayload} from './core.mjs';
import {projectCanonicalFifo} from './canonical-fifo.mjs';
const need=(x,c)=>{if(!x)throw Error(c);},clone=x=>structuredClone(x);
const money=x=>{need(typeof x==='string','PAIRED_EXACT_MONEY');return decimalMicros(x);};
const signed=x=>typeof x==='string'&&x.startsWith('-')?-money(x.slice(1)):money(x);
const fmt=x=>x<0n?'-'+formatMicros(-x):formatMicros(x);
const hash=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const at=x=>{need(typeof x==='string'&&Number.isFinite(Date.parse(x)),'PAIRED_TIME');return Date.parse(x);};
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
export const PAIRED_EXECUTION_LIMITS=Object.freeze({capital:'25.000000',maxExposure:'25.000000',maxOrderCost:'5.000000',maxOrphanPrincipal:'5.000000',maxCumulativeBuy:'100.000000',maxShares:6,maxAccountAgeMs:180000,maxLifetimeMs:30000});
const basis=lots=>lots.reduce((n,l)=>n+BigInt(l.quantity)*money(l.entryPrice),0n);
function account(snapshot,scope,now){
 need(snapshot?.tournamentId===scope.tournamentId&&snapshot.complete===true&&hash(snapshot.positionsSourceHash)
  &&Array.isArray(snapshot.positions)&&Array.isArray(snapshot.openOrders)&&snapshot.openOrders.length===0,'PAIRED_ACCOUNT_INCOMPLETE');
 const start=at(snapshot.startedAt),end=at(snapshot.completedAt);need(start<=end&&end<=now&&now-start<=PAIRED_EXECUTION_LIMITS.maxAccountAgeMs,'PAIRED_ACCOUNT_STALE');
 const cash=snapshot.cashReceipt;
 need(cash?.kind==='tournament_cash'&&cash.tournamentId===scope.tournamentId&&cash.joined===true
  &&cash.method==='GET'&&cash.endpoint==='/tournaments/'+scope.tournamentSlug
  &&cash.fields?.myBalance?.available===true&&hash(cash.sourceBodyHash)&&at(cash.receivedAt)>=start&&at(cash.receivedAt)<=end,'PAIRED_CASH_RECEIPT');
 const amount=money(cash.fields.myBalance.decimal),positions={};
 for(const p of snapshot.positions){need(id(p.exchangeId)&&id(p.marketId)&&Number.isSafeInteger(p.quantity)&&!Object.hasOwn(positions,p.exchangeId),'PAIRED_POSITION_INVALID');if(p.quantity)positions[p.exchangeId]={marketId:p.marketId,quantity:p.quantity};}
 return {start,end,cash:amount,positions};
}
function expectedPositions(s,replace=null){
 const p=clone(s.externalPositions);
 for(const m of s.scope.markets){const owned=replace?.exchangeId===m.exchangeId?replace:s.owned[m.exchangeId];const quantity=owned.lots.reduce((n,l)=>n+(l.side==='no'?-1:1)*l.quantity,0);if(quantity)p[m.exchangeId]={marketId:m.marketId,quantity};else delete p[m.exchangeId];}
 return p;
}
function check(s){
 need(s?.schemaVersion===1&&s.scopeHash===payloadHash(s.scope)&&s.policyHash===payloadHash(PAIRED_EXECUTION_LIMITS),'PAIRED_STATE_IDENTITY');
 const total=Object.values(s.owned).reduce((n,w)=>n+basis(w.lots),0n);
 need(money(s.cash)+total===money(PAIRED_EXECUTION_LIMITS.capital)+signed(s.realizedPnl),'PAIRED_CASH_BASIS_IDENTITY');
 need(total<=money(PAIRED_EXECUTION_LIMITS.maxExposure)&&money(s.cumulativeBuy)<=money(PAIRED_EXECUTION_LIMITS.maxCumulativeBuy),'PAIRED_LEDGER_LIMIT');
 return total;
}
export function createPairedPortfolio(scope,snapshot,readAt){
 need(scope?.ownerId==='Codex'&&typeof scope.runId==='string'&&scope.runId.length>0
  &&typeof scope.tournamentId==='string'&&typeof scope.tournamentSlug==='string'&&/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(scope.tournamentSlug)&&Array.isArray(scope.markets)&&scope.markets.length===2
  &&scope.markets.every(m=>id(m.marketId)&&id(m.exchangeId))&&new Set(scope.markets.map(m=>m.exchangeId)).size===2
  &&new Set(scope.markets.map(m=>m.marketId)).size===2,'PAIRED_SCOPE');
 const a=account(snapshot,scope,at(readAt));need(a.cash>=money(PAIRED_EXECUTION_LIMITS.capital),'PAIRED_INITIAL_CASH');
 need(scope.markets.every(m=>!a.positions[m.exchangeId]),'PAIRED_EXISTING_HOLDING');
 return {schemaVersion:1,scope:clone(scope),scopeHash:payloadHash(scope),policyHash:payloadHash(PAIRED_EXECUTION_LIMITS),
  cash:PAIRED_EXECUTION_LIMITS.capital,realizedPnl:'0.000000',cumulativeBuy:'0.000000',externalPositions:a.positions,
  owned:Object.fromEntries(scope.markets.map(m=>[m.exchangeId,{exchangeId:m.exchangeId,marketId:m.marketId,lots:[],appliedOrderIds:[]}])),
  active:null,balanceMode:null,completed:[],initialAccountEvidenceHash:payloadHash(snapshot),executionAuthority:false};
}
export function reservePairedOrder(s,{intentId,idempotencyKey,marketId,exchangeId,action,quantity,limit,createdAt,expirationDate},snapshot,readAt){
 const total=check(s);need(s.active===null,'PAIRED_ACTIVE_INTENT');const now=at(readAt),a=account(snapshot,s.scope,now);
 need(canonicalPayload(a.positions)===canonicalPayload(expectedPositions(s)),'PAIRED_ACCOUNT_POSITION_DRIFT');
 const m=s.scope.markets.find(m=>m.marketId===marketId&&m.exchangeId===exchangeId);
 need(m&&typeof intentId==='string'&&intentId.length>0&&typeof idempotencyKey==='string'&&idempotencyKey.length>0
  &&!s.completed.some(x=>x.intentId===intentId||x.idempotencyKey===idempotencyKey),'PAIRED_ORDER_IDENTITY');
 need(['buy','sell'].includes(action)&&Number.isSafeInteger(quantity)&&quantity>0&&quantity<=PAIRED_EXECUTION_LIMITS.maxShares,'PAIRED_ORDER_TERMS');
 const p=money(limit);need(p>=5000n&&p<=995000n&&p%5000n===0n,'PAIRED_LIMIT_TICK');
 need(at(createdAt)===now&&at(expirationDate)>now&&at(expirationDate)-now<=PAIRED_EXECUTION_LIMITS.maxLifetimeMs,'PAIRED_EXPIRATION');
 const own=s.owned[exchangeId],q=own.lots.reduce((n,l)=>n+l.quantity,0),reservation=action==='buy'?p*BigInt(quantity):0n;
 const other=s.scope.markets.find(x=>x.exchangeId!==exchangeId),otherQuantity=s.owned[other.exchangeId].lots.reduce((n,l)=>n+l.quantity,0);
 if(q!==otherQuantity){need(action==='buy'?s.balanceMode==='entry'&&q<otherQuantity&&quantity<=otherQuantity-q:q>otherQuantity&&quantity<=q-otherQuantity,'PAIRED_BALANCE_LEG_QUANTITY');}
 need(own.lots.every(l=>l.side==='no')&&(action!=='sell'||q>=quantity),'PAIRED_NAKED_OR_COMPLEMENT_SELL');
 // Every partial outcome must fit the orphan cap. Use the most expensive held
 // shares for the unmatched side; never assume the second order fills.
 for(let filled=1;filled<=quantity;filled++){
  const projected=clone(own.lots);if(action==='buy')projected.push({side:'no',quantity:filled,entryPrice:limit});
  else{let left=filled;while(left){const l=projected[0],n=Math.min(left,l.quantity);left-=n;l.quantity-=n;if(!l.quantity)projected.shift();}}
  const projectedQuantity=q+(action==='buy'?filled:-filled),dominant=projectedQuantity>otherQuantity?projected:s.owned[other.exchangeId].lots;
  let unmatched=Math.abs(projectedQuantity-otherQuantity),orphan=0n;
  for(const l of [...dominant].sort((a,b)=>money(a.entryPrice)>money(b.entryPrice)?-1:money(a.entryPrice)<money(b.entryPrice)?1:0)){const n=Math.min(unmatched,l.quantity);orphan+=BigInt(n)*money(l.entryPrice);unmatched-=n;if(!unmatched)break;}
  need(orphan<=money(PAIRED_EXECUTION_LIMITS.maxOrphanPrincipal),'PAIRED_ORPHAN_LIMIT');
 }
 need(reservation<=money(PAIRED_EXECUTION_LIMITS.maxOrderCost)&&reservation<=money(s.cash)&&reservation<=a.cash
  &&total+reservation<=money(PAIRED_EXECUTION_LIMITS.maxExposure)
  &&money(s.cumulativeBuy)+reservation<=money(PAIRED_EXECUTION_LIMITS.maxCumulativeBuy),'PAIRED_RESERVATION_LIMIT');
 const binding={ownerId:s.scope.ownerId,tournamentId:s.scope.tournamentId,marketId,exchangeId,side:'no',action,quantity,limit,createdAt,expirationDate};
 const wallet={...clone(own),ownerId:s.scope.ownerId,tournamentId:s.scope.tournamentId,cash:s.cash,realizedPnl:s.realizedPnl,accountSignedQuantity:-q};
 const next=clone(s);next.active={preparation:{intentId,idempotencyKey,binding,wallet},before:clone(snapshot),reservation:formatMicros(reservation),beforeStateHash:payloadHash(s)};return next;
}
// Local proposed facts only: an API adapter must supply source-bound complete
// evidence; this pure function never converts its result into a dispatch permit.
export function applyReconciledPairedOrder(s,orderStatus,after,activity,readAt){
 check(s);need(s.active&&orderStatus?.failed===false&&orderStatus.unconfirmedPersistence===false
  &&orderStatus.state?.phase==='terminal','PAIRED_ORDER_NOT_TERMINAL');
 const order=orderStatus.state,pre=s.active.preparation;
 need(order.intentId===pre.intentId&&order.idempotencyKey===pre.idempotencyKey
  &&canonicalPayload(order.binding)===canonicalPayload(pre.binding)&&payloadHash(order.wallet)===payloadHash(pre.wallet),'PAIRED_ORDER_BASELINE_CHANGED');
 need(id(order.orderId)&&!s.completed.some(x=>x.orderId===order.orderId),'PAIRED_ORDER_ALREADY_APPLIED');
 const projection=projectCanonicalFifo({wallet:pre.wallet,binding:{...pre.binding,orderId:order.orderId},...order.evidence});
 need(projection.projectionHash===order.projection?.projectionHash,'PAIRED_ORDER_PROJECTION_CHANGED');
 const now=at(readAt),a=account(after,s.scope,now),b=account(s.active.before,s.scope,at(pre.binding.createdAt));
 const lastFill=Math.max(at(pre.binding.createdAt),...order.evidence.rows.map(f=>at(f.filledAt)));
 need(a.start>=lastFill&&a.start>=at(order.evidence.readAt),'PAIRED_ACCOUNT_BEFORE_TERMINAL_EVIDENCE');
 need(a.cash-b.cash===signed(projection.cashDelta),'PAIRED_OFFICIAL_CASH_MISMATCH');
 need(canonicalPayload(a.positions)===canonicalPayload(expectedPositions(s,projection.wallet)),'PAIRED_OFFICIAL_POSITION_MISMATCH');
 need(activity?.tournamentId===s.scope.tournamentId&&activity.complete===true&&hash(activity.sourceHash)
  &&at(activity.from)<=b.start&&at(activity.through)>=a.end&&at(activity.through)<=now
  &&Array.isArray(activity.events),'PAIRED_ACTIVITY_INCOMPLETE');
 const fills=new Map(order.evidence.rows.map(f=>[f.id,f])),seen=new Set();
 for(const e of activity.events){const f=fills.get(e.fillId);need(e.kind==='fill'&&e.orderId===order.orderId&&f&&!seen.has(e.fillId)
  &&e.exchangeId===pre.binding.exchangeId&&e.marketId===pre.binding.marketId&&at(e.at)===at(f.filledAt),'PAIRED_UNRELATED_ECONOMIC_ACTIVITY');seen.add(e.fillId);}
 need(seen.size===fills.size,'PAIRED_ACTIVITY_FILL_MISSING');
 const next=clone(s);next.cash=projection.wallet.cash;next.realizedPnl=projection.wallet.realizedPnl;
 next.owned[pre.binding.exchangeId]={exchangeId:pre.binding.exchangeId,marketId:pre.binding.marketId,lots:projection.wallet.lots,appliedOrderIds:projection.wallet.appliedOrderIds};
 if(projection.canonicalAction==='buy')next.cumulativeBuy=formatMicros(money(s.cumulativeBuy)+money(projection.nominalPrincipal));
 next.completed.push({intentId:pre.intentId,idempotencyKey:pre.idempotencyKey,orderId:order.orderId,action:pre.binding.action,projectionHash:projection.projectionHash,accountEvidenceHash:payloadHash(after),activityHash:payloadHash(activity),cashDelta:projection.cashDelta,realizedDelta:projection.realizedDelta});next.active=null;
 const quantities=next.scope.markets.map(m=>next.owned[m.exchangeId].lots.reduce((n,l)=>n+l.quantity,0));
 next.balanceMode=quantities[0]===quantities[1]?null:(s.balanceMode??(pre.binding.action==='buy'?'entry':'exit'));check(next);return next;
}
