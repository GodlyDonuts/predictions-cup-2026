import {tradeTapeFocus} from './trade-tape-focus.mjs';
import {wrapCoupledDrain} from './coupled-drain.mjs';
import {focusStableMakerReads} from './stable-maker-read-focus.mjs';
import {focusCoupledBulkReads} from './coupled-bulk-read-focus.mjs';
import {planMakerPairReads} from './maker-read-plan.mjs';
import {probabilityAccountEligibility as realProbabilityAccountEligibility} from './probability-receipt.mjs';
import {mergeMakerTapeFocus} from './stable-maker-host-v1.mjs';
import {mergePaperReadFocus} from "./bulk-dip-read-focus.mjs";
import {prioritizeProductionReads} from './production-read-priority.mjs';
import {mergeProbabilityReadFocus} from './probability-read-focus.mjs';
import {prioritizeAnchorReads} from "./bid-anchor-read-focus.mjs";
import test from 'node:test';
import {validateProductionActivation} from '../execution/production-activation.mjs';
import {paperExperimentRetired} from './experiment-retirement.mjs';
import {planEdgeObservation} from './edge-observation-plan.mjs';
import {prioritizeEdgeTrials} from './edge-trial-projection.mjs';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import {createNativeBookPublication} from './book-publication-v3.mjs';
import {createBookPublication as createReceiptSeed} from './book-publication-v2.mjs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
import * as scheduler from './book-scheduler.mjs';
import {planNativeDiscoveryBooks,recordNativeDiscoveryAttempt} from './native-discovery-plan-v1.mjs';
import {planComparisonSampling,recordComparisonAttempt} from './comparison-sampling.mjs';
import {collectionInputs,shockCollectionRisk} from './collection-inputs.mjs';
import {basketCandidates} from './basket-inputs.mjs';
import {planBasketBooks} from './basket-book-plan.mjs';
import {basketCodeIdentity} from './basket-watch.mjs';
import {createBasketRelationshipReader} from './basket-relations.mjs';
import {createContractEvidenceReader} from './contract-evidence.mjs';
import {focusPairedPackageReads} from './paired-package-read-plan.mjs';
import {exactMoneyFetch,createMoneyReceiptWriter} from '../execution/exact-money-receipts.mjs';
import {timestamp} from '../feed/contract.mjs';
import {networkInterval,validateNetworkBudget} from './transport-network-budget.mjs';
import {collectionCycleWait} from './cycle-wait.mjs';
import {readProfitSprint,passiveHostsCompletelyFlat,scopedRiskMarketIds,focusProfitSprintReads} from './profit-sprint-read-plan.mjs';
import {collectMarketSweep} from './market-sweep.mjs';
import {connectTournament} from './connection.mjs';
import {validateTiming,timingSummary} from './read-timing.mjs';
import * as model from './model.mjs';
import * as breadth from './breadth.mjs';
import * as depth from './depth-scope.mjs';
import {exclusionSet} from './runtime.mjs';
import {replayJournal} from './replay.mjs';
import {accountPositions,accountOrders,accountFills,accountPnl,accountView} from './account.mjs';
import {historyEndpoint,normalizeHistory} from './history.mjs';import {validateDipPolicy} from './dip-shadow.mjs';import {workerHealth,baselineView} from './worker-health.mjs';

const root=fileURLToPath(new URL('.',import.meta.url)).replace(/[/\\]$/,'');
const config=JSON.parse(fs.readFileSync(path.join(root,'config.json'),'utf8'));
const universe=JSON.parse(fs.readFileSync(path.join(root,'price-universe.json'),'utf8'));
const source=fs.readFileSync(path.join(root,'runner.mjs'),'utf8');

