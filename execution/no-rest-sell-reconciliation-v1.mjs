import {createHash} from 'node:crypto';

import {parseNumberLexemes} from './exact-money-receipts.mjs';
import {normalizeOfficialFillTick} from './api-tick-price.mjs';
import {payloadHash,canonicalPayload} from './core.mjs';
import {amount,money,price,instant} from '../paper/fixed.mjs';

// Standalone, read-only evidence reduction. It does not weaken the older exact
// global-cash verifier or change the realtime YES-denominated fill convention.
// REST NO prices are SIDE RELATIVE in the official specification observed
// 2026-10-06: https://sig.thesuper.market/api/v1/docs (Order/Fill/order fills).
export const REST_NO_SELL_RECONCILIATION_CONTRACT_V1=Object.freeze({schemaVersion:1,
 mode:'scoped_rest_no_owned_exit_with_rounded_cash_v1',quantity:1,fee:'0.000000',
 cashRoundingRadius:'0.010000',maxAccountAgeMs:20000,maxReceiptAgeMs:20000,
 maxOrderLifetimeMs:30000,maxPages:10,maxServerClockSkewMs:2000,priceBasis:'side_relative_rest_no',
 documentationUrl:'https://sig.thesuper.market/api/v1/docs',primarySchemaObservedAt:'2026-10-06T13:55:54.072Z',fillQuantityBasis:'outcome_signed_no_negative_even_when_action_sell',positionDeltaBasis:'covered_no_sell_increases_signed_position_toward_zero',nonNullAllSupported:false,executionAuthority:false});
