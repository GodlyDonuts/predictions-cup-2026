import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {randomUUID,createHash} from 'node:crypto';
import {CompetitionController} from './controller.mjs';
import {prepareIntent,decimalMicros,formatMicros,payloadHash} from './core.mjs';
import {atomicJson} from './ledger.mjs';
import {Journal} from '../live-paper/runtime.mjs';
import {digest} from '../live-paper/model.mjs';
import {instantNs} from '../feed/contract.mjs';
import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';
import {openMedianReferenceReader,MEDIAN_REFERENCE_POLICY} from './median-reference.mjs';
import {MEDIAN_PAPER_POLICY,MEDIAN_EXECUTION_POLICY,validateMedianBank as validatePriorBank} from './median-controller.mjs';
import {planMedianProtectedCandidates} from './median-protected-candidate-plan.mjs';

const DEFAULT_WORKSPACE=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const need=(ok,code)=>{if(!ok)throw Error('MEDIAN_EXECUTION_'+code);};
const amount=x=>decimalMicros(String(x)),tick=5000n,iso=n=>new Date(n).toISOString();
const hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const stamp=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&iso(Date.parse(x))===x;
const plain=x=>x!==null&&typeof x==='object'&&!Array.isArray(x)&&Object.getPrototypeOf(x)===Object.prototype;
// Official freshness compares sequence first, then nanosecond asOf. A quiet
// authoritative receipt may advance asOf while retaining its engine sequence.
const compareBook=(a,b)=>BigInt(a.sequence)>BigInt(b.sequence)?1:BigInt(a.sequence)<BigInt(b.sequence)?-1:instantNs(a.sourceAt)>instantNs(b.sourceAt)?1:instantNs(a.sourceAt)<instantNs(b.sourceAt)?-1:0;
export {MEDIAN_PAPER_POLICY};
export const MEDIAN_EXECUTION_POLICY_V2=Object.freeze({...MEDIAN_EXECUTION_POLICY,remainingEntryMarginTicks:1});
export function medianControllerV2Identity(workspace=DEFAULT_WORKSPACE){return collectFrozenPaperFiles(workspace,['execution/median-controller-v2.mjs']);}
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const exactKeys=(x,keys)=>plain(x)&&Object.keys(x).sort().join('|')===[...keys].sort().join('|');
const checkpointKeys=['sequence','hash','stateHash'];
function checkpointValid(p){return exactKeys(p,checkpointKeys)&&Number.isSafeInteger(p.sequence)&&p.sequence>=0&&(p.sequence===0?p.hash==='':hex(p.hash))&&hex(p.stateHash);}
export function medianEntryCapLimit(book,reference){
 need(reference?.outcome==='reference'&&amount(reference.reference)-amount(book.asks[0].price)>=tick,'ENTRY_REFERENCE_GAP');
 const original=amount(book.asks[0].price)+tick,retained=amount(reference.reference)-tick;
 return formatMicros(original<retained?original:retained);
}
// A revision changes neither activation nor handoff. The original activation
// hash pins owner, run, budget, authority and the original entry expiry.
export function validateMedianRevision({revision,bank,priorBank,activation,universe,workspace=DEFAULT_WORKSPACE,archiveDir,now=Date.now()}){
 need(exactKeys(revision,['schemaVersion','kind','revisionId','ownerId','runId','tournamentId','activationHash','priorBankHash','nextBankHash','recordedAt','coreCheckpoint','proposalCheckpoint']),'REVISION_SHAPE');
 need(revision.schemaVersion===1&&revision.kind==='same_run_median_entry_cap_revision'&&/^median-entry-cap-[a-z0-9-]{1,70}$/.test(revision.revisionId)&&revision.ownerId==='Codex'&&revision.runId===activation.runId&&revision.tournamentId===activation.tournamentId&&revision.activationHash===payloadHash(activation)&&revision.priorBankHash===activation.strategyBankHash&&hex(revision.nextBankHash)&&stamp(revision.recordedAt)&&Date.parse(revision.recordedAt)>=Date.parse(activation.startedAt)&&Date.parse(revision.recordedAt)<=now,'REVISION_BINDING');
 need(checkpointValid(revision.coreCheckpoint)&&exactKeys(revision.proposalCheckpoint,[...checkpointKeys,'bytes','sha256'])&&checkpointValid(Object.fromEntries(checkpointKeys.map(k=>[k,revision.proposalCheckpoint[k]])))&&Number.isSafeInteger(revision.proposalCheckpoint.bytes)&&revision.proposalCheckpoint.bytes>=0&&hex(revision.proposalCheckpoint.sha256),'REVISION_CHECKPOINT');
 validatePriorBank(priorBank,activation,universe,workspace);
 need(typeof archiveDir==='string','LEGACY_ARCHIVE_REQUIRED');
 // Traverse the archived entrypoint, not merely a caller-supplied file list.
 validatePriorBank(priorBank,activation,universe,archiveDir);
 need(bank?.schemaVersion===2&&bank.mode==='baal_historical_median_production_v2'&&digest(bank)===revision.nextBankHash,'BANK');
 need(exactKeys(bank,['schemaVersion','mode','tournamentId','universeHash','cutoff','paperBinding','sourcePrefix','policy','codeIdentity','legacyCompatibility']),'BANK_SHAPE');
 for(const k of ['tournamentId','universeHash','cutoff','paperBinding','sourcePrefix'])need(payloadHash(bank[k])===payloadHash(priorBank[k]),'BANK_SCOPE_OR_SOURCE');
 need(payloadHash(bank.policy)===payloadHash(MEDIAN_EXECUTION_POLICY_V2),'POLICY');
 need(exactKeys(bank.legacyCompatibility,['priorBankHash','priorPolicyHash','priorCodeIdentityHash'])&&bank.legacyCompatibility.priorBankHash===revision.priorBankHash&&bank.legacyCompatibility.priorPolicyHash===payloadHash(priorBank.policy)&&bank.legacyCompatibility.priorCodeIdentityHash===payloadHash(priorBank.codeIdentity),'LEGACY_COMPATIBILITY');
 need(plain(bank.codeIdentity)&&payloadHash(bank.codeIdentity)===payloadHash(medianControllerV2Identity(workspace)),'CODE_IDENTITY');
 return bank;
}

