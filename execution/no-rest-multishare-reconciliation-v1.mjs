import {createHash} from 'node:crypto';
import {parseNumberLexemes} from './exact-money-receipts.mjs';
import {normalizeOfficialFillTick} from './api-tick-price.mjs';
import {inspectScopedRestNoAccount} from './no-rest-sell-reconciliation-v2.mjs';
import {canonicalPayload,payloadHash} from './core.mjs';
import {amount,money,price,instant} from '../paper/fixed.mjs';

// Pure evidence reduction, not allocation, write authority, or a replacement for
// the one-share live verifiers. REST prices are side-relative and NO quantities
// are negative even for SELL (official public schema observed 2026-10-06).
export const REST_NO_MULTISHARE_CONTRACT_V1=Object.freeze({schemaVersion:1,
 mode:'scoped_rest_no_multishare_fifo_v1',minimumQuantity:1,maximumQuantity:100,maximumAttributedFills:10000,
 fee:'0.000000',cashRoundingRadius:'0.010000',maxReceiptAgeMs:20000,
 maxOrderLifetimeMs:30000,maxPages:10,averageAbsoluteTolerance:'0.000000000001',
 priceBasis:'side_relative_rest_no',documentationUrl:'https://sig.thesuper.market/api/v1/docs',
 executionAuthority:false});
