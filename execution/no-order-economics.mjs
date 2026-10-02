import {decimalMicros,formatMicros,payloadHash,canonicalPayload} from './core.mjs';
import {normalizeOfficialFillTick} from './api-tick-price.mjs';
const need=(x,code)=>{if(!x)throw Error(code);},scale=1000000n;
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const time=x=>{need(typeof x==='string'&&Number.isFinite(Date.parse(x)),'INVALID_ECONOMICS_TIME');return Date.parse(x);};
const tick=x=>decimalMicros(normalizeOfficialFillTick(x).price);

// Order-local evidence only. No dispatch, ownership adoption or global convention.
// Snapshots require exact gross cash, not available collateral or rounded marks.
export function verifyNoBuyEconomics({binding,ack,fills,before,after,activity,readAt}) {
 need(binding&&id(binding.orderId)&&id(binding.exchangeId)&&id(binding.marketId)
  &&typeof binding.tournamentId==='string'&&binding.tournamentId.length>0
  &&binding.side==='no'&&binding.action==='buy'&&Number.isSafeInteger(binding.quantity)
  &&binding.quantity>0&&binding.quantity<=6,'INVALID_NO_BUY_BINDING');
 const limit=tick(binding.sideRelativeLimit),prepared=time(binding.preparedAt),now=time(readAt);
 need(limit>=5000n&&limit<=995000n&&now>=prepared,'INVALID_NO_BUY_LIMIT_OR_TIME');
 need(ack&&String(ack.orderId)===binding.orderId&&ack.exchangeId===binding.exchangeId
  &&ack.tournamentId===binding.tournamentId&&ack.side==='no'&&ack.action==='buy'
  &&ack.quantity===binding.quantity&&tick(ack.price)===limit&&ack.open===false
  &&ack.remainingQuantity===0&&ack.quantityTraded===binding.quantity,'NONTERMINAL_OR_MISMATCHED_NO_ACK');
 // Aggregated money must retain exact decimal evidence; do not round API doubles.
 const cost=decimalMicros(ack.totalCost);
 need(cost>0n&&cost<=BigInt(binding.quantity)*limit,'INVALID_ACK_PRINCIPAL');
 need(fills?.complete===true&&Array.isArray(fills.rows)&&fills.rows.length<=10000
  &&fills.orderId===binding.orderId&&fills.exchangeId===binding.exchangeId
  &&fills.tournamentId===binding.tournamentId,'INCOMPLETE_NO_FILL_EVIDENCE');
 const unique=new Map();
 for(const row of fills.rows){
  need(row&&Number.isSafeInteger(row.id)&&row.id>0&&row.orderId===binding.orderId
   &&row.marketId===binding.marketId&&row.exchangeId===binding.exchangeId
   &&row.tournamentId===binding.tournamentId&&row.side==='no'
   &&Number.isSafeInteger(row.quantity)&&row.quantity<0,'NO_FILL_IDENTITY_OR_SIGN');
  const at=time(row.filledAt);need(at>=prepared&&at<=now,'NO_FILL_TIME_OUTSIDE_ORDER');tick(row.price);
  if(unique.has(row.id))need(canonicalPayload(unique.get(row.id))===canonicalPayload(row),'CONFLICTING_NO_FILL');
  else unique.set(row.id,row);
 }
 const rows=[...unique.values()].sort((a,b)=>time(a.filledAt)-time(b.filledAt)||a.id-b.id);
 need(rows.reduce((q,r)=>q-r.quantity,0)===binding.quantity,'NO_FILL_TOTAL_MISMATCH');
 const snapshot=s=>{
  need(s&&s.tournamentId===binding.tournamentId&&s.complete===true
   &&s.cashBasis==='gross_cash'&&Array.isArray(s.positions)&&Array.isArray(s.openOrders)
   &&s.openOrders.length===0,'UNISOLATED_ACCOUNT_SNAPSHOT');
  const start=time(s.startedAt),end=time(s.completedAt);need(start<=end&&end<=now&&now-end<=180000,'STALE_ACCOUNT_SNAPSHOT');
  const positions=new Map();for(const p of s.positions){need(id(p.exchangeId)&&id(p.marketId)
   &&Number.isSafeInteger(p.signedQuantity)&&p.signedQuantity!==0&&!positions.has(p.exchangeId),'INVALID_SIGNED_POSITION');
   positions.set(p.exchangeId,{marketId:p.marketId,signedQuantity:p.signedQuantity});}
  return {start,end,cash:decimalMicros(s.cash),positions};
 };
 const pre=snapshot(before),post=snapshot(after);
 need(pre.end<=prepared&&post.start>=Math.max(...rows.map(r=>time(r.filledAt)))
  &&post.start>=pre.end,'ACCOUNT_SNAPSHOT_ORDERING');
 need(activity?.complete===true&&activity.tournamentId===binding.tournamentId
  &&time(activity.from)<=pre.start&&time(activity.through)>=post.end
  &&time(activity.through)<=now&&Array.isArray(activity.rows),'INCOMPLETE_ACTIVITY_WINDOW');
 // Independent complete account activity is needed to exclude unrelated fills,
 // settlement, fees, transfers and collateral changes during the snapshots.
 need(activity.rows.length===rows.length,'ACCOUNT_ACTIVITY_NOT_ISOLATED');
 const activityIds=new Set();for(const a of activity.rows){need(a.kind==='fill'
  &&a.orderId===binding.orderId&&unique.has(a.fillId)&&!activityIds.has(a.fillId)
  &&time(a.at)===time(unique.get(a.fillId).filledAt),'ACCOUNT_ACTIVITY_NOT_ISOLATED');activityIds.add(a.fillId);}
 need(pre.cash-post.cash===cost,'NO_CASH_PRINCIPAL_MISMATCH');
 const old=pre.positions.get(binding.exchangeId),next=post.positions.get(binding.exchangeId);
 need(!old||old.signedQuantity<0,'NO_BUY_WOULD_NET_EXISTING_YES');
 need(next?.marketId===binding.marketId&&(!old||old.marketId===binding.marketId)
  &&next.signedQuantity===(old?.signedQuantity||0)-binding.quantity,'NO_POSITION_DELTA_MISMATCH');
 for(const exchange of new Set([...pre.positions.keys(),...post.positions.keys()]))if(exchange!==binding.exchangeId)
  need(canonicalPayload(pre.positions.get(exchange)||null)===canonicalPayload(post.positions.get(exchange)||null),'UNRELATED_POSITION_CHANGED');
 const candidates=['side_relative','yes_denominated'].map(basis=>{
  const projected=rows.map(r=>{const raw=tick(r.price);return {...r,effectivePrice:formatMicros(basis==='side_relative'?raw:scale-raw)};});
  const principal=projected.reduce((n,r)=>n+BigInt(-r.quantity)*decimalMicros(r.effectivePrice),0n);
  return {basis,principal,projected,valid:principal===cost&&projected.every(r=>decimalMicros(r.effectivePrice)<=limit)};
 }).filter(c=>c.valid);
 need(candidates.length===1,'AMBIGUOUS_OR_INCONSISTENT_NO_PRICE_BASIS');
 const found=candidates[0],result={schemaVersion:1,orderId:binding.orderId,bindingHash:payloadHash(binding),
  evidenceHash:payloadHash({ack,fills,before,after,activity}),readAt,priceBasis:found.basis,
  principalCost:formatMicros(cost),signedPositionDelta:-binding.quantity,filledQuantity:binding.quantity,
  fills:found.projected,scope:'this_terminal_order_only',globalConventionEstablished:false,
  dispatchAuthority:false,authoritativeReconciliation:false};
 return {...result,verificationHash:payloadHash(result)};
}