function journalCheckpoint(file,p,{fixed=false,emptyState=null}={}){
 const bytes=fs.existsSync(file)?fs.readFileSync(file):Buffer.alloc(0);
 if(fixed)need(bytes.length===p.bytes&&sha(bytes)===p.sha256,'LEGACY_PROPOSAL_BYTES');
 const raw=bytes.toString('utf8');need(!raw||raw.endsWith('\n'),'REVISION_JOURNAL_PARTIAL');
 let sequence=0,previous='',state=emptyState,found=p.sequence===0&&p.stateHash===payloadHash(emptyState);
 for(const line of raw.split('\n').filter(Boolean)){
  const row=JSON.parse(line),{hash,...body}=row;need(body.sequence===sequence+1&&body.previous===previous&&hash===digest(body),'REVISION_JOURNAL_CHAIN');sequence=row.sequence;previous=hash;
  if(sequence===p.sequence){need(hash===p.hash&&payloadHash(row.state)===p.stateHash,'REVISION_JOURNAL_CHECKPOINT');state=row.state;found=true;}
 }
 need(found&&(!fixed||sequence===p.sequence&&previous===p.hash),'REVISION_CHECKPOINT_MISSING');return{state,sequence,hash:previous};
}

function validateProposals(s,bankHash,runId,universe,{revisionHash,recordedAt,newBankHash,legacy}={}){
 need(plain(s)&&s.schemaVersion===1&&s.bankHash===bankHash&&s.runId===runId&&plain(s.proposals)&&plain(s.books),'PROPOSAL_STATE');
 if(revisionHash)need(s.revisionHash===revisionHash&&s.activeBankHash===newBankHash,'PROPOSAL_REVISION');
 const scope=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId])),keys=new Set();
 for(const [id,p]of Object.entries(s.proposals)){
  need(/^median-[a-f0-9-]{36}$/.test(id)&&plain(p)&&p.intentId===id&&scope.get(p.marketId)===p.exchangeId&&p.quantity===1&&['buy','sell'].includes(p.action)&&['proposed','dispatched','closed','revoked','expired'].includes(p.status)&&typeof p.idempotencyKey==='string'&&!keys.has(p.idempotencyKey),'PROPOSAL_IDENTITY');keys.add(p.idempotencyKey);
  const prior=legacy?.proposals[id],expectedBank=revisionHash&&!prior?newBankHash:bankHash;
  need(stamp(p.createdAt)&&stamp(p.expiresAt)&&Date.parse(p.expiresAt)-Date.parse(p.createdAt)===MEDIAN_EXECUTION_POLICY.proposalLifetimeMs&&p.bankHash===expectedBank&&p.originalBook?.marketId===p.marketId&&p.originalBook?.exchangeId===p.exchangeId&&hex(p.proofHash)&&p.proofHash===payloadHash(p.proof)&&p.proof.bankHash===expectedBank&&p.proof.decisionAvailableAt===p.createdAt&&payloadHash(p.proof.originalBook)===payloadHash(p.originalBook),'PROPOSAL_PROOF');
  if(prior){const {status:oldStatus,...oldTerms}=prior,{status,...terms}=p;need(payloadHash(oldTerms)===payloadHash(terms)&&(oldStatus==='proposed'?['proposed','expired'].includes(status):oldStatus==='dispatched'?['dispatched','closed'].includes(status):status===oldStatus),'LEGACY_PROPOSAL_CHANGED');}
  else if(revisionHash){need(p.proof.revisionHash===revisionHash&&Date.parse(p.createdAt)>=Date.parse(recordedAt),'NEW_PROPOSAL_REVISION');if(p.action==='buy')need(p.limit===medianEntryCapLimit(p.originalBook,p.proof.reference),'ENTRY_CAP');}
  need(stamp(p.originalBook.requestStartedAt)&&stamp(p.originalBook.receivedAt)&&Date.parse(p.originalBook.requestStartedAt)<=Date.parse(p.originalBook.receivedAt)&&Date.parse(p.originalBook.receivedAt)<=Date.parse(p.createdAt)&&/^\d+$/.test(p.originalBook.sequence)&&typeof p.originalBook.versionKey==='string'&&typeof p.originalBook.sourceAt==='string','PROPOSAL_CLOCK');
  const limit=amount(p.limit);need(limit>=tick&&limit<=995000n&&limit%tick===0n&&['ask_below_earlier_hour_bid_median','profit_target','stop_loss','holding_timeout'].includes(p.reason),'PROPOSAL_TERMS');
  need(p.action==='sell'||p.proof?.reference?.outcome==='reference','PROPOSAL_REFERENCE');
 }
 if(legacy)need(Object.keys(legacy.proposals).every(id=>s.proposals[id]),'LEGACY_PROPOSAL_REMOVED');
 const pending=Object.values(s.proposals).filter(p=>p.status==='proposed');need(new Set(pending.map(p=>p.marketId)).size===pending.length,'DUPLICATE_PROPOSAL');
 for(const [marketId,b]of Object.entries(s.books))need(scope.get(marketId)===b.exchangeId&&/^\d+$/.test(b.sequence)&&typeof b.sourceAt==='string'&&stamp(b.receivedAt)&&hex(b.depthHash),'BOOK_STATE');
 return s;
}

