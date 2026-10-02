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
import {createBookPublication} from './book-publication.mjs';
import {createBasketFollowupSource} from './basket-followup-source.mjs';
import {createComparisonRiskSource} from './comparison-risk-source.mjs';
import {planComparisonSampling,recordComparisonAttempt} from './comparison-sampling.mjs';
import {atomicJson as durableAtomicJson} from '../execution/ledger.mjs';
import {CompetitionController} from '../execution/controller.mjs';
import {loadNetworkBudget,networkInterval} from './transport-network-budget.mjs';
import {collectionCycleWait} from './cycle-wait.mjs';
import {loadMarketSweep,collectMarketSweep} from './market-sweep.mjs';
import {createMarketPricePublication} from './market-price-publication.mjs';
import {createClient} from '@supabase/supabase-js';
import {Capture} from '../feed/capture.mjs';
import {createRealtimeHost} from './realtime-host.mjs';

const root=path.dirname(fileURLToPath(import.meta.url)),workspace=path.dirname(root);
if(process.argv.includes('--live'))throw Error('Competition order execution is disabled');
const configFile=process.argv.includes('--config')?path.resolve(process.argv[process.argv.indexOf('--config')+1]):path.join(root,'config.json');
const c=validateConfig(JSON.parse(fs.readFileSync(configFile,'utf8')));
const runtimeDir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA||workspace,'SigTradingCup','paper-runtime');
const networkBudget=loadNetworkBudget(path.join(runtimeDir,'coordinated-network-budget.json')),networkIntervalMs=networkInterval(c.readIntervalMs,networkBudget);
const stopFile=path.join(workspace,'STOP'),paperStop=path.join(runtimeDir,'STOP'),collectorStop=path.join(runtimeDir,'COLLECTOR_STOP');
if(fs.existsSync(stopFile)||fs.existsSync(paperStop)||fs.existsSync(collectorStop)){console.log(JSON.stringify({status:'stopped',reason:'STOP_present'}));process.exit(0);}
fs.mkdirSync(runtimeDir,{recursive:true,mode:0o700});
const release=lock(path.join(process.env.LOCALAPPDATA||workspace,'SigTradingCup','bot.lock'));
let cleaned=false;function cleanup(){if(!cleaned){cleaned=true;release();}}
process.on('exit',cleanup);
const control=new AbortController();let stopping=false;
const stop=()=>{stopping=true;control.abort();};process.on('SIGINT',stop);process.on('SIGTERM',stop);
const stopWatch=setInterval(()=>{if(fs.existsSync(stopFile)||fs.existsSync(paperStop)||fs.existsSync(collectorStop))stop();},200);
let journal,state,api,server,heartbeat,competition,realtime,realtimeCapture,basketRelationships,bookPublication,basketFollowupSource,basketFollowupBankHash,comparisonRiskSource;
const samplingSessionId=randomUUID();
let universe,coverage,depthPlan,dipBank,scaleBank,responsiveBank,scheduler,schedulerScope;const recentReadTimings=[];
const schedulerFile=path.join(runtimeDir,'book-scheduler.json');
function saveScheduler(next){
 try{durableAtomicJson(schedulerFile,next);scheduler=next;}
 catch{const e=Error('Book scheduler recording failed');e.durableRecordingFailed=true;e.fatal=true;throw e;}
}
function recordAttempt(kind,marketId,samplingPlanId){
 const at=new Date().toISOString();let next=recordSchedulerAttempts(scheduler,schedulerScope,{[kind+'MarketIds']:[marketId],attemptedAt:at});
 if(kind==='book'&&samplingPlanId)next=recordComparisonAttempt(next,schedulerScope,{marketId,at,planId:samplingPlanId});
 saveScheduler(next);
}
function schedulingInputs(){
 const sources=[];
 for(const [name,bank] of [['dip',dipBank],['dip-scale',scaleBank],['dip-responsive',responsiveBank]])if(bank){
  const state=JSON.parse(fs.readFileSync(path.join(runtimeDir,name+'-state.json')));let healthy=false;
  try{const stats=JSON.parse(fs.readFileSync(path.join(runtimeDir,name+'-stats.json')));healthy=stats.bankHash===digest(bank)&&workerHealth(stats).status==='running';}catch{}
  sources.push({state,bank,healthy});
 }
 const inputs=collectionInputs(sources,universe,coverage,Date.now());
  const shockBankFile=path.join(runtimeDir,'shock-bank.json');
  if(fs.existsSync(shockBankFile)){
   const risk=shockCollectionRisk(JSON.parse(fs.readFileSync(path.join(runtimeDir,'shock-state.json'))),JSON.parse(fs.readFileSync(shockBankFile)),universe);
   for(const key of ['heldMarketIds','pendingMarketIds'])inputs[key]=[...new Set([...inputs[key],...risk[key]])];
  }
  const comparisonRisk=comparisonRiskSource?.poll();
  if(comparisonRisk){
   runtime.comparisonRisk=comparisonRisk;
   for(const key of ['heldMarketIds','pendingMarketIds'])inputs[key]=[...new Set([...inputs[key],...comparisonRisk[key]])];
  }
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
function publishBookEvent(method,value){try{return bookPublication?.[method](value);}catch{const e=Error('Public book recording failed');e.durableRecordingFailed=true;e.fatal=true;throw e;}}
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
  journal=new Journal(path.join(runtimeDir,'journal.jsonl'));state=journal.last?restoreState(journal.last.state,c):initialState(c,randomUUID(),new Date().toISOString());assertState(state,c);
  const syncFile=path.join(runtimeDir,'official-sync.json');officialSync=fs.existsSync(syncFile)&&JSON.parse(fs.readFileSync(syncFile)).enabled===true;
  if(officialSync){const file=path.join(runtimeDir,'official-account.json');if(fs.existsSync(file))accountCache=JSON.parse(fs.readFileSync(file));accountCache.errors??={};}
  universe=validateUniverse(JSON.parse(fs.readFileSync(path.join(root,'price-universe.json'),'utf8')),c.tournamentId);
  const marketSweep=loadMarketSweep(path.join(runtimeDir,'market-sweep.json'),universe,networkBudget);
  const pricePublication=marketSweep?createMarketPricePublication({file:path.join(runtimeDir,'market-prices.jsonl'),universe,canOperate:()=>!stopping}):null;
  comparisonRiskSource=createComparisonRiskSource({runtimeDir,universe,now:Date.now});
  bookPublication=createBookPublication({file:path.join(runtimeDir,'public-books.jsonl'),universe,sessionId:randomUUID(),now:Date.now,canOperate:()=>!stopping});
  depthPlan=validateDepthPlan(JSON.parse(fs.readFileSync(path.join(root,'depth-plan.json'),'utf8')),c,universe);
  const dipFile=path.join(runtimeDir,'dip-bank.json');if(fs.existsSync(dipFile)){dipBank=JSON.parse(fs.readFileSync(dipFile));validateDipPolicy(dipBank.policy);if(dipBank.universe.hash!==universe.hash||dipBank.config.feeBps!==0||dipBank.policy.historyReadsPerCycle!==2||dipBank.policy.comparisonBooksPerCycle!==2||dipBank.policy.dynamicBooksPerCycle!==2)throw Error('Invalid dip collection plan');runtime.collectionPlan={mode:'diversified_dip',historyReads:officialSync?1:2,fullBookBudget:4,managedCapacity:3,bulkReads:1,accountReads:'positions plus paginated orders',readIntervalMs:c.readIntervalMs};}
  const coverageFile=path.join(runtimeDir,'price-coverage.json');
  const scaleFile=path.join(runtimeDir,'dip-scale-bank.json');if(fs.existsSync(scaleFile)){scaleBank=JSON.parse(fs.readFileSync(scaleFile));validateConfig(scaleBank.config);validateDipPolicy(scaleBank.policy);if(!dipBank||scaleBank.universe.hash!==universe.hash||scaleBank.policy.profile!=='scale')throw Error('Invalid scaled dip bank');}
  const responsiveFile=path.join(runtimeDir,'dip-responsive-bank.json');if(fs.existsSync(responsiveFile)){responsiveBank=JSON.parse(fs.readFileSync(responsiveFile));validateConfig(responsiveBank.config);validateDipPolicy(responsiveBank.policy);if(responsiveBank.universe.hash!==universe.hash||responsiveBank.policy.executionPolicy!=='responsive_v2')throw Error('Invalid responsive bank');}
  schedulerScope={universe,coreMarketIds:c.marketIds};scheduler=fs.existsSync(schedulerFile)?validateSchedulerState(JSON.parse(fs.readFileSync(schedulerFile)),schedulerScope):createSchedulerState(schedulerScope);
  if(fs.existsSync(coverageFile)){coverage=JSON.parse(fs.readFileSync(coverageFile,'utf8'));if(coverage.universeHash!==universe.hash)coverage=null;}
  if(journal.last&&journal.last.state.engineRevision!==state.engineRevision)commit({type:'paper_engine_upgrade',fromRevision:journal.last.state.engineRevision||1,toRevision:state.engineRevision});
  api=new ReadClient(loadKey(),{interval:c.readIntervalMs,networkBudget,signal:control.signal,authorizeDispatch:spec=>basketRelationships?.authorizes(spec)===true||realtime?.authorizes(spec)===true||competition?.transport.authorizeRequest(spec)===true,onAdmission:next=>atomicJson(path.join(runtimeDir,'rate.json'),{next,manualReview:next===null}),onTiming:event=>{validateTiming(event);fs.appendFileSync(path.join(runtimeDir,'read-timings.jsonl'),JSON.stringify(event)+'\n',{mode:0o600});recentReadTimings.push(event);if(recentReadTimings.length>1000)recentReadTimings.shift();runtime.readTiming=timingSummary(recentReadTimings);}});
  basketRelationships=createBasketRelationshipReader({tournamentId:c.tournamentId,now:Date.now,dispatch:spec=>api.dispatch(spec),canOperate:()=>!stopping&&!api.manualReview&&!fs.existsSync(stopFile)&&!fs.existsSync(paperStop)&&!fs.existsSync(collectorStop)&&!fs.existsSync(path.join(runtimeDir,'BASKET_STOP'))});
  runtime.networkBudget={mode:networkBudget?'coordinated_sole_client':'default',schemaVersion:networkBudget?.schemaVersion??null,intervalMs:networkIntervalMs,writeIntervalMs:api.writeInterval,completionPacing:api.completionPacing};
  if(runtime.collectionPlan)runtime.collectionPlan.readIntervalMs=networkIntervalMs;
  const activationFile=path.join(runtimeDir,'competition-activation.json');
  if(fs.existsSync(activationFile)&&JSON.parse(fs.readFileSync(activationFile)).enabled===true)competition=new CompetitionController({runtimeDir,api,universe,config:c,isStopped:()=>stopping,activation:JSON.parse(fs.readFileSync(activationFile))});
  if(fs.existsSync(path.join(runtimeDir,'rate.json'))){const rate=JSON.parse(fs.readFileSync(path.join(runtimeDir,'rate.json')));if(rate.manualReview||rate.next===null)throw Error('Rate cooldown requires manual review');if(!Number.isFinite(rate.next)||rate.next<0||rate.next>8640000000000000)throw Error('Invalid persisted rate state');api.next=rate.next;}
  const realtimeFile=path.join(runtimeDir,'realtime-capture.json');
  if(fs.existsSync(realtimeFile)){
    const activation=JSON.parse(fs.readFileSync(realtimeFile));
    if(activation.enabled===true){
      if(activation.schemaVersion!==1||activation.mode!=='market_capture_only')throw Error('Invalid realtime capture activation');
      const file='realtime-'+new Date().toISOString().replace(/[:.]/g,'-')+'-'+randomUUID()+'.jsonl';
      realtimeCapture=new Capture(path.join(runtimeDir,file));
      const scope={tournamentId:c.tournamentId,markets:c.marketIds.map(marketId=>({marketId,exchangeId:universe.exchanges.find(m=>m.marketId===marketId).exchangeId}))};
      realtime=createRealtimeHost({dispatch:spec=>api.dispatch(spec),createClient,scope,capture:realtimeCapture,signal:control.signal,
        canOperate:()=>!stopping&&!api.manualReview&&!fs.existsSync(stopFile)&&!fs.existsSync(paperStop)&&!fs.existsSync(collectorStop)&&!fs.existsSync(path.join(runtimeDir,'REALTIME_STOP'))});
      atomicJson(path.join(runtimeDir,'realtime-session.json'),{schemaVersion:1,file,startedAt:new Date().toISOString(),scope});
      runtime.realtime={mode:'market_capture_only',recoveryBudgetPerCycle:1};
    }
  }
  if(networkBudget)commit({type:'collection_network_budget',clockAt:new Date().toISOString(),cadencePolicy:{mode:'coordinated_sole_client',schemaVersion:networkBudget.schemaVersion,intervalMs:api.interval,writeIntervalMs:api.writeInterval,completionPacing:true}});
  runtime.status='connecting';persistStatus();
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
  heartbeat=setInterval(()=>{try{persistStatus();}catch{runtime.status='failed';stop();}},3000);
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
  }
  const cyclesArg=process.argv.indexOf('--cycles');const maxCycles=cyclesArg===-1?Infinity:Number(process.argv[cyclesArg+1]);
  if(!Number.isInteger(maxCycles)&&maxCycles!==Infinity||maxCycles<1)throw Error('Invalid cycle bound');
  while(!stopping&&runtime.cycles<maxCycles){
    const cycleStart=Date.now(),checkpoint=structuredClone(state);const events=[];let successful=0,cycleCommitted=false;
    try{
      if(dipBank)await collectHistory(officialSync?1:2);
      if(officialSync&&!stopping&&api.cooldown<=Date.now())await collectAccountExtra();
      const positions=await api.request('/tournaments/'+c.tournamentSlug+'/portfolio/positions');
      if(!Array.isArray(positions.data.positions))throw Error('Invalid real holdings response');
      if(officialSync){try{accountCache.positions=accountPositions(positions);delete accountCache.errors.positions;publishAccount();}catch{accountCache.errors.positions={at:new Date().toISOString(),reason:'unavailable_or_invalid'};publishAccount();}}
      const orders=await api.orders();
      const excluded=exclusionSet(positions.data.positions,orders);
      if(officialSync){try{accountCache.orders=accountOrders(orders,new Date().toISOString());delete accountCache.errors.orders;publishAccount();}catch{accountCache.errors.orders={at:new Date().toISOString(),reason:'unavailable_or_invalid'};publishAccount();}}
      // Freeze one plan before metadata selection; do not resample shadow state
      // or reorder candidates between the controller and these book requests.
      const basePlan=dipBank?planBookReads(scheduler,schedulerScope,schedulingInputs()):{reads:[...c.marketIds.map(marketId=>({marketId,path:'sourceSnapshots'})),...depthPlan.marketIds.map(marketId=>({marketId,path:'supplementalDepth'}))],entryMarketIds:[]};
      const planned=comparisonSamplingPlan(basketMeasurementPlan(basePlan,excluded));
      if(competition)try{await competition.beforeBooks({positions,orders,ordersAt:new Date().toISOString(),candidateMarketIds:planned.entryMarketIds});}catch(e){competition.failure(e);}
      const supplementalDepth=[],samplingReservations=[],samplingReceipts=[];
      for(const spec of planned.reads){
        if(stopping||api.cooldown>Date.now())break;
        const marketId=spec.marketId,target=spec.path==='sourceSnapshots'?events:supplementalDepth;let receivedBook=false;
        const samplingMember=planned.comparisonSampling?.reservationMarketIds?.includes(marketId);
        recordAttempt('book',marketId,samplingMember?planned.comparisonSampling.planId:null);
        if(samplingMember)samplingReservations.push({marketId,at:new Date().toISOString()});
        try{
          const r=await api.request('/markets/'+marketId+'/orderbook?tournamentId='+c.tournamentId+'&depth=20');
          target.push(r);
          if(planned.comparisonSampling?.planId)samplingReceipts.push({marketId,requestStartedAt:r.requestStartedAt,receivedAt:r.receivedAt});
          receivedBook=true;publishBookEvent('publishBook',{marketId,response:r});
          if(competition)try{await competition.onBook(r);}catch(e){competition.failure(e);}
          const b=normalizeBook(r.data,marketId,c.tournamentId,r.receivedAt,r.requestStartedAt,c.maxDataAgeMs,excluded,spec.path==='sourceSnapshots'?'event_age_v1':dipBank?'authoritative_receipt_v1':'event_age_v1');
          if(b.exchangeId!==universe.exchanges.find(x=>x.marketId===marketId).exchangeId)throw Error('Book exchange identity mismatch');
          if(spec.path==='sourceSnapshots'){try{state=observe(state,b,c);}catch(e){e.fatal=true;throw e;}}
          successful++;
        }catch(e){if(e.durableRecordingFailed)throw e;if(stopping)break;if(!receivedBook)publishBookEvent('invalidate',{marketId,reason:'BOOK_READ_FAILED'});if(e.fatal||api.manualReview||e.status===401||e.status===403)throw e;const reason=e.status?'http_'+e.status:e.message,at=new Date().toISOString();if(spec.path==='sourceSnapshots'){state.skips[marketId]={at,reason};invalidate(state,marketId,at,reason);}target.push({marketId,skipped:reason,at});if(e.status===429||e.status===503)break;}
      }
      runtime.scheduling={managedMarketIds:planned.managedMarketIds,entryMarketIds:planned.entryMarketIds,overCapacity:planned.overCapacity,reads:planned.reads};
      runtime.supplementalDepth={marketIds:planned.reads.filter(x=>x.path==='supplementalDepth').map(x=>x.marketId),observations:supplementalDepth.filter(x=>x.endpoint).length,skips:supplementalDepth.filter(x=>x.skipped).map(x=>({marketId:x.marketId,reason:x.skipped}))};
      let bulkPrices=null,bulkPriceError=null;
      if(!stopping&&api.cooldown<=Date.now()){
        try{
          const sweep=await collectMarketSweep({api,universe,coverage,enabled:!!marketSweep,managedMarketIds:competition?.managedMarketIds()||[],isStopped:()=>stopping,now:Date.now,onSnapshot:snapshot=>pricePublication?.publish(snapshot),onCoverage:next=>atomicJson(coverageFile,next)});
          coverage=sweep.coverage;bulkPrices=sweep.primarySnapshot;bulkPriceError=sweep.error;
          runtime.marketSweep={enabled:!!marketSweep,requestedBatches:sweep.requestedBatches,completedBatches:sweep.completedBatches,complete:sweep.complete,riskPriority:sweep.riskPriority,publication:pricePublication?.status()||null};
          runtime.breadth={universeSize:universe.exchanges.length,observedExchanges:Object.keys(coverage?.quotes||{}).length,lastCollectionAt:coverage?.updatedAt||null,error:bulkPriceError};
        }catch(e){if(e.durableRecordingFailed||!stopping&&(api.manualReview||e.status===401||e.status===403))throw e;bulkPriceError=stopping?'stopped':e.status?'http_'+e.status:e.message;runtime.breadth={...runtime.breadth,error:bulkPriceError};}
      }
      const at=new Date().toISOString();sampleEquity(state,c,at);assertState(state,c);commit({type:'cycle',clockAt:at,sourceSnapshots:events,supplementalDepth,basketMeasurement:planned.basketMeasurement||null,comparisonSampling:planned.comparisonSampling?{...planned.comparisonSampling,reservations:samplingReservations,receipts:samplingReceipts}:null,entryMarketIds:[...new Set([...planned.entryMarketIds,...(planned.managedMarketIds||[])])],bulkPrices,bulkPriceError,accountExclusions:[...excluded],exclusionObservedAt:positions.receivedAt});cycleCommitted=true;
      if(realtime&&!stopping&&api.cooldown<=Date.now())await realtime.service();
      runtime.cycles++;runtime.lastCycleAt=at;runtime.status=stopping?'stopping':successful?'running':'degraded';persistStatus();
      console.log(JSON.stringify({at,event:'paper_cycle',cycle:runtime.cycles,observations:state.observations,fills:Object.fromEntries(Object.entries(state.strategies).map(([k,s])=>[k,s.fills.length])),competitionOrders:competition?Object.values(competition.state.intents).filter(i=>Number.isSafeInteger(i.orderId)&&i.orderId>0).length:0}));
    }catch(e){if(e.durableRecordingFailed)throw e;if(stopping){if(!cycleCommitted)state=checkpoint;break;}const at=new Date().toISOString();invalidate(state,null,at,'account_reconciliation_unavailable');publishBookEvent('invalidate',{marketId:null,reason:'ACCOUNT_OUTAGE'});runtime.errors.push({at,reason:e.status?'http_'+e.status:e.message});runtime.errors=runtime.errors.slice(-20);runtime.status=e.recoverable?'degraded':'failed';persistStatus();if(e.recoverable){commit({type:'account_read_outage',clockAt:at,reason:e.status?'http_'+e.status:'connection_unavailable'});await pauseUntilNextRead(Math.max(10000,api.cooldown-Date.now()));continue;}throw Error('Paper runner stopped: '+runtime.errors.at(-1).reason);}
    if(!stopping&&runtime.cycles<maxCycles){
      runtime.cycleWait=collectionCycleWait({cycleStartedAt:cycleStart,now:Date.now(),cycleIntervalMs:marketSweep?.cycleIntervalMs??c.cycleIntervalMs,managedMarketIds:competition?.managedMarketIds()||[]});
      if(runtime.cycleWait.delayMs>0)await pauseUntilNextRead(runtime.cycleWait.delayMs);
    }
  }
  runtime.status='stopped';commit({type:'stopped'});persistStatus();
}catch(e){const stopped=stopping&&!e.durableRecordingFailed;if(state){runtime.status=stopped?'stopped':'failed';runtime.errors.push({at:new Date().toISOString(),reason:stopped?'stop_requested':e.message});try{persistStatus();}catch{}}console.error(stopped?'Paper runner stopped':'Paper runner failed; see local stats');process.exitCode=stopped?0:1;
}finally{clearInterval(stopWatch);clearInterval(heartbeat);try{publishBookEvent('close',stopping||runtime.status==='stopped'?'STOP':'COLLECTOR_FAILURE');}finally{try{await realtime?.stop();}finally{comparisonRiskSource?.close();basketFollowupSource?.close();realtimeCapture?.close();competition?.close();api?.close();server?.close();cleanup();}}}
