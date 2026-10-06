import fs from 'node:fs';import path from 'node:path';
import {payloadHash} from './core.mjs';
import {acquireOwnerLease,atomicJson} from './ledger.mjs';
import {Journal} from '../live-paper/transport-runtime.mjs';
import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';
import {derivePairedRestPlan} from './paired-rest-controller.mjs';
import {openPairedLiveHost,pairedLiveIdentity} from './paired-live-host.mjs';

const TID='bda92870-621e-47b0-bc3c-3602c5c26f55',clone=x=>structuredClone(x),read=p=>JSON.parse(fs.readFileSync(p,'utf8'));
const need=(ok,code)=>{if(!ok)throw Object.assign(Error('PAIRED_V2_'+code),{code:'PAIRED_V2_'+code});};
const hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x),id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const clock=x=>{const n=Date.parse(x);need(Number.isSafeInteger(n)&&new Date(n).toISOString()===x,'CLOCK');return n;};
const write=(file,value)=>{try{atomicJson(file,value);}catch(e){e.fatal=true;e.durableRecordingFailed=true;throw e;}};
export const pairedLiveV2Identity=workspace=>collectFrozenPaperFiles(workspace,['execution/paired-live-host-v2.mjs','execution/scoped-rest-receipts.mjs']);
function validateRequest(r,u){
 need(r?.schemaVersion===2&&r.mode==='one_pair_from_allowed_markets_v2'&&r.enabled===true&&r.ownerId==='Codex'&&r.tournamentId===TID&&r.tournamentSlug==='midterm-elections'&&typeof r.runId==='string'&&/^sig-pilot-paired-[a-z0-9-]{1,60}$/.test(r.runId),'REQUEST');
 need(u?.tournamentId===TID&&hex(u.hash)&&r.universeHash===u.hash&&Array.isArray(u.exchanges),'UNIVERSE');
 need(Array.isArray(r.allowedMarkets)&&r.allowedMarkets.length>=2&&r.allowedMarkets.length<=300&&new Set(r.allowedMarkets.map(m=>m.marketId)).size===r.allowedMarkets.length&&new Set(r.allowedMarkets.map(m=>m.exchangeId)).size===r.allowedMarkets.length&&r.allowedMarkets.every(m=>m&&Object.keys(m).sort().join('|')==='exchangeId|marketId'&&id(m.marketId)&&id(m.exchangeId)&&u.exchanges.some(x=>x.marketId===m.marketId&&x.exchangeId===m.exchangeId&&x.option==='YES')),'ALLOWED_MARKETS');
 need(clock(r.entryExpiresAt)>clock(r.startedAt)&&clock(r.entryExpiresAt)-clock(r.startedAt)<=3600000&&hex(r.authorityHash),'WINDOW_OR_AUTHORITY');return r;
}
// A candidate is a read-only observation. Neither its positive display flag nor
// metadata pairing alone authorizes it: the original roots, quote/depth, fixed
// conditional floor and source clocks are revalidated by the frozen planner.
export function selectPairedLiveCandidate({request,universe,capture,now}){
 validateRequest(request,universe);need(Number.isSafeInteger(now),'CLOCK');
 if(now<clock(request.startedAt)||now>=clock(request.entryExpiresAt))return{eligible:false,reason:'entry_window_closed'};
 if(capture?.phase!=='complete')return{eligible:false,reason:'awaiting_complete_capture'};
 if(capture.screen?.positiveConditionalScreen!==true)return{eligible:false,reason:'no_qualified_pair'};
 if(!Array.isArray(capture.legs)||capture.legs.length!==2||capture.legs.some(l=>!request.allowedMarkets.some(m=>m.marketId===l.marketId&&m.exchangeId===l.exchangeId)))return{eligible:false,reason:'outside_allowed_markets'};
 try{const markets=capture.legs.map(({marketId,exchangeId})=>({marketId,exchangeId})),scope={ownerId:'Codex',runId:request.runId,tournamentId:TID,tournamentSlug:request.tournamentSlug,markets},selectedAt=new Date(now).toISOString(),plan=derivePairedRestPlan({capture,scope,readAt:selectedAt});
  if(clock(plan.reviewExpiresAt)>clock(request.entryExpiresAt))return{eligible:false,reason:'review_outside_entry_window'};
  return{eligible:true,selection:{schemaVersion:1,requestHash:payloadHash(request),authorityHash:request.authorityHash,selectedAt,scope,capture:clone(capture),captureHash:payloadHash(capture),qualificationPlanHash:plan.planHash,reviewExpiresAt:plan.reviewExpiresAt,originalProtectiveCost:plan.originalProtectiveCost,pairCostCeiling:plan.pairCostCeiling,minimumOrdinaryMargin:plan.minimumOrdinaryMargin??plan.sourceMinimumOrdinaryMargin}};
 }catch{return{eligible:false,reason:'awaiting_fresh_qualified_capture'};}
}

