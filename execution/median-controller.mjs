import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {randomUUID} from 'node:crypto';
import {CompetitionController} from './controller.mjs';
import {prepareIntent,decimalMicros,formatMicros,payloadHash} from './core.mjs';
import {atomicJson} from './ledger.mjs';
import {Journal} from '../live-paper/runtime.mjs';
import {digest} from '../live-paper/model.mjs';
import {instantNs} from '../feed/contract.mjs';
import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';
import {openMedianReferenceReader,MEDIAN_REFERENCE_POLICY} from './median-reference.mjs';

const DEFAULT_WORKSPACE=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const need=(ok,code)=>{if(!ok)throw Error('MEDIAN_EXECUTION_'+code);};
const amount=x=>decimalMicros(String(x)),tick=5000n,iso=n=>new Date(n).toISOString();
const hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const stamp=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&iso(Date.parse(x))===x;
const plain=x=>x!==null&&typeof x==='object'&&!Array.isArray(x)&&Object.getPrototypeOf(x)===Object.prototype;
// Official freshness compares sequence first, then nanosecond asOf. A quiet
// authoritative receipt may advance asOf while retaining its engine sequence.
const compareBook=(a,b)=>BigInt(a.sequence)>BigInt(b.sequence)?1:BigInt(a.sequence)<BigInt(b.sequence)?-1:instantNs(a.sourceAt)>instantNs(b.sourceAt)?1:instantNs(a.sourceAt)<instantNs(b.sourceAt)?-1:0;
export const MEDIAN_PAPER_POLICY=Object.freeze({prospectiveRevision:1,riskPriority:'stop_or_horizon_before_virtual_profit_fill',riskIntentRefresh:'new_intent_new_clock_and_source_on_bid_below_old_limit',stopIsNotGuaranteedLossBound:true,entryDipTicks:[1,2,4],historyMs:3600000,minimumHistorySpanMs:1800000,minimumHistoryReceipts:20,latencyMs:3000,intentLifetimeMs:120000,maximumHoldMs:7200000,lossLimitTicks:4,profitTargetTicks:1,cooldownMs:900000,cashReserve:'90.000000',maxExposure:'3.000000',maxMarkets:3,cumulativeBuyBudget:'25.000000',initialCash:'100.000000',fees:0,depthFractionBps:100,fillAdverseTicks:0,limitCushionTicks:1});
export const MEDIAN_EXECUTION_POLICY=Object.freeze({orderShares:1,dipTicks:1,takeProfitTicks:1,stopLossTicks:4,maximumHoldMs:7200000,cooldownMs:900000,proposalLifetimeMs:120000,orderLifetimeMs:30000,minimumFollowupMs:3000,depthFractionBps:100,maximumBookAgeMs:20000,maximumAccountAgeMs:20000,limitCushionTicks:1,fees:0,executionDifference:'official_order_expires_after_30s_then_reconcile_before_replacement'});
export function medianControllerIdentity(workspace=DEFAULT_WORKSPACE){return collectFrozenPaperFiles(workspace,['execution/median-controller.mjs']);}
export function validateMedianBank(bank,activation,universe,workspace=DEFAULT_WORKSPACE){
 need(activation.strategyId==='historical-median-receipt-v1'&&bank?.schemaVersion===1&&bank.mode==='baal_historical_median_production_v1'&&digest(bank)===activation.strategyBankHash,'BANK');
 need(bank.tournamentId===activation.tournamentId&&bank.universeHash===universe.hash&&universe.tournamentId===activation.tournamentId&&stamp(bank.cutoff)&&Date.parse(bank.cutoff)<=Date.parse(activation.startedAt),'BANK_SCOPE');
 need(hex(bank.paperBinding?.bankHash)&&bank.paperBinding.policyHash===digest(MEDIAN_PAPER_POLICY)&&payloadHash(bank.policy)===payloadHash(MEDIAN_EXECUTION_POLICY),'POLICY');
 const p=bank.sourcePrefix;need(p&&Number.isSafeInteger(p.bytes)&&p.bytes>0&&Number.isSafeInteger(p.sequence)&&p.sequence>0&&hex(p.sha256)&&hex(p.hash),'SOURCE_PREFIX');
 need(plain(bank.codeIdentity)&&payloadHash(bank.codeIdentity)===payloadHash(medianControllerIdentity(workspace)),'CODE_IDENTITY');
 return bank;
}

