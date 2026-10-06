import {createHash} from 'node:crypto';
import {payloadHash} from './core.mjs';
import {parseNumberLexemes} from './exact-money-receipts.mjs';
import {inspectScopedRestNoAccount} from './no-rest-reconciliation.mjs';
import {normalizeOfficialFillTick} from './api-tick-price.mjs';
import {normalizeBook} from '../live-paper/model.mjs';
import {amount,instant} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';

const need=(ok,code)=>{if(!ok)throw Object.assign(Error('PAIRED_EVIDENCE_'+code),{code:'PAIRED_EVIDENCE_'+code});};
const copy=x=>structuredClone(x),sha=x=>createHash('sha256').update(x).digest('hex');
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x),iso=n=>new Date(n).toISOString();
const integer=(v,min=0)=>Number.isSafeInteger(v)&&v>=min;
const fresh=(r,n)=>{const a=instant(r.requestStartedAt),z=instant(r.receivedAt);need(a<=z&&z<=n&&n-a<=20000,'STALE_OR_FUTURE_RECEIPT');};
function sync(fn,...args){const v=fn(...args);need(!v||typeof v.then!=='function','CALLBACK_MUST_BE_SYNCHRONOUS');return copy(v);}
function exactLeg(scope,v){need(v&&scope.markets.some(m=>m.marketId===v.marketId&&m.exchangeId===v.exchangeId),'LEG_SCOPE');return{marketId:v.marketId,exchangeId:v.exchangeId};}