const need=(ok,code)=>{if(!ok)throw Error('REST_NO_MULTI_'+code);};
const sha=x=>createHash('sha256').update(x).digest('hex');
const hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const int=(x,min=0,max=Number.MAX_SAFE_INTEGER)=>Number.isSafeInteger(x)&&x>=min&&x<=max;
const same=(a,b)=>canonicalPayload(a)===canonicalPayload(b);
const keys=(x,allowed)=>need(x&&typeof x==='object'&&!Array.isArray(x)&&Object.keys(x).every(k=>allowed.includes(k)),'UNEXPECTED_METADATA');
const source=r=>({method:r.method,endpoint:r.endpoint,requestStartedAt:r.requestStartedAt,receivedAt:r.receivedAt,sourceBodyHash:r.sourceBodyHash,fetchInvocationId:r.fetchInvocationId??null});
function freeze(x){if(x&&typeof x==='object'){for(const v of Object.values(x))freeze(v);Object.freeze(x);}return x;}
function read(r,method,endpoint,now,fresh=true){
 need(r?.method===method&&r.endpoint===endpoint&&r.status===200&&typeof r.bodyText==='string','RECEIPT_SCOPE');
 need(typeof r.fetchInvocationId==='string'&&r.fetchInvocationId.length>0,'RECEIPT_INVOCATION');
 const start=instant(r.requestStartedAt),end=instant(r.receivedAt);
 need(start<=end&&end<=now&&end-start<=20000&&(!fresh||now-start<=20000),'RECEIPT_TIME');
 const sourceBodyHash=sha(r.bodyText);need(r.sourceBodyHash===sourceBodyHash,'RECEIPT_HASH');
 return {...parseNumberLexemes(r.bodyText),...source({...r,sourceBodyHash}),start,end};
}
function integerAt(r,path,value,min,max){
 const raw=r.numbers.get(JSON.stringify(path));
 need(typeof raw==='string'&&raw.length<=128&&/^-?(0|[1-9]\d*)(\.0+)?$/.test(raw)&&int(value,min,max)&&BigInt(raw.split('.')[0])===BigInt(value),'EXACT_INTEGER');
 return value;
}
function amountAt(r,path){
 const raw=r.numbers.get(JSON.stringify(path));need(typeof raw==='string','MONEY_SOURCE');
 try{return amount(raw);}catch{throw Error('REST_NO_MULTI_EXACT_MONEY');}
}
function tickAt(r,path,value){
 const raw=r.numbers.get(JSON.stringify(path));need(typeof raw==='string','TICK_SOURCE');
 let v;
 try{v=price(raw);}catch{
  // Only the platform's specifically enumerated IEEE complement forms are
  // supported. An arbitrary long decimal which merely rounds to one is not.
  need(raw===JSON.stringify(value),'TICK_LEXEME');
  try{v=amount(normalizeOfficialFillTick(value).price);}catch{throw Error('REST_NO_MULTI_TICK');}
 }
 need(v>=5000n&&v<=995000n,'TICK_RANGE');return v;
}
function rational(raw){
 need(typeof raw==='string'&&raw.length<=128,'AVERAGE_SOURCE');
 const m=/^(-?)(0|[1-9]\d*)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/.exec(raw);
 need(m&&Math.abs(Number(m[4]??0))<=30,'AVERAGE_DECIMAL');
 const shift=(m[3]??'').length-Number(m[4]??0);let n=BigInt(m[2]+(m[3]??'')),d=1n;
 if(shift>=0)d=10n**BigInt(shift);else n*=10n**BigInt(-shift);
 return {n:m[1]? -n:n,d};
}
function averageCheck(raw,principal,quantity){
 if(quantity===0){need(raw===null,'EMPTY_AVERAGE');return null;}
 const {n,d}=rational(raw),delta=n*BigInt(quantity)*1000000n-principal*d;
 need(n>0n&&(delta<0n?-delta:delta)*1000000000000n<=d*BigInt(quantity)*1000000n,'AVERAGE_NOTIONAL');
 return {numeratorMicros:principal.toString(),denominatorShares:quantity,reportedDecimal:raw,
  basis:'exact_fill_notional_divided_by_filled_shares',reportedAverageUsedForPrincipal:false};
}
function account(raw,b,at,requiredFunding='0.000000',reservedElsewhere='0.000000'){
 const a=inspectScopedRestNoAccount({account:raw,binding:b,readAt:at,requiredFunding,reservedElsewhere});
 // Strengthen the shared account parser's historical order quantities using
 // original number lexemes, including closed-order quantityFilled.
 for(const page of raw.scopedOrdersPages){const r=read(page,'GET',page.endpoint,instant(at));
  for(const [i,row] of r.data.data.entries()){
   integerAt(r,['data',i,'id'],row.id,1,Number.MAX_SAFE_INTEGER);
   integerAt(r,['data',i,'quantity'],row.quantity,1,Number.MAX_SAFE_INTEGER);
   integerAt(r,['data',i,'quantityFilled'],row.quantityFilled,0,row.quantity);
   need(row.open===false,'PRIOR_OPEN_ORDER');
  }
 }
 // The shared parser validates exact cash/positions/page scope and completeness.
 const receipts=[raw.cash,raw.positions,...raw.openOrdersPages,...raw.scopedOrdersPages];
 return {...a,sourceReceipts:receipts.map(r=>source(read(r,'GET',r.endpoint,instant(at))))};
}
function projectionProgress(before,after){
 for(const k of ['openOrdersPages','scopedOrdersPages']){
  const first=before[k].map(r=>parseNumberLexemes(r.bodyText).data.coverage.projectedThroughSequence).filter(x=>x!==null),last=after[k].map(r=>parseNumberLexemes(r.bodyText).data.coverage.projectedThroughSequence).filter(x=>x!==null);
  if(first.length&&last.length)need(Math.min(...last)>=Math.max(...first),'ACCOUNT_PROJECTION_ROLLBACK');
 }
}
const history=a=>[...a.scopedOrders].sort((x,y)=>x.id-y.id);
const target=(a,b)=>{const row=a.positions.find(x=>x.exchangeId===b.exchangeId);if(row)need(row.marketId===b.marketId&&row.settled===false,'POSITION_SCOPE');return row?.quantity??0;};
const otherPositions=(a,b)=>a.positions.filter(x=>x.exchangeId!==b.exchangeId&&x.quantity!==0).sort((x,y)=>Number(x.exchangeId)-Number(y.exchangeId));
function priorInventory(proof,rawAfter,pre,b){
 if(proof===null){need(rawAfter===null&&target(pre,b)===0,'FLAT_INITIAL_BUY_REQUIRED');return {lots:[],quantity:0,cost:0n,cumulativeRealized:0n};}
 const {verificationHash,...body}=proof;
 need(hex(verificationHash)&&payloadHash(body)===verificationHash&&proof.mode===REST_NO_MULTISHARE_CONTRACT_V1.mode&&proof.scopedTradeVerified===true&&proof.executionAuthority===false,'PRIOR_PROOF');
 for(const k of ['ownerId','runId','tournamentId','marketId','exchangeId'])need(proof[k]===b[k],'PRIOR_SCOPE');
 const old=account(rawAfter,b,proof.readAt);need(same(old.sourceReceipts,proof.sourceReceipts.after)&&instant(proof.readAt)<=instant(pre.startedAt)&&instant(old.completedAt)<=instant(pre.startedAt)&&same(history(old),history(pre)),'PRIOR_SOURCE_OR_HISTORY');
 need(proof.lastObservedFill===null||proof.lastObservedFill&&int(proof.lastObservedFill.fillId,1)&&instant(proof.lastObservedFill.filledAt)<=instant(proof.readAt),'PRIOR_FILL_WATERMARK');
 need(int(proof.remainingOwnedQuantity)&&target(old,b)===-proof.remainingOwnedQuantity&&target(pre,b)===-proof.remainingOwnedQuantity,'PRIOR_POSITION');
 need(Array.isArray(proof.remainingLots)&&proof.remainingLots.length<=10000&&Array.isArray(proof.attributedBuyFills)&&proof.attributedBuyFills.length<=10000,'PRIOR_LOTS');
 const seen=new Set();let quantity=0,cost=0n,last=null;
 for(const lot of proof.remainingLots){
  need(typeof lot.lotId==='string'&&!seen.has(lot.lotId)&&id(lot.orderId)&&int(lot.fillId,1)&&int(lot.originalQuantity,1,100)&&int(lot.quantity,1,lot.originalQuantity)&&lot.side==='no','PRIOR_LOT');seen.add(lot.lotId);
  for(const k of ['ownerId','runId','tournamentId','marketId','exchangeId'])need(lot[k]===b[k],'PRIOR_LOT_SCOPE');
  const p=price(lot.entryPrice),at=instant(lot.openedAt),original=proof.attributedBuyFills.find(f=>f.lotId===lot.lotId);
  need(original&&same({...lot,quantity:lot.originalQuantity,principalCost:money(p*BigInt(lot.originalQuantity))},original),'PRIOR_LOT_ORIGIN');
  const order=old.scopedOrders.find(o=>String(o.id)===lot.orderId);
  need(order?.side==='no'&&order.action==='buy'&&order.quantityFilled>=lot.originalQuantity&&order.open===false&&at>=instant(order.createdAt),'PRIOR_BUY_ORDER');
  need(last===null||at>last.at||at===last.at&&lot.fillId>last.fillId,'FIFO_ORDER');last={at,fillId:lot.fillId};
  need(lot.principalCost===money(p*BigInt(lot.quantity)),'PRIOR_LOT_COST');quantity+=lot.quantity;cost+=p*BigInt(lot.quantity);
 }
 need(quantity===proof.remainingOwnedQuantity&&money(cost)===proof.remainingOwnedCost,'PRIOR_TOTALS');
 return {lots:structuredClone(proof.remainingLots),quantity,cost,cumulativeRealized:amount(proof.cumulativeRealizedPnl)};
}
function neutralAll(a,cost,b){
 const all=a.data.all;need(all===null||all&&typeof all==='object'&&!Array.isArray(all),'ALL_REQUIRED');
 if(all===null)return {kind:'null_all',exactGlobalCash:false};
 const fields=['fullNotionalCost','effectiveEntryCost','collateralSavings','relationshipIds','componentId','guaranteedPayoutFloorAfter','outstandingAdvanceAfter','collateralRepayment','netBuyingPowerImpact','redemptionCredit'];
 need(same(Object.keys(all).sort(),fields.sort())&&Array.isArray(all.relationshipIds)&&all.relationshipIds.length===0&&all.componentId===null,'NON_NEUTRAL_ALL');
 for(const k of ['fullNotionalCost','effectiveEntryCost'])need(amountAt(a,['all',k])===cost,'ALL_PRINCIPAL');
 for(const k of ['collateralSavings','guaranteedPayoutFloorAfter','outstandingAdvanceAfter','collateralRepayment','redemptionCredit'])need(amountAt(a,['all',k])===0n,'NON_NEUTRAL_ALL');
 need(amountAt(a,['all','netBuyingPowerImpact'])===(b.action==='buy'?cost:-cost),'ALL_CASH_SIGN');
 return {kind:'neutral_all',fullNotionalCost:money(cost),effectiveEntryCost:money(cost),netBuyingPowerImpact:money(b.action==='buy'?cost:-cost),collateralEffects:'zero',relationshipCount:0,exactGlobalCash:false};
}