// Only module wiring is replaced. The real runner body executes with an in-memory
// journal/filesystem, synthetic API/key, fake clock/server/process and no network.
async function run(stopAt,{dipEnabled=false,officialSync=false,invalidBasket=false,basketEnabled=false,contractEvidenceEnabled=false,samplingEnabled=false,riskUnavailable=false,comparisonHeld=[],comparisonPending=[],maxCycles=2,requestMs=6000,officialRiskSchedule=null,marketSweepEnabled=false,historyCandidates=[],paperPackageFocus=false,retiredComparison=false,edgeObservation=false,nativeHeld=[],nativeCache=false,probabilityHeld=[],probabilityCompanion=[],probabilityBreadth=[],screenStartupFailure=false,nativeIntent=false,nativePending=[],quoteHeld=[],quotePending=[],retiredNativePrices=false,stablePaper=false,stableBroadPaper=false,stableBroadRecovery=false,coupledPaper=false,coupledBulkPaper=false,realtimeShared=false,settlementInCollector=false,profitSprint=false,coupledRust=false,coupledRustInvalid=false,dualNoInCollector=false,dualNoFailure=false,dualNoV2InCollector=false,dualNoV2Failure=false,retiredFlatLegacy=false,settlementConsumeFailure=false,settlementStatusFailure=false}={}){
  let startupFailures=0;
  let now=Date.parse('2026-10-02T02:00:00Z'),cycle=0,stopped=false,triggered=false;
  const tapeRequests=[],commits=[],memory=new Map(),listeners={},requests=[],publications=[],claimWrites=[],pairReadClaims=[];
  if(nativeIntent)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','native-dip-intent-active.json'),'{}');
  if(nativeCache)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','native-dip-cache-active.json'),'{}');
  const fixtureStart=now,stamp=offset=>new Date(fixtureStart+offset).toISOString();
  if(retiredFlatLegacy)for(const name of ['no-maker','baal-pair'])memory.set(path.join('/synthetic','SigTradingCup','paper-runtime',name+'-active.json'),JSON.stringify({cohort:'expired-flat-'+name}));
  function dualMock(version,{canOperate}){const enabled=version===1?dualNoInCollector:dualNoV2InCollector;if(!enabled)return null;assert.equal(canOperate(),true);const review=JSON.parse(fs.readFileSync(path.join(root,'../data/canonical-capacity-bank-20261003-v1.json'),'utf8')).review;review.recordedAt=stamp(-1000);review.expiresAt=stamp(3600000);let pendingPair=false;const tag=version===1?'dual_no':'dual_no_v2',cost=version===1?'0.450000':'0.440000';return{bank:{review},focus:()=>({bankHash:(version===1?'f':'e').repeat(64),markets:review.legs.map(l=>({marketId:l.marketId,phase:'depth_required'})),ownedRiskReads:true,executionAuthority:false}),status:()=>({cohortVersion:version,executionAuthority:false,competitionOrders:0,pendingPair,positions:[{marketId:'381',quantity:1,cost}]}),onBook(publication,marketId,account){if(!review.legs.some(l=>l.marketId===marketId))return;publications.push({kind:tag+'_book',marketId,at:iso(),accountEligible:account!==null,positionsAt:account?.positionsAt,ordersAt:account?.ordersAt});pendingPair=true;if(version===1?dualNoFailure:dualNoV2Failure)throw Error('Synthetic retained inventory durability failure');},boundary(reason){pendingPair=false;publications.push({kind:tag+'_boundary',reason});},close(){publications.push({kind:tag+'_close',pendingPair});}};}

  if(profitSprint){const dir=path.join('/synthetic','SigTradingCup','paper-runtime');memory.set(path.join(dir,'COLLECTION_PROFIT_SPRINT'),JSON.stringify({schemaVersion:1,owner:'Codex',mode:'source_bound_and_owned_risk',createdAt:stamp(-1),expiresAt:stamp(3599999)}));memory.set(path.join(dir,'COUPLED_BULK_ENTRY_DRAIN'),'{}');}
  if(retiredComparison)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','ENTRY_COMPARISON_STOP'),JSON.stringify({owner:'Codex',at:new Date(now).toISOString(),reason:'Retired obsolete experiment at human request',restartRequires:'Explicitly selected future experiment'}));
  if(retiredNativePrices)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','NATIVE_PRICE_PAPER_STOP'),JSON.stringify({owner:'Codex',at:new Date(now).toISOString(),reason:'Retired obsolete experiment at human request',restartRequires:'Explicitly selected future experiment'}));
  if(stablePaper){memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','stable-maker-active.json'),JSON.stringify({cohort:'fixture'}));memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','stable-maker-cohorts','fixture','bank.json'),'{}');}
  if(stableBroadPaper){memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','stable-maker-broad-active.json'),JSON.stringify({cohort:'broad-fixture'}));memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','stable-maker-cohorts','broad-fixture','bank.json'),'{}');}
  if(coupledPaper){memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','coupled-maker-active.json'),JSON.stringify({cohort:'coupled-fixture'}));memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','coupled-maker-cohorts','coupled-fixture','bank.json'),'{}');}
  if(coupledBulkPaper){memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','coupled-bulk-active.json'),JSON.stringify({cohort:'bulk-fixture'}));memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','coupled-bulk-cohorts','bulk-fixture','bank.json'),'{}');}
  if(realtimeShared)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','realtime-capture.json'),JSON.stringify({enabled:true}));
  let realtimeJoined=false;
  const pricePublications=[];
  const samplingWrites=[],samplingRequests=[],pauses=[],officialCalls=[];
  if(officialRiskSchedule)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','competition-activation.json'),JSON.stringify({schemaVersion:2,enabled:true,ownerId:'Codex',tournamentId:config.tournamentId,runId:'sig-pilot-runner-fixture',strategyId:'settlement-value-native-v1',handoffId:'daenerys-to-baal-fixture',handoffHash:'a'.repeat(64),strategyBankHash:'b'.repeat(64),releaseCommentId:123,ackCommentId:124,feeBps:0,depthFractionBps:100,maxManagedMarkets:3,maxOrderCost:'1.000000',maxAggregateCost:'10.000000',maxCumulativeBuyCost:'25.000000',cashReserve:'0.000000',sizingCapital:'100.000000',startedAt:stamp(-1000),entryExpiresAt:stamp(3600000)}));
  if(officialRiskSchedule)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','coordinated-network-budget.json'),JSON.stringify({schemaVersion:4,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,executionOwner:'Codex',handoffId:'daenerys-to-baal-fixture',releaseCommentId:123,ackCommentId:124,writesEnabled:true}));
  if(samplingEnabled)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','comparison-sampling.json'),JSON.stringify({schemaVersion:1,mode:'paired_comparison_two_attempts_v1',enabled:true,cohort:'fixture',bankHash:'a'.repeat(64)}));
  if(samplingEnabled)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','entry-comparison-active.json'),JSON.stringify({schemaVersion:1,cohort:'fixture',bankHash:'a'.repeat(64)}));
  if(dipEnabled)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','dip-bank.json'),JSON.stringify({policy:JSON.parse(fs.readFileSync(path.join(root,'dip-policy.json'))),config:{...config,feeBps:0},universe:breadth.validateUniverse(universe,config.tournamentId)}));
  if(dipEnabled){const dir=path.join('/synthetic','SigTradingCup','paper-runtime'),bank=JSON.parse(memory.get(path.join(dir,'dip-bank.json')));memory.set(path.join(dir,'dip-state.json'),JSON.stringify({bankHash:model.digest(bank),wallet:model.initialState(config,'fixture',new Date(now).toISOString()),histories:{},bookHistory:{}}));}
  if(officialSync)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','official-sync.json'),JSON.stringify({enabled:true}));
  if(invalidBasket)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','basket-bank.json'),'{invalid');
  if(basketEnabled){const dir=path.join('/synthetic','SigTradingCup','paper-runtime'),bank={mode:'dual_no_basket_measurement',codeIdentity:basketCodeIdentity(),definitions:[]};memory.set(path.join(dir,'basket-bank.json'),JSON.stringify(bank));memory.set(path.join(dir,'basket-stats.json'),JSON.stringify({bankHash:model.digest(bank),status:'running',updatedAt:stamp(0)}));}
  const iso=()=>new Date(now).toISOString();
  const stop=()=>{triggered=true;stopped=true;listeners.SIGTERM();};
  const interrupt=stage=>{if(cycle===2&&stage===stopAt){stop();throw Error('Stopped');}};
  class Clock extends Date{constructor(...args){super(...(args.length?args:[now]));}static now(){return now;}}
  class Journal{
    constructor(){this.last=null;this.previous='';this.sequence=0;}
    commit(state,event){
      if(cycle===1&&event.type==='market_history'&&stopAt==='history-recording-failure'){stop();throw Error('Injected journal failure');}
      if(cycle===2&&event.type==='cycle'&&stopAt==='recording-failure'){stop();throw Error('Injected journal failure');}
      if(cycle===2&&event.type==='cycle'&&stopAt==='cycle-commit')stop();
      const body={sequence:++this.sequence,previous:this.previous,at:iso(),event:structuredClone(event),state:structuredClone(state)};
      const hash=model.digest(body);commits.push({...body,hash});this.last=body;this.previous=hash;
    }
  }
  class ReadClient{
    constructor(key,{onAdmission=()=>{},authorizeDispatch}={}){this.requests=0;this.cooldown=0;this.next=0;this.onAdmission=onAdmission;this.authorizeDispatch=authorizeDispatch;}
    close(){if(settlementConsumeFailure)publications.push({kind:'account_client_closed'});}
    async dispatch(spec){
      if(realtimeShared&&spec.endpoint.includes('/orderbook?'))return this.request(spec.endpoint);
      if(!contractEvidenceEnabled)throw Error('Synthetic dispatch unavailable');
      assert.equal(this.authorizeDispatch(spec),true);spec.beforeDispatch();spec.beforeNetwork();
      const requestStartedAt=iso();this.requests++;requests.push(spec.endpoint);now+=requestMs;
      if(spec.endpoint.startsWith('/relationships?'))return {endpoint:spec.endpoint,status:200,requestStartedAt,receivedAt:iso(),data:{data:[],pagination:{limit:200,hasMore:false,nextCursor:null}}};
      if(stopAt==='contract-stop'){stop();throw Error('Stopped');}
      if(stopAt==='contract-outage'){const e=Error('Synthetic unavailable');e.status=503;throw e;}
      const marketId=/^\/markets\/(\d+)/.exec(spec.endpoint)[1],s=universe.exchanges.find(x=>x.marketId===marketId);
      const contexts=[{type:'tournament',tournament:{id:config.tournamentId},status:'open',settledWith:null,settledOn:null,exchanges:[{id:s.exchangeId}]}];
      const root={node_id:String(Number(marketId)+1),contract_id:String(Number(marketId)+1),node_type:'contract',contract_type:'Election Outcome',settlement_date:'2026-11-04T17:00:00.000Z',settled_with:null,contract_details:{raceId:'62972',stageId:'98102',usState:'NH',raceName:'U.S. Senate New Hampshire',raceStage:'General',winnerName:marketId==='381'?'Democratic Party':'Republican Party',officeLevel:'Federal',contractType:'Election Outcome',electionDate:'2026-11-03',resolutionType:'Party Winner'}};
      const data=spec.endpoint.includes('/nodes?')?{market_id:marketId,root,contexts}:{id:marketId,status:'open',settledWith:null,settledOn:null,settlementDate:root.settlement_date,isComposite:false,isMultiOutcome:false,exchanges:[{id:s.exchangeId}],contexts};
      return {endpoint:spec.endpoint,status:200,requestStartedAt,receivedAt:iso(),data};
    }
    async pause(delay){
      pauses.push({cycle,delay});
      if(cycle===1&&stopAt==='heartbeat-status-failure'){inHeartbeat=true;try{statusHeartbeat();}finally{inHeartbeat=false;}}
      if(cycle===0&&stopAt==='startup-stop'){stop();throw Error('Stopped');}
      if(cycle===0)now+=delay;
      if((cycle===1&&stopAt==='cycle-wait')||(cycle===2&&stopAt==='outage-wait')||(cycle===3&&stopAt==='dual-no-outage-wait')){stop();throw Error('Stopped');}
    }
    async orders(){if(stopped)throw Error('Read after stop');interrupt('orders');this.requests++;now+=requestMs;return [];}
    async request(endpoint){
      if(stopped)throw Error('Read after stop');
      if(cycle===1&&endpoint.includes('/price-history?')&&stopAt==='history-stop'){stop();throw Error('Stopped');}
      if(endpoint.endsWith('/portfolio/positions'))cycle++;
      const market=/^\/markets\/(\d+)\/orderbook/.exec(endpoint)?.[1];
      if(market&&samplingEnabled){const saved=JSON.parse(memory.get(path.join('/synthetic','SigTradingCup','paper-runtime','book-scheduler.json'))||'{}');samplingRequests.push({market,sampling:structuredClone(saved.comparisonSampling)});}
      if(['381','382'].includes(market)){const saved=JSON.parse(memory.get(path.join('/synthetic','SigTradingCup','paper-runtime','book-scheduler.json'))||'{}');pairReadClaims.push({market,claims:Object.keys(saved.basketFollowups?.claims||{}).length});}
      if(endpoint.includes('/portfolio/fills?')){this.requests++;requests.push(endpoint);now+=requestMs;return {receivedAt:iso(),data:{data:[],pagination:{hasMore:false},coverage:{complete:true},privateField:'do-not-export'}};}
      if(endpoint.includes('/portfolio/pnl?')){this.requests++;requests.push(endpoint);now+=requestMs;return {receivedAt:iso(),data:{period:'day',totalAccountValue:100000,totalHoldingsValue:0,totalCostBasis:0,unrealizedPnl:0,periodPnl:0,privateField:'do-not-export'}};}
      const stage=endpoint.includes('/price-history?')?'history':endpoint.endsWith('/portfolio/positions')?'positions':market?'book-'+market:endpoint.startsWith('/exchanges/prices?')?'bulk-admission':'tournament';
      interrupt(stage);this.requests++;requests.push(endpoint);now+=requestMs;
      if(stage==='tournament'&&['startup-recover','startup-stop','startup-auth'].includes(stopAt)&&startupFailures++<2){const e=Error('Synthetic startup outage');e.recoverable=stopAt!=='startup-auth';throw e;}
      if(stage==='tournament')return {data:{id:config.tournamentId,currencyName:'SUSQies',status:'active',isPendingEnrolment:false}};
      if(stage==='history'){const id=/\/exchanges\/(\d+)\//.exec(endpoint)[1],to=new URLSearchParams(endpoint.split('?')[1]).get('to');return {endpoint,requestStartedAt:iso(),receivedAt:iso(),data:{exchangeId:id,marketId:universe.exchanges.find(x=>x.exchangeId===id).marketId,resolution:'1m',from:new Date(Date.parse(to)-3600000).toISOString(),to,candles:[],coverage:{complete:true,projectedThroughSequence:1}}};}
      if(stage==='positions'){
        if(cycle===2&&stopAt==='outage-wait'||cycle===3&&stopAt==='dual-no-outage-wait'){const e=Error('Synthetic connection failure');e.recoverable=true;throw e;}
        return {receivedAt:iso(),data:{positions:[]}};
      }
      if(stage==='bulk-admission'){
        interrupt('bulk-response');
        const ids=new URLSearchParams(endpoint.split('?')[1]).get('ids').split(',');
        return {endpoint,requestStartedAt:iso(),receivedAt:iso(),data:{data:ids.map(id=>({...universe.exchanges.find(x=>x.exchangeId===id),latestPrice:.405,bestBid:.4,bestAsk:.41,spread:.01})),missingIds:[]}};
      }
      const spec=universe.exchanges.find(x=>x.marketId===market);
      return {endpoint,requestStartedAt:iso(),receivedAt:iso(),data:{contexts:[{type:'tournament',tournament:{id:config.tournamentId},orderbook:{exchanges:[{exchangeId:spec.exchangeId,option:'YES',asOf:{at:iso(),sequence:this.requests},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]}]}}]}};
    }
  }
  const mockfs={existsSync:p=>memory.has(p),mkdirSync(){},readFileSync(p){
    if(memory.has(p))return memory.get(p);
    if(['config.json','price-universe.json','depth-plan.json','dashboard.html'].some(n=>p===path.join(root,n)))return fs.readFileSync(p,'utf8');
    throw Error('Unexpected file access '+p);
  }};
  let statusHeartbeat,inHeartbeat=false;
  const process={argv:['node',path.join(root,'runner.mjs'),'--cycles',String(maxCycles)],env:{LOCALAPPDATA:'/synthetic'},pid:12345,on:(name,f)=>listeners[name]=f,exit(){throw Error('Unexpected exit');},exitCode:0};
  const atomicJson=(p,v)=>{
    if(stopAt==='heartbeat-status-failure'&&inHeartbeat&&!triggered&&p.endsWith('stats.json')&&v.runtime?.cycles===1){triggered=true;const e=Error('PRIVATE_EXCEPTION_SENTINEL');e.code='EACCES';throw e;}
    if(p.endsWith('book-scheduler.json')&&v.comparisonSampling){
      if(stopAt==='sampling-plan-persistence')throw Error('Synthetic sampling persistence failure');
      samplingWrites.push({at:iso(),requestCount:requests.length,state:structuredClone(v.comparisonSampling)});
      if(stopAt==='sampling-plan-stop'&&!triggered)stop();
    }
    if(stopAt==='status-publication'&&cycle===2&&!triggered&&p.endsWith('stats.json')&&v.runtime?.cycles===2){stop();throw Error('Synthetic status failure');}
    if(p.endsWith('book-scheduler.json')&&v.basketFollowups){
      if(stopAt==='claim-recording-failure')throw Error('Injected claim persistence failure');
      if(stopAt==='attempt-recording-failure'&&v.bookAttempts>0){stop();throw Error('Injected attempt persistence failure');}
      claimWrites.push({at:iso(),bookAttempts:v.bookAttempts,claims:structuredClone(v.basketFollowups.claims),requestCount:requests.length});
    }
    memory.set(p,JSON.stringify(v));
    if(stopAt==='claim-stop'&&p.endsWith('book-scheduler.json')&&v.basketFollowups&&!triggered)stop();
  };
  const bindings={wrapCoupledDrain,Capture:class{append(){}close(){}},createClient:()=>{},realtimeCaptureScope:()=>({markets:universe.exchanges.map(x=>({marketId:x.marketId,exchangeId:x.exchangeId})),channelMode:'tournament'}),createRealtimeHost:({dispatch})=>({authorizes:()=>false,status:()=>({mode:'market_capture_only',executionAuthorized:false}),service:async()=>{realtimeJoined=true;},stop:async()=>{},beginSharedSnapshot:marketId=>realtimeJoined?{marketId,endpoint:'/markets/'+marketId+'/orderbook?tournamentId='+config.tournamentId+'&depth=200'}:null,readSharedSnapshot:ticket=>dispatch({method:'GET',endpoint:ticket.endpoint}),completeSharedSnapshot:(ticket,response)=>{assert.equal(response.endpoint,ticket.endpoint);publications.push({kind:'shared_realtime_snapshot',marketId:ticket.marketId,receivedAt:response.receivedAt,requestStartedAt:response.requestStartedAt});},failSharedSnapshot:()=>{}}),focusCoupledBulkReads,readCoupledRustTransport:()=>{if(coupledRustInvalid)throw Error('INVALID');return coupledRust?{mode:'original_coupled_wallet_rust_transport_v1',nativeBackend:{},sourceSnapshot:'x'}:null;},coupledBulkStartupFailure:()=>({status:'failed',failed:true,accountingPreserved:true,accountingVerified:false,liabilitiesUnverified:true,knownRiskMarketIds:['350'],executionAuthority:false}),openCoupledBulkRustHost:async()=>{publications.push({kind:'coupled_rust_open'});return{bank:{legs:[],entryExpiresAt:stamp(86400000)},status:()=>({failed:false,riskMarketIds:[],wallets:{COUPLED:{positions:[],unresolvedQuotes:[],reserved:'0.000000'}}}),focus:()=>({riskMarketIds:[],candidateMarketIds:[]}),onBook(){},onPrice(){},onPage(){},boundary(){},rustTransport:{wallet:{totalMs:1}}};},openCoupledBulkHost:()=>coupledBulkPaper?{dir:path.join('/synthetic','SigTradingCup','paper-runtime','coupled-bulk-cohorts','bulk-fixture'),bank:{legs:universe.exchanges,entryExpiresAt:stamp(86400000)},status:()=>({failed:false,riskMarketIds:[],projectionRecoveryRequests:[],wallets:{COUPLED:{positions:[],unresolvedQuotes:[],reserved:'0.000000'}}}),focus:()=>({cutoff:stamp(0),riskMarketIds:[],candidateMarketIds:['350','351']}),onBook(_publication,marketId){publications.push({kind:'coupled_bulk_book',marketId,at:iso()})},onPrice(row){publications.push({kind:'coupled_bulk_price',sourceSequence:row.sequence,at:iso()})},onPage(){},boundary(){}}:null,openCoupledMakerHost:()=>coupledPaper?{bank:{legs:[{marketId:'350',exchangeId:'1039'},{marketId:'351',exchangeId:'1040'}],entryExpiresAt:stamp(86400000)},status:()=>({failed:false,riskMarketIds:[],projectionRecoveryRequests:[],wallets:{COUPLED:{positions:[],unresolvedQuotes:[],reserved:'0.000000'}}}),focus:()=>({cutoff:stamp(0),riskMarketIds:[],candidateMarketIds:['350','351']}),onBook(_publication,marketId){publications.push({kind:'coupled_paper_book',marketId,at:iso()})},onPage(){},boundary(){}}:null,tradeTapeFocus,focusStableMakerReads,mergeMakerTapeFocus,frozenPaperModule:async({fallback})=>fallback,openStableMakerHost:({activeFileName})=>{const broad=activeFileName==='stable-maker-broad-active.json',enabled=broad?stableBroadPaper:stablePaper;if(!enabled)return null;return{dir:path.join('/synthetic','SigTradingCup','paper-runtime','stable-maker-cohorts',broad?'broad-fixture':'fixture'),bank:{legs:broad?[{marketId:'277',exchangeId:'966'},{marketId:'280',exchangeId:'969'}]:[{marketId:'381',exchangeId:'1070'},{marketId:'382',exchangeId:'1071'}],entryExpiresAt:stamp(86400000)},status:()=>({failed:false,riskMarketIds:broad&&stableBroadRecovery?['277']:!broad&&stableBroadPaper?['381']:[],projectionRecoveryRequests:broad&&stableBroadRecovery?[{marketId:'277',exchangeId:'966',from:stamp(-300000),to:stamp(-100000),key:'exact-original-window'}]:[],wallets:{YES:{positions:[],unresolvedQuotes:!broad&&stableBroadPaper?[{}]:[]}},executionAuthority:false}),focus:()=>({cutoff:stamp(0),riskMarketIds:!broad&&stableBroadPaper?['381']:[],candidateMarketIds:broad?['277','280']:['381','382'],executionAuthority:false}),onBook(_publication,marketId){publications.push({kind:broad?'stable_broad_book':'stable_paper_book',marketId,at:iso()});},onPage(){},boundary(){}};},prioritizeProductionReads,mergePaperReadFocus,readBulkReadFocus:()=>null,prioritizeAnchorReads,readAnchorReadFocus:()=>null,prioritizeEdgeTrials,planEdgeObservation,readEdgeObservationFocus:()=>edgeObservation?{bankHash:'a'.repeat(64)}:null,paperExperimentRetired:(dir,marker)=>paperExperimentRetired(dir,marker,{statSync(file){if(!memory.has(file))throw Error();return {isFile:()=>true,size:Buffer.byteLength(memory.get(file))};},readFileSync:file=>memory.get(file)}),readMarketHistoryInputs:()=>({available:historyCandidates.length>0,candidateMarketIds:historyCandidates}),collectMarketSweep,loadMarketSweep:()=>marketSweepEnabled?{cycleIntervalMs:10000}:null,createMarketPricePublication:()=>({publish(snapshot){if(stopAt==='price-recording-failure')throw Error('Synthetic price durability failure');pricePublications.push(snapshot);return{sequence:pricePublications.length,hash:'d'.repeat(64),event:{snapshot}};},status:()=>({sequence:pricePublications.length})}),collectionCycleWait,validateProductionActivation,productionHandoffCurrent:()=>true,SettlementCompetitionController:class{constructor(){this.state={intents:{}};this.transport={authorizeRequest:()=>false};}eligibleMarketIds(){return [];}managedMarketIds(){return officialRiskSchedule?.[Math.max(0,cycle-1)]||[];}async beforeBooks(){officialCalls.push({kind:"reconcile",cycle});}async onBook(){officialCalls.push({kind:"book",cycle});}failure(e){throw e;}close(){}},...model,...breadth,...depth,...scheduler,planComparisonSampling,recordComparisonAttempt,createComparisonRiskSource:()=>({poll:()=>({configured:samplingEnabled,available:!riskUnavailable,cohort:'fixture',bankHash:'a'.repeat(64),heldMarketIds:comparisonHeld,pendingMarketIds:comparisonPending}),close(){}}),collectionInputs,shockCollectionRisk,networkInterval,loadNetworkBudget:file=>mockfs.existsSync(file)?validateNetworkBudget(JSON.parse(mockfs.readFileSync(file))):null,connectTournament,validateTiming,timingSummary,historyEndpoint,normalizeHistory,validateDipPolicy,workerHealth:(report,at=now)=>workerHealth(report,at),baselineView,accountPositions,accountOrders,accountFills,accountPnl,accountView,fs:mockfs,path,http:{createServer:()=>({once(){},listen(p,h,f){f();},close(){}})},randomUUID,fileURLToPath,
    performance:{now:()=>now},collectorNativeJournalConfig:()=>null,collectorNativePublicationConfig:()=>null,
    openProductionMetadata:()=>({pending:()=>null,authorizes:()=>false}),createBaalReadAttention:()=>({reserve:(n,{makerActive}={})=>({historyReads:n-(makerActive?1:0),tapeReads:makerActive?1:0}),status:()=>({executionAuthority:false})}),createTradeTape:()=>({authorizes:()=>false,status:()=>({status:'running',pages:tapeRequests.length}),async collect(_dispatch,marketIds,focus){const members=new Set(marketIds);assert(marketIds.every(id=>universe.exchanges.some(x=>x.marketId===id)));assert(focus.priorityMarketIds.every(id=>members.has(id)));assert(focus.recoveryWindows.every(r=>members.has(r.marketId)&&universe.exchanges.some(x=>x.marketId===r.marketId&&x.exchangeId===r.exchangeId)));tapeRequests.push(structuredClone({marketIds,...focus}));return this.status();}}),readExpiredFlatPaperRetirement:(_runtime,pointer)=>retiredFlatLegacy?{status:'retired',failed:false,riskMarketIds:[],accountingPreserved:true,executionAuthority:false,retirementReason:'expired_and_verified_flat',retiredPointer:pointer}:null,openNoMakerHost:()=>{publications.push({kind:'legacy_no_maker_factory'});return null;},openBaalPairHost:()=>{publications.push({kind:'legacy_baal_pair_factory'});return null;},
    planMakerPairReads,openCollectorDualNoPaper:async options=>dualMock(1,options),openCollectorDualNoPaperV2:async options=>dualMock(2,options),openCollectorSettlementPaper:async()=>{if(!settlementInCollector)return null;let calls=0,failed=false;const holding={marketId:'195',exchangeId:'884',side:'YES',quantity:1,cost:'0.295000'};return{consume(){calls++;publications.push({kind:'settlement_value_consume',calls});if(settlementConsumeFailure&&calls>=2){failed=true;publications.push({kind:'fundamental_consumer_failure',calls,requestCount:requests.length});throw Error('Synthetic fundamental source binding failure');}},publish(){publications.push({kind:'settlement_value_stats'});},status(){if(failed&&settlementStatusFailure)throw Error('Status unavailable after failure');return{failed,bankHash:'f'.repeat(64),executionAuthority:false,competitionOrders:0,wallets:{1000:{positions:[holding],pending:[],cash:'99.705000',realizedPnl:'0.000000'}},sourceBytes:42};},focus:()=>({bankHash:'f'.repeat(64),markets:[{marketId:'195',phase:settlementConsumeFailure?'held':'depth_required'}],ownedRiskReads:true,executionAuthority:false}),close(){publications.push({kind:'settlement_value_close',failed});}};},
    readProfitSprint:(dir,at=now)=>readProfitSprint(dir,at,mockfs),passiveHostsCompletelyFlat,scopedRiskMarketIds,focusProfitSprintReads,
    mergeProbabilityReadFocus,readProbabilityReadFocus:(_dir,_universe,_now,activeFile)=>activeFile?( (activeFile==='primary-breadth-paper-active.json'?probabilityBreadth:probabilityCompanion).length?{bankHash:'b'.repeat(64),markets:(activeFile==='primary-breadth-paper-active.json'?probabilityBreadth:probabilityCompanion).map(marketId=>({marketId,phase:'depth_required'})),executionAuthority:false}:null):(probabilityHeld.length?{bankHash:'a'.repeat(64),markets:probabilityHeld.map(marketId=>({marketId,phase:'held'})),ownedRiskReads:true,executionAuthority:false}:null),probabilityAccountEligibility:cache=>dualNoInCollector||dualNoV2InCollector?realProbabilityAccountEligibility(cache):null,
    planNativeDiscoveryBooks,recordNativeDiscoveryAttempt,
    openCollectorMarketScreen:()=>{if(screenStartupFailure)throw Error('Injected screen source failure');return{async initialize(){},onPublished(){},projection:()=>({available:true,candidateMarketIds:historyCandidates,executionAuthority:false,source:{bytes:1,sequence:1,hash:'a'.repeat(64)}}),close(){}};},
    openNativeDipHost:()=>nativeHeld.length?{status:()=>({heldMarketIds:nativeHeld,status:'running',executionAuthority:false}),kick(){},async close(){}}:null,
    openNativeDipIntentHost:()=>({status:()=>({heldMarketIds:nativeHeld,pendingMarketIds:nativePending,status:'running',hostRevision:3,executionAuthority:false}),kick(){},async close(){}}),
    openNativeDipCacheHost:()=>({status:()=>({heldMarketIds:nativeHeld,status:'running',hostRevision:2,executionAuthority:false}),kick(){},async close(){}}),
    openNativeQuoteHost:()=>quoteHeld.length||quotePending.length?{status:()=>({heldMarketIds:quoteHeld,pendingMarketIds:quotePending,status:'running',executionAuthority:false}),kick(){},async close(){}}:null,
    createAccountEligibilityPublication:()=>({publish({positions,orders,ordersAt}){if(stopAt==='eligibility-recording-failure')throw Error('Injected eligibility recording failure');publications.push({kind:'account_eligibility',positionsAt:positions.receivedAt,ordersAt,at:iso(),requestCount:requests.length});return{sequence:1,expiresAt:iso()};}}),
    loadKey:()=> 'SYNTHETIC',Journal,ReadClient,exactMoneyFetch,createMoneyReceiptWriter,lock:()=>()=>{if(settlementConsumeFailure)publications.push({kind:'collector_lease_released'});},atomicJson,readPairedPackageFocus:()=>paperPackageFocus?{available:true,needed:paperPackageFocus!=='idle',marketIds:['381','382'],reason:'paper_pending'}:null,focusPairedPackageReads,checkpointCoverage:async(file,value,{canCommit})=>{if(!canCommit())throw Error('COMMIT_BLOCKED');atomicJson(file,value);},durableAtomicJson:atomicJson,exclusionSet,basketCandidates,planBasketBooks,basketCodeIdentity,createBasketRelationshipReader,createContractEvidenceReader,
    writeCollectorStatus:atomicJson,
    createBookPublication({canOperate}){return {status:()=>({books:{},sequence:1,hash:'c'.repeat(64)}),publishBook({marketId,response}){if(stopAt==='public-book-recording-failure'&&cycle===2)throw Error('Synthetic public journal failure');if(canOperate())publications.push({kind:'book',marketId,response:structuredClone(response),receivedAt:response.receivedAt,at:iso(),requestCount:requests.length});},invalidate({marketId,reason}){publications.push({kind:'invalidation',marketId,reason,at:iso()});},close(reason){publications.push({kind:'closed',reason,at:iso()});}};},
    createBasketFollowupSource({bank}){return {poll(){
      if(stopAt==='followup-source-failure')throw Error('Injected damaged source');
      return [{tournamentId:config.tournamentId,observerBankHash:model.digest(bank),pairId:'b'.repeat(64),sourceSequence:123,sourceHash:'c'.repeat(64),cutoff:stamp(-3600000),marketIds:['381','382'],exchangeIds:['1070','1071'],
        sourceMeasurement:{reason:'paired_depth_measurement',cycle:0,every:4,marketIds:['381','382'],exchangeIds:['1070','1071'],receivedAt:stamp(-10000)},
        observation:{pairId:'b'.repeat(64),at:stamp(1000),status:'apparent_positive_basket',apparentPositive:true,newPairEvidence:true,independentPair:false,positivePersistenceCount:1,eligibleForExecution:false,fills:null,pnl:null,pairReceipts:[stamp(0),stamp(1000)],pairVersions:['10:'+timestamp(stamp(0)),'11:'+timestamp(stamp(1000))],basket:{quantity:5,totalCost:'4.975000',conditionalMargin:'0.025000',marginalUnitCost:'0.995000',depthFractionBps:100}}}];
    },status(){return {status:'running',sourceSequence:123,observerSequence:1};},close(){}};},
    sampleEquity(s,c,at){if(stopAt==='sampling-failure'&&cycle===2){stop();throw Error('Synthetic sample failure');}return model.sampleEquity(s,c,at);},
    process,console:{log(){},error(){}},AbortController,structuredClone,Date:Clock,setInterval:(fn,ms)=>{if(ms===3000)statusHeartbeat=fn;return 1;},clearInterval(){},moduleUrl:new URL('./runner.mjs',import.meta.url).href};
  const body=source.replace(/^import .*;\r?\n/gm,'').replaceAll('import.meta.url','moduleUrl');
  await vm.runInNewContext('(async()=>{'+body+'\n})()',bindings);
  return {tapeRequests,pricePublications,commits,process,memory,requests,publications,triggered,claimWrites,pairReadClaims,samplingWrites,samplingRequests,pauses,officialCalls};
}

