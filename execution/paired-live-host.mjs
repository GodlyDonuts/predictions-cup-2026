import fs from 'node:fs';import path from 'node:path';
import {payloadHash,decimalMicros,formatMicros} from './core.mjs';
import {atomicJson} from './ledger.mjs';
import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';
import {openPairedRestController,derivePairedRestPlan} from './paired-rest-controller.mjs';
import {openPairedSharedFunding} from './paired-shared-funding.mjs';
import {createPairedRestEvidenceBridge} from './paired-rest-evidence-bridge.mjs';
import {createPairedRestCycle} from './paired-rest-cycle.mjs';

const need=(ok,code)=>{if(!ok)throw Object.assign(Error(code),{code});};
const TID='bda92870-621e-47b0-bc3c-3602c5c26f55',read=p=>JSON.parse(fs.readFileSync(p,'utf8'));
const durableJson=(file,value)=>{try{atomicJson(file,value);}catch(error){error.fatal=true;error.durableRecordingFailed=true;throw error;}};
export const pairedLiveIdentity=workspace=>collectFrozenPaperFiles(workspace,['execution/paired-live-host.mjs','execution/scoped-rest-receipts.mjs']);
// Optional ONE-pair host inside the existing collector. Its request and human
// authority audit are installed by the root task, not by agent/repository text.
export function openPairedLiveHost({runtimeDir,workspace,api,receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,now=Date.now}){
 const file=path.join(runtimeDir,'paired-live-request.json');if(!fs.existsSync(file))return null;
 const request=read(file),requestHash=payloadHash(request),authorityFile=path.join(runtimeDir,'paired-live-authority.json');
 const authority=read(authorityFile),authorityHash=payloadHash(authority);
 need(request.schemaVersion===1&&request.enabled===true&&request.ownerId==='Codex'&&request.tournamentId===TID&&request.tournamentSlug==='midterm-elections'&&/^sig-pilot-paired-[a-z0-9-]{1,60}$/.test(request.runId),'PAIRED_HOST_REQUEST');
 need(authority.kind==='direct_human_authorized_bot_update'&&authority.ownerId==='Codex'&&authority.threadId==='01a0f8cc-b9bb-77d2-9f4c-2844396f1b79'&&authority.exploratory===true&&authority.profitabilityEstablished===false&&request.authorityHash===authorityHash,'PAIRED_HOST_AUTHORITY');
 need(Array.isArray(request.markets)&&request.markets.length===2&&new Set(request.markets.map(m=>m.marketId)).size===2&&new Set(request.markets.map(m=>m.exchangeId)).size===2&&request.markets.every(m=>[m.marketId,m.exchangeId].every(id=>typeof id==='string'&&/^[1-9]\d*$/.test(id)&&Number.isSafeInteger(Number(id)))),'PAIRED_HOST_MARKETS');
 need(Number.isFinite(Date.parse(request.startedAt))&&Date.parse(request.entryExpiresAt)>Date.parse(request.startedAt)&&Date.parse(request.entryExpiresAt)-Date.parse(request.startedAt)<=3600000,'PAIRED_HOST_WINDOW');
 const code=pairedLiveIdentity(workspace);need(payloadHash(request.codeIdentity)===payloadHash(code),'PAIRED_HOST_CODE');
 const dir=path.join(runtimeDir,'paired-live-runs',request.runId),controllerDir=path.join(dir,'controller'),registryDir=path.join(dir,'funding');fs.mkdirSync(dir,{recursive:true,mode:0o700});
 let controller=null,registry=null,bridge=null,cycle=null,lastFunding=null,closed=false,lastFailure=null;
 const allowed=()=>{try{return !closed&&canOperate()===true&&payloadHash(read(file))===requestHash&&payloadHash(read(authorityFile))===authorityHash&&payloadHash(pairedLiveIdentity(workspace))===payloadHash(code);}catch{return false;}};
 const entryAllowed=()=>allowed()&&now()>=Date.parse(request.startedAt)&&now()<Date.parse(request.entryExpiresAt);
 // Persist server-directed retry delays through the SAME shared limiter file.
 // In-flight 409 delay starts at response/error observation, not fetch start.
 function recordBackoff(error){const delay=error?.retryAfterMs;if(typeof delay!=='number'||!(delay>0))return;try{if(!Number.isSafeInteger(delay)||!Number.isSafeInteger(now()+delay)||now()+delay>8640000000000000){api.manualReview=true;api.onAdmission(null);}else{api.cooldown=Math.max(api.cooldown??0,now()+delay);api.next=Math.max(api.next??0,api.cooldown);api.onAdmission(api.next);}}catch(failure){failure.fatal=true;failure.durableRecordingFailed=true;throw failure;}}
 const scope={ownerId:'Codex',runId:request.runId,tournamentId:TID,tournamentSlug:request.tournamentSlug,markets:request.markets};
 const globalRisk=()=>{const core=getOtherState(),pair=controller?.status();const unresolved=[...Object.values(core.intents).filter(i=>i.status!=='closed').map(i=>i.intentId),...(pair?.orders||[]).filter(i=>i.phase!=='reconciled').map(i=>i.intentId)];return{ownerId:'Codex',runId:request.runId,tournamentId:TID,complete:true,soleCollector:true,stopped:!allowed(),at:new Date(now()).toISOString(),unresolvedIntentIds:unresolved,sourceHash:payloadHash({core,paired:pair,accountLease:getAccountLease()})};};
 function publish(action){const s=controller?.status();durableJson(path.join(runtimeDir,'paired-live-stats.json'),{schemaVersion:1,observedAt:new Date(now()).toISOString(),runId:request.runId,ownerId:'Codex',mode:'official_one_pair_rest',phase:s?.phase??'awaiting_fresh_candidate',action,failed:s?.failed??false,lastFailure,orders:(s?.orders||[]).map(o=>({intentId:o.intentId,orderId:o.orderId,marketId:o.marketId,exchangeId:o.exchangeId,phase:o.phase,limit:o.body.price,quantity:1,cancelPending:o.cancelPending,proofHash:o.proofHash})),owned:s?.owned||[],funding:s?.funding??null,realizedPnl:'0.000000',ordinaryMinimumPayout:s?.phase==='paired'?'1.000000':null,ordinarySurplus:s?.phase==='paired'?formatMicros(1000000n-decimalMicros(s.funding.ownedPrincipal)):null,ordinaryResolutionOnly:true,maximumExceptionalLoss:s?.funding?.ownedPrincipal??'0.000000',profitabilityEstablished:false,additionalPersistentProcesses:0,authorityHash});}
 function cleanup(){let error;for(const handle of [cycle,bridge,registry,controller])try{handle?.close();}catch(e){error??=e;}cycle=bridge=registry=controller=null;lastFunding=null;if(error)throw error;}
 const actorFile=path.join(controllerDir,'paired-rest-journal.jsonl');
 const hasDurableActor=()=>fs.existsSync(actorFile)&&fs.statSync(actorFile).size>0;
 const blocksLegacy=()=>registry?.exclusiveActive()??hasDurableActor();
 function initialize(capture){
  const captureFile=path.join(dir,'capture.json'),restoring=hasDurableActor();
  if(restoring){need(fs.existsSync(captureFile),'PAIRED_HOST_CAPTURE_MISSING');capture=read(captureFile);}
  else{need(capture?.phase==='complete'&&payloadHash(capture.legs)===payloadHash(request.markets),'PAIRED_HOST_CANDIDATE');const plan=derivePairedRestPlan({capture,scope,readAt:new Date(now()).toISOString()});need(Date.parse(plan.reviewExpiresAt)<=Date.parse(request.entryExpiresAt),'PAIRED_HOST_REVIEW_OUTSIDE_REQUEST');}
  // Reject stale/malformed first captures before pinning any journal. Persist
  // the immutable capture first so a crash cannot strand an actor without it.
  try{
  if(!restoring)durableJson(captureFile,capture);
  controller=openPairedRestController({dir:controllerDir,capture,scope,dispatch:s=>api.dispatch(s),canOperate:allowed,assertAccountLease:getAccountLease,installFundingReservation:r=>{lastFunding=registry.installFundingReservation(r);return lastFunding;},getEvidence:context=>bridge.getEvidence(context),rawReceiptOf:receiptOf,now});
  const slotFile=path.join(dir,'activation.json');let activation;
  if(fs.existsSync(slotFile))activation=read(slotFile);else{const status=controller.status();activation={schemaVersion:1,kind:'one_pair_shared_funding',ownerId:'Codex',runId:request.runId,tournamentId:TID,activationAuthorityHash:authorityHash,planHash:status.planHash,captureHash:payloadHash(capture),registryDir,controllerDir,startedAt:new Date(now()).toISOString(),entryExpiresAt:new Date(Math.min(Date.parse(status.reviewExpiresAt),Date.parse(request.entryExpiresAt))).toISOString(),allocatedCapital:'2.000000',maxOrderCost:'1.000000',maxAggregateCost:'10.000000',maxCumulativeBuyCost:'25.000000',maxManagedMarkets:3,quantity:1,maxPairs:1};durableJson(slotFile,activation);}
  need(activation.activationAuthorityHash===authorityHash&&activation.runId===request.runId&&activation.planHash===controller.status().planHash&&activation.captureHash===payloadHash(capture)&&Date.parse(activation.startedAt)>=Date.parse(request.startedAt)&&activation.entryExpiresAt===controller.status().reviewExpiresAt&&Date.parse(activation.entryExpiresAt)<=Date.parse(request.entryExpiresAt),'PAIRED_HOST_ACTIVATION_BINDING');
  registry=openPairedSharedFunding({dir:registryDir,activation,controllerDir,getAccountLease,getOtherState,getPairedFunding:()=>controller.fundingReservation(),getPairedStatus:()=>controller.status(),canOperate:allowed,now});
  lastFunding=registry.installFundingReservation(controller.fundingReservation());
  bridge=createPairedRestEvidenceBridge({api,receiptOf,scope,getGlobalRisk:globalRisk,getFunding:()=>lastFunding,getOtherReserved:()=>{const s=getOtherState();return formatMicros(Object.values(s.intents).filter(i=>i.status!=='closed'&&i.action==='buy').reduce((n,i)=>n+decimalMicros(i.price)*BigInt(i.quantity-i.reconciledFills.reduce((q,f)=>q+f.quantity,0)),0n));},ownsOrder:(id,leg)=>controller.status().orders.some(o=>o.orderId===id&&o.marketId===leg.marketId&&o.exchangeId===leg.exchangeId),canRead:allowed,hasReadHeadroom,now,maxReadsPerOperation:{preparation:5,reconciliation:6,cancel:1}});
  cycle=createPairedRestCycle({controller,bridge,entryAllowed:()=>entryAllowed()&&now()<Date.parse(activation.entryExpiresAt),canOperate:allowed,recoveryAllowed:()=>api.manualReview!==true&&Number.isFinite(api.cooldown??0)&&Number.isFinite(api.next??0)&&Math.max(api.cooldown??0,api.next??0)<=now(),now,onStatus:r=>publish(r.action)});
  }catch(error){const durableFailed=controller?.status().failed===true||registry?.status().failed===true||['EIO','ENOSPC','EROFS','EPERM','EACCES','EBUSY'].includes(error.code);try{cleanup();}catch{error.cleanupFailed=true;}if(durableFailed){error.fatal=true;error.durableRecordingFailed=true;}throw error;}
 }
 try{if(hasDurableActor())initialize(null);publish('host_loaded');}catch(error){try{cleanup();}catch{error.cleanupFailed=true;}throw error;}
 return{authorizes:spec=>bridge?.authorizes(spec)===true||controller?.authorizes(spec)===true,onFetchStart:event=>controller?.onFetchStart(event)??false,blocksLegacyWrites:blocksLegacy,allowsLegacy:spec=>registry?.allowsOther(spec)??!blocksLegacy(),
  async step(capture){if(!allowed())return;try{if(!controller){if(!entryAllowed()||capture?.phase!=='complete'||payloadHash(capture.legs)!==payloadHash(request.markets))return;initialize(capture);}lastFailure=null;await cycle.step();}catch(e){recordBackoff(e);lastFailure=/^[A-Z][A-Z0-9_]+$/.test(e.code||e.message||'')?(e.code||e.message):'PAIRED_HOST_OPERATION_UNAVAILABLE';publish('operation_blocked');if(e.fatal||e.durableRecordingFailed)throw e;}},
  status:()=>({runId:request.runId,initialized:!!controller,phase:controller?.status().phase??'awaiting_fresh_candidate',lastFailure,blocksLegacyWrites:blocksLegacy(),orders:controller?.status().orders.length??0}),
  close(){if(closed)return;closed=true;cleanup();}
 };
}