function validateProposals(s,bankHash,runId,universe){
 need(plain(s)&&s.schemaVersion===1&&s.bankHash===bankHash&&s.runId===runId&&plain(s.proposals)&&plain(s.books),'PROPOSAL_STATE');
 const scope=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId])),keys=new Set();
 for(const [id,p]of Object.entries(s.proposals)){
  need(/^median-[a-f0-9-]{36}$/.test(id)&&plain(p)&&p.intentId===id&&scope.get(p.marketId)===p.exchangeId&&p.quantity===1&&['buy','sell'].includes(p.action)&&['proposed','dispatched','closed','revoked','expired'].includes(p.status)&&typeof p.idempotencyKey==='string'&&!keys.has(p.idempotencyKey),'PROPOSAL_IDENTITY');keys.add(p.idempotencyKey);
  need(stamp(p.createdAt)&&stamp(p.expiresAt)&&Date.parse(p.expiresAt)-Date.parse(p.createdAt)===MEDIAN_EXECUTION_POLICY.proposalLifetimeMs&&p.bankHash===bankHash&&p.originalBook?.marketId===p.marketId&&p.originalBook?.exchangeId===p.exchangeId&&hex(p.proofHash)&&p.proofHash===payloadHash(p.proof)&&p.proof.bankHash===bankHash&&p.proof.decisionAvailableAt===p.createdAt&&payloadHash(p.proof.originalBook)===payloadHash(p.originalBook),'PROPOSAL_PROOF');
  need(stamp(p.originalBook.requestStartedAt)&&stamp(p.originalBook.receivedAt)&&Date.parse(p.originalBook.requestStartedAt)<=Date.parse(p.originalBook.receivedAt)&&Date.parse(p.originalBook.receivedAt)<=Date.parse(p.createdAt)&&/^\d+$/.test(p.originalBook.sequence)&&typeof p.originalBook.versionKey==='string'&&typeof p.originalBook.sourceAt==='string','PROPOSAL_CLOCK');
  const limit=amount(p.limit);need(limit>=tick&&limit<=995000n&&limit%tick===0n&&['ask_below_earlier_hour_bid_median','profit_target','stop_loss','holding_timeout'].includes(p.reason),'PROPOSAL_TERMS');
  need(p.action==='sell'||p.proof?.reference?.outcome==='reference','PROPOSAL_REFERENCE');
 }
 const pending=Object.values(s.proposals).filter(p=>p.status==='proposed');need(new Set(pending.map(p=>p.marketId)).size===pending.length,'DUPLICATE_PROPOSAL');
 for(const [marketId,b]of Object.entries(s.books))need(scope.get(marketId)===b.exchangeId&&/^\d+$/.test(b.sequence)&&typeof b.sourceAt==='string'&&stamp(b.receivedAt)&&hex(b.depthHash),'BOOK_STATE');
 return s;
}