test('real runner captures four scoped metadata reads once before cycles and grants no order eligibility',async()=>{
  const r=await run(null,{basketEnabled:true,contractEvidenceEnabled:true,maxCycles:2});assert.equal(r.process.exitCode,0);
  const evidence=r.commits.filter(x=>x.event.type==='contract_pair_evidence');assert.equal(evidence.length,1);
  assert.equal(evidence[0].event.evidence.orderEligible,false);
  assert.equal(r.requests.filter(p=>/^\/markets\/(381|382)(\/nodes)?\?tournamentId=/.test(p)).length,4);
  assert.equal(r.commits.filter(x=>x.event.type==='cycle').length,2);
});

test('status-heartbeat failure retains sanitized provenance and fails instead of looking like a human STOP',async()=>{
 const r=await run('heartbeat-status-failure');assert.equal(r.process.exitCode,1);assert.equal(r.triggered,true);
 const last=r.commits.at(-1);assert.equal(last.event.type,'stopped');assert.equal(last.event.stopProvenance.cause,'status_publication_failed');assert.equal(last.event.stopProvenance.errorCode,'EACCES');
 const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('paper-runtime'+path.sep+'stats.json'))[1]);assert.equal(stats.runtime.status,'failed');assert.equal(r.publications.at(-1).reason,'COLLECTOR_FAILURE');assert.equal(JSON.stringify(stats).includes('PRIVATE_EXCEPTION_SENTINEL'),false);
});
test('real collector shares its existing slots between prior held paper risk and new empirical research',async()=>{
 const r=await run(null,{dipEnabled:true,nativeCache:true,probabilityHeld:['297'],probabilityCompanion:['356'],maxCycles:2});
 assert.equal(r.process.exitCode,0);const cycles=r.commits.filter(c=>c.event.type==='cycle');assert.equal(cycles.length,2);
 for(const c of cycles){const books=[...(c.event.sourceSnapshots||[]),...(c.event.supplementalDepth||[])].filter(s=>s.endpoint?.includes('/orderbook?'));
  assert.equal(books.length,4);assert(books.some(s=>s.endpoint.startsWith('/markets/297/orderbook?')));assert(books.some(s=>s.endpoint.startsWith('/markets/356/orderbook?')));
 }
 assert(r.requests.every(endpoint=>!endpoint.includes('/orders')||endpoint.startsWith('/me/')));
});
test('metadata outage stays non-authoritative and STOP prevents further startup reads',async()=>{
  for(const stopAt of ['contract-outage','contract-stop']){
    const r=await run(stopAt,{basketEnabled:true,contractEvidenceEnabled:true,maxCycles:2});
    assert.equal(r.commits.filter(x=>x.event.type==='contract_pair_evidence').length,0);
    assert.equal(r.requests.filter(p=>/^\/markets\/(381|382)(\/nodes)?\?tournamentId=/.test(p)).length,1);
    assert.equal(r.commits.filter(x=>x.event.type==='cycle').length,stopAt==='contract-stop'?0:2);
  }
});