/** Caller must obtain all receipts through its existing serialized collector.
 * priorProof is an immutable output of this reducer; its raw priorAfter must be
 * supplied for continuation. Hashes check integrity, not external authority.
 * dispatchReceipt is the caller's durable actual-fetch record, never a queue
 * fence timestamp. This reducer cannot authenticate journal ownership for it.
 */
export function verifyScopedRestNoMultishare({binding:b,before,ack,order,fillPages,after,globalRisk,dispatchReceipt,readAt,reservedFunding='0.000000',reservedElsewhere='0.000000',priorProof=null,priorAfter=null,accountActivity=null}){
 const now=instant(readAt);
 need(b&&['orderId','marketId','exchangeId'].every(k=>id(b[k]))&&['ownerId','runId','intentId','idempotencyKey','tournamentId'].every(k=>typeof b[k]==='string'&&b[k].length>0)&&b.side==='no'&&['buy','sell'].includes(b.action)&&int(b.quantity,1,100),'BINDING');
 const limit=price(b.limit),created=instant(b.createdAt),expires=instant(b.expirationDate);
 need(created<=now&&expires>created&&expires-created<=30000,'BINDING_LIFETIME');
 need(globalRisk?.ownerId===b.ownerId&&globalRisk.runId===b.runId&&globalRisk.tournamentId===b.tournamentId&&globalRisk.complete===true&&globalRisk.soleCollector===true&&globalRisk.stopped===false&&hex(globalRisk.sourceHash)&&Array.isArray(globalRisk.unresolvedIntentIds)&&globalRisk.unresolvedIntentIds.length<=1&&globalRisk.unresolvedIntentIds.every(x=>x===b.intentId)&&instant(globalRisk.at)<=now&&now-instant(globalRisk.at)<=20000,'GLOBAL_UNKNOWN_OR_OWNER');
 need(accountActivity===null,'GLOBAL_ACTIVITY_UNSUPPORTED');
 const funding=amount(reservedFunding),other=amount(reservedElsewhere);need(funding>=0n&&other>=0n&&(b.action!=='buy'||funding>=limit*BigInt(b.quantity)),'FULL_FUNDING_RESERVATION');
 const pre=account(before,b,b.createdAt,b.action==='buy'?reservedFunding:'0.000000',reservedElsewhere),post=account(after,b,readAt);
 projectionProgress(before,after);if(priorAfter!==null)projectionProgress(priorAfter,before);
 need((b.action==='sell'||pre.funded)&&instant(pre.completedAt)<=created&&instant(post.startedAt)>=created,'ACCOUNT_CAUSALITY_OR_FUNDING');
 const prior=priorInventory(priorProof,priorAfter,pre,b);
 need(b.action==='buy'?int(prior.quantity+b.quantity):priorProof!==null&&b.quantity<=prior.quantity,'OWNED_QUANTITY');
 const requestBody={idempotencyKey:b.idempotencyKey,exchangeId:b.exchangeId,side:'no',action:b.action,quantity:b.quantity,price:Number(b.limit),expirationDate:b.expirationDate,tournamentId:b.tournamentId};
 const bodyJson=canonicalPayload(requestBody),bodyHash=payloadHash(requestBody),wire=dispatchReceipt;
 keys(wire,['method','endpoint','requestBodyJson','requestBodyHash','fetchInvocationId','requestStartedAt','ownerId','runId','tournamentId','intentId','idempotencyKey','leaseIdentity','leaseHash','tournamentSlug','marketId','exchangeId','kind']);
 if(wire.kind!==undefined)need(wire.kind==='place','DISPATCH_OPERATION');
 for(const k of ['marketId','exchangeId','tournamentSlug'])if(wire[k]!==undefined)need(wire[k]===b[k],'DISPATCH_SCOPE');
 need(wire?.method==='POST'&&wire.endpoint==='/orders'&&wire.requestBodyJson===bodyJson&&wire.requestBodyHash===bodyHash&&typeof wire.fetchInvocationId==='string'&&wire.fetchInvocationId.length>0&&wire.idempotencyKey===b.idempotencyKey,'DISPATCH_BODY');
 for(const k of ['ownerId','runId','tournamentId','intentId'])need(wire[k]===b[k],'DISPATCH_OWNER');
 keys(wire.leaseIdentity,['ownerId','runId','tournamentId','leaseId']);
 need(wire.leaseIdentity?.ownerId===b.ownerId&&wire.leaseIdentity.tournamentId===b.tournamentId&&typeof wire.leaseIdentity.leaseId==='string'&&wire.leaseIdentity.leaseId.length>0&&wire.leaseHash===payloadHash(wire.leaseIdentity),'DISPATCH_LEASE');
 const a=read(ack,'POST','/orders',now,false),d=a.data;
 need(ack.requestBodyHash===bodyHash&&ack.fetchInvocationId===wire.fetchInvocationId&&a.requestStartedAt===wire.requestStartedAt&&a.start>=created&&a.start<expires,'ORIGINAL_FETCH_PROVENANCE');
 need(String(d?.orderId)===b.orderId&&d.exchangeId===b.exchangeId&&d.action===b.action&&d.side==='no'&&typeof d.open==='boolean'&&tickAt(a,['price'],d.price)===limit,'ACK_CANONICAL');
 integerAt(a,['orderId'],d.orderId,1,Number.MAX_SAFE_INTEGER);integerAt(a,['quantity'],d.quantity,1,b.quantity);
 integerAt(a,['quantityTraded'],d.quantityTraded,0,d.quantity);integerAt(a,['remainingQuantity'],d.remainingQuantity,0,d.quantity);
 need(d.quantityTraded+d.remainingQuantity===d.quantity&&(!d.open||d.remainingQuantity>0),'ACK_QUANTITY');
 const ackCost=amountAt(a,['totalCost']);need(ackCost>=0n&&ackCost<=995000n*BigInt(d.quantityTraded)&&(d.quantityTraded?ackCost>=5000n*BigInt(d.quantityTraded):ackCost===0n),'ACK_NOTIONAL');
 const ackEconomics=neutralAll(a,ackCost,b),terminal=read(order,'GET','/orders/'+b.orderId,now),od=terminal.data;
 need(terminal.start>=a.end&&String(od?.id)===b.orderId&&od.exchangeId===b.exchangeId&&od.tournamentId===b.tournamentId&&od.side==='no'&&od.action===b.action&&od.open===false&&tickAt(terminal,['priceLimit'],od.priceLimit)===limit&&od.expirationDate===b.expirationDate,'TERMINAL_SCOPE');
 // The documented terminal quantity may shrink when unfilled remainder is
 // discarded. It never changes the original request, key or funding bound.
 integerAt(terminal,['id'],od.id,1,Number.MAX_SAFE_INTEGER);integerAt(terminal,['quantity'],od.quantity,1,d.quantity);integerAt(terminal,['quantityFilled'],od.quantityFilled,0,od.quantity);
 const serverCreated=instant(od.createdAt);need(serverCreated>=created-2000&&serverCreated<=Math.min(a.end,expires)+2000,'SERVER_ORDER_CLOCK');
 need(Array.isArray(fillPages)&&fillPages.length>0&&fillPages.length<=10,'FILL_PAGES');
 let cursor=null,lastEnd=terminal.end,checkpoint=null,total=null,reportedAverage;const cursors=new Set(),ids=new Set(),fills=[],fillSources=[];
 for(const [pageIndex,raw] of fillPages.entries()){
  const r=read(raw,'GET','/orders/'+b.orderId+'/fills?limit=200'+(cursor===null?'':'&cursor='+encodeURIComponent(cursor)),now),v=r.data,p=v?.pagination;
  need(r.start>=lastEnd&&Array.isArray(v?.data)&&v.data.length<=200&&String(v.orderId)===b.orderId&&v.exchangeId===b.exchangeId&&v.tournamentId===b.tournamentId,'FILL_SCOPE');lastEnd=r.end;
  integerAt(r,['orderId'],v.orderId,1,Number.MAX_SAFE_INTEGER);integerAt(r,['totalQuantityFilled'],v.totalQuantityFilled,-b.quantity,0);
  need(p?.limit===200&&typeof p.hasMore==='boolean'&&(p.nextCursor===null||typeof p.nextCursor==='string'&&p.nextCursor.length>0&&p.nextCursor.length<=1000)&&v.coverage?.complete===true&&(v.coverage.projectedThroughSequence===null||int(v.coverage.projectedThroughSequence)),'FILL_COVERAGE');
  if(checkpoint!==null&&v.coverage.projectedThroughSequence!==null)need(v.coverage.projectedThroughSequence>=checkpoint,'FILL_PROJECTION_ROLLBACK');checkpoint=v.coverage.projectedThroughSequence??checkpoint;
  const av=v.avgFillPrice===null?null:r.numbers.get(JSON.stringify(['avgFillPrice']));
  if(total!==null)need(total===v.totalQuantityFilled&&av===reportedAverage,'FILL_PROJECTION_CHANGED');total=v.totalQuantityFilled;reportedAverage=av;
  for(const [i,f] of v.data.entries()){
   integerAt(r,['data',i,'id'],f.id,1,Number.MAX_SAFE_INTEGER);integerAt(r,['data',i,'quantity'],f.quantity,-100,-1);
   need(!ids.has(f.id)&&f.side==='no','FILL_IDENTITY');ids.add(f.id);const qty=-f.quantity,px=tickAt(r,['data',i,'price'],f.price),at=instant(f.filledAt);
   need(at>=serverCreated&&at<=r.end&&(b.action==='buy'?px<=limit:px>=limit),'FILL_PRICE_OR_TIME');
   fills.push({id:f.id,orderId:b.orderId,marketId:b.marketId,exchangeId:b.exchangeId,tournamentId:b.tournamentId,side:'no',action:b.action,signedQuantity:-qty,quantity:qty,price:money(px),principal:money(px*BigInt(qty)),filledAt:f.filledAt,source:'rest_order_fills',priceBasis:'side_relative'});
  }
  if(pageIndex<fillPages.length-1){need(p.hasMore&&p.nextCursor!==null&&!cursors.has(p.nextCursor),'FILL_CURSOR');cursors.add(p.nextCursor);cursor=p.nextCursor;}else need(p.hasMore===false&&p.nextCursor===null,'FILL_INCOMPLETE');
  fillSources.push(source(r));
 }
 fills.sort((x,y)=>instant(x.filledAt)-instant(y.filledAt)||x.id-y.id);
 if(priorProof?.lastObservedFill&&fills.length){const last=priorProof.lastObservedFill,first=fills[0];need(instant(first.filledAt)>instant(last.filledAt)||first.filledAt===last.filledAt&&first.id>last.fillId,'CONTINUATION_FILL_CLOCK');}
 const quantity=fills.reduce((n,f)=>n+f.quantity,0),principal=fills.reduce((n,f)=>n+amount(f.principal),0n);
 need(quantity<=od.quantity&&total===-quantity&&od.quantityFilled===quantity&&d.quantityTraded<=quantity&&(d.open||d.quantityTraded===quantity),'COMPLETE_FILL_TOTAL');
 const average=averageCheck(reportedAverage,principal,quantity);
 let ackQuantity=d.quantityTraded,ackPrincipal=0n,lastAckPrice=null;
 for(const f of fills){const q=Math.min(ackQuantity,f.quantity);if(q){need(instant(f.filledAt)<=a.end,'ACK_FILL_CLOCK');ackPrincipal+=amount(f.price)*BigInt(q);lastAckPrice=amount(f.price);ackQuantity-=q;}}
 need(ackQuantity===0&&ackPrincipal===ackCost,'ACK_FILL_NOTIONAL');
 let ackFillPriceSummary=null;
 if(d.quantityTraded){
  const matches=[],rawAverage=a.numbers.get(JSON.stringify(['fillPrice']));
  try{if(tickAt(a,['fillPrice'],d.fillPrice)===lastAckPrice)matches.push('last_ack_prefix_fill_tick');}catch{}
  try{averageCheck(rawAverage,ackCost,d.quantityTraded);matches.push('ack_prefix_weighted_average');}catch{}
  need(matches.length>0,'ACK_FILL_PRICE_SUMMARY');
  ackFillPriceSummary={reportedDecimal:rawAverage,matches,principalDerivedFromSummary:false};
 }else need(d.fillPrice===null,'ACK_UNFILLED_PRICE');
 need(instant(post.startedAt)>=lastEnd&&instant(globalRisk.at)>=instant(post.completedAt),'POST_ACCOUNT_CAUSALITY');
 const priorHistory=history(pre),afterHistory=history(post),oid=Number(b.orderId);
 need(!priorHistory.some(o=>o.id===oid)&&same(priorHistory,afterHistory.filter(o=>o.id!==oid)),'EXTERNAL_SCOPED_ACTIVITY');
 const final=afterHistory.find(o=>o.id===oid);need(final,'SCOPED_ORDER_MISSING');
 for(const k of ['id','exchangeId','tournamentId','side','action','quantity','quantityFilled','priceLimit','open','createdAt','expirationDate'])need(same(final[k],od[k]),'SCOPED_ORDER_MISMATCH');
 const signedDelta=b.action==='buy'?-quantity:quantity,remainingQuantity=prior.quantity+(b.action==='buy'?quantity:-quantity);
 need(target(post,b)===-remainingQuantity&&target(post,b)-target(pre,b)===signedDelta&&same(otherPositions(pre,b),otherPositions(post,b)),'POSITION_DELTA_OR_EXTERNAL_ACTIVITY');
 const cashDelta=b.action==='buy'?-principal:principal,deltaLow=amount(post.cashLower)-amount(pre.cashUpper),deltaHigh=amount(post.cashUpper)-amount(pre.cashLower);
 need(cashDelta>=deltaLow&&cashDelta<=deltaHigh,'CASH_INTERVAL_INCONSISTENT');
 const newReceipts=[...pre.sourceReceipts,source(a),source(terminal),...fillSources,...post.sourceReceipts];
 need(new Set(newReceipts.map(r=>r.fetchInvocationId)).size===newReceipts.length,'REUSED_RECEIPT_INVOCATION');
 const lots=prior.lots,removedLots=[],origins=structuredClone(priorProof?.attributedBuyFills??[]);let removedCost=0n;
 if(b.action==='buy')for(const f of fills){const lot={lotId:b.orderId+':'+f.id,orderId:b.orderId,fillId:f.id,intentId:b.intentId,ownerId:b.ownerId,runId:b.runId,tournamentId:b.tournamentId,marketId:b.marketId,exchangeId:b.exchangeId,side:'no',originalQuantity:f.quantity,quantity:f.quantity,entryPrice:f.price,principalCost:f.principal,openedAt:f.filledAt};lots.push(lot);origins.push(structuredClone(lot));}
 else {let left=quantity;while(left){const lot=lots[0];need(lot,'FIFO_OVERSELL');const q=Math.min(left,lot.quantity),cost=price(lot.entryPrice)*BigInt(q);removedCost+=cost;removedLots.push({...lot,quantity:q,principalCost:money(cost)});left-=q;lot.quantity-=q;if(lot.quantity===0)lots.shift();else lot.principalCost=money(price(lot.entryPrice)*BigInt(lot.quantity));}}
 lots.sort((x,y)=>instant(x.openedAt)-instant(y.openedAt)||x.fillId-y.fillId);
 need(origins.length<=10000,'ATTRIBUTION_SIZE');
 const realized=b.action==='sell'?principal-removedCost:0n,remainingCost=lots.reduce((n,l)=>n+amount(l.principalCost),0n);
 const result={schemaVersion:1,mode:REST_NO_MULTISHARE_CONTRACT_V1.mode,readAt,ownerId:b.ownerId,runId:b.runId,tournamentId:b.tournamentId,marketId:b.marketId,exchangeId:b.exchangeId,orderId:b.orderId,intentId:b.intentId,action:b.action,side:'no',bindingHash:payloadHash(b),priorProofHash:priorProof?.verificationHash??null,
  scopedTradeVerified:true,requestedQuantity:b.quantity,canonicalAcceptedQuantity:d.quantity,admissionDiscardedQuantity:b.quantity-d.quantity,terminalReportedQuantity:od.quantity,terminalReportedUnfilledQuantity:od.quantity-quantity,filledQuantity:quantity,terminalUnfilledQuantity:b.quantity-quantity,signedPositionDelta:signedDelta,scopedPrincipalCost:b.action==='buy'?money(principal):'0.000000',scopedProceeds:b.action==='sell'?money(principal):'0.000000',fillNotional:money(principal),averageFillPrice:average,
  removedPrincipalCost:money(removedCost),realizedPnl:money(realized),cumulativeRealizedPnl:money(prior.cumulativeRealized+realized),remainingOwnedQuantity:remainingQuantity,remainingOwnedCost:money(remainingCost),remainingLots:lots,removedLots,attributedBuyFills:origins,fills,lastObservedFill:fills.length?{filledAt:fills.at(-1).filledAt,fillId:fills.at(-1).id}:priorProof?.lastObservedFill??null,
  requestedLimit:money(limit),requestedExpirationDate:b.expirationDate,postExpiryFillObserved:fills.some(f=>instant(f.filledAt)>expires),canonicalAckFilledQuantity:d.quantityTraded,ackNotional:money(ackCost),ackCoversFinalFill:d.quantityTraded===quantity,ackFillPriceSummary,ackEconomics,
  fee:'0.000000',expectedCanonicalTradeCashDelta:money(cashDelta),accountCash:{basis:'rounded_cent_plus_minus_one_cent',before:{lower:pre.cashLower,upper:pre.cashUpper},after:{lower:post.cashLower,upper:post.cashUpper},observedDeltaInterval:{lower:money(deltaLow),upper:money(deltaHigh)},consistent:true,exactDelta:null},
  funding:{reservedFunding,reservedElsewhere,fullyFundedAtSubmission:b.action==='buy'?true:null,inventoryCoveredAtSubmission:b.action==='sell'?true:null},inventory:{priorSignedQuantity:-prior.quantity,remainingSignedQuantity:-remainingQuantity,unbackedSell:false,foreignLotsAdopted:false},
  sourceReceipts:{before:pre.sourceReceipts,dispatch:{...structuredClone(wire),dispatchRecordHash:payloadHash(wire)},ack:source(a),terminal:source(terminal),fills:fillSources,after:post.sourceReceipts},
  globalIntentCensus:{sourceHash:globalRisk.sourceHash,at:globalRisk.at,unresolvedIntentIds:[...globalRisk.unresolvedIntentIds]},globalActivity:{status:'unknown',independentlyComplete:false},globalCashReconciledExactly:false,globalAccountActivityVerified:false,authoritativeReconciliation:false,executionAuthority:false,newSubmissionAuthorized:false};
 return freeze({...result,verificationHash:payloadHash(result)});
}
