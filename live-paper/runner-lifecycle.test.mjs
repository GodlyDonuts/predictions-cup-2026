import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
import * as scheduler from './book-scheduler.mjs';
import {planComparisonSampling,recordComparisonAttempt} from './comparison-sampling.mjs';
import {collectionInputs,shockCollectionRisk} from './collection-inputs.mjs';
import {basketCandidates} from './basket-inputs.mjs';
import {planBasketBooks} from './basket-book-plan.mjs';
import {basketCodeIdentity} from './basket-watch.mjs';
import {createBasketRelationshipReader} from './basket-relations.mjs';
import {createContractEvidenceReader} from './contract-evidence.mjs';
import {focusPairedPackageReads} from './paired-package-read-plan.mjs';
import {timestamp} from '../feed/contract.mjs';
import {networkInterval,validateNetworkBudget} from './network-budget.mjs';
import {collectionCycleWait} from './cycle-wait.mjs';
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
async function run(stopAt,{dipEnabled=false,officialSync=false,invalidBasket=false,basketEnabled=false,contractEvidenceEnabled=false,samplingEnabled=false,riskUnavailable=false,comparisonHeld=[],comparisonPending=[],maxCycles=2,requestMs=6000,officialRiskSchedule=null,marketSweepEnabled=false,historyCandidates=[],paperPackageFocus=false}={}){
  let startupFailures=0;
  let now=Date.parse('2026-10-02T02:00:00Z'),cycle=0,stopped=false,triggered=false;
  const commits=[],memory=new Map(),listeners={},requests=[],publications=[],claimWrites=[],pairReadClaims=[];
  const fixtureStart=now,stamp=offset=>new Date(fixtureStart+offset).toISOString();
  const pricePublications=[];
  const samplingWrites=[],samplingRequests=[],pauses=[],officialCalls=[];
  if(officialRiskSchedule)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','competition-activation.json'),JSON.stringify({enabled:true}));
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
    close(){}
    async dispatch(spec){
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
      if(cycle===0&&stopAt==='startup-stop'){stop();throw Error('Stopped');}
      if(cycle===0)now+=delay;
      if((cycle===1&&stopAt==='cycle-wait')||(cycle===2&&stopAt==='outage-wait')){stop();throw Error('Stopped');}
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
        if(cycle===2&&stopAt==='outage-wait'){const e=Error('Synthetic connection failure');e.recoverable=true;throw e;}
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
  const process={argv:['node',path.join(root,'runner.mjs'),'--cycles',String(maxCycles)],env:{LOCALAPPDATA:'/synthetic'},pid:12345,on:(name,f)=>listeners[name]=f,exit(){throw Error('Unexpected exit');},exitCode:0};
  const atomicJson=(p,v)=>{
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
  const bindings={readMarketHistoryInputs:()=>({available:historyCandidates.length>0,candidateMarketIds:historyCandidates}),collectMarketSweep,loadMarketSweep:()=>marketSweepEnabled?{cycleIntervalMs:10000}:null,createMarketPricePublication:()=>({publish(snapshot){if(stopAt==='price-recording-failure')throw Error('Synthetic price durability failure');pricePublications.push(snapshot);},status:()=>({sequence:pricePublications.length})}),collectionCycleWait,CompetitionController:class{constructor(){this.state={intents:{}};this.transport={authorizeRequest:()=>false};}managedMarketIds(){return officialRiskSchedule?.[Math.max(0,cycle-1)]||[];}async beforeBooks(){officialCalls.push({kind:"reconcile",cycle});}async onBook(){officialCalls.push({kind:"book",cycle});}failure(e){throw e;}close(){}},...model,...breadth,...depth,...scheduler,planComparisonSampling,recordComparisonAttempt,createComparisonRiskSource:()=>({poll:()=>({configured:samplingEnabled,available:!riskUnavailable,cohort:'fixture',bankHash:'a'.repeat(64),heldMarketIds:comparisonHeld,pendingMarketIds:comparisonPending}),close(){}}),collectionInputs,shockCollectionRisk,networkInterval,loadNetworkBudget:file=>mockfs.existsSync(file)?validateNetworkBudget(JSON.parse(mockfs.readFileSync(file))):null,connectTournament,validateTiming,timingSummary,historyEndpoint,normalizeHistory,validateDipPolicy,workerHealth:(report,at=now)=>workerHealth(report,at),baselineView,accountPositions,accountOrders,accountFills,accountPnl,accountView,fs:mockfs,path,http:{createServer:()=>({once(){},listen(p,h,f){f();},close(){}})},randomUUID,fileURLToPath,
    loadKey:()=> 'SYNTHETIC',Journal,ReadClient,lock:()=>()=>{},atomicJson,readPairedPackageFocus:()=>paperPackageFocus?{available:true,needed:true,marketIds:['381','382'],reason:'paper_pending'}:null,focusPairedPackageReads,checkpointCoverage:async(file,value,{canCommit})=>{if(!canCommit())throw Error('COMMIT_BLOCKED');atomicJson(file,value);},durableAtomicJson:atomicJson,exclusionSet,basketCandidates,planBasketBooks,basketCodeIdentity,createBasketRelationshipReader,createContractEvidenceReader,
    createBookPublication({canOperate}){return {publishBook({marketId,response}){if(stopAt==='public-book-recording-failure'&&cycle===2)throw Error('Synthetic public journal failure');if(canOperate())publications.push({kind:'book',marketId,receivedAt:response.receivedAt,at:iso(),requestCount:requests.length});},invalidate({marketId,reason}){publications.push({kind:'invalidation',marketId,reason,at:iso()});},close(reason){publications.push({kind:'closed',reason,at:iso()});}};},
    createBasketFollowupSource({bank}){return {poll(){
      if(stopAt==='followup-source-failure')throw Error('Injected damaged source');
      return [{tournamentId:config.tournamentId,observerBankHash:model.digest(bank),pairId:'b'.repeat(64),sourceSequence:123,sourceHash:'c'.repeat(64),cutoff:stamp(-3600000),marketIds:['381','382'],exchangeIds:['1070','1071'],
        sourceMeasurement:{reason:'paired_depth_measurement',cycle:0,every:4,marketIds:['381','382'],exchangeIds:['1070','1071'],receivedAt:stamp(-10000)},
        observation:{pairId:'b'.repeat(64),at:stamp(1000),status:'apparent_positive_basket',apparentPositive:true,newPairEvidence:true,independentPair:false,positivePersistenceCount:1,eligibleForExecution:false,fills:null,pnl:null,pairReceipts:[stamp(0),stamp(1000)],pairVersions:['10:'+timestamp(stamp(0)),'11:'+timestamp(stamp(1000))],basket:{quantity:5,totalCost:'4.975000',conditionalMargin:'0.025000',marginalUnitCost:'0.995000',depthFractionBps:100}}}];
    },status(){return {status:'running',sourceSequence:123,observerSequence:1};},close(){}};},
    sampleEquity(s,c,at){if(stopAt==='sampling-failure'&&cycle===2){stop();throw Error('Synthetic sample failure');}return model.sampleEquity(s,c,at);},
    process,console:{log(){},error(){}},AbortController,structuredClone,Date:Clock,setInterval:()=>1,clearInterval(){},moduleUrl:new URL('./runner.mjs',import.meta.url).href};
  const body=source.replace(/^import .*;\r?\n/gm,'').replaceAll('import.meta.url','moduleUrl');
  await vm.runInNewContext('(async()=>{'+body+'\n})()',bindings);
  return {pricePublications,commits,process,memory,requests,publications,triggered,claimWrites,pairReadClaims,samplingWrites,samplingRequests,pauses,officialCalls};
}

test('real runner captures four scoped metadata reads once before cycles and grants no order eligibility',async()=>{
  const r=await run(null,{basketEnabled:true,contractEvidenceEnabled:true,maxCycles:2});assert.equal(r.process.exitCode,0);
  const evidence=r.commits.filter(x=>x.event.type==='contract_pair_evidence');assert.equal(evidence.length,1);
  assert.equal(evidence[0].event.evidence.orderEligible,false);
  assert.equal(r.requests.filter(p=>/^\/markets\/(381|382)(\/nodes)?\?tournamentId=/.test(p)).length,4);
  assert.equal(r.commits.filter(x=>x.event.type==='cycle').length,2);
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
 assert.deepEqual(risk.requests,flat.requests,'risk skips idle without changing endpoints or dropping account reconciliation');
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