// One selection, then the unchanged v1 actor/bridge/funding lifecycle. No new
// client, account lease, limiter, timer, dynamic pair replacement or renewal.
export function openPairedLiveHostV2({runtimeDir,workspace,universe,api,receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,now=Date.now}){
 const file=path.join(runtimeDir,'paired-live-v2-request.json');if(!fs.existsSync(file))return null;
 const legacyRequestFile=path.join(runtimeDir,'paired-live-request.json');need(!fs.existsSync(legacyRequestFile),'LEGACY_REQUEST_MUST_BE_RETIRED');
 need(path.isAbsolute(runtimeDir)&&path.isAbsolute(workspace)&&[receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,now].every(f=>typeof f==='function'),'DEPENDENCIES');
 universe=clone(universe);
 const request=clone(validateRequest(read(file),universe)),requestHash=payloadHash(request),authorityFile=path.join(runtimeDir,'paired-live-authority.json'),authority=read(authorityFile),code=pairedLiveV2Identity(workspace);
 need(payloadHash(code)===payloadHash(request.codeIdentity),'CODE');
 need(payloadHash(authority)===request.authorityHash&&authority.kind==='direct_human_authorized_bot_update'&&authority.ownerId==='Codex'&&authority.threadId==='01a0f8cc-b9bb-77d2-9f4c-2844396f1b79'&&authority.exploratory===true&&authority.profitabilityEstablished===false,'AUTHORITY');
 const dir=path.join(runtimeDir,'paired-live-v2-runs',request.runId),childDir=path.join(dir,'child-runtime'),selectionFile=path.join(dir,'selection.jsonl');
 const lease=acquireOwnerLease({dir,ownerId:'Codex',runId:request.runId,scope:'paired-live-selector-v2',now});let selection=null,delegate=null,closed=false,busy=false,failed=false,waitReason='awaiting_complete_capture',selectionBytes=null;
 const fixedProof=()=>{lease.assertOwnership();need(!fs.existsSync(legacyRequestFile),'LEGACY_REQUEST_MUST_BE_RETIRED');need(payloadHash(read(file))===requestHash&&payloadHash(read(authorityFile))===request.authorityHash&&payloadHash(pairedLiveV2Identity(workspace))===payloadHash(code),'SOURCE_IDENTITY');if(selectionBytes)need(fs.readFileSync(selectionFile).equals(selectionBytes),'SELECTION_CHANGED');else need(!fs.existsSync(selectionFile),'UNEXPECTED_SELECTION');};
 const allowed=()=>{try{if(closed||failed||canOperate()!==true)return false;fixedProof();return true;}catch{return false;}};
 const childActor=()=>path.join(childDir,'paired-live-runs',request.runId,'controller','paired-rest-journal.jsonl');
 function childRequest(){return{schemaVersion:1,enabled:true,ownerId:'Codex',runId:request.runId,tournamentId:TID,tournamentSlug:request.tournamentSlug,markets:clone(selection.scope.markets),startedAt:request.startedAt,entryExpiresAt:selection.reviewExpiresAt,authorityHash:request.authorityHash,codeIdentity:pairedLiveIdentity(workspace)};}
 function ensureExact(file,value){if(fs.existsSync(file))need(payloadHash(read(file))===payloadHash(value),'CHILD_IDENTITY');else write(file,value);}
 function openChild(){need(selection,'SELECTION_REQUIRED');fs.mkdirSync(childDir,{recursive:true,mode:0o700});ensureExact(path.join(childDir,'paired-live-authority.json'),authority);ensureExact(path.join(childDir,'paired-live-request.json'),childRequest());delegate=openPairedLiveHost({runtimeDir:childDir,workspace,api,receiptOf,getAccountLease,getOtherState,canOperate:allowed,hasReadHeadroom,now});need(delegate,'CHILD_REQUIRED');}
 const blocked=()=>selection!==null&&(!delegate?.status().initialized||delegate.blocksLegacyWrites());
 function publicStatus(){const s=delegate?.status();return{hostVersion:2,mode:'official_one_pair_rest',runId:request.runId,requestHash,authorityHash:request.authorityHash,universeHash:request.universeHash,allowedMarketCount:request.allowedMarkets.length,selectedMarkets:clone(selection?.scope.markets??[]),selectionHash:selection?payloadHash(selection):null,captureHash:selection?.captureHash??null,entryExpiresAt:request.entryExpiresAt,reviewExpiresAt:selection?.reviewExpiresAt??null,initialized:s?.initialized??false,phase:s?.initialized?s.phase:selection?'selected_waiting_for_initialization':waitReason,waitReason,lastFailure:s?.lastFailure??null,failed:failed||s?.failed===true,closed,orders:s?.orders??0,blocksLegacyWrites:blocked(),additionalPersistentProcesses:0,additionalAccountClients:0};}
 function publish(){const status=publicStatus(),childStatsFile=path.join(childDir,'paired-live-stats.json');let child={};if(delegate&&fs.existsSync(childStatsFile)){const v=read(childStatsFile);need(v.runId===request.runId&&v.authorityHash===request.authorityHash,'CHILD_STATS');for(const key of ['action','orders','owned','funding','realizedPnl','ordinaryMinimumPayout','ordinarySurplus','ordinaryResolutionOnly','maximumExceptionalLoss','profitabilityEstablished'])if(Object.hasOwn(v,key))child[key]=v[key];}write(path.join(runtimeDir,'paired-live-stats.json'),{schemaVersion:2,...child,...status,orders:child.orders??[],orderCount:status.orders,observedAt:new Date(now()).toISOString()});return status;}
 function cleanup(){let error;try{delegate?.close();}catch(e){error=e;}delegate=null;try{lease.release();}catch(e){error??=e;}if(error)throw error;}
 try{
  if(fs.existsSync(selectionFile)){const bytes=fs.readFileSync(selectionFile);need(bytes.length>0&&bytes.length<=2*1024*1024&&bytes.at(-1)===10,'SELECTION_JOURNAL');const journal=new Journal(selectionFile);need(journal.sequence===1&&journal.last.event.kind==='pair_selected'&&journal.last.event.requestHash===requestHash,'SELECTION_JOURNAL');selection=clone(journal.last.state);const qualified=selectPairedLiveCandidate({request,universe,capture:selection.capture,now:clock(selection.selectedAt)});need(qualified.eligible&&payloadHash(qualified.selection)===payloadHash(selection),'SELECTION_PROOF');selectionBytes=bytes;openChild();}
  else need(!fs.existsSync(childDir),'UNBOUND_CHILD');
  publish();
 }catch(error){try{cleanup();}catch{error.cleanupFailed=true;}throw error;}
 return{
  authorizes:spec=>allowed()&&delegate?.authorizes(spec)===true,onFetchStart:event=>delegate?.onFetchStart(event)??false,
  blocksLegacyWrites:blocked,allowsLegacy:spec=>selection?(delegate?.status().initialized?delegate.allowsLegacy(spec):spec?.method==='GET'):true,
  async step(capture){need(!busy&&!closed,'BUSY_OR_CLOSED');busy=true;try{
   if(!allowed()){waitReason='operational_stop_or_identity_changed';return publish();}
   if(!selection){const result=selectPairedLiveCandidate({request,universe,capture,now:now()});waitReason=result.eligible?null:result.reason;if(!result.eligible)return publish();
    fixedProof();const journal=new Journal(selectionFile);try{journal.commit(result.selection,{kind:'pair_selected',requestHash});}catch(e){failed=true;e.fatal=true;e.durableRecordingFailed=true;throw e;}selection=clone(result.selection);selectionBytes=fs.readFileSync(selectionFile);openChild();
   }
   if(!delegate)openChild();
   if(!fs.existsSync(childActor())){const fresh=selectPairedLiveCandidate({request,universe,capture:selection.capture,now:now()});if(!fresh.eligible){waitReason='selected_source_expired_unexecuted';return publish();}}
   waitReason=null;await delegate.step(selection.capture);return publish();
  }catch(error){if(error.fatal||error.durableRecordingFailed)failed=true;throw error;}finally{busy=false;}},
  status:publicStatus,close(){need(!busy,'BUSY');if(closed)return;closed=true;cleanup();}
 };
}
