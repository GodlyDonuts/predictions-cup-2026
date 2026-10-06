import fs from 'node:fs';import path from 'node:path';
import {payloadHash,decimalMicros,formatMicros} from './core.mjs';
import {acquireOwnerLease,atomicJson} from './ledger.mjs';
import {Journal} from '../live-paper/transport-runtime.mjs';
import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';
import {pairedLiveV2Identity,selectPairedLiveCandidate} from './paired-live-host-v2.mjs';
import {pairedLiveIdentity} from './paired-live-host.mjs';
import {openPairedRestControllerV3} from './paired-rest-controller-v3.mjs';
import {openPairedSharedFunding} from './paired-shared-funding.mjs';
import {createPairedRestEvidenceBridge} from './paired-rest-evidence-bridge.mjs';

const need=(ok,code)=>{if(!ok)throw Object.assign(Error('PAIRED_NEUTRAL_RECOVERY_'+code),{code:'PAIRED_NEUTRAL_RECOVERY_'+code});};
const read=file=>JSON.parse(fs.readFileSync(file,'utf8')),clone=x=>structuredClone(x),hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const TID='bda92870-621e-47b0-bc3c-3602c5c26f55';
export const pairedNeutralRecoveryIdentity=workspace=>collectFrozenPaperFiles(workspace,['execution/paired-neutral-recovery-host-v1.mjs','execution/scoped-rest-receipts.mjs']);
// This is an explicit reconciliation revision, not a replacement trading host.
// It reuses the original actor, funding registry, account owner and one API.
// It cannot issue POST/DELETE, renew a window, select a new pair, or retry a key.
export function openPairedNeutralRecoveryHost({runtimeDir,workspace,universe,api,receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,now=Date.now}){
 const descriptorFile=path.join(runtimeDir,'paired-neutral-ack-recovery.json');if(!fs.existsSync(descriptorFile))return null;
 need(path.isAbsolute(runtimeDir)&&path.isAbsolute(workspace)&&api&&typeof api.dispatch==='function'&&[receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,now].every(f=>typeof f==='function'),'DEPENDENCIES');
 need(canOperate()===true,'STOPPED');
 const descriptor=read(descriptorFile),descriptorHash=payloadHash(descriptor),code=pairedNeutralRecoveryIdentity(workspace);
 need(descriptor.schemaVersion===1&&descriptor.kind==='paired_neutral_ack_recovery_only'&&descriptor.ownerId==='Codex'&&descriptor.tournamentId===TID&&/^sig-pilot-paired-[a-z0-9-]{1,60}$/.test(descriptor.runId)&&['outerRequestHash','selectionHash','childRequestHash','authorityHash','activationHash'].every(k=>hex(descriptor[k]))&&payloadHash(descriptor.codeIdentity)===payloadHash(code),'DESCRIPTOR');
 const outerFile=path.join(runtimeDir,'paired-live-v2-request.json'),legacyFile=path.join(runtimeDir,'paired-live-request.json'),authorityFile=path.join(runtimeDir,'paired-live-authority.json');
 need(!fs.existsSync(legacyFile),'LEGACY_REQUEST_PRESENT');
 const outer=read(outerFile),authority=read(authorityFile),runId=descriptor.runId;
 need(payloadHash(outer)===descriptor.outerRequestHash&&outer.runId===runId&&outer.authorityHash===descriptor.authorityHash&&payloadHash(authority)===descriptor.authorityHash&&payloadHash(outer.codeIdentity)===payloadHash(pairedLiveV2Identity(workspace)),'ORIGINAL_OUTER');
 need(authority.kind==='direct_human_authorized_bot_update'&&authority.ownerId==='Codex'&&authority.threadId==='01a0f8cc-b9bb-77d2-9f4c-2844396f1b79'&&authority.exploratory===true&&authority.profitabilityEstablished===false,'AUTHORITY');
 const dir=path.join(runtimeDir,'paired-live-v2-runs',runId),childDir=path.join(dir,'child-runtime'),selectionFile=path.join(dir,'selection.jsonl');
 const selectionBytes=fs.readFileSync(selectionFile);need(selectionBytes.length>0&&selectionBytes.length<=2*1024*1024&&selectionBytes.at(-1)===10,'SELECTION');
 const selectedJournal=new Journal(selectionFile),selection=clone(selectedJournal.last?.state);
 need(selectedJournal.sequence===1&&selectedJournal.last.event.kind==='pair_selected'&&selectedJournal.last.event.requestHash===descriptor.outerRequestHash&&payloadHash(selection)===descriptor.selectionHash,'SELECTION');
 const qualified=selectPairedLiveCandidate({request:outer,universe,capture:selection.capture,now:Date.parse(selection.selectedAt)});
 need(qualified.eligible&&payloadHash(qualified.selection)===descriptor.selectionHash,'ORIGINAL_SELECTION_PROOF');
 const childRequestFile=path.join(childDir,'paired-live-request.json'),childAuthorityFile=path.join(childDir,'paired-live-authority.json'),childRequest=read(childRequestFile);
 const expectedChild={schemaVersion:1,enabled:true,ownerId:'Codex',runId,tournamentId:TID,tournamentSlug:outer.tournamentSlug,markets:clone(selection.scope.markets),startedAt:outer.startedAt,entryExpiresAt:selection.reviewExpiresAt,authorityHash:descriptor.authorityHash,codeIdentity:pairedLiveIdentity(workspace)};
 need(payloadHash(childRequest)===descriptor.childRequestHash&&payloadHash(childRequest)===payloadHash(expectedChild)&&payloadHash(read(childAuthorityFile))===descriptor.authorityHash,'ORIGINAL_CHILD');
 const runDir=path.join(childDir,'paired-live-runs',runId),controllerDir=path.join(runDir,'controller'),registryDir=path.join(runDir,'funding'),activationFile=path.join(runDir,'activation.json'),captureFile=path.join(runDir,'capture.json');
 const activation=read(activationFile),capture=read(captureFile),scope=clone(selection.scope);
 need(payloadHash(activation)===descriptor.activationHash&&activation.runId===runId&&activation.activationAuthorityHash===descriptor.authorityHash&&activation.captureHash===selection.captureHash&&activation.entryExpiresAt===selection.reviewExpiresAt&&payloadHash(capture)===selection.captureHash&&path.resolve(activation.controllerDir)===controllerDir&&path.resolve(activation.registryDir)===registryDir,'ORIGINAL_ACTIVATION');
 need(['paired-rest-journal.jsonl','paired-shared-funding.jsonl'].every((name,i)=>{const file=path.join(i?registryDir:controllerDir,name);return fs.existsSync(file)&&fs.statSync(file).size>0;}),'EXISTING_JOURNALS_REQUIRED');
 // Selection qualification and actual actor initialization have distinct wall
 // clocks, hence distinct plan hashes. Both original proofs remain bound.
 const priorActor=new Journal(path.join(controllerDir,'paired-rest-journal.jsonl'));
 need(priorActor.last?.state?.plan?.planHash===activation.planHash&&priorActor.last.state.plan.captureHash===selection.captureHash&&payloadHash(priorActor.last.state.plan.scope)===payloadHash(scope),'ORIGINAL_ACTOR_PLAN');
 need(descriptor.neutralAckRevision?.scopeHash===payloadHash(scope)&&descriptor.neutralAckRevision.ownerId==='Codex'&&descriptor.neutralAckRevision.runId===runId,'REVISION_SCOPE');
 const selectorLease=acquireOwnerLease({dir,ownerId:'Codex',runId,scope:'paired-live-selector-v2',now});
 let controller=null,registry=null,bridge=null,lastFunding=null,closed=false,busy=false,failed=false,lastFailure=null,lastAction='recovery_loaded';
 const fixed=()=>{selectorLease.assertOwnership();need(!fs.existsSync(legacyFile)&&payloadHash(read(descriptorFile))===descriptorHash&&payloadHash(pairedNeutralRecoveryIdentity(workspace))===payloadHash(code),'REVISION_CHANGED');for(const [file,hash]of [[outerFile,descriptor.outerRequestHash],[authorityFile,descriptor.authorityHash],[childRequestFile,descriptor.childRequestHash],[childAuthorityFile,descriptor.authorityHash],[activationFile,descriptor.activationHash],[captureFile,selection.captureHash]])need(payloadHash(read(file))===hash,'ORIGINAL_SOURCE_CHANGED');need(fs.readFileSync(selectionFile).equals(selectionBytes),'SELECTION_CHANGED');};
 const allowed=()=>{try{if(closed||failed||canOperate()!==true)return false;fixed();return true;}catch{return false;}};
 const globalRisk=()=>{const core=getOtherState(),pair=controller.status();return{ownerId:'Codex',runId,tournamentId:TID,complete:true,soleCollector:true,stopped:!allowed(),at:new Date(now()).toISOString(),unresolvedIntentIds:[...Object.values(core.intents).filter(i=>i.status!=='closed').map(i=>i.intentId),...pair.orders.filter(o=>o.phase!=='reconciled').map(o=>o.intentId)],sourceHash:payloadHash({core,paired:pair,accountLease:getAccountLease()})};};
 const status=()=>{const s=controller?.status();return{hostVersion:3,mode:'paired_neutral_ack_recovery_only',runId,requestHash:descriptor.outerRequestHash,recoveryDescriptorHash:descriptorHash,authorityHash:descriptor.authorityHash,selectionHash:descriptor.selectionHash,selectedMarkets:clone(scope.markets),entryExpiresAt:outer.entryExpiresAt,reviewExpiresAt:selection.reviewExpiresAt,initialized:!!controller,phase:s?.phase??'recovery_unavailable',orders:s?.orders.length??0,blocksLegacyWrites:true,entryAllowed:false,writeCapability:false,lastAction,lastFailure,failed:failed||s?.failed===true,closed,additionalPersistentProcesses:0,additionalAccountClients:0};};
 function publish(action){lastAction=action;const s=controller?.status(),view={schemaVersion:3,...status(),observedAt:new Date(now()).toISOString(),action,orders:(s?.orders||[]).map(o=>({intentId:o.intentId,orderId:o.orderId,marketId:o.marketId,exchangeId:o.exchangeId,phase:o.phase,limit:o.body.price,quantity:1,cancelPending:o.cancelPending,proofHash:o.proofHash})),orderCount:s?.orders.length??0,owned:s?.owned||[],funding:s?.funding??null,realizedPnl:'0.000000',ordinaryMinimumPayout:null,ordinarySurplus:null,ordinaryResolutionOnly:true,maximumExceptionalLoss:s?.funding.ownedPrincipal??'0.000000',profitabilityEstablished:false};try{atomicJson(path.join(runtimeDir,'paired-live-stats.json'),view);}catch(e){failed=true;e.fatal=true;e.durableRecordingFailed=true;throw e;}return status();}
 function cleanup(){let error;for(const h of [bridge,registry,controller,selectorLease])try{h?.close?h.close():h?.release();}catch(e){error??=e;}bridge=registry=controller=null;if(error)throw error;}
 try{
  need(allowed(),'STOPPED_OR_CHANGED');
  controller=openPairedRestControllerV3({dir:controllerDir,capture,scope,dispatch:async()=>{throw Error('PAIRED_NEUTRAL_RECOVERY_WRITES_DISABLED');},canOperate:allowed,assertAccountLease:getAccountLease,installFundingReservation:r=>{lastFunding=registry.installFundingReservation(r);return lastFunding;},getEvidence:context=>bridge.getEvidence(context),rawReceiptOf:receiptOf,now,neutralAckRevision:descriptor.neutralAckRevision});
  need(controller.status().planHash===activation.planHash,'ORIGINAL_ACTOR_PLAN');
  const originalOrder=controller.status().orders;need(originalOrder.length===1&&originalOrder[0].orderId===descriptor.neutralAckRevision.orderId&&originalOrder[0].marketId===controller.status().planLegs[0].marketId&&['acknowledged','reconciled'].includes(originalOrder[0].phase),'ONLY_ORIGINAL_FIRST_ORDER');
  registry=openPairedSharedFunding({dir:registryDir,activation,controllerDir,getAccountLease,getOtherState,getPairedFunding:()=>controller.fundingReservation(),getPairedStatus:()=>controller.status(),canOperate:allowed,now});
  lastFunding=registry.installFundingReservation(controller.fundingReservation());
  bridge=createPairedRestEvidenceBridge({api,receiptOf,scope,getGlobalRisk:globalRisk,getFunding:()=>lastFunding,getOtherReserved:()=>formatMicros(Object.values(getOtherState().intents).filter(i=>i.status!=='closed'&&i.action==='buy').reduce((n,i)=>n+decimalMicros(i.price)*BigInt(i.quantity-i.reconciledFills.reduce((q,f)=>q+f.quantity,0)),0n)),ownsOrder:(id,leg)=>id===descriptor.neutralAckRevision.orderId&&controller.status().orders.some(o=>o.orderId===id&&o.marketId===leg.marketId&&o.exchangeId===leg.exchangeId),canRead:allowed,hasReadHeadroom,now,maxReadsPerOperation:{preparation:5,reconciliation:6,cancel:1}});
  publish('recovery_loaded');
 }catch(error){if(controller?.status().failed||registry?.status().failed||['EIO','ENOSPC','EROFS','EPERM','EACCES','EBUSY'].includes(error.code)){error.fatal=true;error.durableRecordingFailed=true;}try{cleanup();}catch{error.cleanupFailed=true;}throw error;}
 return{authorizes:spec=>spec?.method==='GET'&&allowed()&&bridge.authorizes(spec)===true,onFetchStart:()=>false,blocksLegacyWrites:()=>true,allowsLegacy:spec=>spec?.method==='GET',status,
  async step(){need(!busy&&!closed,'BUSY_OR_CLOSED');busy=true;try{if(!allowed())return publish('operational_stop_or_identity_changed');const s=controller.status(),o=s.orders.find(o=>o.phase!=='reconciled');if(!o)return publish('original_lot_reconciled_reserve_retained');need(o.orderId===descriptor.neutralAckRevision.orderId,'ORDER_CHANGED');const leg=s.planLegs.find(l=>l.marketId===o.marketId&&l.exchangeId===o.exchangeId);const evidence=await bridge.refreshReconciliation(o.orderId,leg);need(allowed(),'STOPPED_AFTER_READS');await controller.reconcile(evidence);lastFailure=null;return publish('official_scoped_reconciliation');}
   catch(e){lastFailure=/^[A-Z][A-Z0-9_]+$/.test(e.code||e.message||'')?(e.code||e.message):'RECOVERY_EVIDENCE_UNAVAILABLE';if(controller?.status().failed||registry?.status().failed||e.fatal||e.durableRecordingFailed){failed=true;e.fatal=true;e.durableRecordingFailed=true;throw e;}return publish('reconciliation_blocked');}finally{busy=false;}},
  close(){need(!busy,'BUSY');if(closed)return;closed=true;cleanup();}};
}
