import fs from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {createExecutionState,validateExecutionState,prepareIntent,beginDispatch,revalidateIntentEvidence,recordPlacement,reconcileIntent,abandonPreparedIntent,decimalMicros,formatMicros,payloadHash} from './core.mjs';
import {openLedger,atomicJson} from './ledger.mjs';
import {OrderTransport} from './transport.mjs';
import {Journal} from '../live-paper/runtime.mjs';
import {normalizeBook,digest} from '../live-paper/model.mjs';
import {historicalReference} from '../live-paper/history.mjs';
import {dipSignal} from '../live-paper/dip-shadow.mjs';

const TID='bda92870-621e-47b0-bc3c-3602c5c26f55',SLUG='midterm-elections';
const micros=x=>decimalMicros(String(x));
const plain=x=>x!==null&&typeof x==='object'&&!Array.isArray(x)&&Object.getPrototypeOf(x)===Object.prototype;
const safeKey=x=>!['__proto__','constructor','prototype'].includes(x);
const instant=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;
const emptyControl=()=>({schemaVersion:1,exits:{},cancellations:{}});
function validateControl(state){
 const c=state.control;
 if(!plain(c)||c.schemaVersion!==1||Object.keys(c).some(k=>!['schemaVersion','exits','cancellations'].includes(k))||!plain(c.exits)||!plain(c.cancellations))throw Error('INVALID_EXECUTION_CONTROL');
 for(const [exchangeId,x]of Object.entries(c.exits)){
  if(!safeKey(exchangeId)||!plain(x)||Object.keys(x).some(k=>!['marketId','reason','triggeredAt','triggerBid','sourceVersion'].includes(k))||!state.owned[exchangeId]||x.marketId!==state.owned[exchangeId].marketId||!['stop_loss','holding_timeout'].includes(x.reason)||!instant(x.triggeredAt)||typeof x.sourceVersion!=='string'||x.sourceVersion.length>256||micros(x.triggerBid)<=0n||micros(x.triggerBid)>=1000000n)throw Error('INVALID_EXIT_OBLIGATION');
 }
 for(const [id,x]of Object.entries(c.cancellations)){
  const i=state.intents[id];
  if(!safeKey(id)||!plain(x)||Object.keys(x).some(k=>!['orderId','attempts','phase','lastAttemptAt','evidenceAt','reasonCode'].includes(k))||!i||x.orderId!==i.orderId||!Number.isSafeInteger(x.orderId)||x.orderId<=0||!Number.isSafeInteger(x.attempts)||x.attempts<1||!['dispatching','uncertain','acknowledged','reconciled'].includes(x.phase)||!instant(x.lastAttemptAt)||!instant(x.evidenceAt)||(i.status==='closed')!==(x.phase==='reconciled')||(x.reasonCode!==undefined&&(typeof x.reasonCode!=='string'||!/^[A-Z][A-Z0-9_]{0,100}$/.test(x.reasonCode))))throw Error('INVALID_CANCEL_CONTROL');
 }
 return state;
}
export function validateActivation(a){
 if(a?.schemaVersion!==1||a.ownerId!=='Codex'||a.tournamentId!==TID||typeof a.runId!=='string'||!/^sig-pilot-[a-z0-9-]+$/.test(a.runId)||a.enabled!==true||a.formerWriterStopCommentId!==5947535773||a.feeBps!==0)throw Error('INVALID_ACTIVATION');
 if(micros(a.maxOrderCost)>25000000n||micros(a.maxOrderCost)<=0n||micros(a.maxAggregateCost)>100000000n||micros(a.maxCumulativeBuyCost)>100000000n||micros(a.maxCumulativeBuyCost)<micros(a.maxOrderCost)||a.maxManagedMarkets!==3||a.depthFractionBps!==100)throw Error('INVALID_PILOT_LIMITS');
 if(!Number.isFinite(Date.parse(a.startedAt))||!Number.isFinite(Date.parse(a.entryExpiresAt))||Date.parse(a.entryExpiresAt)<=Date.parse(a.startedAt)||Date.parse(a.entryExpiresAt)-Date.parse(a.startedAt)>21600000)throw Error('INVALID_ACTIVATION_WINDOW');
 return a;
}

