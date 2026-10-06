import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {payloadHash,canonicalPayload} from './core.mjs';
import {acquireOwnerLease} from './ledger.mjs';
import {createPairedPortfolio,reservePairedOrder,applyReconciledPairedOrder,PAIRED_EXECUTION_LIMITS} from './paired-portfolio.mjs';
import {openCanonicalOrderJournal} from './canonical-order-journal.mjs';
import {verifyNoBuyEconomics} from './no-order-economics.mjs';
import {Journal} from '../live-paper/transport-runtime.mjs';
import {validateDualNoReview} from '../live-paper/dual-no-review-v1.mjs';
import {amount,money,price,instant,SCALE,TICK} from '../paper/fixed.mjs';
import {instantNs,timestamp} from '../feed/contract.mjs';

// A draft risk envelope, NOT an activation. No client, key, POST/DELETE permit,
// timer or runtime path is accepted by this module. The old portfolio's 25 cash
// units remain a technical accounting baseline, never allocated pilot capital.
export const PAIRED_DRY_RUN_POLICY=Object.freeze({schemaVersion:1,quantity:1,maxPairs:1,
 allocatedCapital:'2.000000',maxOrderCost:'1.000000',maxCumulativeBuy:'2.000000',
 maxFirstLegOrphanPrincipal:'0.500000',minimumOrdinaryMargin:'0.010000',fee:'0.000000',
 lifetimeMs:30000,maxAccountAgeMs:20000,maxBookAgeMs:20000,maxBookSkewMs:10000,depthFractionBps:100});
export const PAIRED_LIVE_CONTRACT_GAPS=Object.freeze([
 'No activation, shared-account lease binding or write authorization is supplied by this package.',
 'The local full-pair reserve must be installed in the sole collector global funding ledger before any future live adapter can dispatch.',
 'Tournament myBalance is documented rounded to two decimals; numeric lexemes do not prove exact gross cash.',
 'A separately verified exact gross-cash producer and complete economic-activity window are required.',
 'REST NO fills are documented side-relative; the existing canonical FIFO accepts NO only from YES-denominated realtime_account_batch. No relabeling is performed.',
 'Two legs are sequential and non-atomic. An unfilled second leg leaves the first owned share at risk.',
 'Ordinary-election minimum payout is conditional; administrative/void outcomes have no proven unconditional pair floor.'
]);
const need=(x,c)=>{if(!x)throw Error('PAIRED_DRY_'+c);};
const clone=x=>structuredClone(x),hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const token=x=>typeof x==='string'&&/^[A-Za-z0-9_.:-]{1,120}$/.test(x)&&!['__proto__','prototype','constructor'].includes(x);
const keys=(x,allowed)=>x!==null&&typeof x==='object'&&!Array.isArray(x)&&Object.getPrototypeOf(x)===Object.prototype&&Object.keys(x).every(k=>allowed.includes(k));
const sha=x=>createHash('sha256').update(x).digest('hex');
const dir=path.dirname(fileURLToPath(import.meta.url));
function codeIdentity(){const files=new Map();const visit=file=>{if(files.has(file))return;const raw=fs.readFileSync(file),source=raw.toString();files.set(file,sha(raw));for(const match of source.matchAll(/(?:from\s*|import\s*)['"](\.[^'"]+)['"]/g))visit(path.resolve(path.dirname(file),match[1]));};visit(fileURLToPath(import.meta.url));return Object.fromEntries([...files].map(([file,hash])=>[path.relative(dir,file).replaceAll('\\','/'),hash]).sort(([a],[b])=>a.localeCompare(b)));}