test('paired sampling persists before requests and preserves four books per cycle',async()=>{
 const r=await run(null,{dipEnabled:true,samplingEnabled:true,maxCycles:4});assert.equal(r.process.exitCode,0);
 const cycles=r.commits.filter(x=>x.event.type==='cycle');assert.equal(cycles.length,4);
 assert.equal(r.requests.filter(x=>x.includes('/orderbook?')).length,16);
 assert.ok(r.samplingWrites.length>0);assert.ok(r.samplingRequests.every(x=>x.sampling));
 assert.ok(cycles.every(x=>x.event.comparisonSampling?.planId));
 assert.ok(cycles.every(x=>x.event.comparisonSampling.reservations.length<=2));
 assert.equal(new Set(cycles[0].event.comparisonSampling.reservations.map(x=>x.marketId)).size,2);
 assert.deepEqual(cycles[0].event.comparisonSampling.reservations.map(x=>x.marketId),cycles[1].event.comparisonSampling.reservations.map(x=>x.marketId));
 assert.ok(cycles.every(x=>x.event.entryMarketIds.length===0),'sampling never creates execution leads');
 const start=r.commits.find(x=>x.event.type==='session_start');assert.ok(start);assert.ok(start.sequence<cycles[0].sequence);
 assert.equal(start.event.sessionId,r.commits.find(x=>x.event.type==='comparison_sampling_session').event.sessionId);
});

