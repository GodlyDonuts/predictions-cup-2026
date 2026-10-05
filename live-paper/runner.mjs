import fs from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import {randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {loadKey} from '../bot/api.mjs';
import {initialState,normalizeBook,observe,report,assertState,sampleEquity,validateConfig,restoreState,invalidate,digest} from './model.mjs';
import {Journal,lock,ReadClient,atomicJson,exclusionSet} from './transport-runtime.mjs';
import {validateUniverse,priceBatch,priceEndpoint,normalizePrices,updateCoverage} from './breadth.mjs';
import {validateDepthPlan} from './depth-scope.mjs';
import {connectTournament} from './connection.mjs';
import {validateTiming,timingSummary} from './read-timing.mjs';
import {baselineView,workerHealth} from './worker-health.mjs';
import {historyEndpoint,normalizeHistory} from './history.mjs';
import {validateDipPolicy} from './dip-shadow.mjs';
import {accountPositions,accountOrders,accountFills,accountPnl,accountView} from './account.mjs';
import {createSchedulerState,validateSchedulerState,planBookReads,planHistoryReads,recordSchedulerAttempts} from './book-scheduler.mjs';
import {collectionInputs,shockCollectionRisk} from './collection-inputs.mjs';
import {basketCandidates} from './basket-inputs.mjs';
import {planBasketBooks} from './basket-book-plan.mjs';
import {basketCodeIdentity} from './basket-watch.mjs';
import {createBasketRelationshipReader} from './basket-relations.mjs';
import {createContractEvidenceReader} from './contract-evidence.mjs';
import {checkpointCoverage} from './coverage-checkpoint.mjs';
import {readPairedPackageFocus,focusPairedPackageReads} from './paired-package-read-plan.mjs';
import {exactMoneyFetch,createMoneyReceiptWriter} from '../execution/exact-money-receipts.mjs';
import {createBookPublication} from './book-publication-v2.mjs';
import {createNativeBookPublication} from './book-publication-v3.mjs';
import {collectorNativePublicationConfig} from './collector-publication-native.mjs';
import {collectorNativeJournalConfig} from './collector-journal-native.mjs';
import {openNativeJournal} from './native-journal-restore.mjs';
import {createBasketFollowupSource} from './basket-followup-source.mjs';
import {createComparisonRiskSource} from './comparison-risk-source.mjs';
import {openNativeDipHost} from './native-dip-host-v1.mjs';
import {openNativeDipCacheHost} from './native-dip-host-v2.mjs';
import {openNativeDipIntentHost} from './native-dip-host-v3.mjs';
import {openNativeQuoteHost} from './native-quote-host-v1.mjs';
import {createAccountEligibilityPublication} from './account-eligibility-publication-v1.mjs';
import {openCollectorMarketScreen} from './collector-market-screen-v1.mjs';
import {planNativeDiscoveryBooks,recordNativeDiscoveryAttempt} from './native-discovery-plan-v1.mjs';
import {planComparisonSampling,recordComparisonAttempt} from './comparison-sampling.mjs';
import {atomicJson as durableAtomicJson} from '../execution/ledger.mjs';
import {CompetitionController} from '../execution/controller.mjs';
import {SettlementCompetitionController} from '../execution/settlement-controller.mjs';
import {validateProductionActivation,productionHandoffCurrent} from '../execution/production-activation.mjs';
import {openProductionMetadata} from './production-metadata.mjs';
import {prioritizeProductionReads} from './production-read-priority.mjs';
import {loadNetworkBudget,networkInterval} from './transport-network-budget.mjs';
import {collectionCycleWait} from './cycle-wait.mjs';
import {loadMarketSweep,collectMarketSweep} from './market-sweep.mjs';
import {createMarketPricePublication} from './market-price-publication.mjs';
import {readMarketHistoryInputs} from './market-history-inputs.mjs';
import {createClient} from '@supabase/supabase-js';
import {Capture} from '../feed/capture.mjs';
import {createRealtimeHost} from './realtime-host.mjs';
import {realtimeCaptureScope} from './realtime-scope.mjs';
import {paperExperimentRetired} from './experiment-retirement.mjs';
import {readEdgeObservationFocus,planEdgeObservation} from './edge-observation-plan.mjs';
import {prioritizeEdgeTrials} from './edge-trial-projection.mjs';
import {readAnchorReadFocus,prioritizeAnchorReads} from './bid-anchor-read-focus.mjs';
import {readBulkReadFocus,mergePaperReadFocus} from './bulk-dip-read-focus.mjs';
import {openDiagnosticFocus,diagnosticDepthFocus} from './diagnostic-read-focus.mjs';
import {readProbabilityReadFocus,mergeProbabilityReadFocus} from './probability-read-focus.mjs';
import {openCanonicalCapacityMonitor} from './canonical-capacity-monitor.mjs';
import {createTradeTape,selectAuxiliaryReads} from './trade-tape.mjs';
import {guardCanonicalCapacity} from './canonical-capacity-guard.mjs';
import {planMakerPairReads} from './maker-read-plan.mjs';
import {createBaalReadAttention} from './baal-read-attention.mjs';
import {openNoMakerHost} from './no-maker-host.mjs';
import {openNoMakerHostV2} from './no-maker-host-v2.mjs';
import {openNoMakerHostV3} from './no-maker-host-v3.mjs';
import {openNoMakerHostV5} from './no-maker-host-v5.mjs';
import {openNoMakerHostV4} from './no-maker-host-v4.mjs';
import {openBaalPairHost} from './baal-pair-host.mjs';
import {openBaalPairHost as openBaalPairHostV2} from './baal-pair-host-v2.mjs';
import {openBaalPairHost as openBaalPairHostV3} from './baal-pair-host-v3.mjs';
import {openBaalPairHost as openBaalPairHostV4} from './baal-pair-host-v4.mjs';
import {probabilityAccountEligibility} from './probability-receipt.mjs';
import {frozenPaperModule} from './frozen-paper-runtime.mjs';

const root=path.dirname(fileURLToPath(import.meta.url)),workspace=path.dirname(root);
if(process.argv.includes('--live'))throw Error('Competition order execution is disabled');
const configFile=process.argv.includes('--config')?path.resolve(process.argv[process.argv.indexOf('--config')+1]):path.join(root,'config.json');
const c=validateConfig(JSON.parse(fs.readFileSync(configFile,'utf8')));
const runtimeDir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA||workspace,'SigTradingCup','paper-runtime');
const networkBudget=loadNetworkBudget(path.join(runtimeDir,'coordinated-network-budget.json')),networkIntervalMs=networkInterval(c.readIntervalMs,networkBudget);
// Old enabled activations are historical evidence, never a restart permit.
const productionActivationFile=path.join(runtimeDir,'competition-activation.json');
const productionActivation=fs.existsSync(productionActivationFile)?JSON.parse(fs.readFileSync(productionActivationFile,'utf8')):null;
if(productionActivation?.enabled===true){validateProductionActivation(productionActivation);const authorityKeys=productionActivation.authorityKind==='direct_human_assignment'?['authorityKind','assignmentCommentId']:['releaseCommentId'];if(!productionHandoffCurrent(runtimeDir,productionActivation)||networkBudget?.schemaVersion!==4||['handoffId','ackCommentId',...authorityKeys].some(k=>networkBudget[k]!==productionActivation[k]))throw Error('PRODUCTION_HANDOFF_OR_BUDGET_REQUIRED');}
else if(networkBudget?.schemaVersion===4)throw Error('PRODUCTION_BUDGET_WITHOUT_ACTIVATION');
const stopFile=path.join(workspace,'STOP'),paperStop=path.join(runtimeDir,'STOP'),collectorStop=path.join(runtimeDir,'COLLECTOR_STOP');
if(fs.existsSync(stopFile)||fs.existsSync(paperStop)||fs.existsSync(collectorStop)){console.log(JSON.stringify({status:'stopped',reason:'STOP_present'}));process.exit(0);}
fs.mkdirSync(runtimeDir,{recursive:true,mode:0o700});
const release=lock(path.join(process.env.LOCALAPPDATA||workspace,'SigTradingCup','bot.lock'));
let cleaned=false;function cleanup(){if(!cleaned){cleaned=true;release();}}
process.on('exit',cleanup);
const control=new AbortController();let stopping=false;
const stop=(cause='unspecified',errorCode=null)=>{
 if(stopping)return;
 runtime.stopProvenance={at:new Date().toISOString(),cause,errorCode:/^[A-Z0-9_]{1,80}$/.test(errorCode||'')?errorCode:null,pid:process.pid};
 // Preserve the cause independently when a status-file failure caused shutdown.
 // Exception messages, paths, account data and headers never enter this record.
 try{const fd=fs.openSync(path.join(runtimeDir,'collector-lifecycle.jsonl'),'a',0o600);try{fs.writeSync(fd,JSON.stringify(runtime.stopProvenance)+'\n');fs.fsyncSync(fd);}finally{fs.closeSync(fd);}}catch{}
 console.log(JSON.stringify({event:'collector_stopping',...runtime.stopProvenance}));
 stopping=true;control.abort();
};process.on('SIGINT',()=>stop('SIGINT'));process.on('SIGTERM',()=>stop('SIGTERM'));
const stopWatch=setInterval(()=>{for(const [file,cause]of [[stopFile,'workspace_STOP'],[paperStop,'runtime_STOP'],[collectorStop,'COLLECTOR_STOP']])if(fs.existsSync(file)){stop(cause);break;}},200);
let journal,state,api,server,heartbeat,competition,realtime,realtimeCapture,basketRelationships,contractEvidence,bookPublication,basketFollowupSource,basketFollowupBankHash,comparisonRiskSource;
let productionMetadata;
const samplingSessionId=randomUUID();
let universe,coverage,depthPlan,dipBank,scaleBank,responsiveBank,scheduler,schedulerScope;const recentReadTimings=[];
let diagnosticSource,diagnosticLoading=false,diagnosticFailed=false;
let canonicalCapacity,tradeTape,noMaker,baalReadAttention,baalPair,nativeDip,nativeQuote,eligibilityPublication,collectorMarketScreen;
function collectDiagnosticFocus(){
 if(!diagnosticSource&&!diagnosticLoading&&!diagnosticFailed){diagnosticLoading=true;try{const source=openDiagnosticFocus(runtimeDir,universe,{deferBootstrap:true});source.initialize({stopped:()=>stopping}).then(()=>{diagnosticSource=source;diagnosticLoading=false;}).catch(()=>{diagnosticFailed=true;diagnosticLoading=false;});}catch{diagnosticFailed=true;diagnosticLoading=false;}}
 if(!diagnosticSource){runtime.diagnosticReadFocus={status:diagnosticFailed?'failed':'loading',executionAuthority:false};return null;}
 try{diagnosticSource.consume();const projection=diagnosticSource.project({pid:process.pid,id:samplingSessionId},'collector-diagnostic');const focus=diagnosticDepthFocus(projection,universe,{pid:process.pid,lockId:samplingSessionId});runtime.diagnosticReadFocus={status:focus?'running':'unavailable',...focus,projection};return focus;}catch{diagnosticFailed=true;diagnosticSource=null;runtime.diagnosticReadFocus={status:'failed',executionAuthority:false};return null;}
}
const schedulerFile=path.join(runtimeDir,'book-scheduler.json');
function saveScheduler(next){
 try{durableAtomicJson(schedulerFile,next);scheduler=next;}
 catch{const e=Error('Book scheduler recording failed');e.durableRecordingFailed=true;e.fatal=true;throw e;}
}
function recordAttempt(kind,marketId,samplingPlanId,nativeDiscovery=false){
 const at=new Date().toISOString();let next=recordSchedulerAttempts(scheduler,schedulerScope,{[kind+'MarketIds']:[marketId],attemptedAt:at});
 if(kind==='book'&&samplingPlanId)next=recordComparisonAttempt(next,schedulerScope,{marketId,at,planId:samplingPlanId});
 if(kind==='book'&&['381','382'].includes(marketId))next.packageReadLastAttemptAt={...(next.packageReadLastAttemptAt||{}),[marketId]:at};
 if(kind==='book'&&nativeDiscovery)next=recordNativeDiscoveryAttempt(next,marketId);
 saveScheduler(next);
}
function schedulingInputs(){
 const sources=[];
 for(const [name,bank] of [['dip',dipBank],['dip-scale',scaleBank],['dip-responsive',responsiveBank]])if(bank&&!paperExperimentRetired(runtimeDir,'DIP_STOP')){
  const state=JSON.parse(fs.readFileSync(path.join(runtimeDir,name+'-state.json')));let healthy=false;
  try{const stats=JSON.parse(fs.readFileSync(path.join(runtimeDir,name+'-stats.json')));healthy=stats.bankHash===digest(bank)&&workerHealth(stats).status==='running';}catch{}
  sources.push({state,bank,healthy});
 }
 const inputs=collectionInputs(sources,universe,coverage,Date.now());
 const historyScreen=collectorMarketScreen?collectorMarketScreen.projection():readMarketHistoryInputs(runtimeDir,universe,Date.now());runtime.marketHistoryScreen=historyScreen;
 if(historyScreen.available)inputs.candidateMarketIds=[...new Set([...historyScreen.candidateMarketIds,...inputs.candidateMarketIds])];
  const shockBankFile=path.join(runtimeDir,'shock-bank.json');
  if(fs.existsSync(shockBankFile)&&!paperExperimentRetired(runtimeDir,'SHOCK_STOP')){
   const risk=shockCollectionRisk(JSON.parse(fs.readFileSync(path.join(runtimeDir,'shock-state.json'))),JSON.parse(fs.readFileSync(shockBankFile)),universe);
   for(const key of ['heldMarketIds','pendingMarketIds'])inputs[key]=[...new Set([...inputs[key],...risk[key]])];
  }
  const comparisonRisk=paperExperimentRetired(runtimeDir,'ENTRY_COMPARISON_STOP')?{configured:false,available:true,reason:'OPERATOR_RETIRED_PAPER_EXPERIMENT',heldMarketIds:[],pendingMarketIds:[]}:comparisonRiskSource?.poll();
  if(comparisonRisk){
   runtime.comparisonRisk=comparisonRisk;
   for(const key of ['heldMarketIds','pendingMarketIds'])inputs[key]=[...new Set([...inputs[key],...comparisonRisk[key]])];
  }
  if(noMaker)try{const risk=noMaker.status().riskMarketIds;inputs.heldMarketIds=[...new Set([...inputs.heldMarketIds,...risk])];}catch{runtime.noMaker={status:'failed',executionAuthority:false};noMaker=null;}
  if(baalPair)try{const risk=baalPair.status().riskMarketIds;inputs.heldMarketIds=[...new Set([...inputs.heldMarketIds,...risk])];}catch{runtime.baalPair={status:'failed',executionAuthority:false};baalPair=null;}
  if(nativeDip){const risk=nativeDip.status();runtime.nativeDip=risk;inputs.heldMarketIds=[...new Set([...inputs.heldMarketIds,...risk.heldMarketIds])];inputs.pendingMarketIds=[...new Set([...inputs.pendingMarketIds,...(risk.pendingMarketIds||[])])];}
  if(nativeQuote){const risk=nativeQuote.status();runtime.nativeQuote=risk;inputs.heldMarketIds=[...new Set([...inputs.heldMarketIds,...risk.heldMarketIds])];inputs.pendingMarketIds=[...new Set([...inputs.pendingMarketIds,...risk.pendingMarketIds])];}
  inputs.priorityManagedMarketIds=competition?.managedMarketIds()||[];
  inputs.heldMarketIds=[...new Set([...inputs.heldMarketIds,...inputs.priorityManagedMarketIds])];return inputs;
}
function basketMeasurementPlan(basePlan,excluded){
 if(!dipBank)return basePlan;
 if(runtime.comparisonRisk?.configured&&!runtime.comparisonRisk.available){runtime.basketMeasurement={reason:'comparison_risk_unavailable'};return basePlan;}
 let plan;
 try{
  const file=path.join(runtimeDir,'basket-bank.json');
  if(!fs.existsSync(file)){runtime.basketMeasurement={reason:'not_initialized'};return basePlan;}
  const bank=JSON.parse(fs.readFileSync(file)),stats=JSON.parse(fs.readFileSync(path.join(runtimeDir,'basket-stats.json')));
  if(bank.mode!=='dual_no_basket_measurement'||stats.bankHash!==digest(bank)||digest(bank.codeIdentity)!==digest(basketCodeIdentity()))throw Error('Basket measurement identity mismatch');
  if(workerHealth(stats).status!=='running'||fs.existsSync(path.join(runtimeDir,'BASKET_STOP'))){runtime.basketMeasurement={reason:'observer_unavailable'};return basePlan;}
  const now=Date.now(),candidates=basketCandidates(bank.definitions,coverage,universe,now),excludedMarketIds=universe.exchanges.filter(m=>excluded.has(m.exchangeId)).map(m=>m.marketId);
  const bankHash=digest(bank);
  if(!basketFollowupSource||basketFollowupBankHash!==bankHash){basketFollowupSource?.close();basketFollowupSource=createBasketFollowupSource({runtimeDir,bank,universe,now:Date.now});basketFollowupBankHash=bankHash;}
  const followups=basketFollowupSource.poll();
  runtime.basketFollowupSource={...basketFollowupSource.status(),candidates:followups.length};
  plan=planBasketBooks(basePlan,schedulerScope,{candidates,followups,excludedMarketIds,cycle:runtime.cycles,now});
 }catch{runtime.basketMeasurement={reason:'observer_invalid_or_unavailable'};return basePlan;}
 // A consumed research anchor must survive a crash before either pair read.
 // Install this state before recordAttempt so later writes preserve the claim.
 if(plan.basketMeasurement?.persistSchedulerStateBeforeReads){
  saveScheduler(plan.state);
 }
 runtime.basketMeasurement=plan.basketMeasurement;return plan;
}
function comparisonSamplingPlan(basePlan){
 let planned;
 try{
  const file=path.join(runtimeDir,'comparison-sampling.json');
  if(!fs.existsSync(file)){runtime.comparisonSampling={enabled:false,reason:'not_configured'};return basePlan;}
  const config=JSON.parse(fs.readFileSync(file,'utf8')),risk=runtime.comparisonRisk;
  if(!config||Object.keys(config).sort().join(',')!=='bankHash,cohort,enabled,mode,schemaVersion'||config.schemaVersion!==1||config.mode!=='paired_comparison_two_attempts_v1'||config.enabled!==true)throw Error('Invalid sampling configuration');
  if(!risk?.configured||!risk.available||risk.cohort!==config.cohort||risk.bankHash!==config.bankHash){runtime.comparisonSampling={enabled:false,reason:'comparison_evidence_unavailable'};return basePlan;}
  planned=planComparisonSampling(basePlan,schedulerScope,{now:Date.now(),sessionId:samplingSessionId});
  planned.comparisonSampling=planned.measurement;
 }catch{runtime.comparisonSampling={enabled:false,reason:'sampling_evidence_invalid'};return basePlan;}
 // Persist the phase before metadata or book work; an unattempted member stays
 // pending. A later pre-request reservation may be consumed by STOP/failure,
 // which is an uncertain attempt, never an observed quote or simulated fill.
 saveScheduler(planned.state);
 runtime.comparisonSampling=planned.comparisonSampling;
 return planned;
}
let accountCache={errors:{}},officialSync=false;
function publishAccount(){atomicJson(path.join(runtimeDir,'official-account.json'),accountCache);}
async function collectAccountExtra(){
 const slot=runtime.cycles%3,kind=slot===0?'fills':slot===1?'pnl':'orderHistory',endpoint=slot===0?'/tournaments/midterm-elections/portfolio/fills?limit=100':slot===1?'/tournaments/midterm-elections/portfolio/pnl?period=day':'/orders?status=all&limit=200&tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55';
 try{const r=await api.request(endpoint);try{accountCache[kind]=slot===0?accountFills(r):slot===1?accountPnl(r):accountOrders(r.data.data,r.receivedAt,{openOnly:false});}catch(e){e.projectionInvalid=true;throw e;}delete accountCache.errors[kind];publishAccount();}
 catch(e){if(stopping)return;if(api.manualReview||e.status===401||e.status===403)throw e;accountCache.errors[kind]={at:new Date().toISOString(),reason:e.status?'http_'+e.status:e.projectionInvalid?'invalid_projection':'connection_unavailable'};publishAccount();}
}
let runtime={status:'starting',pid:process.pid,runtimeDir,readRequests:0,errors:[],cycles:0,lastCycleAt:null,dashboard:'http://127.0.0.1:'+c.port};
function persistStatus(){const r=report(state,c);if(realtime)atomicJson(path.join(runtimeDir,'realtime-stats.json'),{...realtime.status(),pid:process.pid});atomicJson(path.join(runtimeDir,'stats.json'),{...r,runtime:{...runtime,readRequests:api?.requests||0}});return r;}
function commit(event){try{journal.commit(state,event);}catch{const e=Error('Paper journal recording failed');e.durableRecordingFailed=true;throw e;}}
function publishBookEvent(method,value){try{const result=bookPublication?.[method](value);if(['invalidate','close'].includes(method)||method==='publishBook'&&result===false){
 if(noMaker)try{noMaker.boundary(method);runtime.noMaker=noMaker.status();}catch{runtime.noMaker={status:'failed',executionAuthority:false};noMaker=null;}
 if(baalPair)try{baalPair.boundary(method);runtime.baalPair=baalPair.status();}catch{runtime.baalPair={status:'failed',executionAuthority:false};baalPair=null;}
 }return result;}catch{const e=Error('Public book recording failed');e.durableRecordingFailed=true;e.fatal=true;throw e;}}