function quote(book,leg,readAt){
 const now=instant(readAt);need(book?.marketId===leg.marketId&&book.exchangeId===leg.exchangeId&&book.instrument===leg.exchangeId+':YES'&&book.freshnessPolicy==='authoritative_receipt_v1','BOOK_SCOPE');
 need(keys(book,['marketId','exchangeId','instrument','receivedAt','requestStartedAt','sourceAt','sequence','versionKey','bids','asks','freshnessPolicy','sourceEventAgeMs']),'BOOK_SCHEMA');
 const start=instant(book.requestStartedAt),end=instant(book.receivedAt);
 need(start<=end&&end<=now&&now-start<=PAIRED_DRY_RUN_POLICY.maxBookAgeMs,'BOOK_STALE');
 need(typeof book.sequence==='string'&&/^(0|[1-9]\d*)$/.test(book.sequence)&&book.sourceAt===timestamp(book.sourceAt)&&book.versionKey===book.sequence+':'+book.sourceAt&&instantNs(book.sourceAt)<=BigInt(end+2000)*1000000n,'BOOK_VERSION');
 for(const [name,descending] of [['bids',true],['asks',false]]){
  const rows=book[name];need(Array.isArray(rows)&&rows.length>0&&rows.length<=20,'BOOK_DEPTH');let last=null;
  for(const l of rows){const p=price(l.price);need(keys(l,['price','quantity'])&&Number.isSafeInteger(l.quantity)&&l.quantity>0&&(last===null||(descending?p<last:p>last)),'BOOK_LEVEL');last=p;}
 }
 need(price(book.bids[0].price)<price(book.asks[0].price),'BOOK_CROSSED');
 const level=book.bids.find(l=>Math.floor(l.quantity/100)>=1);need(level,'ONE_PERCENT_DEPTH');
 const raw=SCALE-price(level.price),limit=raw+TICK;need(limit<=SCALE-TICK,'NO_LIMIT');
 return{raw,limit,depth:Math.floor(level.quantity/100),start,end};
}
export function planPairedDryRun({capture,scope,readAt}){
 const now=instant(readAt);need(capture?.phase==='complete'&&capture.executionAuthority===false&&capture.orders===0&&capture.fills===0&&token(capture.name),'CAPTURE_COMPLETE');
 validateDualNoReview(capture.review);const review=capture.review;
 need(scope?.ownerId==='Codex'&&token(scope.runId)&&scope.tournamentId===review.tournamentId&&/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(scope.tournamentSlug)&&payloadHash(scope.markets)===payloadHash(capture.legs),'SCOPE');
 need(Array.isArray(capture.books)&&capture.books.length===2&&Array.isArray(capture.roots)&&capture.roots.length===2&&payloadHash(capture.roots)===review.evidenceHash,'CAPTURE_HASH');
 need(now>=instant(review.recordedAt)&&now<instant(review.expiresAt),'REVIEW_EXPIRED');
 const quotes=capture.legs.map((l,i)=>{
  const root=capture.roots[i],r=review.legs[i],b=capture.books[i];
  need(l.marketId===r.marketId&&l.exchangeId===r.exchangeId&&root.marketId===l.marketId&&root.tournamentId===scope.tournamentId&&root.rootHash===payloadHash(root.root)&&root.rootHash===r.rulesHash&&payloadHash(root.root)===payloadHash(r.root)&&hex(root.responseHash)&&hex(b.responseHash),'ROOT_BINDING');
  need(instant(root.requestStartedAt)<=instant(root.receivedAt)&&instant(root.receivedAt)<=instant(review.recordedAt)&&now-instant(root.receivedAt)<=300000,'ROOT_CLOCK');
  need(b.marketId===l.marketId&&b.requestStartedAt===b.book.requestStartedAt&&b.receivedAt===b.book.receivedAt,'BOOK_WRAPPER');
  const q=quote(b.book,l,readAt);need(q.start>=instant(review.recordedAt),'RULES_BEFORE_BOOK');return q;
 });
 need(Math.max(...quotes.map(q=>q.end))-Math.min(...quotes.map(q=>q.start))<=PAIRED_DRY_RUN_POLICY.maxBookSkewMs,'BOOK_SKEW');
 const total=quotes.reduce((n,q)=>n+q.limit,0n);need(total<=SCALE-amount(PAIRED_DRY_RUN_POLICY.minimumOrdinaryMargin),'ORDINARY_MARGIN');
 const order=[0,1].sort((a,b)=>quotes[a].limit===quotes[b].limit?Number(capture.legs[a].exchangeId)-Number(capture.legs[b].exchangeId):Number(quotes[a].limit-quotes[b].limit));
 need(quotes[order[0]].limit<=amount(PAIRED_DRY_RUN_POLICY.maxFirstLegOrphanPrincipal),'ORPHAN_BUDGET');
 const body={schemaVersion:1,mode:'paired_no_dry_run_v1',name:capture.name,scope:clone(scope),policy:PAIRED_DRY_RUN_POLICY,
  createdAt:readAt,reviewExpiresAt:review.expiresAt,captureHash:payloadHash(capture),reviewHash:payloadHash(review),
  legs:order.map(i=>({...capture.legs[i],originalLimit:money(quotes[i].limit),originalRawNoAsk:money(quotes[i].raw),eligibleDepth:quotes[i].depth,book:clone(capture.books[i].book)})),
  pairCostCeiling:money(total),conditionalOrdinaryMinimumPayout:'1.000000',conditionalOrdinarySurplus:money(SCALE-total),
  unconditionalPayoutFloor:null,exceptionLoss:money(-total),priceBasis:'side_relative_no_limit_from_yes_bid_complement',executionAuthority:false};
 return {...body,planHash:payloadHash(body)};
}
function planCheck(plan){const {planHash,...body}=plan??{};need(hex(planHash)&&planHash===payloadHash(body)&&body.mode==='paired_no_dry_run_v1'&&payloadHash(body.policy)===payloadHash(PAIRED_DRY_RUN_POLICY)&&body.executionAuthority===false&&body.legs?.length===2,'PLAN_IDENTITY');
 need(keys(body,['schemaVersion','mode','name','scope','policy','createdAt','reviewExpiresAt','captureHash','reviewHash','legs','pairCostCeiling','conditionalOrdinaryMinimumPayout','conditionalOrdinarySurplus','unconditionalPayoutFloor','exceptionLoss','priceBasis','executionAuthority'])&&body.schemaVersion===1&&token(body.name)&&hex(body.captureHash)&&hex(body.reviewHash)&&instant(body.reviewExpiresAt)>instant(body.createdAt),'PLAN_SCHEMA');
 need(keys(body.scope,['ownerId','runId','tournamentId','tournamentSlug','markets'])&&body.scope.ownerId==='Codex'&&token(body.scope.runId)&&body.scope.markets?.length===2&&body.scope.markets.every(m=>keys(m,['marketId','exchangeId']))&&payloadHash([...body.scope.markets].sort((a,b)=>Number(a.marketId)-Number(b.marketId)))===payloadHash(body.legs.map(({marketId,exchangeId})=>({marketId,exchangeId})).sort((a,b)=>Number(a.marketId)-Number(b.marketId))),'PLAN_SCOPE');
 const quotes=body.legs.map(l=>{need(keys(l,['marketId','exchangeId','originalLimit','originalRawNoAsk','eligibleDepth','book']),'PLAN_LEG_SCHEMA');const q=quote(l.book,l,body.createdAt);need(money(q.limit)===l.originalLimit&&money(q.raw)===l.originalRawNoAsk&&q.depth===l.eligibleDepth,'PLAN_LEG_PRICE');return q;});
 const cost=quotes[0].limit+quotes[1].limit;need(cost===amount(body.pairCostCeiling)&&cost<=990000n&&quotes[0].limit<=quotes[1].limit&&quotes[0].limit<=500000n&&body.conditionalOrdinaryMinimumPayout==='1.000000'&&body.unconditionalPayoutFloor===null&&body.conditionalOrdinarySurplus===money(SCALE-cost)&&body.exceptionLoss===money(-cost)&&body.priceBasis==='side_relative_no_limit_from_yes_bid_complement','PLAN_BUDGET');
}
function accountCheck(a,plan,readAt){
 const now=instant(readAt);need(a?.tournamentId===plan.scope.tournamentId&&a.complete===true&&Array.isArray(a.openOrders)&&a.openOrders.length===0&&hex(a.positionsSourceHash),'ACCOUNT_COMPLETE');
 need(keys(a,['tournamentId','complete','positionsSourceHash','positions','openOrders','startedAt','completedAt','cashPrecision','cashReceipt'])&&Array.isArray(a.positions)&&a.positions.every(p=>keys(p,['marketId','exchangeId','quantity'])),'ACCOUNT_SCHEMA');
 need(instant(a.startedAt)<=instant(a.completedAt)&&instant(a.completedAt)<=now&&now-instant(a.startedAt)<=20000,'ACCOUNT_STALE');
 // This is an explicit upstream proof contract. A hash/boolean is not an
 // independent audit of its producer; dry-run output never grants authority.
 const p=a.cashPrecision;need(keys(p,['basis','scale','rounded','sourceHash'])&&p?.basis==='gross_cash'&&p.scale===6&&p.rounded===false&&hex(p.sourceHash)&&p.sourceHash===a.cashReceipt?.sourceBodyHash,'EXACT_GROSS_CASH_PRODUCER_REQUIRED');
 need(keys(a.cashReceipt,['kind','tournamentId','joined','method','endpoint','sourceBodyHash','receivedAt','fields'])&&keys(a.cashReceipt.fields,['myBalance'])&&keys(a.cashReceipt.fields.myBalance,['available','decimal']),'CASH_SCHEMA');
 need(amount(a.cashReceipt.fields.myBalance.decimal)>=amount(plan.pairCostCeiling),'FULL_PAIR_FUNDING');
}
function globalCheck(g,plan,readAt,ownIntent=null){
 const now=instant(readAt);need(g?.tournamentId===plan.scope.tournamentId&&g.ownerId===plan.scope.ownerId&&g.runId===plan.scope.runId&&g.complete===true&&g.soleCollector===true&&g.stopped===false&&hex(g.sourceHash)&&Array.isArray(g.unresolvedIntentIds)&&g.unresolvedIntentIds.every(token)&&g.unresolvedIntentIds.every(id=>id===ownIntent),'GLOBAL_UNKNOWN_OR_OWNER');
 need(keys(g,['tournamentId','ownerId','runId','complete','soleCollector','stopped','sourceHash','unresolvedIntentIds','at']),'GLOBAL_SCHEMA');
 need(instant(g.at)<=now&&now-instant(g.at)<=20000,'GLOBAL_STALE');
}
function nextTerms(s){
 need(!s.portfolio.active&&s.portfolio.completed.length<2,'ACTIVE_OR_COMPLETE');
 if(s.portfolio.completed.length===0)return{leg:s.plan.legs[0],limit:s.plan.legs[0].originalLimit,index:0};
 const first=s.portfolio.owned[s.plan.legs[0].exchangeId];need(first.lots.reduce((n,l)=>n+l.quantity,0)===1,'FIRST_LEG_UNFILLED');
 const paid=first.lots.reduce((n,l)=>n+amount(l.entryPrice)*BigInt(l.quantity),0n),remaining=amount(s.plan.pairCostCeiling)-paid;
 need(remaining>=TICK&&remaining<=SCALE-TICK,'SECOND_REMAINING_CAP');return{leg:s.plan.legs[1],limit:money(remaining),index:1};
}
function fence(s,{tournamentId,account,globalRisk,book,readAt},prepared=false){
 need(tournamentId===s.plan.scope.tournamentId,'BOOK_TOURNAMENT_SCOPE');
 accountCheck(account,s.plan,readAt);const binding=s.portfolio.active?.preparation.binding;
 globalCheck(globalRisk,s.plan,readAt,prepared?s.portfolio.active.preparation.intentId:null);
 need(instant(readAt)<instant(s.plan.reviewExpiresAt),'REVIEW_EXPIRED');
 const terms=prepared?{leg:binding,limit:binding.limit}:nextTerms(s),q=quote(book,terms.leg,readAt);
 const prior=s.lastBooks[book.exchangeId];need(BigInt(book.sequence)>=BigInt(prior.sequence)&&instantNs(book.sourceAt)>=instantNs(prior.sourceAt)&&instant(book.requestStartedAt)>=instant(prior.requestStartedAt),'BOOK_ROLLBACK');
 if(book.versionKey===prior.versionKey)need(payloadHash([book.bids,book.asks])===payloadHash([prior.bids,prior.asks]),'BOOK_VERSION_CONFLICT');
 need(instant(account.completedAt)>=instant(s.accountCompletedAt),'ACCOUNT_CLOCK_ROLLBACK');
 need(instant(account.completedAt)<=q.start,'ACCOUNT_NOT_KNOWN_BEFORE_BOOK');
 need(q.raw<=price(terms.limit),'SECOND_LEG_PRICE_EXCEEDS_CAP');
 if(prepared){need(instant(readAt)<instant(binding.expirationDate),'ORDER_EXPIRED');
  // Core reserve performs exact external/own position agreement and cash tests.
  const clear=clone(s.portfolio);clear.active=null;
  reservePairedOrder(clear,{...binding,intentId:s.portfolio.active.preparation.intentId,idempotencyKey:s.portfolio.active.preparation.idempotencyKey,createdAt:readAt},account,readAt);
 }
 return terms;
}
function reconcile(s,event){
 accountCheck(event.after,s.plan,event.readAt);
 need(keys(event.activity,['tournamentId','complete','sourceHash','from','through','events'])&&Array.isArray(event.activity.events)&&event.activity.events.every(e=>keys(e,['kind','fillId','orderId','exchangeId','marketId','at'])),'ACTIVITY_SCHEMA');
 const before=s.portfolio.active.before,order=event.orderStatus.state;
 let economics=null;
 if(order.evidence.ack.quantityTraded===1){
  const snapshot=a=>({tournamentId:a.tournamentId,complete:a.complete,cashBasis:a.cashPrecision.basis,cash:a.cashReceipt.fields.myBalance.decimal,positions:a.positions.filter(p=>p.quantity!==0).map(p=>({marketId:p.marketId,exchangeId:p.exchangeId,signedQuantity:p.quantity})),openOrders:a.openOrders,startedAt:a.startedAt,completedAt:a.completedAt});
  economics=verifyNoBuyEconomics({binding:{...order.binding,orderId:order.orderId,sideRelativeLimit:order.binding.limit,preparedAt:order.binding.createdAt},ack:{...order.evidence.ack,tournamentId:s.plan.scope.tournamentId},fills:{complete:true,orderId:order.orderId,exchangeId:order.binding.exchangeId,tournamentId:s.plan.scope.tournamentId,rows:order.evidence.rows.map(f=>({...f,orderId:String(f.orderId)}))},before:snapshot(before),after:snapshot(event.after),activity:{...event.activity,rows:event.activity.events},readAt:event.readAt});
 }
 const portfolio=applyReconciledPairedOrder(s.portfolio,event.orderStatus,event.after,event.activity,event.readAt);
 need(amount(portfolio.cumulativeBuy)<=amount(s.plan.pairCostCeiling)&&amount(portfolio.cumulativeBuy)<=amount(PAIRED_DRY_RUN_POLICY.maxCumulativeBuy),'CUMULATIVE_BUDGET');
 return{...s,portfolio,accountCompletedAt:event.after.completedAt,reconciliations:[...s.reconciliations,{orderId:order.orderId,economics,activityHash:payloadHash(event.activity)}]};
}
function reduce(s,e){
 if(e.kind==='initialize'){need(keys(e,['kind','plan','account','globalRisk','readAt']),'EVENT_SCHEMA');need(s===null,'ALREADY_INITIALIZED');planCheck(e.plan);accountCheck(e.account,e.plan,e.readAt);globalCheck(e.globalRisk,e.plan,e.readAt);return{plan:clone(e.plan),portfolio:createPairedPortfolio(e.plan.scope,e.account,e.readAt),lastBooks:Object.fromEntries(e.plan.legs.map(l=>[l.exchangeId,clone(l.book)])),accountCompletedAt:e.account.completedAt,reconciliations:[],executionAuthority:false};}
 need(s,'NOT_INITIALIZED');
 if(e.kind==='prepare'){
  need(keys(e,['kind','tournamentId','account','globalRisk','book','readAt']),'EVENT_SCHEMA');
  const t=fence(s,e),intentId=s.plan.name+':leg:'+t.index,key='dry:'+s.plan.planHash.slice(0,40)+':'+t.index;
  need(price(t.limit)<=amount(PAIRED_DRY_RUN_POLICY.maxOrderCost),'ORDER_CAP');
  const portfolio=reservePairedOrder(s.portfolio,{intentId,idempotencyKey:key,marketId:t.leg.marketId,exchangeId:t.leg.exchangeId,action:'buy',quantity:1,limit:t.limit,createdAt:e.readAt,expirationDate:new Date(instant(e.readAt)+30000).toISOString()},e.account,e.readAt);
  return{...s,portfolio,lastBooks:{...s.lastBooks,[t.leg.exchangeId]:clone(e.book)},accountCompletedAt:e.account.completedAt};
 }
 need(e.kind==='reconcile'&&s.portfolio.active&&keys(e,['kind','orderStatus','after','activity','readAt']),'EVENT_OR_ACTIVE');return reconcile(s,e);
}