test('sampling persistence failure and STOP occur before any scheduled book',async()=>{
 for(const code of ['sampling-plan-persistence','sampling-plan-stop']){
  const r=await run(code,{dipEnabled:true,samplingEnabled:true});assert.equal(r.process.exitCode,code==='sampling-plan-persistence'?1:0);
  assert.equal(r.requests.filter(x=>x.includes('/orderbook?')).length,0);
 }
});

test('unavailable comparison evidence retains held books but disables optional overlays',async()=>{
 const r=await run(null,{dipEnabled:true,samplingEnabled:true,riskUnavailable:true,basketEnabled:true,comparisonHeld:['305'],comparisonPending:['277']});
 assert.equal(r.process.exitCode,0);assert.equal(r.samplingWrites.length,0);assert.equal(r.claimWrites.length,0);
 for(const row of r.commits.filter(x=>x.event.type==='cycle')){
  assert.equal(row.event.comparisonSampling,null);
  assert.ok(row.event.sourceSnapshots.some(x=>x.endpoint?.startsWith('/markets/305/')));
  assert.ok(row.event.sourceSnapshots.some(x=>x.endpoint?.startsWith('/markets/277/')));
 }
});

test('followup claim is durably installed before pair reads and remains through scheduler attempts',async()=>{
 const r=await run(null,{dipEnabled:true,basketEnabled:true});assert.equal(r.process.exitCode,0);
 assert.ok(r.claimWrites.length>0);assert.equal(r.claimWrites[0].bookAttempts,0);
 assert.deepEqual(r.pairReadClaims.slice(0,2),[{market:'381',claims:1},{market:'382',claims:1}]);
 const saved=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('book-scheduler.json'))[1]);assert.equal(Object.keys(saved.basketFollowups.claims).length,1);
 const cycle=r.commits.find(r=>r.event.type==='cycle');assert.equal(cycle.event.basketMeasurement.reason,'paired_depth_followup');assert.equal(cycle.event.entryMarketIds.includes('381'),false);
 assert.equal(r.requests.filter(p=>p.includes('/orderbook?')).length,8);
});

test('failed followup claim publication cannot dispatch either leg or silently fall back after an uncertain save',async()=>{
 const r=await run('claim-recording-failure',{dipEnabled:true,basketEnabled:true});assert.equal(r.process.exitCode,1);assert.equal(r.pairReadClaims.length,0);assert.equal(r.requests.filter(p=>p.includes('/orderbook?')).length,0);
});

test('STOP after durable followup claim leaves it consumed without reading the pair',async()=>{
 const r=await run('claim-stop',{dipEnabled:true,basketEnabled:true});assert.equal(r.process.exitCode,0);assert.equal(r.triggered,true);assert.equal(r.pairReadClaims.length,0);assert.equal(r.claimWrites.length,1);
});

test('later scheduler replacement failure preserves the claim and fails before a read even with STOP',async()=>{
 const r=await run('attempt-recording-failure',{dipEnabled:true,basketEnabled:true});assert.equal(r.process.exitCode,1);assert.equal(r.triggered,true);assert.equal(r.pairReadClaims.length,0);assert.equal(r.claimWrites.length,1);
 const saved=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('book-scheduler.json'))[1]);assert.equal(saved.bookAttempts,0);assert.equal(Object.keys(saved.basketFollowups.claims).length,1);
});

test('damaged optional followup source preserves original collector book budget and candidates',async()=>{
 const base=await run(null,{dipEnabled:true}),r=await run('followup-source-failure',{dipEnabled:true,basketEnabled:true});assert.equal(r.process.exitCode,0);assert.equal(r.claimWrites.length,0);
 assert.deepEqual(r.requests.filter(p=>p.includes('/orderbook?')),base.requests.filter(p=>p.includes('/orderbook?')));
});

test('public depth is published at receipt before the later cycle journal, without extra reads',async()=>{
 const r=await run(null),books=r.publications.filter(p=>p.kind==='book'),bookReads=r.requests.filter(p=>p.includes('/orderbook?'));
 assert.equal(r.process.exitCode,0);assert.equal(books.length,bookReads.length);assert.ok(books.length>0);
 for(const b of books){assert.equal(b.at,b.receivedAt);assert.equal(r.requests[b.requestCount-1],'/markets/'+b.marketId+'/orderbook?tournamentId='+config.tournamentId+'&depth=20');}
 const cycles=r.commits.filter(c=>c.event.type==='cycle');assert.ok(books.some(b=>Date.parse(b.at)<Date.parse(cycles[0].at)));
 assert.equal(r.publications.at(-1).kind,'closed');assert.equal(r.publications.at(-1).reason,'STOP');
});
test('public depth source records outage and STOP without retaining a live session',async()=>{
 const r=await run('outage-wait');assert.equal(r.process.exitCode,0);assert.ok(r.publications.some(p=>p.kind==='invalidation'&&p.marketId===null&&p.reason==='ACCOUNT_OUTAGE'));assert.equal(r.publications.at(-1).kind,'closed');assert.equal(r.publications.at(-1).reason,'STOP');
});
test('public depth durability failure remains fatal and always closes publication',async()=>{
 const r=await run('public-book-recording-failure');assert.equal(r.process.exitCode,1);assert.equal(r.publications.at(-1).kind,'closed');assert.equal(r.publications.at(-1).reason,'COLLECTOR_FAILURE');assert.equal(r.commits.filter(c=>c.event.type==='cycle').length,1);
});

for(const stage of ['positions','orders',...config.marketIds.map(id=>'book-'+id),'book-342','book-376','bulk-admission','bulk-response','cycle-wait','outage-wait','cycle-commit','sampling-failure','status-publication']){
  test('STOP at '+stage+' preserves replayable committed accounting',async()=>{
    const result=await run(stage),last=result.commits.at(-1);
    assert.equal(result.triggered,true);assert.equal(result.process.exitCode,0);assert.equal(last.event.type,'stopped');
    const replay=replayJournal(result.commits.map(r=>JSON.stringify(r)).join('\n')+'\n',config);
    assert.equal(last.state.observations,replay.observations);
    for(const name of Object.keys(last.state.strategies)){
      const durable=last.state.strategies[name],reconstructed=replay.strategies[name];
      assert.equal(durable.cash,reconstructed.cash);assert.equal(durable.fees,reconstructed.fees);
      assert.equal(durable.realized,reconstructed.realizedPnl);assert.equal(durable.fills.length,reconstructed.fills);
    }
    assert.equal(replay.reportedAt,last.at);
  });
}
test('a journal failure concurrent with STOP fails closed without a later state-only append',async()=>{
  const r=await run('recording-failure');assert.equal(r.triggered,true);assert.equal(r.process.exitCode,1);
  assert.deepEqual(r.commits.map(x=>x.event.type),['started','cycle']);
  const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);
  assert.equal(stats.runtime.status,'failed');assert.match(stats.runtime.errors.at(-1).reason,/recording failed/);
});
test('ordinary bounded completion remains source/accounting equivalent',async()=>{
  const r=await run('none'),last=r.commits.at(-1),replay=replayJournal(r.commits.map(x=>JSON.stringify(x)).join('\n')+'\n',config);
  assert.equal(r.process.exitCode,0);assert.equal(last.state.observations,8);assert.equal(replay.observations,8);assert.equal(replay.strategies.buy_hold_benchmark.fills,4);
});

test('invalid optional basket observer preserves original book reads and official candidate metadata',async()=>{
 const base=await run('none',{dipEnabled:true}),r=await run('none',{dipEnabled:true,invalidBasket:true});
 assert.equal(r.process.exitCode,0);assert.deepEqual(r.requests.filter(p=>p.startsWith('/markets/')),base.requests.filter(p=>p.startsWith('/markets/')));
 assert.deepEqual(r.commits.filter(c=>c.event.type==='cycle').map(c=>c.event.entryMarketIds),base.commits.filter(c=>c.event.type==='cycle').map(c=>c.event.entryMarketIds));
 const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('paper-runtime'+path.sep+'stats.json'))[1]);assert.equal(stats.runtime.basketMeasurement.reason,'observer_invalid_or_unavailable');
});

test('runner persists startup backoff, recovers into ordinary collection and stops during retry',async()=>{
 const recovered=await run('startup-recover');assert.equal(recovered.process.exitCode,0);assert.equal(recovered.commits.at(-1).state.observations,8);const rate=JSON.parse([...recovered.memory.entries()].find(([p])=>p.endsWith('rate.json'))[1]);assert.ok(Number.isFinite(rate.next));assert.equal(rate.manualReview,false);
 const stopped=await run('startup-stop');assert.equal(stopped.process.exitCode,0);assert.equal(stopped.requests.length,1);assert.deepEqual(stopped.commits.map(x=>x.event.type),['stopped']);
 const auth=await run('startup-auth');assert.equal(auth.process.exitCode,1);assert.equal(auth.requests.length,1);assert.equal(auth.commits.length,0);
});
test('dip collection rotates comparisons, records sparse historical reads, preserves replay and original slot count',async()=>{
 const r=await run('none',{dipEnabled:true}),raw=r.commits.map(r=>JSON.stringify(r)).join('\n')+'\n',last=r.commits.at(-1),replay=replayJournal(raw,config);
 assert.equal(r.process.exitCode,0);assert.equal(r.commits.filter(r=>r.event.type==='market_history').length,4);assert.equal(r.requests.filter(p=>p.startsWith('/markets/')).length,8);assert.equal(last.state.observations,4);assert.equal(replay.observations,4);assert.equal(last.state.strategies.mean_reversion.cash,replay.strategies.mean_reversion.cash);
});
test('STOP or durable recording failure during historical backfill cannot continue to an account/book read',async()=>{
 const stop=await run('history-stop',{dipEnabled:true});assert.equal(stop.process.exitCode,0);assert.equal(stop.triggered,true);assert.equal(stop.commits.at(-1).event.type,'stopped');
 const fail=await run('history-recording-failure',{dipEnabled:true});assert.equal(fail.process.exitCode,1);assert.equal(fail.commits.at(-1).event.type,'cycle');assert.equal(fail.requests.filter(p=>p.endsWith('/portfolio/positions')).length,1);
});