// This host runs inside the only collector, sharing its exact queue and backoff.
// Core and intent journals are both durable; inconsistency blocks all writes.
export class CompetitionController {
 constructor({runtimeDir,api,universe,config,isStopped,activation,now=Date.now}){
  this.dir=path.join(runtimeDir,'competition');this.runtimeDir=runtimeDir;this.api=api;this.universe=universe;this.config=config;this.now=now;this.isStopped=isStopped;
  this.activation=validateActivation(activation);this.activationHash=digest(activation);this.status='reconciling';this.reason=null;this.selected=[];this.cash=null;this.account=null;this.markets={};this.currentPermit=null;this.bootstrapped=false;this.readiness={complete:false};
  this.ledger=openLedger({dir:this.dir,ownerId:'Codex',runId:activation.runId,scope:TID,isStopped:()=>!this.canWrite(),now});
  try{
   this.journal=new Journal(path.join(this.dir,'core-journal.jsonl'));
   this.state=this.journal.last?validateExecutionState(validateControl(this.journal.last.state)):validateExecutionState({...createExecutionState({tournamentId:TID,policy:{maxOrderCost:activation.maxOrderCost,maxAggregateCost:activation.maxAggregateCost,maxCumulativeBuyCost:activation.maxCumulativeBuyCost,maxManagedMarkets:3,cashReserve:'90000.000000'}}),control:emptyControl()});
   if(this.state.tournamentId!==TID||this.state.policy.maxOrderCost!==activation.maxOrderCost||this.state.policy.maxAggregateCost!==activation.maxAggregateCost||this.state.policy.maxCumulativeBuyCost!==activation.maxCumulativeBuyCost)throw Error('CORE_POLICY_MISMATCH');
   this.assertLedgers();
   this.transport=new OrderTransport({scope:{tournamentId:TID,tournamentSlug:SLUG,exchanges:universe.exchanges},dispatch:s=>api.dispatch(s),now,canWrite:()=>this.canWrite(),ownsOrder:id=>Object.values(this.state.intents).some(i=>String(i.orderId)===String(id)),validateLease:lease=>{this.ledger.snapshot();return lease.runId===activation.runId&&lease.ownerId==='Codex'&&lease.scope===TID;},validatePermit:(permit,spec)=>this.admit(permit,spec),validateNetworkPermit:(permit,spec)=>this.networkAdmit(permit,spec)});
   this.lease={ownerId:'Codex',runId:activation.runId,scope:TID};
   this.publish();
  }catch(e){this.ledger.close();throw e;}
 }
 canOperate(){
  if(this.halted||this.isStopped()||fs.existsSync(path.join(this.runtimeDir,'EXECUTION_STOP')))return false;
  try{return digest(JSON.parse(fs.readFileSync(path.join(this.runtimeDir,'competition-activation.json'))))===this.activationHash;}catch{return false;}
 }
 canWrite(){return this.bootstrapped===true&&this.canOperate();}
 checkedBook(response,marketId){
  const b=normalizeBook(response.data,marketId,TID,response.receivedAt,response.requestStartedAt,20000,new Set(),'authoritative_receipt_v1');
  const expected=this.universe.exchanges.filter(x=>x.marketId===marketId);
  if(expected.length!==1||expected[0].exchangeId!==b.exchangeId)throw Error('BOOK_EXCHANGE_IDENTITY_MISMATCH');
  return b;
 }
 async bootstrap(){
  this.bootstrapped=false;this.readiness={complete:false,at:new Date(this.now()).toISOString()};
  try{
   if(!this.canOperate())throw Error('EXECUTION_STOPPED');
   await this.cashRefresh();await this.refreshAccount();
   const marketId='377',market=await this.transport.market(marketId),ctx=market.data.contexts[0];
   if(ctx.status!=='open'||ctx.settledWith!==null||market.data.status!=='open'||market.data.isComposite||market.data.isMultiOutcome)throw Error('BOOTSTRAP_MARKET_NOT_OPEN');
   const response=await this.transport.orderbook(marketId),book=this.checkedBook(response,marketId),now=this.now();
   for(const p of [this.cash,this.account.positions,this.account.orders])if(!Number.isFinite(Date.parse(p.at))||Date.parse(p.at)>now||now-Date.parse(p.at)>this.state.policy.maxAccountAgeMs)throw Error('STALE_BOOTSTRAP_ACCOUNT');
   if(now-Date.parse(book.receivedAt)>this.state.policy.maxBookAgeMs||!this.canOperate())throw Error('BOOTSTRAP_NOT_READY');
   this.bootstrapped=true;this.readiness={complete:true,at:new Date(now).toISOString(),marketId,exchangeId:book.exchangeId,bookReceivedAt:book.receivedAt};this.status='monitoring';this.reason=null;
  }catch(e){this.status='bootstrap_blocked';this.reason=/^[A-Z][A-Z0-9_]+$/.test(e?.code||e?.message||'')?(e.code||e.message):'BOOTSTRAP_READ_UNAVAILABLE';this.readiness={...this.readiness,reason:this.reason};}
  this.publish();return {...this.readiness};
 }
 assertLedgers(){
  const durable=this.ledger.snapshot().intents;
  if(Object.keys(durable).length!==Object.keys(this.state.intents).length)throw Error('EXECUTION_JOURNAL_DIVERGENCE');
  for(const [id,i] of Object.entries(this.state.intents))if(!durable[id]||durable[id].payloadHash!==i.payloadHash||durable[id].status!==i.status||String(durable[id].orderId||'')!==String(i.orderId||''))throw Error('EXECUTION_JOURNAL_DIVERGENCE');
 }
 commit(result,event){const state=validateExecutionState(validateControl(result.state));this.journal.commit(state,event);this.state=state;}
 updateControl(mutator,event){const state=structuredClone(this.state);mutator(state.control);this.commit({state},event);}
 record(result,id,event){
  const i=result.state.intents[id];
  // Only authoritative order/fill/account reconciliation may discharge an exit.
  if(event==='official_reconciliation'){
   const state=structuredClone(result.state);
   if(!state.owned[i.exchangeId])delete state.control.exits[i.exchangeId];
   if(i.status==='closed'&&state.control.cancellations[id])state.control.cancellations[id].phase='reconciled';
   result={...result,state};
  }
  this.ledger.recordOutcome(id,{status:i.status,...(i.orderId?{orderId:String(i.orderId)}:{}),reasonCode:result.code,...(i.status==='uncertain'?{retryAllowed:i.retryAllowed===true,earliestRetryAt:i.retryAfterAt}:{})});
  this.commit(result,{type:event,intentId:id,code:result.code});this.assertLedgers();
 }
 managedMarketIds(){return [...new Set([...Object.values(this.state.owned).map(x=>x.marketId),...Object.values(this.state.intents).filter(x=>x.status!=='closed').map(x=>x.marketId)])];}
 active(){return Object.values(this.state.intents).find(i=>i.status!=='closed');}
 snapshot(book){return {tournamentId:TID,cash:this.cash,positions:this.account?.positions,orders:this.account?.orders,...(book?{book}:{})};}
 async cashRefresh(){const r=await this.transport.tournament();if(r.data.status!=='active'||r.data.isPendingEnrolment)throw Error('TOURNAMENT_NOT_ACTIVE');this.cash={tournamentId:TID,at:r.receivedAt,amount:r.data.myBalance};}
 setAccount(positions,orders,ordersAt){
  if(!Array.isArray(positions?.data?.positions)||!Array.isArray(orders))throw Error('ACCOUNT_INCOMPLETE');
  this.account={positions:{tournamentId:TID,at:positions.receivedAt,complete:true,rows:positions.data.positions.map(x=>({exchangeId:x.exchangeId,marketId:x.marketId,quantity:x.quantity}))},orders:{tournamentId:TID,at:ordersAt,complete:true,rows:orders.map(x=>({...x}))}};
 }
 async refreshAccount(){const p=await this.transport.positions(),o=await this.transport.orders('open');this.setAccount(p,o.data.data,o.receivedAt);}
 async reconcile(i){
  const order=await this.transport.order(i.orderId),fills=await this.transport.fills(i.orderId);
  await this.cashRefresh();await this.refreshAccount();
  const evidenceAt=new Date(Math.min(Date.parse(order.requestStartedAt),Date.parse(fills.requestStartedAt))).toISOString();
  const r=reconcileIntent(this.state,i.intentId,{at:evidenceAt,fillsReceivedAt:fills.receivedAt,snapshot:this.snapshot(),order:order.data,fills:{...fills.data,complete:fills.data.pagination.hasMore===false,rows:fills.data.data}},this.now());
  if(!r.ok){
   this.status='reconciliation_blocked';this.reason=r.code;
   // Legacy detail can remain open past expiry while the open-order collection
   // already filters it out. All earlier core reconciliation checks must pass.
   if(r.code==='OPEN_ORDER_RECONCILIATION_MISMATCH'&&order.data.open&&this.now()>=Date.parse(i.expirationDate))await this.cancelExpired(i,{at:evidenceAt,snapshot:this.snapshot(),order:order.data});
   return;
  }
  this.record(r,i.intentId,'official_reconciliation');this.status=r.intent.status==='closed'?'monitoring':'order_pending';this.reason=r.code;
  if(r.intent.status==='reconciled'&&this.now()>=Date.parse(r.intent.expirationDate))await this.cancelExpired(r.intent,{at:evidenceAt,snapshot:this.snapshot(),order:order.data});
 }
 async beforeBooks({positions,orders,ordersAt,candidateMarketIds}){
  this.selected=[];this.currentPermit=null;this.assertLedgers();
  if(!this.canOperate()){this.status='stopped';this.publish();return;}
  const needsBootstrap=!this.bootstrapped;
  if(needsBootstrap&&!(await this.bootstrap()).complete)return;
  if(!needsBootstrap)this.setAccount(positions,orders,ordersAt);
  let active=this.active();
  if(active?.status==='prepared'){this.record(abandonPreparedIntent(this.state,active.intentId,'RESTARTED_BEFORE_DISPATCH',this.now()),active.intentId,'abandoned');active=null;}
  if(active?.status==='dispatching'){const r=recordPlacement(this.state,active.intentId,{status:null,code:'RESTART_UNCONFIRMED'},this.now());this.record(r,active.intentId,'interrupted_submission');active=this.active();}
  if(active?.orderId){await this.reconcile(active);this.publish();return;}
  if(active){
   this.status='submission_uncertain';this.reason=active.reasonCode;
   // Recovery is deliberate: do not turn an ambiguous request into a new entry.
   if(active.retryAllowed&&this.now()>=Date.parse(active.retryAfterAt)&&this.now()>Date.parse(active.expirationDate))await this.submit(active,null,true);
   this.publish();return;
  }
  const held=Object.values(this.state.owned).map(x=>x.marketId),scope=new Set(this.universe.exchanges.map(x=>x.marketId));
  this.selected=[...new Set([...held,...candidateMarketIds.filter(id=>scope.has(id))])].slice(0,3);
  if(!this.selected.length){this.status='waiting_for_signal';this.publish();return;}
  await this.cashRefresh();
  // At most one new candidate receives extra metadata reads in a cycle.
  const candidate=this.selected.find(id=>!held.includes(id));this.selected=[...held,...(candidate?[candidate]:[])];
  for(const id of this.selected){delete this.markets[id];const r=await this.transport.market(id),ctx=r.data.contexts[0];if(ctx.status!=='open'||ctx.settledWith!==null||r.data.status!=='open'||r.data.isComposite||r.data.isMultiOutcome)continue;this.markets[id]={at:r.receivedAt,title:r.data.title,status:'open'};}
  this.status='monitoring';this.reason=null;this.publish();
 }
 admit(permit,spec){
  if(permit.intentId!==this.currentPermit?.intentId||permit.runId!==this.activation.runId)return false;
  if(spec.method==='DELETE'){
   const i=this.active(),e=this.currentPermit.evidence,now=this.now(),durable=this.ledger.snapshot().intents[permit.intentId];
   if(permit.kind!=='cancel'||this.currentPermit.kind!=='cancel'||!i||i.intentId!==permit.intentId||!i.orderId||permit.orderId!==i.orderId||String(durable?.orderId)!==String(i.orderId)||spec.endpoint!=='/orders/'+i.orderId||now<Date.parse(i.expirationDate)||e?.order.id!==i.orderId||e.order.open!==true||e.order.tournamentId!==TID||e.order.exchangeId!==i.exchangeId||e.order.side!=='yes'||e.order.action!==i.action||micros(e.order.priceLimit)!==micros(i.price)||Date.parse(e.order.expirationDate)!==Date.parse(i.expirationDate)||!Number.isFinite(Date.parse(e.at))||Date.parse(e.at)>now||now-Date.parse(e.at)>this.state.policy.maxAccountAgeMs)return false;
   for(const k of ['cash','positions','orders']){const p=e.snapshot?.[k];if(p?.tournamentId!==TID||!Number.isFinite(Date.parse(p.at))||Date.parse(p.at)>now||now-Date.parse(p.at)>this.state.policy.maxAccountAgeMs||(k!=='cash'&&(p.complete!==true||!Array.isArray(p.rows))))return false;}
   this.updateControl(c=>{c.cancellations[i.intentId]={orderId:i.orderId,attempts:(c.cancellations[i.intentId]?.attempts||0)+1,phase:'dispatching',lastAttemptAt:new Date(now).toISOString(),evidenceAt:e.at};},{type:'cancel_dispatch_admitted',intentId:i.intentId,orderId:i.orderId});
   return true;
  }
  if(spec.method!=='POST'||permit.kind!==undefined||this.currentPermit.kind!==undefined)return false;
  const i=this.state.intents[permit.intentId];if(!i||i.payloadHash!==payloadHash(spec.body)||spec.bodyJson!==i.bodyJson)return false;
  const r=beginDispatch(this.state,i.intentId,{snapshot:this.currentPermit.snapshot,now:this.now(),retry:permit.retry===true});if(!r.ok){this.reason=r.code;return false;}
  this.ledger.markDispatching(i.intentId,{retry:permit.retry===true});this.commit(r,{type:'dispatch_admitted',intentId:i.intentId});return true;
 }
 networkAdmit(permit,spec){
  if(permit.intentId!==this.currentPermit?.intentId||permit.runId!==this.activation.runId)return false;
  const i=this.state.intents[permit.intentId],now=this.now();
  if(!i||i.status==='closed')return false;
  if(spec.method==='DELETE'){
   const e=this.currentPermit.evidence,c=this.state.control.cancellations[i.intentId],durable=this.ledger.snapshot().intents[i.intentId];
   if(permit.kind!=='cancel'||this.currentPermit.kind!=='cancel'||permit.orderId!==i.orderId||String(durable?.orderId)!==String(i.orderId)||spec.endpoint!=='/orders/'+i.orderId||c?.phase!=='dispatching'||e?.order.id!==i.orderId||now<Date.parse(i.expirationDate)||!Number.isFinite(Date.parse(e.at))||Date.parse(e.at)>now||now-Date.parse(e.at)>this.state.policy.maxAccountAgeMs)return false;
   return ['cash','positions','orders'].every(k=>{const p=e.snapshot?.[k];return p?.tournamentId===TID&&Number.isFinite(Date.parse(p.at))&&Date.parse(p.at)<=now&&now-Date.parse(p.at)<=this.state.policy.maxAccountAgeMs&&(k==='cash'||p.complete===true&&Array.isArray(p.rows));});
  }
  if(spec.method!=='POST'||i.status!=='dispatching'||i.payloadHash!==payloadHash(spec.body)||i.bodyJson!==spec.bodyJson)return false;
  if(permit.retry===true&&this.currentPermit.retry===true&&now>=Date.parse(i.expirationDate))return true;
  const r=revalidateIntentEvidence(this.state,i.intentId,this.currentPermit.snapshot,now);if(!r.ok)this.reason=r.code;return r.ok;
 }
 async cancelExpired(i,evidence){
  this.currentPermit={kind:'cancel',intentId:i.intentId,evidence};
  try{
   await this.transport.cancel(i.orderId,{permit:{kind:'cancel',intentId:i.intentId,orderId:i.orderId,runId:this.activation.runId},lease:this.lease});
   this.updateControl(c=>{c.cancellations[i.intentId].phase='acknowledged';c.cancellations[i.intentId].reasonCode='CANCEL_ACKNOWLEDGED';},{type:'cancel_response',intentId:i.intentId,orderId:i.orderId});
   this.status='cancel_pending_reconciliation';this.reason='CANCEL_ACKNOWLEDGED';
  }catch(e){
   if(this.state.control.cancellations[i.intentId]?.phase==='dispatching')this.updateControl(c=>{c.cancellations[i.intentId].phase='uncertain';c.cancellations[i.intentId].reasonCode='CANCEL_UNCONFIRMED';},{type:'cancel_uncertain',intentId:i.intentId,orderId:i.orderId});
   this.status='cancel_pending_reconciliation';this.reason='CANCEL_UNCONFIRMED';
  }finally{this.currentPermit=null;this.publish();}
 }
 async submit(i,snapshot,retry=false){
  this.currentPermit={intentId:i.intentId,snapshot,retry};
  try{const r=await this.transport.place(i.body,{permit:{intentId:i.intentId,runId:this.activation.runId,retry},lease:this.lease});this.record(recordPlacement(this.state,i.intentId,{status:r.status,response:r.data},this.now()),i.intentId,'placement_response');}
  catch(e){
   if(this.state.intents[i.intentId]?.status==='dispatching')this.record(recordPlacement(this.state,i.intentId,{status:e.status??null,code:e.apiCode||'TRANSPORT_UNCONFIRMED',retryAfterMs:e.retryAfterMs??0},this.now()),i.intentId,'placement_uncertain');
   else if(this.state.intents[i.intentId]?.status==='prepared')this.record(abandonPreparedIntent(this.state,i.intentId,'ADMISSION_BLOCKED',this.now()),i.intentId,'not_submitted');
   else throw e;
  }finally{this.currentPermit=null;this.publish();}
 }
 async onBook(response){
  const marketId=/^\/markets\/(\d+)\/orderbook/.exec(response.endpoint)?.[1];
  if(!this.canWrite()||this.active()||!this.selected.includes(marketId)||!this.markets[marketId]||this.now()-Date.parse(this.markets[marketId].at)>180000)return;
  const b=this.checkedBook(response,marketId),owned=this.state.owned[b.exchangeId];
  const bank=JSON.parse(fs.readFileSync(path.join(this.runtimeDir,'dip-responsive-bank.json'))),shadow=JSON.parse(fs.readFileSync(path.join(this.runtimeDir,'dip-responsive-state.json')));
  if(shadow.bankHash!==digest(bank)||bank.policy.executionPolicy!=='responsive_v2')throw Error('PILOT_REFERENCE_IDENTITY_CHANGED');
  const ask=micros(b.asks[0].price),bid=micros(b.bids[0].price);let action,limit,quantity,reason;
  if(owned){
   const entry=Object.values(this.state.intents).filter(i=>i.exchangeId===b.exchangeId&&i.action==='buy'&&i.filledQuantity>0).at(-1),age=this.now()-Date.parse(entry?.reconciledFills?.[0]?.filledAt||entry?.preparedAt||new Date(this.now()).toISOString()),avg=micros(owned.cost)/BigInt(owned.quantity);
   const risk=bid<=avg-micros(bank.policy.stopLoss)||age>=bank.policy.maximumHoldMs;
   if(risk&&!this.state.control.exits[b.exchangeId])this.updateControl(c=>{c.exits[b.exchangeId]={marketId,reason:bid<=avg-micros(bank.policy.stopLoss)?'stop_loss':'holding_timeout',triggeredAt:new Date(this.now()).toISOString(),triggerBid:formatMicros(bid),sourceVersion:b.versionKey};},{type:'exit_obligation_latched',exchangeId:b.exchangeId,marketId});
   const obligation=this.state.control.exits[b.exchangeId];
   if(!obligation&&bid<avg+micros(bank.policy.takeProfit))return;
   action='sell';limit=bid;quantity=owned.quantity;reason=obligation?'latched_'+obligation.reason:'take_profit';
  }else{
   if(this.now()>Date.parse(this.activation.entryExpiresAt)){this.status='entry_window_closed';this.publish();return;}
   const identity=bank.identities[marketId];if(!identity?.state||!identity?.office)return;
   if(Object.values(this.state.owned).some(p=>bank.identities[p.marketId]?.state===identity.state))return;
   if(this.account.positions.rows.some(p=>p.exchangeId===b.exchangeId&&p.quantity!==0)||this.account.orders.rows.some(o=>o.exchangeId===b.exchangeId))return;
   const last=Object.values(this.state.intents).filter(i=>i.exchangeId===b.exchangeId&&i.status==='closed').at(-1);if(last&&this.now()-Date.parse(last.closedAt)<bank.policy.cooldownMs)return;
   const ref=historicalReference(shadow.histories[b.exchangeId],shadow.bookHistory[b.exchangeId],b.requestStartedAt,bank.policy),signal=dipSignal(ref,Number(ask+5000n)/1e6,Number(bid-5000n)/1e6,bank.policy);if(!signal.admitted)return;
   const cumulative=Object.values(this.state.intents).filter(i=>i.action==='buy'&&i.attempts>0).reduce((n,i)=>n+micros(i.price)*BigInt(i.quantity),0n);
   action='buy';limit=ask;quantity=Number((micros(this.activation.maxCumulativeBuyCost)-cumulative)/limit);reason='historical_discount';
  }
  const available=(action==='buy'?b.asks:b.bids).filter(x=>micros(x.price)===limit).reduce((n,x)=>n+x.quantity,0);
  quantity=Math.min(quantity,Number(micros(this.activation.maxOrderCost)/limit),Math.floor(available*this.activation.depthFractionBps/10000));
  if(!Number.isSafeInteger(quantity)||quantity<1)return;
  const spec={intentId:'intent-'+randomUUID(),idempotencyKey:randomUUID(),marketId,exchangeId:b.exchangeId,side:'yes',action,quantity,price:formatMicros(limit),expirationDate:new Date(this.now()+30000).toISOString(),attribution:{runId:this.activation.runId,strategyId:'historical-dip-responsive-v2',modelHash:shadow.bankHash,signalId:b.versionKey,reason}};
  const snapshot=this.snapshot({tournamentId:TID,marketId,exchangeId:b.exchangeId,at:b.receivedAt,requestStartedAt:b.requestStartedAt,bids:b.bids,asks:b.asks}),r=prepareIntent(this.state,spec,snapshot,this.now());
  if(!r.ok){this.reason=r.code;this.publish();return;}
  this.ledger.prepareIntent({method:'POST',path:'/orders',intentId:spec.intentId,idempotencyKey:spec.idempotencyKey,body:r.intent.body,bodyJson:r.intent.bodyJson,payloadHash:r.intent.payloadHash,attribution:{ownerId:'Codex',runId:this.activation.runId,strategyId:spec.attribution.strategyId,modelHash:shadow.bankHash,marketId,exchangeId:b.exchangeId}});
  this.commit(r,{type:'intent_prepared',intentId:spec.intentId,reason});await this.submit(r.intent,snapshot);
 }
 publish(){
  const intents=Object.values(this.state.intents),fills=intents.flatMap(i=>i.reconciledFills.map(f=>({...f,action:i.action,marketId:i.marketId,exchangeId:i.exchangeId,orderId:i.orderId})));let cashFlow=0n;
  for(const f of fills)cashFlow+=(f.action==='sell'?1n:-1n)*micros(f.price)*BigInt(f.quantity);
  const cost=Object.values(this.state.owned).reduce((n,p)=>n+micros(p.cost),0n),realized=cashFlow+cost;
  atomicJson(path.join(this.runtimeDir,'competition-stats.json'),{mode:'official_quant_execution',owner:'Codex',runId:this.activation.runId,updatedAt:new Date(this.now()).toISOString(),status:this.status,reason:this.reason,readiness:this.readiness,limits:{maxOrderCost:this.activation.maxOrderCost,maxAggregateCost:this.activation.maxAggregateCost,maxCumulativeBuyCost:this.activation.maxCumulativeBuyCost,maxManagedMarkets:3,limitExpiryMs:30000},ordersSubmitted:intents.filter(i=>i.attempts>0).length,openIntent:this.active()?{intentId:this.active().intentId,status:this.active().status,orderId:this.active().orderId}:null,positions:this.state.owned,exitObligations:this.state.control.exits,cancellations:this.state.control.cancellations,fills,realizedPnl:(realized<0n?'-':'')+formatMicros(realized<0n?-realized:realized),addedFees:'0.000000',profitabilityEstablished:false});
 }
 failure(error){const transient=[429,500,502,503,504].includes(error?.status)||error?.code==='REQUEST_UNCONFIRMED';this.halted=!transient;this.status=transient?'reconciliation_retry':'blocked';this.reason=typeof error?.code==='string'?error.code:'EXECUTION_RECONCILIATION_UNAVAILABLE';this.publish();}
 close(){this.ledger.close();}
}