// Local journal ownership is deliberately separate from the live account lease.
// All operations below simulate/record a dry run. No shared dispatcher is called.
export function openPairedDryRun({dir:folder,plan,account,globalRisk,now=Date.now,canCommit=()=>true}={}){
 need(path.isAbsolute(folder??'')&&typeof now==='function'&&typeof canCommit==='function','DEPENDENCIES');
 planCheck(plan);plan=clone(plan);const lease=acquireOwnerLease({dir:folder,ownerId:'Codex',runId:plan.scope.runId,scope:'paired-dry-run',now});
 let closed=false,failed=false,order=null,state=null,size=0,rawHash,identity;const file=path.join(folder,'paired-dry-run.jsonl'),code=codeIdentity();
 const operate=()=>{need(!closed&&!failed&&canCommit()===true,'STOPPED_OR_POISONED');lease.assertOwnership();};
 try{
  if(!fs.existsSync(file))fs.closeSync(fs.openSync(file,'wx',0o600));
  const bytes=fs.readFileSync(file);need(bytes.length===0||bytes.at(-1)===10,'PARTIAL_JOURNAL');const journal=new Journal(file);
  for(const line of bytes.toString().split('\n').filter(Boolean)){const row=JSON.parse(line);need(payloadHash(row.event.codeIdentity)===payloadHash(code),'CODE_CHANGED');
   if(row.event.input.kind==='reconcile'){const reference=row.event.input.orderStatus,prior=openCanonicalOrderJournal({dir:path.join(folder,'leg-'+state.portfolio.completed.length),readOnly:true});try{need(prior.status().hash===reference.hash&&payloadHash(prior.status().state)===payloadHash(reference.state),'ORDER_JOURNAL_CHANGED');}finally{prior.close();}}
   state=reduce(state,row.event.input);need(payloadHash(state)===payloadHash(row.state),'REPLAY_STATE');}
  size=bytes.length;rawHash=sha(bytes);identity=fs.statSync(file);if(state)need(state.plan.planHash===plan.planHash,'PLAN_CHANGED');
  const append=input=>{operate();const prior=fs.readFileSync(file),stat=fs.statSync(file);need(stat.ino===identity.ino&&stat.birthtimeMs===identity.birthtimeMs&&prior.length===size&&sha(prior)===rawHash&&payloadHash(codeIdentity())===payloadHash(code),'JOURNAL_CHANGED');
   const next=reduce(state,input);need(size+Buffer.byteLength(canonicalPayload({input,state:next}))<64*1024*1024,'JOURNAL_SIZE');operate();
   try{journal.commit(next,{codeIdentity:code,input});operate();const after=fs.readFileSync(file);need(after.subarray(0,size).equals(prior)&&after.length>size,'JOURNAL_CHANGED');state=next;size=after.length;rawHash=sha(after);}catch(e){failed=true;throw e;}
  };
  const ensureOrder=()=>{if(!state.portfolio.active)return null;if(!order){const index=state.portfolio.completed.length;order=openCanonicalOrderJournal({dir:path.join(folder,'leg-'+index),canCommit:()=>{operate();return true;}});order.prepare(state.portfolio.active.preparation);}return order;};
  if(!state)append({kind:'initialize',plan,account,globalRisk,readAt:new Date(now()).toISOString()});
  ensureOrder();
  const view=()=>{const p=state.portfolio,filled=p.scope.markets.map(m=>p.owned[m.exchangeId].lots.reduce((n,l)=>n+l.quantity,0)),cost=amount(p.cumulativeBuy),active=order?.status()??null;
   return{mode:'paired_no_dry_run_v1',planHash:plan.planHash,sequence:journal.sequence,hash:journal.previous,closed,failed,
    state:clone(state),order:active,allocatedCapital:PAIRED_DRY_RUN_POLICY.allocatedCapital,allocatedCash:money(amount(PAIRED_DRY_RUN_POLICY.allocatedCapital)-cost),
    accountingBaselineCapital:PAIRED_EXECUTION_LIMITS.capital,pairFundingReserved:money(filled.every(q=>q===1)||p.completed.length===2||p.completed.length===1&&filled.every(q=>q===0)?0n:amount(plan.pairCostCeiling)-cost),
    phase:active?.state?.phase==='uncertain'||active?.state?.phase==='dispatching'?'unknown_blocks_all':p.active?'order_pending':filled.every(q=>q===1)?'paired_reconciled':p.completed.length===2?'orphaned':p.completed.length===1&&filled.every(q=>q===0)?'first_leg_unfilled':'ready',
    executionAuthority:false,ordersSubmitted:0,additionalApiClients:0,liveContractGaps:PAIRED_LIVE_CONTRACT_GAPS};};
  return{authorizes:()=>false,status:view,
   prepareNext(input){operate();append({kind:'prepare',...input});ensureOrder();return{kind:'wouldDispatch',method:'POST',endpoint:'/orders',body:clone(order.status().state.requestBody),bodyJson:canonicalPayload(order.status().state.requestBody),payloadHash:order.status().state.requestHash,executionAuthority:false};},
   checkFence(input){operate();need(state.portfolio.active&&order.status().state.phase==='prepared','NOT_PREPARED');fence(state,input,true);return true;},
   simulateDispatch(input){this.checkFence(input);order.markDispatch();return view();},
   recordUnknown(reason){operate();ensureOrder().uncertain(reason);return view();},
   bind(orderId){operate();ensureOrder().bind(orderId);return view();},
   observe(input){operate();ensureOrder().observe(input);return view();},
   reconcile({after,activity,readAt}){operate();const status=ensureOrder().status();append({kind:'reconcile',orderStatus:status,after,activity,readAt});order.close();order=null;return view();},
   exactRecovery(){operate();const s=ensureOrder()?.status().state;need(s&&['uncertain','dispatching'].includes(s.phase),'NO_UNKNOWN');return{kind:'reconciliation_only',knownOrderId:s.orderId,originalBody:clone(s.requestBody),originalPayloadHash:s.requestHash,idempotencyKey:s.idempotencyKey,replacementAllowed:false,automaticRetryAllowed:false,executionAuthority:false};},
   cancelRequest(readAt){operate();const s=ensureOrder()?.status().state;need(s?.orderId&&['bound','observed'].includes(s.phase)&&instant(readAt)>=instant(s.binding.expirationDate),'CANCEL_NOT_KNOWN_EXPIRED');return{kind:'wouldCancel',method:'DELETE',endpoint:'/orders/'+s.orderId,intentId:s.intentId,requiresFreshScopedReconciliation:true,closureInferred:false,executionAuthority:false};},
   close(){if(closed)return;closed=true;try{order?.close();}finally{lease.release();}}
  };
 }catch(e){try{order?.close();}finally{lease.release();}throw e;}
}