test('official sync replaces historical slots and stores only curated account projections outside the paper journal',async()=>{
 const r=await run('none',{dipEnabled:true,officialSync:true});assert.equal(r.process.exitCode,0);assert.equal(r.requests.filter(x=>x.includes('/price-history?')).length,2);assert.equal(r.requests.filter(x=>x.includes('/portfolio/fills?')||x.includes('/portfolio/pnl?')).length,2);const cache=JSON.parse(r.memory.get(path.join('/synthetic','SigTradingCup','paper-runtime','official-account.json')));assert.equal(cache.pnl.accountValue,100000);assert.equal(cache.fills.rows.length,0);assert.ok(!JSON.stringify(cache).includes('do-not-export'));assert.ok(!JSON.stringify(r.commits).includes('do-not-export'));assert.equal(replayJournal(r.commits.map(x=>JSON.stringify(x)).join('\n')+'\n',config).observations,4);
});
test('real runner skips its cycle-floor idle for owned or pending official risk and retains reconciliation before books',async()=>{
 const flat=await run('none',{requestMs:100}),risk=await run('none',{requestMs:100,officialRiskSchedule:[['277'],['277']]});
 assert.equal(risk.process.exitCode,0);assert.ok(flat.pauses.some(p=>p.cycle===1&&p.delay>20000));assert.equal(risk.pauses.filter(p=>p.cycle===1).length,0);
 for(const cycle of [1,2]){const calls=risk.officialCalls.filter(x=>x.cycle===cycle);assert.equal(calls[0].kind,'reconcile');assert.equal(calls.filter(x=>x.kind==='book').length,6);}
 assert.deepEqual([...risk.requests].sort(),[...flat.requests].sort(),'priority changes order while preserving every endpoint, count and account reconciliation');
 assert(risk.requests.find(x=>x.includes('/orderbook?')).startsWith('/markets/277/'),'official held risk receives its book first');
});
test('flat transition restores the original cycle wait and STOP/outage still stop or back off',async()=>{
 const cleared=await run('none',{maxCycles:3,requestMs:100,officialRiskSchedule:[['277'],[],[]]});assert.equal(cleared.process.exitCode,0);assert.equal(cleared.pauses.filter(p=>p.cycle===1).length,0);assert.ok(cleared.pauses.some(p=>p.cycle===2&&p.delay>20000));
 const stopped=await run('positions',{requestMs:100,officialRiskSchedule:[['277'],['277']]});assert.equal(stopped.process.exitCode,0);assert.equal(stopped.requests.filter(x=>x.includes('/orderbook?')).length,6);assert.equal(stopped.commits.filter(x=>x.event.type==='cycle').length,1);
 const outage=await run('outage-wait',{requestMs:100,maxCycles:3,officialRiskSchedule:[['277'],['277'],['277']]});assert.equal(outage.process.exitCode,0);assert.ok(outage.pauses.some(p=>p.cycle===2&&p.delay>=10000));assert.equal(outage.commits.filter(x=>x.event.type==='account_read_outage').length,1);
});

test('enabled real runner publishes all universe batches with frozen primary replay and ten second cycle budget',async()=>{const r=await run(null,{marketSweepEnabled:true,maxCycles:1,requestMs:1000});assert.equal(r.process.exitCode,0);assert.equal(r.pricePublications.length,3);assert.equal(r.pricePublications.reduce((n,s)=>n+s.quotes.length+s.missingIds.length,0),universe.exchanges.length);assert.equal(r.commits.filter(x=>x.event.type==='cycle').length,1);assert.equal(r.commits.find(x=>x.event.type==='cycle').event.bulkPrices.quotes.length,100);});
test('enabled sweep publication failure halts real runner before subsequent bulk requests',async()=>{const r=await run('price-recording-failure',{marketSweepEnabled:true,maxCycles:1,requestMs:1000});assert.equal(r.process.exitCode,1);assert.equal(r.requests.filter(x=>x.startsWith('/exchanges/prices?')).length,1);assert.equal(r.commits.filter(x=>x.event.type==='cycle').length,0);});

test('verified market-wide history proposals enter the existing bounded scheduler without extra book slots',async()=>{const r=await run(null,{dipEnabled:true,historyCandidates:['336'],maxCycles:1,requestMs:1000});assert.equal(r.process.exitCode,0);const books=r.requests.filter(x=>x.includes('/orderbook'));assert.equal(books.length,4);assert(books.some(x=>x.includes('/markets/336/')));});
test('real runner services both pending package legs using exactly the original four book slots',async()=>{const r=await run(null,{dipEnabled:true,paperPackageFocus:true,maxCycles:1,requestMs:1000});assert.equal(r.process.exitCode,0);const books=r.requests.filter(x=>x.includes('/orderbook'));assert.equal(books.length,4);assert.ok(books.some(x=>x.includes('/markets/381/')));assert.ok(books.some(x=>x.includes('/markets/382/')));});

test('actual runner observes idle paired strategy within same four slots before any signal exists',async()=>{const r=await run(null,{dipEnabled:true,paperPackageFocus:'idle',maxCycles:1,requestMs:1000});assert.equal(r.process.exitCode,0);const books=r.requests.filter(x=>x.includes('/orderbook'));assert.equal(books.length,4);assert(books.some(x=>x.includes('/markets/381/')));assert(books.some(x=>x.includes('/markets/382/')));});

test('real durable package timestamp ledger survives ordinal book attempts and suppresses immediate repeat',async()=>{const r=await run(null,{dipEnabled:true,paperPackageFocus:'idle',maxCycles:2,requestMs:1000});assert.equal(r.process.exitCode,0);const state=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('book-scheduler.json'))[1]);assert.equal(typeof state.bookLastAttempt['381'],'number');assert.equal(typeof state.packageReadLastAttemptAt['381'],'string');assert.equal(typeof state.packageReadLastAttemptAt['382'],'string');const books=r.requests.filter(x=>x.includes('/orderbook'));assert.equal(books.length,8);assert.equal(books.filter(x=>x.includes('/markets/381/')).length,1);assert.equal(books.filter(x=>x.includes('/markets/382/')).length,1);});

test('operator-retired comparison skips obsolete risk without suppressing official books',async()=>{
 const r=await run(null,{dipEnabled:true,samplingEnabled:true,riskUnavailable:true,comparisonHeld:['305'],comparisonPending:['277'],retiredComparison:true,officialRiskSchedule:[['369'],['369']],maxCycles:2});
 assert.equal(r.process.exitCode,0);assert.equal(r.samplingWrites.length,0);
 for(const cycle of r.commits.filter(x=>x.event.type==='cycle')){assert.ok([...cycle.event.sourceSnapshots,...cycle.event.supplementalDepth].some(x=>x.marketId==='369'||x.endpoint?.startsWith('/markets/369/')),'official held market retains monitoring');assert.equal(cycle.event.comparisonSampling,null);}
});

test('real collector repeats discovery block with same four-book budget and retains risk',async()=>{const r=await run(null,{dipEnabled:true,edgeObservation:true,officialRiskSchedule:[[],['369'],['369']],maxCycles:3,requestMs:100});assert.equal(r.process.exitCode,0);const cycles=r.commits.filter(x=>x.event.type==='cycle');const pinned=cycles[0].event.edgeObservation.marketIds;assert.equal(pinned.length,2);for(const c of cycles){assert.equal(c.event.sourceSnapshots.length+c.event.supplementalDepth.length,4);assert.deepEqual(c.event.edgeObservation.marketIds,pinned);if(c!==cycles[0])assert.ok([...c.event.sourceSnapshots,...c.event.supplementalDepth].some(x=>x.endpoint?.startsWith('/markets/369/')));}const saved=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('book-scheduler.json'))[1]);assert.deepEqual(saved.edgeObservation.marketIds,pinned);});