async function collectHistory(limit=2){
 const inputs=schedulingInputs(),bookPlan=planBookReads(scheduler,schedulerScope,inputs);
 const plan=planHistoryReads(scheduler,schedulerScope,{...inputs,candidateMarketIds:bookPlan.entryMarketIds},{budget:limit});
 runtime.historyActiveMarketIds=plan.activeMarketIds;
 runtime.historyScheduling={mode:plan.allocationMode,at:plan.allocationAt,dueMarketCount:plan.dueMarketCount,receiptCooldownCount:plan.receiptCooldownMarketIds.length,attemptCooldownCount:plan.attemptCooldownMarketIds.length,selected:plan.reads.map(({marketId,reason,priority,reservedDiscovery})=>({marketId,reason,priority,reservedDiscovery}))};
 for(const spec of plan.reads){
  if(stopping||api.cooldown>Date.now())break;
  recordAttempt('history',spec.marketId);
  try{const response=await api.request(historyEndpoint(spec.exchangeId,new Date().toISOString()));let h;try{h=normalizeHistory(response,spec);}catch(e){const d=response.data;runtime.history={...runtime.history,validation:{scopeMatches:d?.exchangeId===spec.exchangeId&&d?.marketId===spec.marketId,resolutionMatches:d?.resolution==='1m',candleCount:Array.isArray(d?.candles)?d.candles.length:null,completeType:typeof d?.coverage?.complete,sequenceType:typeof d?.coverage?.projectedThroughSequence,fromValid:Number.isFinite(Date.parse(d?.from)),toValid:Number.isFinite(Date.parse(d?.to)),firstCandleTypes:Array.isArray(d?.candles)&&d.candles.length?Object.fromEntries(['time','open','high','low','close','vwap','volume','tradeCount'].map(k=>[k,typeof d.candles[0][k]])):null}};if(['Invalid historical response scope','Invalid historical window','Invalid candle clock/count','Invalid candle price','Invalid OHLC bounds'].includes(e.message))e.historyValidation=e.message;throw e;}commit({type:'market_history',clockAt:h.receivedAt,history:h,allocation:{mode:plan.allocationMode,at:plan.allocationAt,reason:spec.reason,priority:spec.priority,reservedDiscovery:spec.reservedDiscovery}});runtime.history={...runtime.history,lastAt:h.receivedAt,lastExchange:spec.exchangeId,lastCandleCount:h.candles.length,successful:(runtime.history?.successful||0)+1};}
  catch(e){if(e.durableRecordingFailed)throw e;if(stopping)break;if(api.manualReview||e.status===401||e.status===403)throw e;runtime.history={...runtime.history,lastErrorAt:new Date().toISOString(),lastError:e.status?'http_'+e.status:e.historyValidation||'history_unavailable_or_invalid'};}
 }
}
async function pauseUntilNextRead(ms){try{await api.pause(ms);}catch(e){if(!stopping)throw e;}}
try{
  const journalStarted=performance.now(),nativeJournal=collectorNativeJournalConfig(runtimeDir),journalFile=path.join(runtimeDir,'journal.jsonl');
  journal=nativeJournal?await openNativeJournal({runtimeDir,binding:nativeJournal.nativeBackend,file:journalFile,receiptDir:path.join(runtimeDir,'collector-native-journal-receipts'),signal:control.signal,canOperate:()=>!stopping&&!fs.existsSync(stopFile)&&!fs.existsSync(paperStop)&&!fs.existsSync(collectorStop)}):new Journal(journalFile);
  state=journal.last?restoreState(journal.last.state,c):initialState(c,randomUUID(),new Date().toISOString());assertState(state,c);
  runtime.originalJournal={revision:nativeJournal?2:1,startupReplayMs:performance.now()-journalStarted,...(nativeJournal?{native:journal.nativeRestore}:{})};
  const syncFile=path.join(runtimeDir,'official-sync.json');officialSync=fs.existsSync(syncFile)&&JSON.parse(fs.readFileSync(syncFile)).enabled===true;
  if(officialSync){const file=path.join(runtimeDir,'official-account.json');if(fs.existsSync(file))accountCache=JSON.parse(fs.readFileSync(file));accountCache.errors??={};}
  universe=validateUniverse(JSON.parse(fs.readFileSync(path.join(root,'price-universe.json'),'utf8')),c.tournamentId);
  const marketSweep=loadMarketSweep(path.join(runtimeDir,'market-sweep.json'),universe,networkBudget);
  const pricePublication=marketSweep?createMarketPricePublication({file:path.join(runtimeDir,'market-prices.jsonl'),universe,canOperate:()=>!stopping}):null;
  comparisonRiskSource=createComparisonRiskSource({runtimeDir,universe,now:Date.now});
  const nativeOptions={runtimeDir,universe,signal:control.signal,canOperate:()=>!stopping};nativeDip=fs.existsSync(path.join(runtimeDir,'native-dip-intent-active.json'))?openNativeDipIntentHost(nativeOptions):fs.existsSync(path.join(runtimeDir,'native-dip-cache-active.json'))?openNativeDipCacheHost(nativeOptions):openNativeDipHost(nativeOptions);
  nativeDip?.kick();
  eligibilityPublication=createAccountEligibilityPublication({file:path.join(runtimeDir,'account-eligibility.jsonl'),universe,canOperate:()=>!stopping});
  nativeQuote=openNativeQuoteHost(nativeOptions);nativeQuote?.kick();
  if(nativeDip&&pricePublication)try{collectorMarketScreen=openCollectorMarketScreen({runtimeDir,universe,publisher:pricePublication,signal:control.signal,canOperate:()=>!stopping});collectorMarketScreen.initialize();}catch{const unavailable={available:false,status:'unavailable',reason:'COLLECTOR_MARKET_SCREEN_STARTUP_FAILED',candidateMarketIds:[],executionAuthority:false};collectorMarketScreen={projection:()=>unavailable,onPublished(){},close(){}};runtime.marketHistoryScreen=unavailable;}
  const publicationReplayStarted=performance.now();
  const nativePublication=collectorNativePublicationConfig(runtimeDir,universe),publicationOptions={file:path.join(runtimeDir,'public-books.jsonl'),universe,sessionId:randomUUID(),now:Date.now,canOperate:()=>!stopping};
  bookPublication=nativePublication?await createNativeBookPublication({...publicationOptions,runtimeDir,nativeBackend:nativePublication.nativeBackend,receiptDir:path.join(runtimeDir,'collector-native-publication-receipts')}):createBookPublication(publicationOptions);
  runtime.bookPublisher={revision:nativePublication?3:2,startupReplayMs:performance.now()-publicationReplayStarted,frozenPublisherFileChanged:false,...(nativePublication?{native:bookPublication.startup()}:{} )};
  try{canonicalCapacity=guardCanonicalCapacity(openCanonicalCapacityMonitor({runtimeDir,universe}));runtime.canonicalCapacity=canonicalCapacity?.status()||{status:'not_initialized',executionAuthority:false};}catch{runtime.canonicalCapacity={status:'unavailable',executionAuthority:false};}
  depthPlan=validateDepthPlan(JSON.parse(fs.readFileSync(path.join(root,'depth-plan.json'),'utf8')),c,universe);
  const dipFile=path.join(runtimeDir,'dip-bank.json');if(fs.existsSync(dipFile)){dipBank=JSON.parse(fs.readFileSync(dipFile));validateDipPolicy(dipBank.policy);if(dipBank.universe.hash!==universe.hash||dipBank.config.feeBps!==0||dipBank.policy.historyReadsPerCycle!==2||dipBank.policy.comparisonBooksPerCycle!==2||dipBank.policy.dynamicBooksPerCycle!==2)throw Error('Invalid dip collection plan');runtime.collectionPlan={mode:'diversified_dip',historyReads:officialSync?1:2,fullBookBudget:4,managedCapacity:3,bulkReads:marketSweep?Math.ceil(universe.exchanges.length/100):1,accountReads:'positions plus paginated orders',readIntervalMs:c.readIntervalMs};}
  const coverageFile=path.join(runtimeDir,'price-coverage.json');
  const scaleFile=path.join(runtimeDir,'dip-scale-bank.json');if(fs.existsSync(scaleFile)){scaleBank=JSON.parse(fs.readFileSync(scaleFile));validateConfig(scaleBank.config);validateDipPolicy(scaleBank.policy);if(!dipBank||scaleBank.universe.hash!==universe.hash||scaleBank.policy.profile!=='scale')throw Error('Invalid scaled dip bank');}
  const responsiveFile=path.join(runtimeDir,'dip-responsive-bank.json');if(fs.existsSync(responsiveFile)){responsiveBank=JSON.parse(fs.readFileSync(responsiveFile));validateConfig(responsiveBank.config);validateDipPolicy(responsiveBank.policy);if(responsiveBank.universe.hash!==universe.hash||responsiveBank.policy.executionPolicy!=='responsive_v2')throw Error('Invalid responsive bank');}
  schedulerScope={universe,coreMarketIds:c.marketIds};scheduler=fs.existsSync(schedulerFile)?validateSchedulerState(JSON.parse(fs.readFileSync(schedulerFile)),schedulerScope):createSchedulerState(schedulerScope);
  if(fs.existsSync(coverageFile)){coverage=JSON.parse(fs.readFileSync(coverageFile,'utf8'));if(coverage.universeHash!==universe.hash)coverage=null;}
  if(journal.last&&journal.last.state.engineRevision!==state.engineRevision)commit({type:'paper_engine_upgrade',fromRevision:journal.last.state.engineRevision||1,toRevision:state.engineRevision});
  let moneyReceiptWriter=null;
  const moneyFetch=exactMoneyFetch((...args)=>globalThis.fetch(...args),{tournamentId:c.tournamentId,tournamentSlug:c.tournamentSlug,onReceipt:receipt=>{
    moneyReceiptWriter??=createMoneyReceiptWriter(path.join(runtimeDir,'exact-money-receipts.jsonl'),{canCommit:()=>!stopping&&!control.signal.aborted&&!fs.existsSync(stopFile)&&!fs.existsSync(paperStop)&&!fs.existsSync(collectorStop)});
    const checkpoint=moneyReceiptWriter(receipt);runtime.exactMoneyReceipts={sequence:checkpoint.sequence,lastKind:receipt.kind,receivedAt:receipt.receivedAt,exact:Object.values(receipt.fields).every(x=>x.available),executionAuthority:false};
  }});
  api=new ReadClient(loadKey(),{fetchFn:moneyFetch,interval:c.readIntervalMs,networkBudget,signal:control.signal,authorizeDispatch:spec=>productionMetadata?.authorizes(spec)===true||tradeTape?.authorizes(spec)===true||contractEvidence?.authorizes(spec)===true||basketRelationships?.authorizes(spec)===true||realtime?.authorizes(spec)===true||competition?.transport.authorizeRequest(spec)===true,onAdmission:next=>atomicJson(path.join(runtimeDir,'rate.json'),{next,manualReview:next===null}),onTiming:event=>{validateTiming(event);fs.appendFileSync(path.join(runtimeDir,'read-timings.jsonl'),JSON.stringify(event)+'\n',{mode:0o600});recentReadTimings.push(event);if(recentReadTimings.length>1000)recentReadTimings.shift();runtime.readTiming=timingSummary(recentReadTimings);}});
  productionMetadata=openProductionMetadata({runtimeDir,universe,dispatch:spec=>api.dispatch(spec),canOperate:()=>!stopping,now:Date.now});
  try{const activeFile=path.join(runtimeDir,'no-maker-active.json'),revision=fs.existsSync(activeFile)?JSON.parse(fs.readFileSync(activeFile)).hostRevision||1:1;if(![1,2,3,4,5].includes(revision))throw Error('Unknown maker host revision');const selected=revision===5?{open:openNoMakerHostV5,path:'live-paper/no-maker-host-v5.mjs',name:'openNoMakerHostV5'}:revision===4?{open:openNoMakerHostV4,path:'live-paper/no-maker-host-v4.mjs',name:'openNoMakerHostV4'}:revision===3?{open:openNoMakerHostV3,path:'live-paper/no-maker-host-v3.mjs',name:'openNoMakerHostV3'}:revision===2?{open:openNoMakerHostV2,path:'live-paper/no-maker-host-v2.mjs',name:'openNoMakerHostV2'}:{open:openNoMakerHost,path:'live-paper/no-maker-host.mjs',name:'openNoMakerHost'};let factory=selected.open;if(fs.existsSync(activeFile)){const manifest=JSON.parse(fs.readFileSync(activeFile)),bank=JSON.parse(fs.readFileSync(path.join(runtimeDir,'no-maker-cohorts',manifest.cohort,'bank.json')));const module=await frozenPaperModule({runtimeDir,bank,modulePath:selected.path,fallback:{[selected.name]:factory}});factory=module[selected.name];}noMaker=factory({runtimeDir,canOperate:()=>!stopping&&!api.manualReview});runtime.noMaker=noMaker?.status()||{status:'not_initialized',executionAuthority:false};}catch{runtime.noMaker={status:'failed',executionAuthority:false};}
  try{const pairFile=path.join(runtimeDir,'baal-pair-active.json'),revision=fs.existsSync(pairFile)?JSON.parse(fs.readFileSync(pairFile)).hostRevision||1:1;if(![1,2,3,4].includes(revision))throw Error('Unknown pair host revision');let factory=revision===4?openBaalPairHostV4:revision===3?openBaalPairHostV3:revision===2?openBaalPairHostV2:openBaalPairHost;if(fs.existsSync(pairFile)){const manifest=JSON.parse(fs.readFileSync(pairFile)),bank=JSON.parse(fs.readFileSync(path.join(runtimeDir,'baal-pair-cohorts',manifest.cohort,'bank.json')));const module=await frozenPaperModule({runtimeDir,bank,modulePath:revision===4?'live-paper/baal-pair-host-v4.mjs':revision===3?'live-paper/baal-pair-host-v3.mjs':revision===2?'live-paper/baal-pair-host-v2.mjs':'live-paper/baal-pair-host.mjs',fallback:{openBaalPairHost:factory}});factory=module.openBaalPairHost;}baalPair=factory({runtimeDir,canOperate:()=>!stopping&&!api.manualReview});runtime.baalPair=baalPair?.status()||{status:'not_initialized',executionAuthority:false};}catch{runtime.baalPair={status:'failed',executionAuthority:false};}
  try{tradeTape=createTradeTape({runtimeDir,universe,canOperate:()=>!stopping&&!api.manualReview,onPage:row=>{if(noMaker)try{noMaker.onPage(row);runtime.noMaker=noMaker.status();}catch{runtime.noMaker={status:'failed',executionAuthority:false};noMaker=null;}}});runtime.tradeTape=tradeTape.status();}catch{runtime.tradeTape={status:'unavailable',executionAuthority:false};}
  baalReadAttention=createBaalReadAttention({runtimeDir,tournamentId:universe.tournamentId,canOperate:()=>!stopping&&!api.manualReview});
  basketRelationships=createBasketRelationshipReader({tournamentId:c.tournamentId,now:Date.now,dispatch:spec=>api.dispatch(spec),canOperate:()=>!stopping&&!api.manualReview&&!fs.existsSync(stopFile)&&!fs.existsSync(paperStop)&&!fs.existsSync(collectorStop)&&!fs.existsSync(path.join(runtimeDir,'BASKET_STOP'))});
  contractEvidence=createContractEvidenceReader({tournamentId:c.tournamentId,legs:[{marketId:'381',exchangeId:'1070',contractId:'382'},{marketId:'382',exchangeId:'1071',contractId:'383'}],now:Date.now,dispatch:spec=>api.dispatch(spec),canOperate:()=>!stopping&&!api.manualReview&&!fs.existsSync(stopFile)&&!fs.existsSync(paperStop)&&!fs.existsSync(collectorStop)&&!fs.existsSync(path.join(runtimeDir,'BASKET_STOP'))});
  runtime.networkBudget={mode:networkBudget?.schemaVersion===3?'coordinated_shared_read_only':networkBudget?'coordinated_sole_client':'default',schemaVersion:networkBudget?.schemaVersion??null,intervalMs:networkIntervalMs,writeIntervalMs:api.writeInterval,completionPacing:api.completionPacing,...(networkBudget?.schemaVersion===3?{maxReadsPerMinute:20,writesEnabled:false,executionOwner:'Daenerys'}:{})};
  if(runtime.collectionPlan)runtime.collectionPlan.readIntervalMs=networkIntervalMs;
  const activationFile=path.join(runtimeDir,'competition-activation.json');
  if(productionActivation?.enabled===true)competition=new SettlementCompetitionController({runtimeDir,api,universe,config:c,isStopped:()=>stopping,activation:productionActivation});
  if(fs.existsSync(path.join(runtimeDir,'rate.json'))){const rate=JSON.parse(fs.readFileSync(path.join(runtimeDir,'rate.json')));if(rate.manualReview||rate.next===null)throw Error('Rate cooldown requires manual review');if(!Number.isFinite(rate.next)||rate.next<0||rate.next>8640000000000000)throw Error('Invalid persisted rate state');api.next=rate.next;}
  const realtimeFile=path.join(runtimeDir,'realtime-capture.json');
  if(fs.existsSync(realtimeFile)){
    const activation=JSON.parse(fs.readFileSync(realtimeFile));
    if(activation.enabled===true){
      const scope=realtimeCaptureScope(activation,c,universe);
      const file='realtime-'+new Date().toISOString().replace(/[:.]/g,'-')+'-'+randomUUID()+'.jsonl';
      realtimeCapture=new Capture(path.join(runtimeDir,file));
      realtime=createRealtimeHost({dispatch:spec=>api.dispatch(spec),createClient,scope,capture:realtimeCapture,signal:control.signal,
        canMintToken:()=>api.readOnly!==true,
        canOperate:()=>!stopping&&!api.manualReview&&!fs.existsSync(stopFile)&&!fs.existsSync(paperStop)&&!fs.existsSync(collectorStop)&&!fs.existsSync(path.join(runtimeDir,'REALTIME_STOP'))});
      atomicJson(path.join(runtimeDir,'realtime-session.json'),{schemaVersion:1,file,startedAt:new Date().toISOString(),scope});
      runtime.realtime={mode:'market_capture_only',channelMode:scope.channelMode,marketCount:scope.markets.length,recoveryBudgetPerCycle:1};
    }
  }
  if(networkBudget)commit({type:'collection_network_budget',clockAt:new Date().toISOString(),cadencePolicy:{mode:runtime.networkBudget.mode,schemaVersion:networkBudget.schemaVersion,intervalMs:api.interval,writeIntervalMs:api.writeInterval,completionPacing:true,...(networkBudget.schemaVersion===3?{maxReadsPerMinute:20,writesEnabled:false}:{})}});
  runtime.startupToConnectionMs=performance.now();runtime.status='connecting';persistStatus();
  const html=fs.readFileSync(path.join(root,'dashboard.html'));
  server=http.createServer((req,res)=>{
    res.setHeader('Cache-Control','no-store');res.setHeader('X-Content-Type-Options','nosniff');
    if(req.method!=='GET'){res.writeHead(405);return res.end();}
    if(req.url==='/api/stats'){res.setHeader('Content-Type','application/json');const legacy=report(state,c);let primary=legacy;try{const baseline=JSON.parse(fs.readFileSync(path.join(runtimeDir,'fee-free-stats.json'),'utf8'));primary=baselineView(baseline,legacy,runtime);}catch{primary={...legacy,feeScenario:'historical_fee_stress',baselineWorkerStatus:'unavailable'};}return res.end(JSON.stringify({...primary,runtime:{...runtime,readRequests:api.requests}}));}
    if(req.url==='/api/account'){res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(accountView(accountCache,runtime.status,new Date().toISOString(),dipBank?.identities)));}
    if(req.url==='/api/execution'){res.setHeader('Content-Type','application/json');try{return res.end(fs.readFileSync(path.join(runtimeDir,'competition-stats.json')));}catch{return res.end(JSON.stringify({mode:'official_quant_execution',status:'not_enabled',ordersSubmitted:0}));}}
    if(req.url==='/api/realtime'){res.setHeader('Content-Type','application/json');return res.end(JSON.stringify(realtime?realtime.status():{mode:'market_capture_only',status:'not_enabled'}));}
    if(req.url==='/api/passive'){res.setHeader('Content-Type','application/json');try{const r=JSON.parse(fs.readFileSync(path.join(runtimeDir,'passive-stats.json')));return res.end(JSON.stringify({...r,workerHealth:workerHealth({reportedAt:r.reportedAt,status:r.runtime?.status})}));}catch{res.writeHead(503);return res.end(JSON.stringify({mode:'passive_opportunity_measurement',status:'unavailable',simulatedFills:null,pnl:null}));}}
    if(req.url==='/api/basket'){res.setHeader('Content-Type','application/json');try{const r=JSON.parse(fs.readFileSync(path.join(runtimeDir,'basket-stats.json')));return res.end(JSON.stringify({...r,workerHealth:workerHealth(r)}));}catch{res.writeHead(503);return res.end(JSON.stringify({mode:'dual_no_basket_measurement',status:'unavailable',simulatedFills:null,pnl:null}));}}
    if(req.url==='/api/ml'){res.setHeader('Content-Type','application/json');try{const r=JSON.parse(fs.readFileSync(path.join(runtimeDir,'ml-stats.json'),'utf8'));return res.end(JSON.stringify({...r,workerHealth:workerHealth(r)}));}catch{res.writeHead(503);return res.end(JSON.stringify({status:'unavailable'}));}}
    if(req.url==='/api/dip'){res.setHeader('Content-Type','application/json');try{const r=JSON.parse(fs.readFileSync(path.join(runtimeDir,'dip-stats.json'),'utf8'));return res.end(JSON.stringify({...r,workerHealth:workerHealth(r),collectorStatus:runtime.status,collectionPlan:runtime.collectionPlan}));}catch{res.writeHead(503);return res.end(JSON.stringify({status:'unavailable'}));}}
    if(req.url==='/api/dip-scale'||req.url==='/api/dip-responsive'||req.url==='/api/shock'){res.setHeader('Content-Type','application/json');try{const r=JSON.parse(fs.readFileSync(path.join(runtimeDir,req.url==='/api/shock'?'shock-stats.json':req.url==='/api/dip-responsive'?'dip-responsive-stats.json':'dip-scale-stats.json'),'utf8'));return res.end(JSON.stringify({...r,workerHealth:workerHealth(r),collectorStatus:runtime.status,collectionPlan:runtime.collectionPlan}));}catch{res.writeHead(503);return res.end(JSON.stringify({status:'unavailable'}));}}
    if(req.url==='/api/breadth'){res.setHeader('Content-Type','application/json');return res.end(JSON.stringify({...(coverage||{universeSize:universe.exchanges.length,quotes:{}}),collectionAgeMs:coverage?Date.now()-Date.parse(coverage.updatedAt):null}));}
    if(req.url==='/'){res.setHeader('Content-Type','text/html; charset=utf-8');return res.end(html);}res.writeHead(404);res.end();
  });
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(c.port,'127.0.0.1',resolve);});
  heartbeat=setInterval(()=>{try{persistStatus();}catch(e){runtime.status='failed';stop('status_publication_failed',e.code);}},3000);
  await connectTournament(api,c,{stopped:()=>stopping,now:Date.now,pause:pauseUntilNextRead,onRetry:async({attempts,delay,reason})=>{
    const at=new Date().toISOString();invalidate(state,null,at,'startup_connection_unavailable');
    publishBookEvent('invalidate',{marketId:null,reason:'ACCOUNT_OUTAGE'});
    if(journal.last)commit({type:'account_read_outage',clockAt:at,reason,phase:'startup'});
    api.next=Math.max(api.next,Date.now()+delay);api.onAdmission(Math.max(api.next,api.cooldown));
    runtime.status='connecting_retry';runtime.startupRetry={attempts,delay,reason,at};persistStatus();
    if(dipBank&&!stopping)await collectHistory();
  }});
  if(!journal.last&&!stopping)commit({type:'started',config:c});
  if(!stopping&&fs.existsSync(path.join(runtimeDir,'entry-comparison-active.json')))commit({type:'session_start',clockAt:new Date().toISOString(),sessionId:samplingSessionId,scope:'collector'});
  if(!stopping&&fs.existsSync(path.join(runtimeDir,'comparison-sampling.json')))commit({type:'comparison_sampling_session',clockAt:new Date().toISOString(),sessionId:samplingSessionId,mode:'paired_comparison_two_attempts_v1',authorization:'read_allocation_only'});
  runtime.status=stopping?'stopping':'running';persistStatus();
  if(!stopping&&fs.existsSync(path.join(runtimeDir,'basket-bank.json'))){
    const capture=await basketRelationships.capture();
    atomicJson(path.join(runtimeDir,'basket-relationships.json'),capture);
    commit({type:'basket_relationship_capture',clockAt:capture.finishedAt,capture});
    runtime.basketRelationships={at:capture.finishedAt,complete:capture.complete,reason:capture.reasonCode,relationships:capture.relationships.length,pages:capture.snapshots.length,evidenceHash:capture.evidenceHash};
    if(!stopping&&api.cooldown<=Date.now()){
      try{
        const evidence=await contractEvidence.capture();
        atomicJson(path.join(runtimeDir,'contract-pair-evidence.json'),evidence);
        commit({type:'contract_pair_evidence',clockAt:evidence.finishedAt,evidence});
        runtime.contractPairEvidence={at:evidence.finishedAt,validUntil:evidence.validUntil,evidenceHash:evidence.evidenceHash,orderEligible:false,status:'captured'};
      }catch(e){
        if(e.durableRecordingFailed||api.manualReview||e.status===401||e.status===403)throw e;
        runtime.contractPairEvidence={at:new Date().toISOString(),status:stopping?'stopped':'unavailable',orderEligible:false,reason:e.status?'http_'+e.status:/^CONTRACT_[A-Z_]+$/.test(e.message)?e.message:'capture_unavailable'};
      }
    }
  }
  const cyclesArg=process.argv.indexOf('--cycles');const maxCycles=cyclesArg===-1?Infinity:Number(process.argv[cyclesArg+1]);
  if(!Number.isInteger(maxCycles)&&maxCycles!==Infinity||maxCycles<1)throw Error('Invalid cycle bound');
  while(!stopping&&runtime.cycles<maxCycles){
    const cycleStart=Date.now(),checkpoint=structuredClone(state);const events=[];let successful=0,cycleCommitted=false;
    try{
      if(dipBank){
        const makerStatus=noMaker?.status(),makerRisk=makerStatus?.riskMarketIds||[],makerActive=!!noMaker&&!makerStatus.failed&&(makerRisk.length>0||Date.now()<Date.parse(noMaker.bank.reviewExpiresAt));
        const auxiliary=baalReadAttention.reserve(officialSync?1:2,{makerActive});runtime.auxiliaryReadAllocation=auxiliary;
        if(productionMetadata?.pending()&&(auxiliary.historyReads||auxiliary.tapeReads)&&!stopping&&api.cooldown<=Date.now()){
          await productionMetadata.consumeOne();
          if(auxiliary.historyReads)auxiliary.historyReads--;else auxiliary.tapeReads--;
          runtime.productionMetadata={status:'requested_contract_read_completed',executionAuthority:false,additionalReadSlots:0};
        }
        if(auxiliary.tapeReads&&tradeTape&&!stopping&&api.cooldown<=Date.now()){
          try{runtime.tradeTape=await tradeTape.collect(spec=>api.dispatch(spec),['363','297','381','382'],{priorityMarketIds:makerActive?(makerRisk.length?makerRisk:noMaker.bank.legs.map(l=>l.marketId)):[],recoveryWindows:makerStatus?.projectionRecoveryRequests||[]});}
          catch(e){if(e.durableRecordingFailed||api.manualReview||e.status===401||e.status===403)throw e;if(!stopping)runtime.tradeTape={...tradeTape.status(),status:'degraded',lastErrorAt:new Date().toISOString(),reason:e.status?'http_'+e.status:/^TRADE_TAPE_[A-Z_]+$/.test(e.message)?e.message:'trade_tape_unavailable'};}
        }
        if(auxiliary.historyReads)await collectHistory(auxiliary.historyReads);
      }
      if(officialSync&&!stopping&&api.cooldown<=Date.now())await collectAccountExtra();
      const positions=await api.request('/tournaments/'+c.tournamentSlug+'/portfolio/positions');
      if(!Array.isArray(positions.data.positions))throw Error('Invalid real holdings response');
      if(officialSync){try{accountCache.positions=accountPositions(positions);delete accountCache.errors.positions;publishAccount();}catch{accountCache.errors.positions={at:new Date().toISOString(),reason:'unavailable_or_invalid'};publishAccount();}}
      const orders=await api.orders();
      const excluded=exclusionSet(positions.data.positions,orders);
      const eligibilityOrdersAt=new Date().toISOString();
      try{runtime.accountEligibility=eligibilityPublication?.publish({positions,orders,ordersAt:eligibilityOrdersAt});}catch{const e=Error('Account eligibility recording failed');e.durableRecordingFailed=true;e.fatal=true;throw e;}
      if(officialSync){try{accountCache.orders=accountOrders(orders,new Date().toISOString());delete accountCache.errors.orders;publishAccount();}catch{accountCache.errors.orders={at:new Date().toISOString(),reason:'unavailable_or_invalid'};publishAccount();}}
      // Freeze one plan before metadata selection; do not resample shadow state
      // or reorder candidates between the controller and these book requests.
      const probabilityFocus=mergeProbabilityReadFocus([readProbabilityReadFocus(runtimeDir,universe,Date.now()),readProbabilityReadFocus(runtimeDir,universe,Date.now(),'calibrated-poll-paper-active.json')]),bulkFocus=readBulkReadFocus(runtimeDir,universe,Date.now()),anchorFocus=readAnchorReadFocus(runtimeDir,universe,Date.now()),diagnosticFocus=collectDiagnosticFocus(),paperFocus=mergePaperReadFocus(anchorFocus,bulkFocus,probabilityFocus,diagnosticFocus);
      const inputs=schedulingInputs();
      if(nativeDip){for(const x of paperFocus.markets||[])if(['held','exit_pending','entry_pending'].includes(x.phase)){const key=x.phase==='entry_pending'?'pendingMarketIds':'heldMarketIds';inputs[key]=[...new Set([...inputs[key],x.marketId])];}
       const members=new Map(universe.exchanges.map(x=>[x.exchangeId+':YES',x.marketId])),legacy=state.strategies.mean_reversion;
       for(const [rows,key] of [[legacy.positions,'heldMarketIds'],[legacy.pending,'pendingMarketIds']])for(const id of Object.keys(rows)){const marketId=members.get(id);if(!marketId)throw Error('Unmapped original active paper risk');inputs[key]=[...new Set([...inputs[key],marketId])];}}
      const productionCandidates=competition?.eligibleMarketIds()||[];
      const basePlan=nativeDip?planNativeDiscoveryBooks(scheduler,schedulerScope,inputs,{screenedMarketIds:[...new Set([...productionCandidates,...(runtime.marketHistoryScreen?.candidateMarketIds||[])])],excludedMarketIds:universe.exchanges.filter(x=>excluded.has(x.exchangeId)).map(x=>x.marketId),ownedRiskMarketIds:[...new Set([...(competition?.managedMarketIds()||[]),...(probabilityFocus?.ownedRiskReads===true?probabilityFocus.markets.filter(x=>x.phase==='held').map(x=>x.marketId):[])])]}):dipBank?planBookReads(scheduler,schedulerScope,inputs):{reads:[...c.marketIds.map(marketId=>({marketId,path:'sourceSnapshots'})),...depthPlan.marketIds.map(marketId=>({marketId,path:'supplementalDepth'}))],entryMarketIds:[]};
      const pairedPlan=focusPairedPackageReads(comparisonSamplingPlan(basketMeasurementPlan(basePlan,excluded)),readPairedPackageFocus(runtimeDir,universe,Date.now()),{now:Date.now(),lastAttempts:scheduler?.packageReadLastAttemptAt||{}});
      const observationFocus=readEdgeObservationFocus(runtimeDir,universe,Date.now()),observationExclusions=universe.exchanges.filter(x=>excluded.has(x.exchangeId)).map(x=>x.marketId);
      const observation=planEdgeObservation(pairedPlan,scheduler,universe,{focus:observationFocus,now:Date.now(),excludedMarketIds:observationExclusions,coverage});
      if(observation.state!==scheduler)saveScheduler(observation.state);
      const trialPlan=prioritizeEdgeTrials(observation.plan,observationFocus,universe,{now:Date.now(),excludedMarketIds:observationExclusions});
      runtime.probabilityReadFocus=probabilityFocus;
      let planned=prioritizeAnchorReads(trialPlan,paperFocus,{excludedMarketIds:observationExclusions,ownedRiskMarketIds:probabilityFocus?.ownedRiskReads===true?probabilityFocus.markets.filter(x=>x.phase==='held').map(x=>x.marketId):[],lastAttempts:scheduler?.bookLastAttempt||{}});runtime.bulkDipReadFocus=bulkFocus;runtime.bidAnchorReadFocus=planned.bidAnchorReadFocus||null;runtime.edgeObservation=planned.edgeObservation||null;runtime.edgeTrialObservation=planned.edgeTrialObservation||null;
      if(canonicalCapacity)try{planned=canonicalCapacity.plan(planned,{cycle:runtime.cycles,now:Date.now(),excludedExchangeIds:[...excluded]});}catch{canonicalCapacity=null;runtime.canonicalCapacity={status:'failed',executionAuthority:false};}
      const pairReview=noMaker?.bank.review||baalPair?.bank.review;
      if(pairReview)try{planned=planMakerPairReads(planned,pairReview,{now:Date.now(),excludedExchangeIds:[...excluded]});}catch{runtime.makerPairFocus={reason:'maker_pair_invalid',executionAuthority:false};}
      runtime.makerPairFocus=planned.makerPairFocus||runtime.makerPairFocus||null;
      runtime.canonicalCapacityFocus=planned.canonicalCapacityFocus||null;
      runtime.paperPackageFocus=planned.paperPackageFocus||null;
      if(competition)planned=prioritizeProductionReads(planned,{candidateMarketIds:productionCandidates,ownedMarketIds:competition.managedMarketIds()});
      if(competition)try{await competition.beforeBooks({positions,orders,ordersAt:new Date().toISOString(),candidateMarketIds:planned.reads.map(x=>x.marketId).filter(id=>productionCandidates.includes(id))});}catch(e){competition.failure(e);}
      const supplementalDepth=[],samplingReservations=[],samplingReceipts=[];
      for(const spec of planned.reads){
        if(stopping||api.cooldown>Date.now())break;
        const marketId=spec.marketId,target=spec.path==='sourceSnapshots'?events:supplementalDepth;let receivedBook=false;
        const samplingMember=planned.comparisonSampling?.reservationMarketIds?.includes(marketId);
        recordAttempt('book',marketId,samplingMember?planned.comparisonSampling.planId:null,spec.nativeDiscovery===true);
        if(samplingMember)samplingReservations.push({marketId,at:new Date().toISOString()});
        try{
          if(competition)try{await competition.beforeBook?.(marketId);}catch(e){competition.failure(e);}
          const r=await api.request('/markets/'+marketId+'/orderbook?tournamentId='+c.tournamentId+'&depth=20');
          target.push(r);
          if(planned.comparisonSampling?.planId)samplingReceipts.push({marketId,requestStartedAt:r.requestStartedAt,receivedAt:r.receivedAt});
          receivedBook=true;
          // Official execution has its own immutable native receipt/intent
          // barrier. Serve it before file-only paper reducers age the quote.
          if(competition)try{await competition.onBook(r);}catch(e){competition.failure(e);}
          publishBookEvent('publishBook',{marketId,response:r});
          if(noMaker)try{noMaker.onBook(bookPublication.status(),marketId,probabilityAccountEligibility(accountCache));runtime.noMaker=noMaker.status();}catch{runtime.noMaker={status:'failed',executionAuthority:false};noMaker=null;}
          if(baalPair)try{baalPair.onBook(bookPublication.status(),marketId,probabilityAccountEligibility(accountCache));runtime.baalPair=baalPair.status();}catch{runtime.baalPair={status:'failed',executionAuthority:false};baalPair=null;}
          if(canonicalCapacity)try{canonicalCapacity.inspect(bookPublication.status());runtime.canonicalCapacity=canonicalCapacity.status();}catch{canonicalCapacity=null;runtime.canonicalCapacity={status:'failed',executionAuthority:false};}
          const b=normalizeBook(r.data,marketId,c.tournamentId,r.receivedAt,r.requestStartedAt,c.maxDataAgeMs,excluded,spec.path==='sourceSnapshots'?'event_age_v1':dipBank?'authoritative_receipt_v1':'event_age_v1');
          if(b.exchangeId!==universe.exchanges.find(x=>x.marketId===marketId).exchangeId)throw Error('Book exchange identity mismatch');
          if(spec.path==='sourceSnapshots'){try{state=observe(state,b,c);}catch(e){e.fatal=true;throw e;}}
          successful++;
        }catch(e){if(e.durableRecordingFailed)throw e;if(stopping)break;if(!receivedBook)publishBookEvent('invalidate',{marketId,reason:'BOOK_READ_FAILED'});if(e.fatal||api.manualReview||e.status===401||e.status===403)throw e;const reason=e.status?'http_'+e.status:e.message,at=new Date().toISOString();if(spec.path==='sourceSnapshots'){state.skips[marketId]={at,reason};invalidate(state,marketId,at,reason);}target.push({marketId,skipped:reason,at});if(e.status===429||e.status===503)break;}
      }
      runtime.scheduling={managedMarketIds:planned.managedMarketIds,entryMarketIds:planned.entryMarketIds,overCapacity:planned.overCapacity,reads:planned.reads,nativeDiscovery:planned.nativeDiscovery||null};
      runtime.supplementalDepth={marketIds:planned.reads.filter(x=>x.path==='supplementalDepth').map(x=>x.marketId),observations:supplementalDepth.filter(x=>x.endpoint).length,skips:supplementalDepth.filter(x=>x.skipped).map(x=>({marketId:x.marketId,reason:x.skipped}))};
      let bulkPrices=null,bulkPriceError=null;
      if(!stopping&&api.cooldown<=Date.now()){
        try{
          const sweep=await collectMarketSweep({api,universe,coverage,enabled:!!marketSweep,managedMarketIds:competition?.managedMarketIds()||[],isStopped:()=>stopping,now:Date.now,onSnapshot:snapshot=>{pricePublication?.publish(snapshot);collectorMarketScreen?.onPublished(snapshot);},onCoverage:next=>checkpointCoverage(coverageFile,next,{canCommit:()=>!stopping,onFault:fault=>{runtime.coveragePersistenceFault={at:new Date().toISOString(),...fault};}})});
          coverage=sweep.coverage;bulkPrices=sweep.primarySnapshot;bulkPriceError=sweep.error;
          runtime.marketSweep={enabled:!!marketSweep,requestedBatches:sweep.requestedBatches,completedBatches:sweep.completedBatches,complete:sweep.complete,riskPriority:sweep.riskPriority,publication:pricePublication?.status()||null};
          runtime.breadth={universeSize:universe.exchanges.length,observedExchanges:Object.keys(coverage?.quotes||{}).length,lastCollectionAt:coverage?.updatedAt||null,error:bulkPriceError};
        }catch(e){if(e.durableRecordingFailed||!stopping&&(api.manualReview||e.status===401||e.status===403))throw e;bulkPriceError=stopping?'stopped':e.status?'http_'+e.status:e.message;runtime.breadth={...runtime.breadth,error:bulkPriceError};}
      }
      const at=new Date().toISOString();sampleEquity(state,c,at);assertState(state,c);commit({type:'cycle',clockAt:at,sourceSnapshots:events,supplementalDepth,nativeDiscovery:planned.nativeDiscovery?{...planned.nativeDiscovery,source:runtime.marketHistoryScreen?.source||null}:null,basketMeasurement:planned.basketMeasurement||null,edgeObservation:planned.edgeObservation||null,edgeTrialObservation:planned.edgeTrialObservation||null,comparisonSampling:planned.comparisonSampling?{...planned.comparisonSampling,reservations:samplingReservations,receipts:samplingReceipts}:null,entryMarketIds:[...new Set([...planned.entryMarketIds,...(planned.managedMarketIds||[])])],bulkPrices,bulkPriceError,accountExclusions:[...excluded],exclusionObservedAt:positions.receivedAt});cycleCommitted=true;
      if(realtime&&!stopping&&api.cooldown<=Date.now())await realtime.service();
      nativeDip?.kick();if(nativeDip)runtime.nativeDip=nativeDip.status();
      nativeQuote?.kick();if(nativeQuote)runtime.nativeQuote=nativeQuote.status();
      runtime.cycles++;runtime.lastCycleAt=at;runtime.status=stopping?'stopping':successful?'running':'degraded';persistStatus();
      console.log(JSON.stringify({at,event:'paper_cycle',cycle:runtime.cycles,observations:state.observations,fills:Object.fromEntries(Object.entries(state.strategies).map(([k,s])=>[k,s.fills.length])),competitionOrders:competition?Object.values(competition.state.intents).filter(i=>Number.isSafeInteger(i.orderId)&&i.orderId>0).length:0}));
    }catch(e){if(e.durableRecordingFailed)throw e;if(stopping){if(!cycleCommitted)state=checkpoint;break;}const at=new Date().toISOString();invalidate(state,null,at,'account_reconciliation_unavailable');publishBookEvent('invalidate',{marketId:null,reason:'ACCOUNT_OUTAGE'});runtime.errors.push({at,reason:e.status?'http_'+e.status:e.message});runtime.errors=runtime.errors.slice(-20);runtime.status=e.recoverable?'degraded':'failed';persistStatus();if(e.recoverable){commit({type:'account_read_outage',clockAt:at,reason:e.status?'http_'+e.status:'connection_unavailable'});await pauseUntilNextRead(Math.max(10000,api.cooldown-Date.now()));continue;}throw Error('Paper runner stopped: '+runtime.errors.at(-1).reason);}
    if(!stopping&&runtime.cycles<maxCycles){
      runtime.cycleWait=collectionCycleWait({cycleStartedAt:cycleStart,now:Date.now(),cycleIntervalMs:marketSweep?.cycleIntervalMs??c.cycleIntervalMs,managedMarketIds:competition?.managedMarketIds()||[]});
      if(runtime.cycleWait.delayMs>0)await pauseUntilNextRead(runtime.cycleWait.delayMs);
    }
  }
  runtime.status=runtime.stopProvenance?.cause==='status_publication_failed'?'failed':'stopped';
  if(runtime.status==='failed')process.exitCode=1;
  commit({type:'stopped',stopProvenance:runtime.stopProvenance||null});persistStatus();
}catch(e){const stopped=stopping&&!e.durableRecordingFailed&&runtime.stopProvenance?.cause!=='status_publication_failed';if(state){runtime.status=stopped?'stopped':'failed';runtime.errors.push({at:new Date().toISOString(),reason:stopped?'stop_requested':e.message});try{persistStatus();}catch{}}console.error(stopped?'Paper runner stopped':'Paper runner failed; see local stats');process.exitCode=stopped?0:1;
}finally{clearInterval(stopWatch);clearInterval(heartbeat);try{publishBookEvent('close',runtime.status==='failed'?'COLLECTOR_FAILURE':'STOP');}finally{try{collectorMarketScreen?.close();await nativeDip?.close();await nativeQuote?.close();await realtime?.stop();}finally{comparisonRiskSource?.close();basketFollowupSource?.close();realtimeCapture?.close();competition?.close();api?.close();server?.close();cleanup();}}}