// One in-process controller, the collector's queue, and the existing execution
// lease own both journals. Proposals are not orders or paper fills.
export class MedianCompetitionController extends CompetitionController{
 constructor(options){
  super(options);
  try{
   this.workspace=options.workspace||DEFAULT_WORKSPACE;
   this.bankFile=path.join(this.runtimeDir,'production-banks',this.activation.strategyBankHash+'.json');
   this.bank=validateMedianBank(JSON.parse(fs.readFileSync(this.bankFile,'utf8')),this.activation,this.universe,this.workspace);
   this.proposalJournal=new Journal(path.join(this.dir,'median-proposals.jsonl'));
   this.proposals=validateProposals(this.proposalJournal.last?.state||{schemaVersion:1,bankHash:this.activation.strategyBankHash,runId:this.activation.runId,proposals:{},books:{}},this.activation.strategyBankHash,this.activation.runId,this.universe);
   for(const i of Object.values(this.state.intents))need(this.proposals.proposals[i.intentId]?.idempotencyKey===i.idempotencyKey&&i.attribution.modelHash===this.activation.strategyBankHash,'INTENT_PROPOSAL_MISMATCH');
   this.reference=null;this.referenceReady=false;this.referenceFailure=null;this.candidateIds=[];this.publish();
  }catch(e){this.close();throw e;}
 }
 bankCurrent(){validateMedianBank(JSON.parse(fs.readFileSync(this.bankFile,'utf8')),this.activation,this.universe,this.workspace);return true;}
 async initializeMedianReference(){
  if(this.referenceReady)return this.reference.status();
  need(!this.referenceFailure,'REFERENCE_FAILED_RESTART_REQUIRED');
  this.bankCurrent();this.reference=await openMedianReferenceReader({file:path.join(this.runtimeDir,'market-prices.jsonl'),universe:this.universe,sourcePrefix:this.bank.sourcePrefix,cutoff:this.bank.cutoff});
  await this.reference.refresh();this.referenceReady=true;this.referenceFailure=null;return this.reference.status();
 }
 bootstrapMarketId(){return this.universe.exchanges[0].marketId;}
 eligibleMarketIds(){return [...new Set([...this.managedMarketIds(),...Object.values(this.proposals?.proposals||{}).filter(p=>p.status==='proposed').map(p=>p.marketId),...(this.candidateIds||[])])];}
 writeProposals(next,event){
  this.ledger.snapshot();need(this.canOperate(),'STOPPED');this.bankCurrent();validateProposals(next,this.activation.strategyBankHash,this.activation.runId,this.universe);
  this.proposalJournal.commit(next,event);this.proposals=next;
 }
 updateProposals(mutator,event){const next=structuredClone(this.proposals);mutator(next);this.writeProposals(next,event);}
 syncProposals(){
  let changed=false;const next=structuredClone(this.proposals);
  for(const p of Object.values(next.proposals)){const i=this.state.intents[p.intentId];if(i&&p.status!==(i.status==='closed'?'closed':'dispatched')){p.status=i.status==='closed'?'closed':'dispatched';changed=true;}else if(!i&&p.status==='proposed'&&this.now()>=Date.parse(p.expiresAt)){p.status='expired';changed=true;}}
  if(changed)this.writeProposals(next,{type:'median_intent_projection',at:iso(this.now())});
 }
 async beforeBooks(args){
  if(!this.canOperate())return super.beforeBooks(args);
  this.bankCurrent();
  try{if(!this.referenceFailure){if(!this.referenceReady)await this.initializeMedianReference();else await this.reference.refresh();}}catch(e){this.referenceFailure=/^[A-Z0-9_]+$/.test(e.message)?e.message:'MEDIAN_REFERENCE_UNAVAILABLE';this.referenceReady=false;}
  this.candidateIds=[...new Set(args.candidateMarketIds||[])].filter(id=>this.universe.exchanges.some(x=>x.marketId===id));
  await super.beforeBooks({...args,candidateMarketIds:this.eligibleMarketIds()});
  if(this.canOperate())this.syncProposals();
 }
 async beforeBook(marketId){
  if(!this.canWrite()||this.active()||!this.selected.includes(marketId))return;
  this.bankCurrent();const now=this.now(),clocks=[this.cash?.at,this.account?.positions.at,this.account?.orders.at].map(Date.parse);
  if(clocks.some(t=>!Number.isFinite(t)||t>now||now-t>10000)){await this.cashRefresh();await this.refreshAccount();}
 }
 async beforeCandidateRead(marketId){
  need(this.universe.exchanges.some(x=>x.marketId===marketId),'CANDIDATE_SCOPE');
  if(!this.canOperate())return false;
  if(!this.bootstrapped&&!(await this.bootstrap()).complete)return false;
  if(this.active())return false;
  this.bankCurrent();
  // Auxiliary reads follow the cycle's bulk-price publication. Consume that
  // verified frontier before the original book request, preserving the native
  // newest-point exclusion rather than using the earlier protected-read view.
  try{if(!this.referenceFailure){if(!this.referenceReady)await this.initializeMedianReference();else await this.reference.refresh();}}catch(e){this.referenceFailure=/^[A-Z0-9_]+$/.test(e.message)?e.message:'MEDIAN_REFERENCE_UNAVAILABLE';this.referenceReady=false;}
  const response=await this.transport.market(marketId),ctx=response.data.contexts[0];
  if(ctx.status!=='open'||ctx.settledWith!==null||response.data.status!=='open'||response.data.isComposite||response.data.isMultiOutcome)return false;
  this.markets[marketId]={at:response.receivedAt,status:'open'};this.selected=[...new Set([...this.managedMarketIds(),marketId])];
  this.candidateIds=[...new Set([...this.candidateIds,marketId])].slice(-3);
  await this.beforeBook(marketId);return this.canWrite();
 }
 evidenceAdmitted(book,snapshot,now){
  if(!book||!snapshot||!Number.isFinite(now))return false;
  const request=Date.parse(book.requestStartedAt),received=Date.parse(book.receivedAt);
  if(!Number.isFinite(request)||!Number.isFinite(received)||request>received||received>now||now-received>20000||received-request>20000)return false;
  return ['cash','positions','orders'].every(k=>{const p=snapshot[k],at=Date.parse(p?.at);return p?.tournamentId===this.activation.tournamentId&&Number.isFinite(at)&&at<=request&&now-at<=20000&&(k==='cash'||p.complete===true&&Array.isArray(p.rows));});
 }
 depthAdmitted(p,book){return(p.action==='buy'?book.asks:book.bids).some(l=>l.quantity>=100&&(p.action==='buy'?amount(l.price)<=amount(p.limit):amount(l.price)>=amount(p.limit)));}
 entryReferenceAdmitted(i,permit,now){
  // Expired identical recovery is not a new order, even after the entry window.
  if(permit.retry===true&&this.currentPermit?.retry===true&&i.attempts>0&&now>=Date.parse(i.expirationDate))return true;
  this.bankCurrent();const p=this.proposals.proposals[i.intentId],proof=this.currentPermit?.referenceProof,b=proof?.followupBook;
  if(!p||!['proposed','dispatched'].includes(p.status)||!proof||proof.bankHash!==this.activation.strategyBankHash||proof.proposalHash!==payloadHash(p.proof)||proof.proposalId!==p.intentId||proof.followupHash!==payloadHash(b)||i.idempotencyKey!==p.idempotencyKey||i.quantity!==1||i.action!==p.action||amount(i.price)!==amount(p.limit)||!this.evidenceAdmitted(b,this.currentPermit.snapshot,now)||!this.depthAdmitted(p,b))return false;
  const snapshotBook=this.currentPermit.snapshot.book;
  if(snapshotBook.marketId!==b.marketId||snapshotBook.exchangeId!==b.exchangeId||snapshotBook.at!==b.receivedAt||snapshotBook.requestStartedAt!==b.requestStartedAt||payloadHash({bids:snapshotBook.bids,asks:snapshotBook.asks})!==payloadHash({bids:b.bids,asks:b.asks}))return false;
  if(now>=Date.parse(p.expiresAt)||Date.parse(b.requestStartedAt)<Date.parse(p.createdAt)+3000||compareBook(b,p.originalBook)<=0)return false;
  if(i.action==='buy'&&(!this.entryWindowAdmitted(now)||!this.referenceReady||this.referenceFailure))return false;
  if(i.action==='sell'&&this.state.owned[i.exchangeId]?.quantity!==1)return false;
  return true;
 }
 checkProgress(book){
  const prior=this.proposals.books[book.marketId],depthHash=payloadHash({bids:book.bids,asks:book.asks});
  if(prior){const version=compareBook(book,prior);need(Date.parse(book.receivedAt)>=Date.parse(prior.receivedAt)&&version>=0,'BOOK_ROLLBACK');if(version===0)need(depthHash===prior.depthHash,'BOOK_SEQUENCE_CONFLICT');}
  this.updateProposals(s=>{s.books[book.marketId]={exchangeId:book.exchangeId,sequence:book.sequence,sourceAt:book.sourceAt,receivedAt:book.receivedAt,depthHash};},{type:'median_book',marketId:book.marketId,at:iso(this.now())});
 }
 ownedPolicy(book){
  const own=this.state.owned[book.exchangeId];if(!own)return null;
  const entries=Object.values(this.state.intents).filter(i=>i.exchangeId===book.exchangeId&&i.action==='buy'&&i.filledQuantity>0);
  need(own.quantity===1&&entries.length&&entries.every(i=>i.attribution.modelHash===this.activation.strategyBankHash),'OWNED_BANK_OR_QUANTITY');
  const entry=entries.at(-1);need(entry.reconciledFills.length===1&&entry.reconciledFills[0].quantity===1,'OWNED_FILL');
  const cost=amount(own.cost),bid=amount(book.bids[0].price),at=Date.parse(entry.reconciledFills[0].filledAt);
  const risk=bid<=cost-4n*tick?'stop_loss':this.now()-at>=7200000?'holding_timeout':null;
  if(risk&&!this.state.control.exits[book.exchangeId])this.updateControl(c=>{c.exits[book.exchangeId]={marketId:book.marketId,reason:risk,triggeredAt:iso(this.now()),triggerBid:book.bids[0].price,sourceVersion:book.versionKey};},{type:'exit_obligation_latched',exchangeId:book.exchangeId});
  const obligation=this.state.control.exits[book.exchangeId];
  if(obligation)return{reason:obligation.reason,limit:formatMicros(bid>tick?bid-tick:tick)};
  if(bid>=cost+tick)return{reason:'profit_target',limit:formatMicros(bid-tick>cost+tick?bid-tick:cost+tick)};
  return null;
 }
 createProposal(book,action,limit,reason,reference=null){
  const intentId='median-'+randomUUID(),createdAt=iso(this.now()),proof={bankHash:this.activation.strategyBankHash,paperBinding:this.bank.paperBinding,reference,originalBook:book,decisionAvailableAt:createdAt,referencePolicy:MEDIAN_REFERENCE_POLICY};
  const p={intentId,idempotencyKey:randomUUID(),bankHash:this.activation.strategyBankHash,marketId:book.marketId,exchangeId:book.exchangeId,action,quantity:1,limit,reason,createdAt,expiresAt:iso(this.now()+120000),originalBook:structuredClone(book),proof,proofHash:payloadHash(proof),status:'proposed'};
  this.updateProposals(s=>{s.proposals[intentId]=p;},{type:'median_proposal',intentId,action,reason,at:createdAt});return p;
 }
 async onBook(response){
  const marketId=/^\/markets\/(\d+)\/orderbook/.exec(response.endpoint)?.[1];
  if(!this.canWrite()||this.active()||!this.selected.includes(marketId)||!this.markets[marketId]||this.now()-Date.parse(this.markets[marketId].at)>180000)return;
  this.bankCurrent();const b=this.checkedBook(response,marketId),snapshot=this.snapshot({tournamentId:this.activation.tournamentId,marketId,exchangeId:b.exchangeId,at:b.receivedAt,requestStartedAt:b.requestStartedAt,bids:b.bids,asks:b.asks});
  if(!this.evidenceAdmitted(b,snapshot,this.now())){this.reason='MEDIAN_STALE_OR_NONCAUSAL_ACCOUNT';this.publish();return;}
  this.checkProgress(b);this.syncProposals();const owned=this.state.owned[b.exchangeId],exit=this.ownedPolicy(b);
  let p=Object.values(this.proposals.proposals).find(x=>x.marketId===marketId&&x.status==='proposed');
  if(p&&(this.now()>=Date.parse(p.expiresAt)||p.action==='buy'&&owned||p.action==='sell'&&!owned||p.action==='sell'&&exit&&p.reason==='profit_target'&&exit.reason!=='profit_target'||p.action==='sell'&&p.reason!=='profit_target'&&amount(b.bids[0].price)<amount(p.limit))){
   this.updateProposals(s=>{s.proposals[p.intentId].status=this.now()>=Date.parse(p.expiresAt)?'expired':'revoked';},{type:'median_proposal_revoked',intentId:p.intentId,at:iso(this.now())});p=null;
  }
  if(p){
   if(Date.parse(b.requestStartedAt)<Date.parse(p.createdAt)+3000||compareBook(b,p.originalBook)<=0||!this.depthAdmitted(p,b))return;
   if(p.action==='buy'&&(!this.entryWindowAdmitted(this.now())||!this.referenceReady||this.referenceFailure))return;
   const spec={intentId:p.intentId,idempotencyKey:p.idempotencyKey,marketId,exchangeId:b.exchangeId,side:'yes',action:p.action,quantity:1,price:p.limit,expirationDate:iso(Math.min(this.now()+30000,Date.parse(p.expiresAt))),attribution:{runId:this.activation.runId,strategyId:this.activation.strategyId,modelHash:this.activation.strategyBankHash,signalId:p.proofHash,reason:p.reason}},r=prepareIntent(this.state,spec,snapshot,this.now());
   if(!r.ok){this.reason=r.code;this.publish();return;}
   const referenceProof={bankHash:this.activation.strategyBankHash,proposalId:p.intentId,proposalHash:p.proofHash,followupBook:b,followupHash:payloadHash(b)};
   this.ledger.prepareIntent({method:'POST',path:'/orders',intentId:p.intentId,idempotencyKey:p.idempotencyKey,body:r.intent.body,bodyJson:r.intent.bodyJson,payloadHash:r.intent.payloadHash,attribution:{ownerId:'Codex',runId:this.activation.runId,strategyId:this.activation.strategyId,modelHash:this.activation.strategyBankHash,marketId,exchangeId:b.exchangeId}});
   this.commit(r,{type:'median_intent_prepared',intentId:p.intentId,referenceProof});this.syncProposals();await this.submit(r.intent,snapshot,false,referenceProof);if(this.canOperate())this.syncProposals();return;
  }
  if(owned){if(exit)this.createProposal(b,'sell',exit.limit,exit.reason);this.publish();return;}
  if(this.reconciliationRiskOnly||!this.entryWindowAdmitted(this.now())||!this.referenceReady||this.referenceFailure)return;
  if(this.account.positions.rows.some(x=>x.exchangeId===b.exchangeId&&x.quantity!==0)||this.account.orders.rows.some(x=>x.exchangeId===b.exchangeId))return;
  const lastExit=Object.values(this.state.intents).filter(i=>i.exchangeId===b.exchangeId&&i.action==='sell'&&i.filledQuantity>0&&i.status==='closed').at(-1);
  if(lastExit&&this.now()-Date.parse(lastExit.reconciledFills.at(-1).filledAt)<900000)return;
  const pending=Object.values(this.proposals.proposals).filter(x=>x.status==='proposed'&&x.action==='buy');
  if(Object.keys(this.state.owned).length+pending.length>=3)return;
  const reference=this.reference.reference({marketId,exchangeId:b.exchangeId,bookRequestStartedAt:b.requestStartedAt,availableAt:iso(this.now()),bestAsk:b.asks[0].price});
  this.lastReference={marketId,outcome:reference.outcome,reason:reference.reason||null,reference:reference.reference,sourceProof:reference.sourceProof};
  if(reference.outcome!=='reference'){this.status='waiting_for_signal';this.reason=reference.reason;this.publish();return;}
  const limit=amount(b.asks[0].price)+tick,spent=Object.values(this.state.intents).filter(i=>i.action==='buy').reduce((n,i)=>n+i.reconciledFills.reduce((v,f)=>v+amount(f.price)*BigInt(f.quantity),0n),0n),reserved=pending.reduce((n,x)=>n+amount(x.limit),0n);
  if(limit>995000n||spent+reserved+limit>amount(this.activation.maxCumulativeBuyCost))return;
  this.createProposal(b,'buy',formatMicros(limit),'ask_below_earlier_hour_bid_median',reference);this.status='median_proposal_pending';this.reason='AWAITING_LATER_INDEPENDENT_BOOK';this.publish();
 }
 publish(){
  super.publish();if(!this.bank||!this.proposals)return;
  const file=path.join(this.runtimeDir,'competition-stats.json'),s=JSON.parse(fs.readFileSync(file));
  Object.assign(s,{strategyId:this.activation.strategyId,strategyBankHash:this.activation.strategyBankHash,median:{referenceReady:this.referenceReady===true,referenceFailure:this.referenceFailure||null,reference:this.reference?.status()||null,lastReference:this.lastReference||null,pendingProposals:Object.values(this.proposals.proposals).filter(p=>p.status==='proposed').map(p=>({intentId:p.intentId,marketId:p.marketId,exchangeId:p.exchangeId,action:p.action,quantity:p.quantity,limit:p.limit,reason:p.reason,createdAt:p.createdAt,expiresAt:p.expiresAt})),journalSequence:this.proposalJournal.sequence,journalHash:this.proposalJournal.previous,policy:this.bank.policy,paperBinding:this.bank.paperBinding,executionAuthority:true}});atomicJson(file,s);
 }
}
