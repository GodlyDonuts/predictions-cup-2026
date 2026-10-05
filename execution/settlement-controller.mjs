import fs from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {CompetitionController} from './controller.mjs';
import {prepareIntent,decimalMicros,formatMicros,conservativeCashFloor,payloadHash} from './core.mjs';
import {digest} from '../live-paper/model.mjs';
import {nativePublicationBinding} from '../live-paper/native-publication.mjs';
import {runSettlementValue} from '../live-paper/settlement-value-native.mjs';
import {validateProductionActivation} from './production-activation.mjs';
import {productionEntryState} from './production-entry-state.mjs';
import {PROBABILITY_RECEIPT_POLICY} from '../live-paper/probability-receipt.mjs';
import {RECEIPT_PRODUCTION_POLICY} from './receipt-production-bank.mjs';

const need=(ok,code)=>{if(!ok)throw Error('SETTLEMENT_EXECUTION_'+code);};
const units=decimalMicros;
export function validateSettlementBank(bank,activation,universe){
 validateProductionActivation(activation);
 const receipt=activation.strategyId==='probability-receipt-native-v1';
 need(bank?.schemaVersion===(receipt?2:1)&&bank.mode===(receipt?'baal_probability_receipt_production_v1':'baal_settlement_production_v1')&&digest(bank)===activation.strategyBankHash,'BANK');
 if(receipt){
  need(bank.candidates?.length<=3&&bank.paperBinding?.bankHash?.match(/^[a-f0-9]{64}$/)&&bank.paperBinding.policyHash===digest(PROBABILITY_RECEIPT_POLICY),'PAPER_BINDING');
  need(digest(bank.receiptPolicy)===digest(RECEIPT_PRODUCTION_POLICY)&&bank.policy.minimumEdge==='0.020000'&&bank.policy.maximumSpread==='0.020000','RECEIPT_POLICY');
 }
 need(bank.candidates?.length>0&&bank.candidates.length<=600&&new Set(bank.candidates.map(c=>c.id)).size===bank.candidates.length,'CANDIDATES');
 for(const c of bank.candidates){need(c.side==='YES'&&c.id===c.marketId+':YES'&&universe.exchanges.some(x=>x.marketId===c.marketId&&x.exchangeId===c.exchangeId)&&/^[a-f0-9]{64}$/.test(c.predicateHash)&&Number.isSafeInteger(c.settlesAtMs)&&c.riskGroups?.length>0,'SCOPE');}
 need(bank.contracts&&bank.candidates.every(c=>/^[a-f0-9]{64}$/.test(bank.contracts[c.marketId]?.rootHash)&&bank.contracts[c.marketId].predicateHash===c.predicateHash&&bank.contracts[c.marketId].exchangeId===c.exchangeId),'CONTRACT_BINDINGS');
 const p=bank.policy;
 need(p?.maxOrderCost===activation.maxOrderCost&&p.maxExposure===activation.maxAggregateCost&&p.cumulativeBuyBudget===activation.maxCumulativeBuyCost&&p.cashReserve===activation.cashReserve&&p.depthFractionBps===100&&p.maximumBookAgeMs===20000&&p.maximumResearchAgeMs===21600000&&p.slippageTicks===1&&p.fractionalKellyBps<=2500&&units(p.minimumEdge)>=20000n&&units(p.maximumSpread)<=50000n,'POLICY');
 return bank;
}
export class SettlementCompetitionController extends CompetitionController{
 constructor(options){
  validateProductionActivation(options.activation);
  super(options);
  try{
   this.bankFile=path.join(this.runtimeDir,'production-banks',this.activation.strategyBankHash+'.json');
   this.bank=validateSettlementBank(JSON.parse(fs.readFileSync(this.bankFile,'utf8')),this.activation,this.universe);
   this.executable=nativePublicationBinding(this.runtimeDir,this.bank.nativeBackend);
   this.receiptSequence=0;this.latestNative=null;this.checkedContracts={};
  }catch(e){this.close();throw e;}
 }
 eligibleMarketIds(){
  if(!this.bank)return[];
  const now=this.now();
  return this.bank.candidates.filter(c=>{const r=c.research;return r&&r.settlementReviewed&&r.flags.length===0&&r.recordedAtMs<=now&&r.sourceAtMs<=now&&now<r.expiresAtMs&&now-r.sourceAtMs<=this.bank.policy.maximumResearchAgeMs;}).map(c=>c.marketId);
 }
 bootstrapMarketId(){return this.bank.candidates[0].marketId;}
 async beforeBook(marketId){
  if(!this.canWrite()||this.active()||!this.selected.includes(marketId))return;
  const now=this.now(),clocks=[this.cash?.at,this.account?.positions.at,this.account?.orders.at].map(Date.parse);
  // Keep ten seconds of headroom for the queued book and durable dispatch.
  // Reconcile before requesting the decision book, never after it as if causal.
  if(clocks.some(t=>!Number.isFinite(t)||t>now||now-t>10000)){
   await this.cashRefresh();await this.refreshAccount();
   this.lastFreshnessRepair={at:new Date(this.now()).toISOString(),marketId,addedReads:3,networkQueue:'existing_serialized_account_queue'};
  }
 }
 async beforeBooks(args){
  this.checkedContracts={};await super.beforeBooks(args);
  if(!this.canWrite()||this.active())return;
  for(const id of this.selected){const expected=this.bank.contracts[id];if(!expected)continue;const r=await this.transport.nodes(id);if(payloadHash(r.data.root)!==expected.rootHash){this.reason='CURRENT_CONTRACT_CHANGED';continue;}this.checkedContracts[id]={at:r.receivedAt,rootHash:expected.rootHash};}
 }
 bankCurrent(){return digest(JSON.parse(fs.readFileSync(this.bankFile,'utf8')))===this.activation.strategyBankHash&&nativePublicationBinding(this.runtimeDir,this.bank.nativeBackend)===this.executable;}
 nativeInput(book,skipIntent){
  need(this.bankCurrent(),'BANK_CHANGED');
  const now=this.now(),intents=Object.values(this.state.intents),positions=Object.entries(this.state.owned).map(([exchangeId,p])=>{
   const entry=intents.filter(i=>i.exchangeId===exchangeId&&i.action==='buy'&&i.filledQuantity>0).at(-1);
   need(entry,'OWNED_SOURCE');return{candidateId:p.marketId+':YES',quantity:p.quantity,cost:p.cost,enteredAtMs:Date.parse(entry.reconciledFills?.[0]?.filledAt||entry.preparedAt)};
  });
  const contract=this.checkedContracts[book.marketId];
  need(contract&&contract.rootHash===this.bank.contracts[book.marketId].rootHash&&Date.parse(contract.at)<=Date.parse(book.requestStartedAt)&&now-Date.parse(contract.at)<=180000,'CONTRACT_NOT_CURRENT');
  // Maximum attempted purchase principal conservatively consumes the lifetime
  // budget. An intent under its own fence is not counted a second time.
  const cumulative=intents.filter(i=>i.intentId!==skipIntent&&i.action==='buy'&&i.attempts>0).reduce((n,i)=>n+units(i.price)*BigInt(i.quantity),0n);
  const reservations=intents.filter(i=>i.intentId!==skipIntent&&i.action==='buy'&&i.status==='prepared').map(i=>({candidateId:i.marketId+':YES',maximumCost:formatMicros(units(i.price)*BigInt(i.quantity))}));
  const knownAtMs=Math.max(Date.parse(this.account.positions.at),Date.parse(this.account.orders.at));
  const candidates=this.bank.candidates.map(c=>({...structuredClone(c),book:c.exchangeId===book.exchangeId?{requestStartedAtMs:Date.parse(book.requestStartedAt),receivedAtMs:Date.parse(book.receivedAt),availableAtMs:book.productionAvailableAtMs,sequence:book.productionSequence,sourceHash:digest(book),representation:'direct_yes',bids:book.bids,asks:book.asks}:null,account:{positionsAtMs:Date.parse(this.account.positions.at),ordersAtMs:Date.parse(this.account.orders.at),knownAtMs,complete:this.account.positions.complete&&this.account.orders.complete,positionExcluded:this.account.positions.rows.some(p=>p.exchangeId===c.exchangeId&&p.quantity!==0),openOrderExcluded:this.account.orders.rows.some(o=>o.exchangeId===c.exchangeId)}}));
  const input={schemaVersion:1,mode:'settlement_value_policy_v1',asOfMs:now,policy:this.bank.policy,wallet:{capital:this.activation.sizingCapital,cash:formatMicros(conservativeCashFloor(this.cash.amount)),cumulativeBuys:formatMicros(cumulative),positions,reservations},candidates};
  if(this.bank.mode==='baal_probability_receipt_production_v1'){
   input.schemaVersion=2;input.mode='probability_receipt_policy_v1';input.receiptPolicy=this.bank.receiptPolicy;
   const lastExitedAtMs={};for(const i of intents)if(i.action==='sell'&&i.status==='closed'&&i.filledQuantity>0&&!this.state.owned[i.exchangeId]){const id=i.marketId+':YES',at=Date.parse(i.closedAt);lastExitedAtMs[id]=Math.max(lastExitedAtMs[id]||0,at);}
   input.receiptState={riskExits:Object.fromEntries(Object.entries(this.state.control.exits).map(([exchangeId,x])=>[x.marketId+':YES',x.reason])),lastExitedAtMs};
  }
  return input;
 }
 evaluate(book,skipIntent){
  const input=this.nativeInput(book,skipIntent),checked=runSettlementValue({input,dir:path.join(this.dir,'native-decisions',randomUUID()),executable:this.executable});
  need(checked.binaryHash===this.bank.nativeBackend.binaryHash&&checked.result.engineHash===this.bank.nativeBackend.engineHash,'NATIVE_IDENTITY');
  const decision=checked.result.decisions.find(c=>c.exchangeId===book.exchangeId);
  need(decision,'DECISION_SCOPE');
  this.latestNative={at:new Date(this.now()).toISOString(),inputHash:checked.inputHash,outputHash:checked.outputHash,wallMs:checked.wallMs,decision,profitabilityEstablished:false};
  return{decision,proof:{bankHash:this.activation.strategyBankHash,bookHash:digest(book),inputHash:checked.inputHash,outputHash:checked.outputHash}};
 }
 entryReferenceAdmitted(intent,permit,now){
  if(permit.retry===true&&this.currentPermit.retry===true&&intent.attempts>0&&now>=Date.parse(intent.expirationDate))return true;
  if(intent.action==='buy'&&!this.entryWindowAdmitted(now))return false;
  try{
   const proof=this.currentPermit.referenceProof,book=this.currentPermit.settlementBook;
   if(!book||proof?.bankHash!==this.activation.strategyBankHash||proof.bookHash!==digest(book))return false;
   const{decision:d}=this.evaluate(book,intent.intentId);
   const action=intent.action==='buy'?'candidate_buy':'reduce';
   if(d.action!==action||d.quantity<intent.quantity||intent.action==='buy'&&units(intent.price)>units(d.hardLimitWithCushion)||intent.action==='sell'&&this.bank.receiptPolicy&&units(intent.price)<units(d.hardLimitWithCushion))return false;
   return true;
  }catch{return false;}
 }
 async submit(intent,snapshot,retry,referenceProof){
  // Base submission retains the durable idempotency, queue and network fences.
  this.preparedSettlementBook=referenceProof?.settlementBook||this.preparedSettlementBook;
  await super.submit(intent,snapshot,retry,referenceProof);
 }
 admit(permit,spec){
  if(this.currentPermit&&this.preparedSettlementBook)this.currentPermit.settlementBook=this.preparedSettlementBook;
  return super.admit(permit,spec);
 }
 async onBook(response){
  const marketId=/^\/markets\/(\d+)\/orderbook/.exec(response.endpoint)?.[1];
  if(!this.canWrite()||this.active()||!this.selected.includes(marketId)||!this.markets[marketId]||this.now()-Date.parse(this.markets[marketId].at)>180000)return;
  const original=this.checkedBook(response,marketId),book={...original,productionAvailableAtMs:this.now(),productionSequence:++this.receiptSequence};
  const owned=this.state.owned[book.exchangeId];if(this.reconciliationRiskOnly&&!owned)return;
  if(!owned&&!this.entryWindowAdmitted(this.now())){this.entryDecision(book,'WINDOW_BLOCKED',{detailCode:this.reason});return;}
  const {decision:d,proof}=this.evaluate(book);
  this.status='monitoring';this.reason=d.reason;
  if(d.riskExit&&!this.state.control.exits[book.exchangeId])this.updateControl(c=>{c.exits[book.exchangeId]={marketId,reason:d.riskExit,triggeredAt:new Date(this.now()).toISOString(),triggerBid:book.bids[0].price,sourceVersion:book.versionKey};},{type:'exit_obligation_latched',exchangeId:book.exchangeId,marketId});
  if(!['candidate_buy','reduce'].includes(d.action)||d.quantity<1){this.publish();return;}
  const action=d.action==='candidate_buy'?'buy':'sell',price=action==='buy'||this.bank.receiptPolicy?d.hardLimitWithCushion:book.bids[0].price;
  const quantity=Math.min(d.quantity,Number(units(this.activation.maxOrderCost)/units(price)));
  if(quantity<1)return;
  const spec={intentId:'intent-'+randomUUID(),idempotencyKey:randomUUID(),marketId,exchangeId:book.exchangeId,side:'yes',action,quantity,price,expirationDate:new Date(this.now()+30000).toISOString(),attribution:{runId:this.activation.runId,strategyId:this.activation.strategyId,modelHash:this.activation.strategyBankHash,signalId:proof.outputHash,reason:d.reason}};
  const snapshot=this.snapshot({tournamentId:this.activation.tournamentId,marketId,exchangeId:book.exchangeId,at:book.receivedAt,requestStartedAt:book.requestStartedAt,bids:book.bids,asks:book.asks}),r=prepareIntent(this.state,spec,snapshot,this.now());
  if(!r.ok){this.reason=r.code;this.publish();return;}
  const referenceProof={...proof,settlementBook:book};
  this.ledger.prepareIntent({method:'POST',path:'/orders',intentId:spec.intentId,idempotencyKey:spec.idempotencyKey,body:r.intent.body,bodyJson:r.intent.bodyJson,payloadHash:r.intent.payloadHash,attribution:{ownerId:'Codex',runId:this.activation.runId,strategyId:spec.attribution.strategyId,modelHash:spec.attribution.modelHash,marketId,exchangeId:book.exchangeId}});
  this.commit(r,{type:'intent_prepared',intentId:spec.intentId,reason:d.reason,referenceProof});
  await this.submit(r.intent,snapshot,false,referenceProof);
 }
 publish(){
  super.publish();
  if(this.bank){const f=path.join(this.runtimeDir,'competition-stats.json'),s=JSON.parse(fs.readFileSync(f));s.strategyId=this.activation.strategyId;s.strategyBankHash=this.activation.strategyBankHash;s.latestNative=this.latestNative;s.eligibleResearchMarkets=this.eligibleMarketIds();s.lastFreshnessRepair=this.lastFreshnessRepair||null;
   Object.assign(s,productionEntryState({activation:this.activation,nowMs:this.now(),researchMarketIds:s.eligibleResearchMarkets,owned:this.state.owned,activeIntent:this.active(),status:s.status,reason:s.reason}));
   fs.writeFileSync(f+'.native.tmp',JSON.stringify(s,null,2),{mode:0o600});fs.renameSync(f+'.native.tmp',f);}
 }
}