// One in-process controller, the collector's queue, and the existing execution
// lease own both journals. Proposals are not orders or paper fills.
export class MedianCompetitionControllerV2 extends CompetitionController{
 constructor(options){
  super(options);
  try{
   this.workspace=options.workspace||DEFAULT_WORKSPACE;
   this.revisionFile=path.join(this.dir,'median-entry-cap-revision.json');this.revision=JSON.parse(fs.readFileSync(this.revisionFile,'utf8'));this.revisionHash=payloadHash(this.revision);
   this.bankFile=path.join(this.runtimeDir,'production-banks',this.revision.nextBankHash+'.json');
   this.priorBankFile=path.join(this.runtimeDir,'production-banks',this.activation.strategyBankHash+'.json');
   this.archiveDir=path.join(this.runtimeDir,'production-code',this.activation.strategyBankHash);
   this.bank=JSON.parse(fs.readFileSync(this.bankFile,'utf8'));this.priorBank=JSON.parse(fs.readFileSync(this.priorBankFile,'utf8'));this.bankCurrent();
   const empty={schemaVersion:1,bankHash:this.activation.strategyBankHash,runId:this.activation.runId,proposals:{},books:{}};
   this.legacyProposals=validateProposals(journalCheckpoint(path.join(this.dir,'median-proposals.jsonl'),this.revision.proposalCheckpoint,{fixed:true,emptyState:empty}).state,this.activation.strategyBankHash,this.activation.runId,this.universe);
   const emptyCore={schemaVersion:1,tournamentId:this.state.tournamentId,policy:this.state.policy,intents:{},owned:{},control:{schemaVersion:1,exits:{},cancellations:{}}};
   this.legacyCore=journalCheckpoint(this.journal.file,this.revision.coreCheckpoint,{emptyState:emptyCore}).state;
   need(payloadHash(this.legacyCore.policy)===payloadHash(this.state.policy)&&this.legacyCore.tournamentId===this.activation.tournamentId,'REVISION_CORE_POLICY');
   need(Object.values(this.legacyCore.intents).every(i=>i.status==='closed'&&!i.acceptanceUncertain),'REVISION_UNRESOLVED_INTENT');
   need(Object.values(this.legacyCore.control?.cancellations||{}).every(c=>c.phase==='reconciled'),'REVISION_UNRESOLVED_CANCEL');
   need(Object.values(this.legacyProposals.proposals).every(p=>p.status!=='proposed'||Date.parse(p.expiresAt)<=Date.parse(this.revision.recordedAt)),'REVISION_LIVE_LEGACY_PROPOSAL');
   this.proposalJournal=new Journal(path.join(this.dir,'median-proposals-v2.jsonl'));
   if(!this.proposalJournal.last){
    need(this.journal.sequence===this.revision.coreCheckpoint.sequence&&this.journal.previous===this.revision.coreCheckpoint.hash&&!this.active(),'REVISION_CORE_MOVED');
    this.ledger.snapshot();need(this.canOperate(),'STOPPED');
    const state={...structuredClone(this.legacyProposals),activeBankHash:this.policyBankHash,revisionHash:this.revisionHash};this.proposals=this.validateProposalState(state);this.assertRevisionIntents();
    this.proposalJournal.commit(state,{type:'same_run_median_entry_cap_revision',revisionHash:this.revisionHash,revision:structuredClone(this.revision)});
   }
   this.proposals=this.validateProposalState(this.proposalJournal.last.state);this.assertRevisionIntents();
   this.reference=null;this.referenceReady=false;this.referenceFailure=null;this.candidateIds=[];this.publish();
  }catch(e){this.close();throw e;}
 }
 get policyBankHash(){return this.revision?.nextBankHash;}
 get schedulingBankHash(){return this.policyBankHash;}
 bankCurrent(){
  need(payloadHash(JSON.parse(fs.readFileSync(this.revisionFile,'utf8')))===this.revisionHash,'REVISION_CHANGED');
  validateMedianRevision({revision:this.revision,bank:JSON.parse(fs.readFileSync(this.bankFile,'utf8')),priorBank:JSON.parse(fs.readFileSync(this.priorBankFile,'utf8')),activation:this.activation,universe:this.universe,workspace:this.workspace,archiveDir:this.archiveDir,now:this.now()});
  if(this.legacyProposals)journalCheckpoint(path.join(this.dir,'median-proposals.jsonl'),this.revision.proposalCheckpoint,{fixed:true,emptyState:this.legacyProposals});return true;
 }
 validateProposalState(s){return validateProposals(s,this.activation.strategyBankHash,this.activation.runId,this.universe,{revisionHash:this.revisionHash,recordedAt:this.revision.recordedAt,newBankHash:this.policyBankHash,legacy:this.legacyProposals});}
 assertRevisionIntents(){
  const durable=this.ledger.snapshot().intents;
  for(const [id,i]of Object.entries(this.legacyCore.intents))need(payloadHash(this.state.intents[id])===payloadHash(i),'LEGACY_INTENT_CHANGED');
  for(const i of Object.values(this.state.intents)){const p=this.proposals.proposals[i.intentId],a=durable[i.intentId]?.attribution;need(p&&p.idempotencyKey===i.idempotencyKey&&i.attribution.modelHash===p.bankHash&&i.attribution.runId===this.activation.runId&&i.attribution.strategyId===this.activation.strategyId&&i.marketId===p.marketId&&i.exchangeId===p.exchangeId&&i.action===p.action&&i.quantity===p.quantity&&amount(i.price)===amount(p.limit),'INTENT_PROPOSAL_MISMATCH');need(a?.ownerId==='Codex'&&a.runId===this.activation.runId&&a.modelHash===i.attribution.modelHash&&a.marketId===i.marketId&&a.exchangeId===i.exchangeId&&a.strategyId===i.attribution.strategyId,'LEDGER_ATTRIBUTION_MISMATCH');}
  for(const id of Object.keys(this.state.owned))this.ownedEntry(id);
 }
 ownedEntry(exchangeId){
  const own=this.state.owned[exchangeId];if(!own)return null;let entry=null;
  for(const i of Object.values(this.state.intents).filter(i=>i.exchangeId===exchangeId).sort((a,b)=>a.ordinal-b.ordinal))for(const f of i.reconciledFills){
   need(f.quantity===1,'OWNED_FILL');if(i.action==='buy'){need(!entry,'OWNED_QUANTITY');entry={intentId:i.intentId,modelHash:i.attribution.modelHash,fillId:f.id,cost:f.price,filledAt:f.filledAt,marketId:i.marketId};}else{need(entry,'OWNED_PROVENANCE');entry=null;}
  }
  need(entry&&own.quantity===1&&own.marketId===entry.marketId&&amount(own.cost)===amount(entry.cost)&&[this.activation.strategyBankHash,this.policyBankHash].includes(entry.modelHash),'OWNED_BANK_OR_QUANTITY');
  if(entry.modelHash===this.activation.strategyBankHash)need(this.legacyCore.intents[entry.intentId],'LEGACY_LOT_NOT_AT_REVISION');return entry;
 }
 async initializeMedianReference(){
  if(this.referenceReady)return this.reference.status();
  need(!this.referenceFailure,'REFERENCE_FAILED_RESTART_REQUIRED');
  this.bankCurrent();this.reference=await openMedianReferenceReader({file:path.join(this.runtimeDir,'market-prices.jsonl'),universe:this.universe,sourcePrefix:this.bank.sourcePrefix,cutoff:this.bank.cutoff});
  await this.reference.refresh();this.referenceReady=true;this.referenceFailure=null;return this.reference.status();
 }
 bootstrapMarketId(){return this.universe.exchanges[0].marketId;}
 eligibleMarketIds(){return [...new Set([...this.managedMarketIds(),...Object.values(this.proposals?.proposals||{}).filter(p=>p.status==='proposed').map(p=>p.marketId),...(this.candidateIds||[])])];}
 writeProposals(next,event){
  this.ledger.snapshot();need(this.canOperate(),'STOPPED');this.bankCurrent();this.validateProposalState(next);
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
  const now=this.now(),hintAt=iso(now),hints=[];
  if(this.referenceReady&&!this.referenceFailure)for(const q of args.protectedBulkHints||[]){try{
   // This clock is a metadata-ranking probe only. onBook independently uses
   // the actual original request and never dispatches against a bulk quote.
   const r=this.reference.reference({marketId:q.marketId,exchangeId:q.exchangeId,bookRequestStartedAt:hintAt,availableAt:hintAt,bestAsk:q.bestAsk});
   if(r.outcome==='reference'&&r.sourceProof.frontier.sequence>=q.sourceSequence)hints.push({...q,outcome:r.outcome,reference:r.reference,ask:r.ask});
  }catch{}}
  const cycle=args.metadataCycle??((this.protectedMetadataCycle??0)+1);
  const plan=planMedianProtectedCandidates({universe:this.universe,runId:this.activation.runId,bankHash:this.schedulingBankHash,plannedMarketIds:args.candidateMarketIds||[],ownedMarketIds:Object.values(this.state.owned).map(x=>x.marketId),proposals:Object.values(this.proposals.proposals).filter(p=>p.status==='proposed'&&Date.parse(p.expiresAt)>now),bulkHints:hints,now,cycle,lastMetadataCycles:this.protectedMetadataCycles||{}});
  this.candidateIds=plan.nextMetadataMarketId?[plan.nextMetadataMarketId]:[];
  await super.beforeBooks({...args,candidateMarketIds:plan.candidateMarketIds});
  this.protectedMetadataCycle=cycle;
  if(plan.nextMetadataMarketId&&this.selected.includes(plan.nextMetadataMarketId))this.protectedMetadataCycles=plan.lastMetadataCycles;
  this.lastProtectedMetadataPlan=plan;
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
  if(!p||p.bankHash!==this.policyBankHash||i.attribution.modelHash!==p.bankHash||!['proposed','dispatched'].includes(p.status)||!proof||proof.bankHash!==p.bankHash||proof.revisionHash!==this.revisionHash||proof.proposalHash!==payloadHash(p.proof)||p.proofHash!==payloadHash(p.proof)||proof.proposalId!==p.intentId||proof.followupHash!==payloadHash(b)||i.idempotencyKey!==p.idempotencyKey||i.quantity!==1||i.action!==p.action||amount(i.price)!==amount(p.limit)||!this.evidenceAdmitted(b,this.currentPermit.snapshot,now)||!this.depthAdmitted(p,b))return false;
  if(p.action==='buy'&&p.limit!==medianEntryCapLimit(p.originalBook,p.proof.reference))return false;
  const snapshotBook=this.currentPermit.snapshot.book;
  if(snapshotBook.marketId!==b.marketId||snapshotBook.exchangeId!==b.exchangeId||snapshotBook.at!==b.receivedAt||snapshotBook.requestStartedAt!==b.requestStartedAt||payloadHash({bids:snapshotBook.bids,asks:snapshotBook.asks})!==payloadHash({bids:b.bids,asks:b.asks}))return false;
  if(now>=Date.parse(p.expiresAt)||Date.parse(b.requestStartedAt)<Date.parse(p.createdAt)+3000||compareBook(b,p.originalBook)<=0)return false;
  if(i.action==='buy'&&(!this.entryWindowAdmitted(now)||!this.referenceReady||this.referenceFailure))return false;
  if(i.action==='sell'&&(this.state.owned[i.exchangeId]?.quantity!==1||payloadHash(p.proof.ownedEntry)!==payloadHash(this.ownedEntry(i.exchangeId))))return false;
  return true;
 }
 checkProgress(book){
  const prior=this.proposals.books[book.marketId],depthHash=payloadHash({bids:book.bids,asks:book.asks});
  if(prior){const version=compareBook(book,prior);need(Date.parse(book.receivedAt)>=Date.parse(prior.receivedAt)&&version>=0,'BOOK_ROLLBACK');if(version===0)need(depthHash===prior.depthHash,'BOOK_SEQUENCE_CONFLICT');}
  this.updateProposals(s=>{s.books[book.marketId]={exchangeId:book.exchangeId,sequence:book.sequence,sourceAt:book.sourceAt,receivedAt:book.receivedAt,depthHash};},{type:'median_book',marketId:book.marketId,at:iso(this.now())});
 }
 ownedPolicy(book){
  const own=this.state.owned[book.exchangeId];if(!own)return null;
  const entry=this.ownedEntry(book.exchangeId),policy=entry.modelHash===this.activation.strategyBankHash?this.priorBank.policy:this.bank.policy;
  const cost=amount(own.cost),bid=amount(book.bids[0].price),at=Date.parse(entry.filledAt);
  const risk=bid<=cost-BigInt(policy.stopLossTicks)*tick?'stop_loss':this.now()-at>=policy.maximumHoldMs?'holding_timeout':null;
  if(risk&&!this.state.control.exits[book.exchangeId])this.updateControl(c=>{c.exits[book.exchangeId]={marketId:book.marketId,reason:risk,triggeredAt:iso(this.now()),triggerBid:book.bids[0].price,sourceVersion:book.versionKey};},{type:'exit_obligation_latched',exchangeId:book.exchangeId});
  const obligation=this.state.control.exits[book.exchangeId];
  if(obligation)return{reason:obligation.reason,limit:formatMicros(bid>tick?bid-tick:tick)};
  const target=cost+BigInt(policy.takeProfitTicks)*tick;
  if(bid>=target)return{reason:'profit_target',limit:formatMicros(bid-tick>target?bid-tick:target)};
  return null;
 }
 createProposal(book,action,limit,reason,reference=null){
  const intentId='median-'+randomUUID(),createdAt=iso(this.now()),proof={bankHash:this.policyBankHash,revisionHash:this.revisionHash,paperBinding:this.bank.paperBinding,reference,originalBook:book,decisionAvailableAt:createdAt,referencePolicy:MEDIAN_REFERENCE_POLICY,...(action==='sell'?{ownedEntry:this.ownedEntry(book.exchangeId)}:{})};
  const p={intentId,idempotencyKey:randomUUID(),bankHash:this.policyBankHash,marketId:book.marketId,exchangeId:book.exchangeId,action,quantity:1,limit,reason,createdAt,expiresAt:iso(this.now()+120000),originalBook:structuredClone(book),proof,proofHash:payloadHash(proof),status:'proposed'};
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
   const spec={intentId:p.intentId,idempotencyKey:p.idempotencyKey,marketId,exchangeId:b.exchangeId,side:'yes',action:p.action,quantity:1,price:p.limit,expirationDate:iso(Math.min(this.now()+30000,Date.parse(p.expiresAt))),attribution:{runId:this.activation.runId,strategyId:this.activation.strategyId,modelHash:p.bankHash,signalId:p.proofHash,reason:p.reason}},r=prepareIntent(this.state,spec,snapshot,this.now());
   if(!r.ok){this.reason=r.code;this.publish();return;}
   const referenceProof={bankHash:p.bankHash,revisionHash:this.revisionHash,proposalId:p.intentId,proposalHash:p.proofHash,followupBook:b,followupHash:payloadHash(b)};
   this.ledger.prepareIntent({method:'POST',path:'/orders',intentId:p.intentId,idempotencyKey:p.idempotencyKey,body:r.intent.body,bodyJson:r.intent.bodyJson,payloadHash:r.intent.payloadHash,attribution:{ownerId:'Codex',runId:this.activation.runId,strategyId:this.activation.strategyId,modelHash:p.bankHash,marketId,exchangeId:b.exchangeId}});
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
  const originalLimit=amount(b.asks[0].price)+tick,limit=amount(medianEntryCapLimit(b,reference)),spent=Object.values(this.state.intents).filter(i=>i.action==='buy').reduce((n,i)=>n+i.reconciledFills.reduce((v,f)=>v+amount(f.price)*BigInt(f.quantity),0n),0n),reserved=pending.reduce((n,x)=>n+amount(x.limit),0n);
  // Retain the baseline's conservative proposal budget admission. Only the
  // actual fixed new proposal limit and its reservation become smaller.
  if(originalLimit>995000n||spent+reserved+originalLimit>amount(this.activation.maxCumulativeBuyCost))return;
  if(limit>995000n||spent+reserved+limit>amount(this.activation.maxCumulativeBuyCost))return;
  this.createProposal(b,'buy',formatMicros(limit),'ask_below_earlier_hour_bid_median',reference);this.status='median_proposal_pending';this.reason='AWAITING_LATER_INDEPENDENT_BOOK';this.publish();
 }
 publish(){
  super.publish();if(!this.bank||!this.proposals)return;
  const file=path.join(this.runtimeDir,'competition-stats.json'),s=JSON.parse(fs.readFileSync(file));
  Object.assign(s,{strategyId:this.activation.strategyId,strategyBankHash:this.activation.strategyBankHash,policyBankHash:this.policyBankHash,schedulingBankHash:this.schedulingBankHash,medianRevision:{revisionId:this.revision.revisionId,revisionHash:this.revisionHash,priorBankHash:this.activation.strategyBankHash,nextBankHash:this.policyBankHash,activationHash:this.activationHash,entryExpiresAt:this.activation.entryExpiresAt},median:{referenceReady:this.referenceReady===true,referenceFailure:this.referenceFailure||null,reference:this.reference?.status()||null,lastReference:this.lastReference||null,pendingProposals:Object.values(this.proposals.proposals).filter(p=>p.status==='proposed').map(p=>({intentId:p.intentId,bankHash:p.bankHash,marketId:p.marketId,exchangeId:p.exchangeId,action:p.action,quantity:p.quantity,limit:p.limit,reason:p.reason,createdAt:p.createdAt,expiresAt:p.expiresAt})),journalSequence:this.proposalJournal.sequence,journalHash:this.proposalJournal.previous,policy:this.bank.policy,paperBinding:this.bank.paperBinding,executionAuthority:true}});atomicJson(file,s);
 }
}