test('real collector protects native paper holdings inside the existing four reads',async()=>{const r=await run(null,{nativeHeld:['277'],maxCycles:2,requestMs:100});assert.equal(r.process.exitCode,0);for(const c of r.commits.filter(x=>x.event.type==='cycle')){assert.equal(c.event.sourceSnapshots.length+c.event.supplementalDepth.length,4);assert([...c.event.sourceSnapshots,...c.event.supplementalDepth].some(x=>x.marketId==='277'||x.endpoint?.startsWith('/markets/277/')));}});
test('real collector selects cached native continuation and retains the same held read budget',async()=>{const r=await run(null,{nativeHeld:['277'],nativeCache:true,maxCycles:2,requestMs:100});assert.equal(r.process.exitCode,0);const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.nativeDip.hostRevision,2);for(const c of r.commits.filter(x=>x.event.type==='cycle')){assert.equal(c.event.sourceSnapshots.length+c.event.supplementalDepth.length,4);assert([...c.event.sourceSnapshots,...c.event.supplementalDepth].some(x=>x.marketId==='277'||x.endpoint?.startsWith('/markets/277/')));}});
test('real collector retains verified paper risk first, spends the free slot on screened and fair discovery, and persists attempts',async()=>{const lead=universe.exchanges.find(x=>!config.marketIds.includes(x.marketId)&&!['381','382','297'].includes(x.marketId)).marketId;const r=await run(null,{nativeHeld:['381','382'],probabilityHeld:['297'],nativeCache:true,marketSweepEnabled:true,historyCandidates:[lead],maxCycles:4,requestMs:100});assert.equal(r.process.exitCode,0);const cycles=r.commits.filter(x=>x.event.type==='cycle');assert.equal(cycles.length,4);for(const c of cycles){const books=[...c.event.sourceSnapshots,...c.event.supplementalDepth].map(x=>/^\/markets\/(\d+)\//.exec(x.endpoint)?.[1]);assert.equal(books.length,4);assert(books.includes('381')&&books.includes('382')&&books.includes('297'));assert.equal(c.event.nativeDiscovery.requestsAdded,0);assert.equal(c.event.nativeDiscovery.executionAuthority,false);}assert.equal(cycles[0].event.nativeDiscovery.marketId,lead);assert.equal(cycles[3].event.nativeDiscovery.reservedFair,true);const saved=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('book-scheduler.json'))[1]);assert.equal(saved.nativeDiscoveryAttempts,4);assert.equal(r.pricePublications.length,12);});
test('optional screen startup failure retains risk collection without falling back to unverified leads',async()=>{const r=await run(null,{nativeHeld:['381','382'],probabilityHeld:['297'],nativeCache:true,marketSweepEnabled:true,historyCandidates:['277'],screenStartupFailure:true,maxCycles:1,requestMs:100});assert.equal(r.process.exitCode,0);const c=r.commits.find(x=>x.event.type==='cycle');assert.equal(c.event.nativeDiscovery.reason,'persisted_fair_discovery');assert.equal(c.event.sourceSnapshots.length+c.event.supplementalDepth.length,4);});

test('real collector prioritizes verified native pending instruments within the same four-book budget',async()=>{const r=await run(null,{nativeHeld:['381','382'],nativePending:['280'],probabilityHeld:['297'],nativeIntent:true,marketSweepEnabled:true,maxCycles:2,requestMs:100});assert.equal(r.process.exitCode,0);const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.nativeDip.hostRevision,3);for(const c of r.commits.filter(x=>x.event.type==='cycle')){const books=[...c.event.sourceSnapshots,...c.event.supplementalDepth].map(x=>/^\/markets\/(\d+)\//.exec(x.endpoint)?.[1]);assert.equal(books.length,4);for(const market of ['381','382','297','280'])assert(books.includes(market));assert.equal(c.event.nativeDiscovery.reason,'protected_budget_full');}});
test('new native quote wallets share four existing book slots and fresh eligibility is recorded before books',async()=>{const r=await run(null,{nativeHeld:['381'],quoteHeld:['382'],quotePending:['280'],probabilityHeld:['297'],nativeIntent:true,marketSweepEnabled:true,maxCycles:2,requestMs:100});assert.equal(r.process.exitCode,0);for(const c of r.commits.filter(x=>x.event.type==='cycle')){const books=[...c.event.sourceSnapshots,...c.event.supplementalDepth].map(x=>/^\/markets\/(\d+)\//.exec(x.endpoint)?.[1]);assert.equal(books.length,4);for(const market of ['381','382','297','280'])assert(books.includes(market));}const first=r.publications.findIndex(x=>x.kind==='book'),eligible=r.publications.findIndex(x=>x.kind==='account_eligibility');assert(eligible>=0&&eligible<first);});
test('eligibility durability failure closes the public session before any book or later read',async()=>{const r=await run('eligibility-recording-failure',{nativeIntent:true,maxCycles:2,requestMs:100});assert.equal(r.process.exitCode,1);assert(!r.publications.some(x=>x.kind==='book'));assert.equal(r.publications.at(-1).kind,'closed');});


test('human-retired price experiments release paper capacity while preserving official reconciliation and four reads',async()=>{const r=await run(null,{dipEnabled:true,nativeIntent:true,nativeHeld:['277'],quoteHeld:['280'],retiredNativePrices:true,probabilityHeld:['297'],officialRiskSchedule:[['369'],['369']],maxCycles:2,requestMs:100});assert.equal(r.process.exitCode,0);const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.nativeDip.status,'retired');assert.equal(stats.runtime.nativeQuote.status,'retired');assert.equal(stats.runtime.retiredPaperStrategies.accountingPreserved,true);assert(!stats.runtime.scheduling.managedMarketIds.includes('277'));assert(stats.runtime.scheduling.managedMarketIds.includes('297'),'current forecast paper inventory still retains its slot after old native hosts retire');for(const c of r.commits.filter(x=>x.event.type==='cycle')){const books=[...c.event.sourceSnapshots,...c.event.supplementalDepth];assert.equal(books.length,4);assert(books.some(x=>x.endpoint.includes('/markets/369/')));assert.equal(c.state.observations,0);}assert(r.officialCalls.some(x=>x.kind==='reconcile'));});

test('the new passive host receives existing scoped books after eligibility with no extra read slots',async()=>{const r=await run(null,{dipEnabled:true,retiredNativePrices:true,stablePaper:true,probabilityHeld:['297'],probabilityCompanion:['356'],marketSweepEnabled:true,maxCycles:3,requestMs:100});assert.equal(r.process.exitCode,0);for(const c of r.commits.filter(x=>x.event.type==='cycle'))assert.equal(c.event.sourceSnapshots.length+c.event.supplementalDepth.length,4);assert(r.publications.some(x=>x.kind==='stable_paper_book'&&x.marketId==='381'));assert(r.publications.some(x=>x.kind==='stable_paper_book'&&x.marketId==='382'));assert(r.publications.findIndex(x=>x.kind==='account_eligibility')<r.publications.findIndex(x=>x.kind==='stable_paper_book'));});

test('a broad frozen host takes spare slots while narrow pending and existing forecast inventory keep their reads',async()=>{const r=await run(null,{dipEnabled:true,retiredNativePrices:true,stablePaper:true,stableBroadPaper:true,probabilityHeld:['297','356'],marketSweepEnabled:true,maxCycles:3,requestMs:100});assert.equal(r.process.exitCode,0);for(const c of r.commits.filter(x=>x.event.type==='cycle')){const books=[...c.event.sourceSnapshots,...c.event.supplementalDepth];assert.equal(books.length,4);for(const id of ['297','356','381'])assert(books.some(x=>x.endpoint.includes('/markets/'+id+'/')),JSON.stringify({missing:id,books:books.map(x=>x.endpoint)}));}assert(r.publications.some(x=>x.kind==='stable_broad_book'));assert(r.publications.some(x=>x.kind==='stable_paper_book'&&x.marketId==='381'));});

test('real runner tape dispatch follows broad bank with no narrow host and exact pending recovery',async()=>{for(const pending of [false,true]){const r=await run(null,{dipEnabled:true,retiredNativePrices:true,stableBroadPaper:true,stableBroadRecovery:pending,marketSweepEnabled:true,maxCycles:3,requestMs:100});assert.equal(r.process.exitCode,0);assert(r.tapeRequests.length>0,'tape path must actually execute');for(const p of r.tapeRequests){assert.deepEqual(Array.from(p.marketIds),['277','280']);assert.deepEqual(Array.from(p.priorityMarketIds),pending?['277']:['277','280']);if(pending)assert.equal(p.recoveryWindows[0].key,'exact-original-window');}assert(r.commits.every(c=>c.event.type!=='cycle'||c.event.sourceSnapshots.length+c.event.supplementalDepth.length===4));}});

test('coupled maker shares the single tape and four books with existing broad and forecast wallets',async()=>{const r=await run(null,{dipEnabled:true,retiredNativePrices:true,stableBroadPaper:true,coupledPaper:true,probabilityHeld:['297','356'],maxCycles:3,requestMs:100});assert.equal(r.process.exitCode,0);assert(r.tapeRequests.length>0);for(const p of r.tapeRequests)for(const id of ['350','351','277','280'])assert(p.marketIds.includes(id));assert(r.publications.some(x=>x.kind==='coupled_paper_book'&&['350','351'].includes(x.marketId)));for(const c of r.commits.filter(x=>x.event.type==='cycle')){const books=[...c.event.sourceSnapshots,...c.event.supplementalDepth];assert.equal(books.length,4);for(const id of ['297','356'])assert(books.some(x=>x.endpoint.includes('/markets/'+id+'/')));}});

test('bulk successor consumes each committed price receipt without adding clients or overwriting prior pending wallets',async()=>{const r=await run(null,{dipEnabled:true,retiredNativePrices:true,stablePaper:true,stableBroadPaper:true,stableBroadRecovery:true,coupledBulkPaper:true,probabilityHeld:['297','356'],marketSweepEnabled:true,maxCycles:3,requestMs:100});assert.equal(r.process.exitCode,0);assert(r.publications.some(x=>x.kind==='coupled_bulk_price'));assert(r.publications.some(x=>x.kind==='coupled_bulk_book'));assert(!r.publications.some(x=>x.kind==='stable_broad_book'&&x.marketId==='280'));for(const c of r.commits.filter(x=>x.event.type==='cycle')){const books=[...c.event.sourceSnapshots,...c.event.supplementalDepth];assert.equal(books.length,4);for(const id of ['297','356'])assert(books.some(x=>x.endpoint.includes('/markets/'+id+'/')));}assert(r.tapeRequests.every(p=>p.marketIds.length===universe.exchanges.length));});

test('joined realtime capture cannot replace the four frozen consumer depth20 receipts',async()=>{const r=await run(null,{dipEnabled:true,realtimeShared:true,maxCycles:2,requestMs:100});assert.equal(r.process.exitCode,0);const books=r.requests.filter(x=>x.includes('/orderbook'));assert.equal(books.length,8);assert(books.every(x=>x.endsWith('depth=20')));assert.equal(r.publications.filter(x=>x.kind==='shared_realtime_snapshot').length,0);});

test('real runner receipts remain accepted by the actual frozen native publisher after realtime joins',async t=>{
 const r=await run(null,{dipEnabled:true,realtimeShared:true,maxCycles:2,requestMs:100});
 const books=r.publications.filter(x=>x.kind==='book');assert.equal(books.length,8);
 const sourceRoot=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
 const nativeBackend=JSON.parse(fs.readFileSync(new URL('../data/probability-native-deployment-20261004.json',import.meta.url))).nativeBackend;
 const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-receipt-integration-'));
 t.after(()=>{assert(path.resolve(runtimeDir).startsWith(path.resolve(os.tmpdir())+path.sep));fs.rmSync(runtimeDir,{recursive:true,force:true});});
 const binaryDir=path.join(runtimeDir,'native-builds',nativeBackend.binaryHash);fs.mkdirSync(binaryDir,{recursive:true});
 for(const name of ['baal-backend.exe','libunwind.dll'])fs.copyFileSync(path.join(sourceRoot,'native-builds',nativeBackend.binaryHash,name),path.join(binaryDir,name));
 let clock=Date.parse(books[0].receivedAt)-2;
 const file=path.join(runtimeDir,'books.jsonl'),scope=breadth.validateUniverse(universe,config.tournamentId);
 const seed=createReceiptSeed({file,universe:scope,sessionId:'runner-receipt-seed',now:()=>clock});clock++;seed.close('STOP');clock++;
 const writer=await createNativeBookPublication({file,runtimeDir,nativeBackend,universe:scope,sessionId:'runner-depth20-integration',receiptDir:path.join(runtimeDir,'receipts'),now:()=>clock});
 t.after(()=>writer.close('STOP'));
 for(const row of books){clock=Date.parse(row.receivedAt);assert.equal(writer.publishBook({marketId:row.marketId,response:row.response}),true,row.response.endpoint);}
 assert.equal(writer.status().sequence,11);assert(Object.keys(writer.status().books).length>0);
 const bad=structuredClone(books.at(-1));bad.response.endpoint=bad.response.endpoint.replace('depth=20','depth=200');clock++;
 assert.equal(writer.publishBook({marketId:bad.marketId,response:bad.response}),false);
 clock++;const recovery=structuredClone(books.at(-1).response);recovery.requestStartedAt=new Date(clock).toISOString();recovery.receivedAt=new Date(clock).toISOString();
 assert.equal(writer.publishBook({marketId:books.at(-1).marketId,response:recovery}),true);
 assert(Object.keys(writer.status().books).length>0);
});

test('third primary-paper cohort shares four ordinary reads while prior holdings retain first service',async()=>{const r=await run(null,{dipEnabled:true,nativeCache:true,probabilityHeld:['297','356'],probabilityBreadth:['195'],coupledBulkPaper:true,retiredNativePrices:true,maxCycles:2});assert.equal(r.process.exitCode,0);const cycles=r.commits.filter(x=>x.event.type==='cycle');assert.equal(cycles.length,2);for(const c of cycles){const books=[...c.event.sourceSnapshots,...c.event.supplementalDepth];assert.equal(books.length,4);assert(books.some(x=>x.endpoint?.startsWith('/markets/195/orderbook?')));assert(books.some(x=>x.endpoint?.startsWith('/markets/297/orderbook?')));assert(books.some(x=>x.endpoint?.startsWith('/markets/356/orderbook?')));}});

test('funded value consumer shares the actual publisher and existing Governor slot, with no extra read client',async()=>{const options={dipEnabled:true,nativeCache:true,probabilityHeld:['297','356'],probabilityBreadth:['195'],coupledBulkPaper:true,retiredNativePrices:true,maxCycles:2};const previous=await run(null,options),r=await run(null,{...options,settlementInCollector:true});assert.equal(r.process.exitCode,0);assert.deepEqual(r.requests,previous.requests);assert(r.publications.filter(x=>x.kind==='settlement_value_consume').length>=9);assert(r.publications.some(x=>x.kind==='settlement_value_close'));const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.settlementPaper.executionAuthority,false);assert.equal(stats.runtime.settlementPaper.competitionOrders,0);});

test('bounded source sprint reallocates real collector reads without losing account eligibility or source holdings',async()=>{const options={dipEnabled:true,nativeCache:true,officialSync:true,probabilityHeld:['195','356'],coupledBulkPaper:true,retiredNativePrices:true,marketSweepEnabled:true,maxCycles:2};const ordinary=await run(null,options),fast=await run(null,{...options,profitSprint:true});assert.equal(fast.process.exitCode,0);const books=r=>r.requests.filter(x=>/orderbook/.test(x));assert.deepEqual(books(fast).map(x=>x.match(/markets\/(\d+)/)[1]),['356','195','356','195']);assert(books(fast).every(x=>x.endsWith('depth=20')));assert(books(ordinary).length>books(fast).length);assert.equal(fast.requests.filter(x=>/portfolio\/positions/.test(x)).length,ordinary.requests.filter(x=>/portfolio\/positions/.test(x)).length);assert.equal(fast.requests.filter(x=>x.includes('/prices')).length,2);assert(!fast.requests.some(x=>/historical/.test(x)));});

test('explicit Rust transport opt-in chooses native host without another account client',async()=>{const r=await run(null,{dipEnabled:true,coupledBulkPaper:true,coupledRust:true,retiredNativePrices:true,maxCycles:1});assert.equal(r.process.exitCode,0);assert(r.publications.some(x=>x.kind==='coupled_rust_open'));const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.coupledRustTransport.mode,'original_coupled_wallet_rust_transport_v1');});
test('invalid Rust transport does not silently fall back or claim old liabilities flat',async()=>{const r=await run(null,{dipEnabled:true,coupledBulkPaper:true,coupledRustInvalid:true,retiredNativePrices:true,profitSprint:true,probabilityHeld:['195'],maxCycles:1});assert(!r.publications.some(x=>x.kind==='coupled_rust_open'));const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.coupledBulk.liabilitiesUnverified,true);assert.equal(stats.runtime.coupledBulk.accountingVerified,false);});

test('dual NO pair uses adjacent existing slots beside protected195/356 on the same account queue',async()=>{const r=await run(null,{dipEnabled:true,officialSync:true,nativeCache:true,probabilityHeld:['195','356'],dualNoInCollector:true,retiredNativePrices:true,requestMs:1000,maxCycles:2});assert.equal(r.process.exitCode,0);for(const cycle of r.commits.filter(x=>x.event.type==='cycle')){const books=[...cycle.event.sourceSnapshots,...cycle.event.supplementalDepth],ids=books.map(x=>x.marketId||x.endpoint.match(/markets\/(\d+)/)[1]);assert.equal(books.length,4);assert(ids.includes('195'));assert(ids.includes('356'));assert(ids.includes('381'));assert(ids.includes('382'));assert.equal(Math.abs(ids.indexOf('381')-ids.indexOf('382')),1);}assert(r.publications.filter(x=>x.kind==='dual_no_book').length>=4);assert(r.publications.filter(x=>x.kind==='dual_no_book').every(x=>x.accountEligible&&Date.parse(x.positionsAt)<=Date.parse(x.at)&&Date.parse(x.ordersAt)<=Date.parse(x.at)));assert(r.requests.filter(x=>x.includes('/portfolio/positions')).length>=2);});
test('dual NO cached pair clears across account outage and final STOP boundary',async()=>{const r=await run('dual-no-outage-wait',{dipEnabled:true,officialSync:true,dualNoInCollector:true,dualNoV2InCollector:true,retiredNativePrices:true,probabilityHeld:['195','356'],requestMs:1000,maxCycles:3});assert.equal(r.process.exitCode,0);assert(r.publications.some(x=>x.kind==='dual_no_book'));const invalid=r.publications.findIndex(x=>x.kind==='invalidation'&&x.reason==='ACCOUNT_OUTAGE');assert(invalid>=0);assert(r.publications.slice(invalid+1).some(x=>x.kind==='dual_no_boundary'&&x.reason==='invalidate'));assert(r.publications.some(x=>x.kind==='closed'&&x.reason==='STOP'));assert(r.publications.some(x=>x.kind==='dual_no_boundary'&&x.reason==='close'));assert(r.publications.some(x=>x.kind==='dual_no_close'&&x.pendingPair===false));assert(r.publications.slice(invalid+1).some(x=>x.kind==='dual_no_v2_boundary'&&x.reason==='invalidate'));assert(r.publications.some(x=>x.kind==='dual_no_v2_boundary'&&x.reason==='close'));assert(r.publications.some(x=>x.kind==='dual_no_v2_close'&&x.pendingPair===false));});
test('dual NO durability failure stops collection and retains known inventory status',async()=>{const r=await run(null,{dipEnabled:true,officialSync:true,dualNoInCollector:true,dualNoFailure:true,retiredNativePrices:true,probabilityHeld:['195','356'],maxCycles:2});assert.equal(r.process.exitCode,1);assert.equal(r.publications.filter(x=>x.kind==='dual_no_book').length,1);const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.status,'failed');assert.equal(stats.runtime.dualNoPaper.positions[0].cost,'0.450000');assert(r.publications.some(x=>x.kind==='closed'&&x.reason==='COLLECTOR_FAILURE'));assert(r.publications.some(x=>x.kind==='dual_no_boundary'&&x.reason==='close'));});

test('verified expired-flat retirements bypass original bank/archive/journal factories and preserve accounting status',async()=>{const r=await run(null,{dipEnabled:true,retiredFlatLegacy:true,retiredNativePrices:true,maxCycles:1});assert.equal(r.process.exitCode,0);assert(!r.publications.some(x=>x.kind.startsWith('legacy_')));const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);for(const key of ['noMaker','baalPair']){assert.equal(stats.runtime[key].status,'retired');assert.equal(stats.runtime[key].accountingPreserved,true);assert.equal(stats.runtime[key].retirementReason,'expired_and_verified_flat');}});
test('v1 and v2 dual-NO cohorts share pair reads and account receipts but retain separate attributed wallets',async()=>{const options={dipEnabled:true,officialSync:true,nativeCache:true,probabilityHeld:['195','356'],dualNoInCollector:true,retiredNativePrices:true,requestMs:1000,maxCycles:2},baseline=await run(null,options),r=await run(null,{...options,dualNoV2InCollector:true});assert.equal(r.process.exitCode,0);assert.deepEqual(r.requests,baseline.requests);assert.deepEqual(r.publications.filter(x=>x.kind==='dual_no_v2_book').map(x=>[x.marketId,x.at,x.accountEligible]),r.publications.filter(x=>x.kind==='dual_no_book').map(x=>[x.marketId,x.at,x.accountEligible]));const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.dualNoPaper.positions[0].cost,'0.450000');assert.equal(stats.runtime.dualNoPaperV2.positions[0].cost,'0.440000');assert(r.publications.some(x=>x.kind==='dual_no_close'&&!x.pendingPair));assert(r.publications.some(x=>x.kind==='dual_no_v2_close'&&!x.pendingPair));});
test('v2 fatal durability failure stops next pair read and preserves both cohort inventory statuses',async()=>{const r=await run(null,{dipEnabled:true,officialSync:true,dualNoInCollector:true,dualNoV2InCollector:true,dualNoV2Failure:true,retiredNativePrices:true,probabilityHeld:['195','356'],requestMs:1000,maxCycles:2});assert.equal(r.process.exitCode,1);assert.equal(r.publications.filter(x=>x.kind==='dual_no_v2_book').length,1);assert.equal(r.publications.filter(x=>x.kind==='dual_no_book').length,1);const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.status,'failed');assert.equal(stats.runtime.dualNoPaper.positions[0].cost,'0.450000');assert.equal(stats.runtime.dualNoPaperV2.positions[0].cost,'0.440000');assert(r.publications.some(x=>x.kind==='dual_no_boundary'&&x.reason==='close'));assert(r.publications.some(x=>x.kind==='dual_no_v2_boundary'&&x.reason==='close'));});

test('fundamental consumer failure preserves attributed holding, halts further reads and closes owned leases',async()=>{for(const settlementStatusFailure of [false,true]){const r=await run(null,{dipEnabled:true,officialSync:true,nativeCache:true,probabilityHeld:['195','356'],settlementInCollector:true,settlementConsumeFailure:true,settlementStatusFailure,retiredNativePrices:true,requestMs:1000,maxCycles:3});assert.equal(r.process.exitCode,1);const failures=r.publications.filter(x=>x.kind==='fundamental_consumer_failure');assert.equal(failures.length,1);assert.equal(r.requests.length,failures[0].requestCount);assert.equal(r.requests.filter(x=>x.includes('/orderbook?')).length,1);const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);assert.equal(stats.runtime.status,'failed');assert.equal(stats.runtime.settlementPaper.wallets[1000].positions[0].cost,'0.295000');assert.equal(stats.runtime.settlementPaper.bankHash,'f'.repeat(64));assert.equal(stats.runtime.settlementPaper.executionAuthority,false);assert(r.publications.some(x=>x.kind==='settlement_value_close'));assert(r.publications.some(x=>x.kind==='account_client_closed'));assert(r.publications.some(x=>x.kind==='collector_lease_released'));}});