// Read-only adapter over the caller's EXISTING serialized ReadClient. Every GET
// is identity-fenced; no credential, independent client, rate limiter or timer.
// getEvidence is synchronous and never queues a read from a write-admission
// callback. Raw bodies remain private in memory until the scoped writer consumes
// them. Clearing a failed refresh never falls back to an earlier account/book.
export function createPairedRestEvidenceBridge({api,receiptOf,scope,getGlobalRisk,getFunding,getOtherReserved,ownsOrder,canRead,hasReadHeadroom,now=Date.now,maxReadsPerOperation={preparation:23,reconciliation:33,cancel:1}}){
 need(api&&typeof api.dispatch==='function'&&[receiptOf,getGlobalRisk,getFunding,getOtherReserved,ownsOrder,canRead,hasReadHeadroom,now].every(f=>typeof f==='function'),'DEPENDENCIES');
 need(typeof scope?.tournamentId==='string'&&/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(scope.tournamentId)&&/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(scope.tournamentSlug)&&Array.isArray(scope.markets)&&scope.markets.length===2&&scope.markets.every(m=>id(m.marketId)&&id(m.exchangeId))&&new Set(scope.markets.map(m=>m.marketId)).size===2&&new Set(scope.markets.map(m=>m.exchangeId)).size===2,'SCOPE');
 scope=copy(scope);const bounds={preparation:23,reconciliation:33,cancel:1};for(const k of Object.keys(bounds))need(integer(maxReadsPerOperation[k],1)&&maxReadsPerOperation[k]<=bounds[k],'READ_BOUND');maxReadsPerOperation=copy(maxReadsPerOperation);
 let cache=null,busy=false,closed=false,operation=null,reads=0,lastFailure=null,lastEnd=null;const issued=new WeakMap(),receiptWatermarks=new Map(),bookWatermarks=new Map(),projectionWatermarks=new Map();
 const allowed=()=>{need(!closed&&canRead()===true,'STOPPED');};
 const own=(orderId,leg)=>{need(id(orderId)&&sync(ownsOrder,orderId,leg)===true,'ORDER_NOT_OWNED');};
 const clear=()=>{cache=null;};
 function localGlobal(){const g=sync(getGlobalRisk);need(g&&g.tournamentId===scope.tournamentId&&hex(g.sourceHash)&&instant(g.at)<=now()&&now()-instant(g.at)<=20000,'GLOBAL_RISK_CLOCK_OR_SCOPE');return g;}
 function localFunding(){const f=sync(getFunding),other=sync(getOtherReserved);need(typeof other==='string'&&amount(other)>=0n,'OTHER_RESERVE');if(f!==null&&f!==undefined)need(hex(f.reservationHash)&&hex(f.globalLedgerHash)&&typeof f.amount==='string'&&amount(f.amount)>=0n&&instant(f.confirmedAt)<=now()&&now()-instant(f.confirmedAt)<=20000,'FUNDING_CLOCK_OR_SCHEMA');return{funding:f??null,reservedElsewhere:other};}
 function validateAccount(account,leg,n){return inspectScopedRestNoAccount({account,binding:{...scope,exchangeId:leg.exchangeId},readAt:iso(n)});}
 function checkBook(r,leg,n){fresh(r,n);const data=parseNumberLexemes(r.bodyText).data,b=normalizeBook(data,leg.marketId,scope.tournamentId,r.receivedAt,r.requestStartedAt,20000,new Set(),'authoritative_receipt_v1');need((!Object.hasOwn(data,'marketId')||data.marketId===leg.marketId)&&data.contexts.length===1&&data.contexts[0].type==='tournament'&&data.contexts[0].orderbook.exchanges.length===1&&b.exchangeId===leg.exchangeId&&b.bids.length<=20&&b.asks.length<=20,'BOOK_SCOPE');const old=bookWatermarks.get(leg.exchangeId);if(old){need(BigInt(b.sequence)>=BigInt(old.sequence)&&instantNs(b.sourceAt)>=instantNs(old.sourceAt)&&instant(b.requestStartedAt)>=instant(old.requestStartedAt),'BOOK_ROLLBACK');if(b.versionKey===old.versionKey)need(payloadHash([b.bids,b.asks])===payloadHash([old.bids,old.asks]),'BOOK_VERSION_CONFLICT');}return b;}
 function getEvidence(context={}){
  try{allowed();need(!busy&&cache,'CACHE_UNAVAILABLE');if(context.marketId!==undefined)need(context.marketId===cache.leg.marketId&&context.exchangeId===cache.leg.exchangeId,'CACHE_LEG_MISMATCH');if(context.orderId!==undefined&&context.orderId!==null)need(context.orderId===cache.orderId,'CACHE_ORDER_MISMATCH');const n=now();
   if(cache.kind==='preparation'){need(!context.operation||['prepare','place'].includes(context.operation),'CACHE_OPERATION');validateAccount(cache.account,cache.leg,n);const b=checkBook(cache.book,cache.leg,n);need(instant(b.requestStartedAt)>=instant(cache.accountCompletedAt),'BOOK_BEFORE_ACCOUNT');return{tournamentId:scope.tournamentId,account:copy(cache.account),book:copy(cache.book),globalRisk:localGlobal(),...localFunding()};}
   if(cache.kind==='cancel'){need(!context.operation||context.operation==='cancel','CACHE_OPERATION');own(cache.orderId,cache.leg);fresh(cache.order,n);return{order:copy(cache.order)};}
   need(!context.operation||context.operation==='reconcile','CACHE_OPERATION');own(cache.orderId,cache.leg);fresh(cache.order,n);cache.fillPages.forEach(r=>fresh(r,n));validateAccount(cache.after,cache.leg,n);return{order:copy(cache.order),fillPages:copy(cache.fillPages),after:copy(cache.after),globalRisk:localGlobal()};
  }catch(e){clear();lastFailure=/^PAIRED_EVIDENCE_[A-Z_]+$/.test(e.code??'')?e.code:'PAIRED_EVIDENCE_INVALID_CACHE';throw Object.assign(Error(lastFailure),{code:lastFailure});}
 }
 function authorizes(spec){try{allowed();const p=issued.get(spec?.beforeDispatch);return !!p&&spec.method==='GET'&&spec.endpoint===p.endpoint&&spec.body===undefined&&spec.bodyJson===undefined&&spec.beforeNetwork===p.beforeNetwork&&Object.keys(spec).every(k=>['method','endpoint','body','bodyJson','beforeDispatch','beforeNetwork'].includes(k));}catch{return false;}}
 async function read(endpoint){
  allowed();need(reads<maxReadsPerOperation[operation],'READ_BOUND');const budget=(admittedAlready=false)=>need(sync(hasReadHeadroom,maxReadsPerOperation[operation]-reads-Number(admittedAlready))===true,'READ_HEADROOM');budget();let admitted=false;
  const beforeDispatch=()=>{allowed();budget();need(!admitted,'DUPLICATE_ADMISSION');admitted=true;return true;};
  const beforeNetwork=()=>{allowed();need(admitted,'NOT_ADMITTED');budget(true);return true;};
  const spec={method:'GET',endpoint,beforeDispatch,beforeNetwork};issued.set(beforeDispatch,{endpoint,beforeNetwork});const began=now();
  try{const response=await api.dispatch(spec);allowed();need(admitted&&response?.endpoint===endpoint&&(response.status??200)===200,'READ_RESPONSE');
   const r=sync(receiptOf,response,{method:'GET',requestBodyHash:null});need(r?.method==='GET'&&r.endpoint===endpoint&&r.status===200&&typeof r.bodyText==='string'&&Buffer.byteLength(r.bodyText)<=8*1024*1024&&hex(r.sourceBodyHash)&&r.sourceBodyHash===sha(r.bodyText)&&r.requestBodyHash===null&&Object.keys(r).every(k=>['method','endpoint','status','bodyText','sourceBodyHash','requestBodyHash','requestStartedAt','receivedAt','fetchInvocationId'].includes(k)),'RAW_RECEIPT');fresh(r,now());need(instant(r.requestStartedAt)>=began&&instant(r.requestStartedAt)>=instant(response.requestStartedAt)&&instant(r.receivedAt)<=instant(response.receivedAt)&&(lastEnd===null||instant(r.requestStartedAt)>=lastEnd),'RECEIPT_CAUSALITY');need(payloadHash(parseNumberLexemes(r.bodyText).data)===payloadHash(response.data),'RAW_DATA_MISMATCH');
   const prior=receiptWatermarks.get(endpoint);if(prior)need(instant(r.requestStartedAt)>=prior.start&&instant(r.receivedAt)>=prior.end,'RECEIPT_ROLLBACK');receiptWatermarks.set(endpoint,{start:instant(r.requestStartedAt),end:instant(r.receivedAt)});lastEnd=instant(r.receivedAt);return copy(r);
  }finally{reads++;issued.delete(beforeDispatch);}
 }
 async function pages(base,check=()=>{}){let cursor=null,checkpoint=projectionWatermarks.get(base)??null;const all=[],seenCursors=new Set(),ids=new Set();for(let i=0;i<10;i++){
  const r=await read(base+(cursor===null?'':'&cursor='+encodeURIComponent(cursor))),d=parseNumberLexemes(r.bodyText).data,p=d?.pagination;
  need(Array.isArray(d?.data)&&d.data.length<=200&&p?.limit===200&&typeof p.hasMore==='boolean'&&(p.nextCursor===null||typeof p.nextCursor==='string'&&p.nextCursor.length>0&&p.nextCursor.length<=1000)&&d.coverage?.complete===true&&(d.coverage.projectedThroughSequence===null||integer(d.coverage.projectedThroughSequence)),'PAGES_INCOMPLETE');
  if(checkpoint!==null&&d.coverage.projectedThroughSequence!==null)need(d.coverage.projectedThroughSequence>=checkpoint,'PROJECTION_ROLLBACK');checkpoint=d.coverage.projectedThroughSequence??checkpoint;
  for(const row of d.data){need(integer(row.id,1)&&!ids.has(row.id),'PAGE_DUPLICATE_ID');ids.add(row.id);}check(d,r);all.push(r);if(!p.hasMore){need(p.nextCursor===null,'PAGE_CURSOR');if(checkpoint!==null)projectionWatermarks.set(base,checkpoint);return all;}need(p.nextCursor!==null&&!seenCursors.has(p.nextCursor),'PAGE_CURSOR');seenCursors.add(p.nextCursor);cursor=p.nextCursor;
 }throw Object.assign(Error('PAIRED_EVIDENCE_PAGES_INCOMPLETE'),{code:'PAIRED_EVIDENCE_PAGES_INCOMPLETE'});}
 async function account(leg){const cash=await read('/tournaments/'+scope.tournamentSlug),positions=await read('/tournaments/'+scope.tournamentSlug+'/portfolio/positions'),openOrdersPages=await pages('/orders?status=open&limit=200&tournamentId='+scope.tournamentId),scopedOrdersPages=await pages('/orders?status=all&exchangeId='+leg.exchangeId+'&limit=200&tournamentId='+scope.tournamentId);const result={cash,positions,openOrdersPages,scopedOrdersPages};validateAccount(result,leg,now());return result;}
 function orderCheck(r,orderId,leg,open){const d=parseNumberLexemes(r.bodyText).data;need(integer(d?.id,1)&&String(d.id)===orderId&&d.exchangeId===leg.exchangeId&&d.tournamentId===scope.tournamentId&&d.side==='no'&&d.action==='buy'&&d.quantity===1&&d.open===open&&instant(d.createdAt)<=now()+2000&&instant(d.expirationDate)>instant(d.createdAt),'ORDER_SCOPE_OR_PHASE');normalizeOfficialFillTick(d.priceLimit);if(!open)need(integer(d.quantityFilled)&&d.quantityFilled<=1,'TERMINAL_QUANTITY');return d;}
 async function run(kind,legValue,orderId,fn){need(!busy,'BUSY');clear();allowed();const leg=exactLeg(scope,legValue);if(orderId!==null)own(orderId,leg);busy=true;operation=kind;reads=0;lastEnd=null;lastFailure=null;try{need(sync(hasReadHeadroom,maxReadsPerOperation[kind])===true,'READ_HEADROOM');const value=await fn(leg);allowed();cache=value;return{kind,leg:copy(leg),orderId,completedAt:iso(now()),readCount:reads};}catch(e){clear();lastFailure=/^PAIRED_EVIDENCE_[A-Z_]+$/.test(e.code??'')?e.code:'PAIRED_EVIDENCE_READ_FAILED';throw Object.assign(Error(lastFailure),{code:lastFailure});}finally{busy=false;operation=null;}}
 return{authorizes,getEvidence,
  refreshPreparation:leg=>run('preparation',leg,null,async l=>{const a=await account(l),info=validateAccount(a,l,now()),book=await read('/markets/'+l.marketId+'/orderbook?tournamentId='+scope.tournamentId+'&depth=20'),b=checkBook(book,l,now());need(instant(b.requestStartedAt)>=instant(info.completedAt),'BOOK_BEFORE_ACCOUNT');validateAccount(a,l,now());bookWatermarks.set(l.exchangeId,b);return{kind:'preparation',leg:l,orderId:null,account:a,accountCompletedAt:info.completedAt,book};}),
  refreshReconciliation:async(orderId,leg)=>{await run('reconciliation',leg,orderId,async l=>{const order=await read('/orders/'+orderId),od=orderCheck(order,orderId,l,false);let totals=null;
   const fillPages=await pages('/orders/'+orderId+'/fills?limit=200',(d,r)=>{need(d.orderId===Number(orderId)&&d.exchangeId===l.exchangeId&&d.tournamentId===scope.tournamentId&&[0,-1].includes(d.totalQuantityFilled),'FILL_SCOPE');const avg=d.avgFillPrice===null?null:normalizeOfficialFillTick(d.avgFillPrice).price,identity=payloadHash([d.totalQuantityFilled,avg]);need(totals===null||totals===identity,'FILL_TOTAL_CHANGED');totals=identity;for(const f of d.data){need(f.side==='no'&&f.quantity===-1&&instant(f.filledAt)>=instant(od.createdAt)&&instant(f.filledAt)<=instant(r.receivedAt),'FILL_SCHEMA');normalizeOfficialFillTick(f.price);}});
   const decoded=fillPages.map(r=>parseNumberLexemes(r.bodyText).data),fills=decoded.flatMap(d=>d.data),last=decoded.at(-1);need(fills.length<=1&&fills.length===od.quantityFilled&&last.totalQuantityFilled===-fills.length&&(fills.length===0?last.avgFillPrice===null:normalizeOfficialFillTick(last.avgFillPrice).price===normalizeOfficialFillTick(fills[0].price).price&&amount(normalizeOfficialFillTick(fills[0].price).price)<=amount(normalizeOfficialFillTick(od.priceLimit).price)),'FILL_TOTAL_MISMATCH');
   const after=await account(l);need(instant(after.cash.requestStartedAt)>=instant(fillPages.at(-1).receivedAt),'POST_ACCOUNT_CAUSALITY');return{kind:'reconciliation',leg:l,orderId,order,fillPages,after};});return getEvidence({operation:'reconcile',orderId,...exactLeg(scope,leg)});},
  refreshCancel:async(orderId,leg)=>run('cancel',leg,orderId,async l=>{const order=await read('/orders/'+orderId);orderCheck(order,orderId,l,true);return{kind:'cancel',leg:l,orderId,order};}),
  status:()=>({mode:'paired_rest_evidence_bridge_v1',busy,closed,cachedKind:cache?.kind??null,cachedLeg:cache?.leg?copy(cache.leg):null,lastReadCount:reads,lastFailure,additionalApiClients:0,executionAuthority:false}),
  clear,close(){need(!busy,'BUSY');clear();closed=true;receiptWatermarks.clear();bookWatermarks.clear();projectionWatermarks.clear();}
 };
}