const need=(x,c)=>{if(!x)throw Error('REST_NO_'+c);};
const hash=x=>createHash('sha256').update(x).digest('hex');
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const integer=(v,min=0)=>Number.isSafeInteger(v)&&v>=min;
const tick=v=>amount(normalizeOfficialFillTick(v).price);
function exactInteger(r,path,value){const lexeme=r.numbers.get(JSON.stringify(path));need(typeof lexeme==='string'&&lexeme.length<=128&&/^-?(0|[1-9]\d*)(\.0+)?$/.test(lexeme)&&Number.isSafeInteger(value)&&BigInt(lexeme.split('.')[0])===BigInt(value),'EXACT_INTEGER_SOURCE');}
function read(r,method,endpoint,now,{fresh=true}={}){
 need(r?.method===method&&r.endpoint===endpoint&&r.status===200&&typeof r.bodyText==='string','RECEIPT_SCOPE');
 const start=instant(r.requestStartedAt),end=instant(r.receivedAt);need(start<=end&&end<=now&&end-start<=20000&&(!fresh||now-end<=20000),'RECEIPT_TIME');
 const sourceBodyHash=hash(r.bodyText);if(r.sourceBodyHash!==undefined)need(r.sourceBodyHash===sourceBodyHash,'RECEIPT_HASH');
 const parsed=parseNumberLexemes(r.bodyText);return{...parsed,start,end,sourceBodyHash,endpoint,requestStartedAt:r.requestStartedAt,receivedAt:r.receivedAt};
}
function scopeCheck(scope){need(typeof scope?.tournamentId==='string'&&scope.tournamentId.length>0&&/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(scope.tournamentSlug),'SCOPE');}
function cash(r,scope,now){
 const v=read(r,'GET','/tournaments/'+scope.tournamentSlug,now);need(v.data?.id===scope.tournamentId&&v.data.isPendingEnrolment===false,'CASH_SCOPE');
 const raw=v.numbers.get(JSON.stringify(['myBalance']));need(raw!==undefined,'CASH_NUMERIC_SOURCE');let displayed;try{displayed=amount(raw);}catch{throw Error('REST_NO_CASH_DECIMAL');}
 need(displayed>=0n&&displayed%10000n===0n,'CASH_NOT_CENT_ROUNDED');
 return{...v,displayed,lower:displayed>10000n?displayed-10000n:0n,upper:displayed+10000n};
}
export function roundedTournamentCashInterval(receipt,{tournamentId,tournamentSlug,readAt,requiredFunding='0.000000',reservedElsewhere='0.000000'}){
 const scope={tournamentId,tournamentSlug};scopeCheck(scope);const c=cash(receipt,scope,instant(readAt)),needed=amount(requiredFunding)+amount(reservedElsewhere);need(amount(requiredFunding)>=0n&&amount(reservedElsewhere)>=0n,'FUNDING_NONNEGATIVE');
 return{basis:'rounded_tournament_cash_interval',displayed:money(c.displayed),lower:money(c.lower),upper:money(c.upper),roundingRadius:'0.010000',requiredFunding,reservedElsewhere,conservativelyFunded:c.lower>=needed,sourceBodyHash:c.sourceBodyHash,receivedAt:c.receivedAt,exactGrossCash:false,executionAuthority:false};
}
function pages(receipts,{endpoint,now,check}){
 need(Array.isArray(receipts)&&receipts.length>0&&receipts.length<=10,'PAGES_INCOMPLETE');let cursor=null,lastEnd=-Infinity,checkpoint=null;const seen=new Set(),rows=[],sources=[];
 for(let i=0;i<receipts.length;i++){
  const r=read(receipts[i],'GET',endpoint+(cursor===null?'':'&cursor='+encodeURIComponent(cursor)),now),d=r.data;
  need(r.start>=lastEnd&&Array.isArray(d?.data)&&d.data.length<=200,'PAGE_CLOCK_OR_DATA');lastEnd=r.end;
  const p=d.pagination;need(p&&p.limit===200&&typeof p.hasMore==='boolean'&&(p.nextCursor===null||typeof p.nextCursor==='string'&&p.nextCursor.length>0&&p.nextCursor.length<=1000),'PAGE_PAGINATION');
  need(d.coverage?.complete===true&&(d.coverage.projectedThroughSequence===null||integer(d.coverage.projectedThroughSequence)),'PAGE_COVERAGE');
  if(checkpoint!==null&&d.coverage.projectedThroughSequence!==null)need(d.coverage.projectedThroughSequence>=checkpoint,'PROJECTION_ROLLBACK');
  checkpoint=d.coverage.projectedThroughSequence??checkpoint;
  check?.(d,r);rows.push(...d.data);sources.push(r);
  if(i<receipts.length-1){need(p.hasMore&&p.nextCursor!==null&&!seen.has(p.nextCursor),'PAGE_CURSOR');seen.add(p.nextCursor);cursor=p.nextCursor;}
  else need(p.hasMore===false&&p.nextCursor===null,'PAGES_INCOMPLETE');
 }
 return{rows,sources,checkpoint};
}
function snapshot(s,scope,now){
 need(s&&Array.isArray(s.openOrdersPages)&&Array.isArray(s.scopedOrdersPages),'ACCOUNT_RECEIPTS');const c=cash(s.cash,scope,now),p=read(s.positions,'GET','/tournaments/'+scope.tournamentSlug+'/portfolio/positions',now);
 need(Array.isArray(p.data?.positions)&&p.data.positions.length<=10000&&p.data.pagination===undefined&&p.data.summary&&['totalMarketValue','totalCostBasis','totalUnrealizedPnl'].every(k=>typeof p.data.summary[k]==='number'&&Number.isFinite(p.data.summary[k])),'POSITIONS_INCOMPLETE');const positions=new Map();
 for(const [i,row]of p.data.positions.entries()){need(id(row.marketId)&&id(row.exchangeId)&&Number.isSafeInteger(row.quantity)&&typeof row.settled==='boolean'&&!positions.has(row.exchangeId),'POSITION_SCHEMA');exactInteger(p,['positions',i,'quantity'],row.quantity);positions.set(row.exchangeId,{marketId:row.marketId,quantity:row.quantity,settled:row.settled});}
 const o=pages(s.openOrdersPages,{endpoint:'/orders?status=open&limit=200&tournamentId='+scope.tournamentId,now});need(o.rows.length===0,'OPEN_ACCOUNT_ORDER');
 const h=pages(s.scopedOrdersPages,{endpoint:'/orders?status=all&exchangeId='+scope.exchangeId+'&limit=200&tournamentId='+scope.tournamentId,now}),history=new Map();
 for(const row of h.rows){need(integer(row.id,1)&&!history.has(row.id)&&row.exchangeId===scope.exchangeId&&row.tournamentId===scope.tournamentId&&['yes','no'].includes(row.side)&&['buy','sell'].includes(row.action)&&integer(row.quantity,1)&&typeof row.open==='boolean'&&instant(row.createdAt)<=h.sources.at(-1).end&&(row.expirationDate===null||Number.isFinite(instant(row.expirationDate))),'SCOPED_ORDER_SCHEMA');
  need(!row.open||row.expirationDate!==null&&instant(row.expirationDate)<=h.sources[0].start,'SCOPED_ORDER_OPEN_CONTRADICTION');history.set(row.id,row);}
 const all=[c,p,...o.sources,...h.sources],start=Math.min(...all.map(v=>v.start)),end=Math.max(...all.map(v=>v.end));need(now-start<=20000,'ACCOUNT_STALE');
 return{cash:c,positions,history,start,end,sources:all.map(v=>({endpoint:v.endpoint,requestStartedAt:v.requestStartedAt,receivedAt:v.receivedAt,sourceBodyHash:v.sourceBodyHash}))};
}
function sameExternal(before,after,exchangeId){
 for(const id of new Set([...before.keys(),...after.keys()]))if(id!==exchangeId){const zero=v=>v?.quantity?v:null;need(canonicalPayload(zero(before.get(id)))===canonicalPayload(zero(after.get(id))),'EXTERNAL_POSITION_CHANGED');}
}
// Shared preflight parser for the bounded paired lifecycle. It exposes curated
// account facts, not a write permit, and uses the same complete receipt checks.
export function inspectScopedRestNoAccount({account,binding,readAt,requiredFunding='0.000000',reservedElsewhere='0.000000'}){
 scopeCheck(binding);need(id(binding.exchangeId),'SCOPE');const a=snapshot(account,binding,instant(readAt));
 const required=amount(requiredFunding)+amount(reservedElsewhere);need(amount(requiredFunding)>=0n&&amount(reservedElsewhere)>=0n,'FUNDING_NONNEGATIVE');
 return{tournamentId:binding.tournamentId,positions:[...a.positions].map(([exchangeId,v])=>({exchangeId,...v})),scopedOrders:[...a.history.values()],startedAt:new Date(a.start).toISOString(),completedAt:new Date(a.end).toISOString(),cashLower:money(a.cash.lower),cashUpper:money(a.cash.upper),funded:a.cash.lower>=required,sourceReceipts:a.sources,executionAuthority:false};
}
// All inputs are supplied by the existing serialized collector. Raw receipt
// bodies are parsed locally but never returned, stored, logged or transmitted.
// Unknown GLOBAL economic activity is retained as unknown. A terminal, scoped
// one-share lot can still have an exact basis; these are different assertions.
export function verifyScopedRestNoSell({binding,before,ack,order,fillPages,after,globalRisk,accountActivity=null,readAt,originalBuyProof,originalAfter}){
 const now=instant(readAt),b=binding;scopeCheck(b);
 need(id(b.orderId)&&id(b.marketId)&&id(b.exchangeId)&&typeof b.ownerId==='string'&&typeof b.runId==='string'&&typeof b.intentId==='string'&&typeof b.idempotencyKey==='string'&&b.side==='no'&&b.action==='sell'&&b.quantity===1,'BINDING');
 const limit=price(b.limit),created=instant(b.createdAt),expires=instant(b.expirationDate);need(expires>created&&expires-created<=30000&&created<=now,'BINDING_LIFETIME');
 need(globalRisk?.tournamentId===b.tournamentId&&globalRisk.ownerId===b.ownerId&&globalRisk.runId===b.runId&&globalRisk.soleCollector===true&&globalRisk.complete===true&&globalRisk.stopped===false&&hex(globalRisk.sourceHash)&&Array.isArray(globalRisk.unresolvedIntentIds)&&globalRisk.unresolvedIntentIds.length<=1&&globalRisk.unresolvedIntentIds.every(x=>x===b.intentId)&&instant(globalRisk.at)<=now&&now-instant(globalRisk.at)<=20000,'GLOBAL_UNKNOWN_OR_OWNER');

 const pre=snapshot(before,b,created),post=snapshot(after,b,now);need(pre.end<=created&&post.start>=created,'ACCOUNT_CAUSALITY');
 const proof=originalBuyProof,{verificationHash,...originalProofBody}=proof??{};
 need(hex(verificationHash)&&payloadHash(originalProofBody)===verificationHash&&['scoped_rest_no_lot_with_rounded_cash_v1','scoped_rest_no_lot_with_rounded_cash_neutral_all_v2'].includes(proof.mode)&&proof.scopedLotVerified===true&&proof.filledQuantity===1&&proof.signedPositionDelta===-1&&proof.ownerId===b.ownerId&&proof.runId===b.runId&&proof.tournamentId===b.tournamentId&&proof.marketId===b.marketId&&proof.exchangeId===b.exchangeId&&proof.executionAuthority===false,'ORIGINAL_BUY_PROVENANCE');
 const lot=proof.lot;need(lot?.side==='no'&&lot.quantity===1&&typeof lot.lotId==='string'&&lot.lotId.length>0&&lot.ownerId===b.ownerId&&lot.runId===b.runId&&lot.orderId===proof.orderId&&lot.intentId===proof.intentId&&lot.entryPrice===proof.scopedPrincipalCost&&Array.isArray(proof.fills)&&proof.fills.length===1&&proof.fills[0].price===lot.entryPrice&&proof.fills[0].side==='no'&&proof.fills[0].signedQuantity===-1&&proof.fills[0].quantity===1&&proof.fills[0].priceBasis==='side_relative'&&proof.fills[0].source==='rest_order_fills'&&proof.fills[0].filledAt===lot.openedAt,'ORIGINAL_BUY_LOT');price(lot.entryPrice);
 const original=snapshot(originalAfter,b,instant(proof.readAt));need(original.end<=pre.start&&original.positions.get(b.exchangeId)?.marketId===b.marketId&&original.positions.get(b.exchangeId).quantity===-1&&original.positions.get(b.exchangeId).settled===false&&Array.isArray(proof.sourceReceipts?.after)&&original.sources.length===proof.sourceReceipts.after.length&&original.sources.every(s=>proof.sourceReceipts.after.some(p=>p.endpoint===s.endpoint&&p.sourceBodyHash===s.sourceBodyHash&&p.receivedAt===s.receivedAt)),'ORIGINAL_AFTER_SOURCE');
 need(canonicalPayload([...original.history])===canonicalPayload([...pre.history]),'TARGET_HISTORY_SINCE_BUY_CHANGED');const originalOrder=pre.history.get(Number(proof.orderId));need(originalOrder?.side==='no'&&originalOrder.action==='buy'&&originalOrder.quantity===1&&originalOrder.quantityFilled===1&&originalOrder.open===false,'ORIGINAL_BUY_ORDER_REQUIRED');

 need(pre.positions.get(b.exchangeId)?.marketId===b.marketId&&pre.positions.get(b.exchangeId).quantity===-1&&pre.positions.get(b.exchangeId).settled===false,'PRIOR_OWNED_NO_REQUIRED');
 const originalBody={idempotencyKey:b.idempotencyKey,exchangeId:b.exchangeId,side:'no',action:'sell',quantity:1,price:Number(b.limit),expirationDate:b.expirationDate,tournamentId:b.tournamentId};
 const a=read(ack,'POST','/orders',now,{fresh:false});need(ack.requestBodyHash===payloadHash(originalBody)&&a.start>=created,'ACK_REQUEST_BINDING');
 need(a.start<expires,'LATE_ACK_ORIGINAL_WIRE_PROOF_NOT_IMPLEMENTED');
 need(a.data?.all===null,'UNSUPPORTED_NO_SELL_ALL');
 const d=a.data;need(integer(d?.orderId,1)&&String(d.orderId)===b.orderId&&d.exchangeId===b.exchangeId&&d.side==='no'&&d.action==='sell'&&d.quantity===1&&tick(d.price)===limit&&typeof d.open==='boolean'&&integer(d.quantityTraded)&&integer(d.remainingQuantity)&&d.quantityTraded+d.remainingQuantity===1,'ACK_CANONICAL');
 for(const field of ['quantity','quantityTraded','remainingQuantity'])exactInteger(a,[field],d[field]);
 const totalLexeme=a.numbers.get(JSON.stringify(['totalCost']));let ackCost;try{ackCost=amount(totalLexeme);}catch{throw Error('REST_NO_EXACT_ACK_NOTIONAL');}need(ackCost>=0n&&ackCost<=995000n&&ackCost%5000n===0n&&(d.quantityTraded===0?ackCost===0n:ackCost>0n&&ackCost>=limit),'ACK_NOTIONAL');
 const terminal=read(order,'GET','/orders/'+b.orderId,now),od=terminal.data;
 need(integer(od?.id,1)&&String(od.id)===b.orderId&&od.exchangeId===b.exchangeId&&od.tournamentId===b.tournamentId&&od.side==='no'&&od.action==='sell'&&od.quantity===1&&integer(od.quantityFilled)&&od.quantityFilled<=1&&tick(od.priceLimit)===limit&&od.open===false&&instant(od.expirationDate)===expires&&instant(od.createdAt)>=created-2000&&instant(od.createdAt)<=Math.min(a.end,expires)+2000,'TERMINAL_ORDER');
 let totals=null,average=null;
 const f=pages(fillPages,{endpoint:'/orders/'+b.orderId+'/fills?limit=200',now,check:(data,receipt)=>{
  need(integer(data.orderId,1)&&String(data.orderId)===b.orderId&&data.exchangeId===b.exchangeId&&data.tournamentId===b.tournamentId&&Number.isSafeInteger(data.totalQuantityFilled)&&[0,-1].includes(data.totalQuantityFilled),'FILLS_SCOPE_OR_TOTAL');
  exactInteger(receipt,['totalQuantityFilled'],data.totalQuantityFilled);for(const [i,row]of data.data.entries())exactInteger(receipt,['data',i,'quantity'],row.quantity);
  const av=data.avgFillPrice===null?null:tick(data.avgFillPrice);if(totals!==null)need(totals===data.totalQuantityFilled&&average===av,'FILL_PROJECTION_CHANGED');totals=data.totalQuantityFilled;average=av;
 }});
 need(f.sources[0].start>=terminal.end,'FILLS_BEFORE_TERMINAL');
 const seen=new Set(),fills=[];let principal=0n,quantity=0;
 for(const row of f.rows){need(integer(row.id,1)&&!seen.has(row.id)&&row.side==='no'&&row.quantity===-1,'FILL_IDENTITY_OR_QUANTITY');seen.add(row.id);
  const at=instant(row.filledAt),p=tick(row.price);need(at>=instant(od.createdAt)&&at<=f.sources.at(-1).end&&at<=now&&p>0n&&p>=limit,'FILL_PRICE_OR_TIME');quantity++;principal+=p;
  fills.push({id:row.id,orderId:b.orderId,marketId:b.marketId,exchangeId:b.exchangeId,tournamentId:b.tournamentId,side:'no',signedQuantity:-1,quantity:1,price:money(p),filledAt:row.filledAt,source:'rest_order_fills',priceBasis:'side_relative'});
 }
 need(quantity<=1&&totals===-quantity&&(quantity===0?average===null:average===principal)&&d.quantityTraded<=quantity,'COMPLETE_FILL_TOTAL');
 need(od.quantityFilled===quantity,'ORDER_FILL_TOTAL');
 if(d.quantityTraded===1)need(principal===ackCost,'ACK_FINAL_NOTIONAL_MISMATCH');
 else need(d.fillPrice===null,'ACK_UNFILLED_PRICE');
 if(d.quantityTraded===1)need(tick(d.fillPrice)===principal,'ACK_FILL_PRICE');
 need(post.start>=Math.max(terminal.end,f.sources.at(-1).end,...fills.map(v=>instant(v.filledAt))),'POST_ACCOUNT_BEFORE_ORDER_EVIDENCE');
 need(instant(globalRisk.at)>=post.end,'GLOBAL_RISK_BEFORE_ACCOUNT');
 const oid=Number(b.orderId);need(!pre.history.has(oid)&&post.history.has(oid),'SCOPED_ORDER_MISSING_OR_PREEXISTING');
 for(const key of new Set([...pre.history.keys(),...post.history.keys()]))if(key!==oid)need(canonicalPayload(pre.history.get(key)??null)===canonicalPayload(post.history.get(key)??null),'EXTERNAL_SCOPED_ORDER_ACTIVITY');
 const finalOrder=post.history.get(oid);for(const key of ['id','exchangeId','tournamentId','side','action','quantity','priceLimit','open','createdAt','expirationDate'])need(canonicalPayload(finalOrder[key])===canonicalPayload(od[key]),'SCOPED_ORDER_DETAIL_MISMATCH');
 need(integer(finalOrder.quantityFilled)&&finalOrder.quantityFilled===quantity,'SCOPED_ORDER_FILL_TOTAL');
 const target=post.positions.get(b.exchangeId);need(quantity===0?target?.marketId===b.marketId&&target.quantity===-1&&target.settled===false:!target?.quantity,'SCOPED_POSITION_DELTA');if(target)need(target.marketId===b.marketId&&target.settled===false,'SCOPED_POSITION_IDENTITY');sameExternal(pre.positions,post.positions,b.exchangeId);
 const deltaLow=post.cash.lower-pre.cash.upper,deltaHigh=post.cash.upper-pre.cash.lower,expected=principal;need(expected>=deltaLow&&expected<=deltaHigh,'CASH_INTERVAL_INCONSISTENT');
 let activityStatus='unknown',activityHash=null;
 if(accountActivity!==null){need(accountActivity.tournamentId===b.tournamentId&&hex(accountActivity.sourceHash)&&typeof accountActivity.complete==='boolean','ACTIVITY_SCHEMA');
  if(accountActivity.complete){need(instant(accountActivity.from)<=pre.start&&instant(accountActivity.through)>=post.end&&instant(accountActivity.through)<=now&&Array.isArray(accountActivity.events),'ACTIVITY_WINDOW');
   need(accountActivity.events.length===fills.length&&accountActivity.events.every(e=>e.kind==='fill'&&e.orderId===b.orderId&&e.marketId===b.marketId&&e.exchangeId===b.exchangeId&&fills.some(v=>v.id===e.fillId&&v.filledAt===e.at))&&new Set(accountActivity.events.map(e=>e.fillId)).size===fills.length,'UNRELATED_OR_MISSING_ACTIVITY');activityStatus='independently_complete_isolated';activityHash=payloadHash(accountActivity);
  }else{need(Array.isArray(accountActivity.events)&&accountActivity.events.every(e=>e.kind==='fill'&&e.orderId===b.orderId&&e.marketId===b.marketId&&e.exchangeId===b.exchangeId&&fills.some(v=>v.id===e.fillId&&v.filledAt===e.at)),'KNOWN_UNRELATED_ACTIVITY');activityStatus='incomplete';activityHash=payloadHash(accountActivity);}
 }
 const sources={before:pre.sources,ack:{endpoint:a.endpoint,sourceBodyHash:a.sourceBodyHash,receivedAt:a.receivedAt,requestBodyHash:ack.requestBodyHash},terminal:{endpoint:terminal.endpoint,sourceBodyHash:terminal.sourceBodyHash,receivedAt:terminal.receivedAt},fills:f.sources.map(r=>({endpoint:r.endpoint,sourceBodyHash:r.sourceBodyHash,receivedAt:r.receivedAt})),after:post.sources};
 const result={schemaVersion:1,mode:REST_NO_SELL_RECONCILIATION_CONTRACT_V1.mode,readAt,bindingHash:payloadHash(b),orderId:b.orderId,intentId:b.intentId,ownerId:b.ownerId,runId:b.runId,tournamentId:b.tournamentId,marketId:b.marketId,exchangeId:b.exchangeId,
  scopedExitVerified:true,filledQuantity:quantity,signedPositionDelta:quantity,scopedProceeds:money(principal),originalBuyProofHash:originalBuyProof.verificationHash,originalLotId:originalBuyProof.lot.lotId,originalPrincipalCost:originalBuyProof.scopedPrincipalCost,removedPrincipalCost:quantity?originalBuyProof.scopedPrincipalCost:'0.000000',realizedPnl:money(principal-(quantity?amount(originalBuyProof.scopedPrincipalCost):0n)),expectedCanonicalTradeCashDelta:money(expected),fills,
  remainingOwnedLot:quantity===0?structuredClone(originalBuyProof.lot):null,canonicalAckFilledQuantity:d.quantityTraded,ackCoversFinalFill:d.quantityTraded===quantity,ackNotional:money(ackCost),
  requestedLimit:money(limit),requestedExpirationDate:b.expirationDate,postExpiryFillObserved:fills.some(v=>instant(v.filledAt)>expires),
  globalIntentCensus:{sourceHash:globalRisk.sourceHash,at:globalRisk.at,unresolvedIntentIds:[...globalRisk.unresolvedIntentIds]},
  inventory:{priorSignedQuantity:-1,remainingSignedQuantity:quantity?0:-1,originalLotId:originalBuyProof.lot.lotId,unbackedSell:false},
  accountCash:{basis:'rounded_cent_with_conservative_plus_minus_one_cent',before:{displayed:money(pre.cash.displayed),lower:money(pre.cash.lower),upper:money(pre.cash.upper)},after:{displayed:money(post.cash.displayed),lower:money(post.cash.lower),upper:money(post.cash.upper)},observedDeltaInterval:{lower:money(deltaLow),upper:money(deltaHigh)},consistent:true,exactDelta:null},
  scopedOrderActivity:{complete:true,beforeOrderCount:pre.history.size,afterOrderCount:post.history.size,newOrderIds:[b.orderId],unrelatedChanges:0},
  globalActivity:{status:activityStatus,evidenceHash:activityHash,independentlyComplete:activityStatus==='independently_complete_isolated'},
  globalCashReconciledExactly:false,globalAccountActivityVerified:activityStatus==='independently_complete_isolated',sourceReceipts:sources,
  authoritativeReconciliation:false,executionAuthority:false,newSubmissionAuthorized:false};
 return{...result,verificationHash:payloadHash(result)};
}
