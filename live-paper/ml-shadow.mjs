import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {digest,initialState,normalizeBook,stepForecast,report,invalidate,assertState} from './model.mjs';
import {atomicJson,lock} from './runtime.mjs';
import {features,predict,validateTrainingScope} from './ml-bank.mjs';
import {amount,fee,TICK} from '../paper/fixed.mjs';
import {scopedSnapshots} from './depth-scope.mjs';
import {historyPolicy,resetHistory} from './history-policy.mjs';
import {updateContext,contextFeatures} from './breadth-context.mjs';
import {ROUND_TRIP,targetPolicy} from './roundtrip-target.mjs';

export function verifySource(raw){
 if(!raw||!raw.endsWith('\n'))return null;
 let previous='',clock=-Infinity;const records=[],prefixHashes=[],prefix=createHash('sha256');
 for(const [i,line]of raw.slice(0,-1).split('\n').entries()){
  let r;try{r=JSON.parse(line);}catch{throw Error('Source JSON invalid');}
  const {hash,...body}=r;
  if(r.sequence!==i+1||r.previous!==previous||hash!==digest(body))throw Error('Source integrity failure');
  const next=Date.parse(r.event?.clockAt||r.at);if(!Number.isFinite(next)||next<clock)throw Error('Source clock rollback');clock=next;
  prefix.update(line+'\n');prefixHashes.push(prefix.copy().digest('hex'));previous=hash;records.push(r);
 }
 Object.defineProperty(records,'prefixHashes',{value:prefixHashes});return records;
}

export function requireSourceMetadata(bank){
 targetPolicy(bank.targetPolicy);validateTrainingScope(bank.trainingScope,bank.requireInstrumentCoverage,bank.targetPolicy);
 if(!Number.isSafeInteger(bank.sourceRecords)||bank.sourceRecords<1||typeof bank.sourceHash!=='string'||!/^[a-f0-9]{64}$/.test(bank.sourceHash)||!Number.isFinite(Date.parse(bank.trainingCaptureThrough)))throw Error('Missing or invalid frozen training source metadata');
}
export function bindTrainingSource(records,bank,{allowUnboundFixture=false}={}){
 if(allowUnboundFixture&&bank.sourceHash===undefined&&bank.sourceRecords===undefined)return null;
 requireSourceMetadata(bank);
 const n=bank.sourceRecords;
 if(!Number.isSafeInteger(n)||n<1||n>records.length||typeof bank.sourceHash!=='string'||!/^[a-f0-9]{64}$/.test(bank.sourceHash))throw Error('Invalid frozen training source');
 const hash=records.prefixHashes?.[n-1]||createHash('sha256').update(records.slice(0,n).map(r=>JSON.stringify(r)).join('\n')+'\n').digest('hex');
 if(hash!==bank.sourceHash||Date.parse(records[n-1].event?.clockAt||records[n-1].at)!==Date.parse(bank.trainingCaptureThrough))throw Error('Frozen training source mismatch');
 return {sourceRecords:n,sourceHash:hash,lastRecordHash:records[n-1].hash};
}

export function advanceShadow(input,records,bank,{allowUnboundFixture=false}={}){
 const state=structuredClone(input),c=bank.config;
 const policy=historyPolicy(bank.historyPolicy);targetPolicy(bank.targetPolicy);validateTrainingScope(bank.trainingScope,bank.requireInstrumentCoverage,bank.targetPolicy);
 if(state.bankHash!==digest(bank))throw Error('Frozen model bank changed');
 const binding=bindTrainingSource(records,bank,{allowUnboundFixture});
 if(state.sourceBinding&&digest(state.sourceBinding)!==digest(binding))throw Error('Frozen training binding changed');
 if(binding)state.sourceBinding=binding;
 if(state.sourceSequence>records.length||(state.sourceSequence&&records[state.sourceSequence-1].hash!==state.sourceHash))throw Error('Source prefix changed on resume');
 for(const r of records){
  if(r.sequence<=state.sourceSequence)continue;
  const e=r.event;state.sourceSequence=r.sequence;state.sourceHash=r.hash;state.captureThrough=e.clockAt||r.at;
  if(Date.parse(state.captureThrough)<=Date.parse(bank.trainingCaptureThrough))continue;
  if(e.type==='account_read_outage'){for(const wallet of Object.values(state.wallets))invalidate(wallet,null,state.captureThrough,'account_outage');state.histories={};continue;}
  if(e.type!=='cycle')continue;
  for(const snap of scopedSnapshots(e,c)){
   if(snap.skipped){for(const wallet of Object.values(state.wallets))invalidate(wallet,snap.marketId,snap.at||state.captureThrough,snap.skipped);resetHistory(state.histories,snap.marketId,policy);continue;}
   const match=/^\/markets\/(\d+)\/orderbook\?/.exec(snap.endpoint||'');if(!match||!c.marketIds.includes(match[1])||Date.parse(snap.requestStartedAt)<=Date.parse(bank.trainingCaptureThrough))continue;
   let b;try{b=normalizeBook(snap.data,match[1],c.tournamentId,snap.receivedAt,snap.requestStartedAt,c.maxDataAgeMs,new Set(e.accountExclusions));}catch{resetHistory(state.histories,match[1],policy);for(const w of Object.values(state.wallets))invalidate(w,match[1],snap.receivedAt,'invalid_book');continue;}
   let history=state.histories[b.instrument]||[];
   if(history.length&&Date.parse(b.receivedAt)-Date.parse(history.at(-1).at)>120000)history=[];
   const base=features(b,history),x=bank.breadthContext?[...base,...contextFeatures(state.breadth,b,c.tournamentId)]:base,mid=x[11];let advanced=false;
   for(const model of bank.selected)for(const margin of [.005,.01,.015]){
    const id=model.id+'-edge'+margin;
    if(bank.requireInstrumentCoverage){
     const support=model.coverage?.find(x=>x.marketId===b.marketId);
     if(!support?.eligible||!support.trainingInstruments?.includes(b.instrument)){
      state.unsupportedInstruments??={};state.unsupportedInstruments[id+':'+b.instrument]={modelId:model.id,marketId:b.marketId,instrument:b.instrument,at:b.receivedAt,reason:'not_in_frozen_training_support'};
      const old=state.wallets[id];if(old){invalidate(old,b.marketId,b.receivedAt,'unsupported_instrument');delete old.strategies.mean_reversion.pending[b.instrument];}
      continue;
     }
    }
    const w=state.wallets[id]??initialState(c,id,bank.trainingCaptureThrough);
    const same=w.books[b.instrument]?.versionKey===b.versionKey;
    const ask=amount(b.asks[0].price)+TICK*BigInt(c.slippageTicks),entryCost=Number(ask+fee(ask,1,{feeBps:c.feeBps,feePerShare:'0.000000'}))/1e6;
    const roundtrip=targetPolicy(bank.targetPolicy)===ROUND_TRIP,forecast=history.length>=6?predict(model,x):null;
    const exitValue=roundtrip||forecast===null?null:mid+forecast,netEdge=roundtrip?forecast:exitValue===null?null:exitValue-entryCost;
    state.signalMetrics??={};const metrics=state.signalMetrics[id]??={observations:0,warmup:0,invalidForecast:0,edgeBelowMargin:0,edgePass:0,zeroTopAskCapacity:0};
    if(!same){metrics.observations++;if(history.length<6)metrics.warmup++;else if(roundtrip?!Number.isFinite(netEdge):!Number.isFinite(exitValue)||exitValue<0||exitValue>1)metrics.invalidForecast++;else if(netEdge<margin)metrics.edgeBelowMargin++;else metrics.edgePass++;if(Math.floor(b.asks[0].quantity*c.depthFractionBps/10000)<1)metrics.zeroTopAskCapacity++;}
    state.wallets[id]=stepForecast(w,b,c,{exitValue,...(roundtrip?{netEdge}:{}),horizonMs:model.horizon*1000,minimumEdge:margin});
    state.predictions[id]={at:b.receivedAt,exitValue,netEdge,entryCost,modelRmse:model.rmse,margin,instrument:b.instrument,warmupRemaining:Math.max(0,6-history.length)};
    if(!same)advanced=true;
   }
   if(advanced)state.histories[b.instrument]=[...history,{at:b.receivedAt,mid,marketId:b.marketId}].slice(-30);
  }
  if(bank.breadthContext)state.breadth=updateContext(state.breadth,e.bulkPrices,state.captureThrough);
 }
 return state;
}
export function shadowReport(s,bank,now){
 const candidates=Object.entries(s.wallets).map(([id,w])=>{assertState(w,bank.config);const r=report(w,bank.config,now).strategies.mean_reversion;return {id,cash:r.cash,realizedPnl:r.realizedPnl,referencePnl:r.referencePnl,liquidationPnl:r.liquidationPnl,intents:r.intents,fills:r.fills,closedTrades:r.closedTrades,fees:r.fees,positions:r.positions.map(p=>({instrument:p.instrument,quantity:p.quantity,unsoldShares:p.unsoldShares})),prediction:s.predictions[id],signalMetrics:s.signalMetrics?.[id]||null};});
 return {mode:'prospective_ml_paper',updatedAt:now,bankHash:s.bankHash,sourceBinding:s.sourceBinding||null,unsupportedInstruments:Object.values(s.unsupportedInstruments||{}),targetPolicy:bank.targetPolicy||'legacy_decision_time_bid_forecast_not_executable_roundtrip_target',trainingCaptureThrough:bank.trainingCaptureThrough,captureThrough:s.captureThrough,modelsAttempted:bank.candidatesAttempted,modelsTrained:bank.modelsTrained,modelRejections:bank.rejected.length,candidateWallets:candidates.length,sourceSequence:s.sourceSequence,candidates,competitionOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:bank.limitations};
}
export function combineReports(base,cohorts){
 const reports=[{name:'ridge-original',report:base},...cohorts];
 return {...base,status:base.status==='failed'?'failed':reports.some(x=>x.report.status==='failed')?'degraded':base.status,modelsAttempted:reports.reduce((n,x)=>n+x.report.modelsAttempted,0),modelsTrained:reports.reduce((n,x)=>n+x.report.modelsTrained,0),modelRejections:reports.reduce((n,x)=>n+(x.report.modelRejections||0),0),candidateWallets:reports.reduce((n,x)=>n+x.report.candidateWallets,0),cohorts:reports.map(({name,report:r})=>({name,status:r.status,bankHash:r.bankHash,sourceBinding:r.sourceBinding,targetPolicy:r.targetPolicy,trainingCaptureThrough:r.trainingCaptureThrough,modelsTrained:r.modelsTrained,candidateWallets:r.candidateWallets})),candidates:reports.flatMap(({name,report:r})=>r.candidates.map(c=>({...c,id:name+'/'+c.id,cohort:name})))};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url)));
 const release=lock(path.join(dir,'ml-shadow.lock'));process.on('exit',release);
 const bank=JSON.parse(fs.readFileSync(path.join(dir,'ml-bank.json'),'utf8'));
 requireSourceMetadata(bank);
 const stateFile=path.join(dir,'ml-shadow-state.json');let s=fs.existsSync(stateFile)?JSON.parse(fs.readFileSync(stateFile,'utf8')):{schemaVersion:1,bankHash:digest(bank),sourceSequence:0,sourceHash:null,wallets:{},histories:{},predictions:{}};
 const once=process.argv.includes('--once');let failed=false,partialReads=0;
 const cohorts=new Map(),cohortRoot=path.join(dir,'ml-cohorts');
 function discover(){
  if(!fs.existsSync(cohortRoot))return;
  for(const entry of fs.readdirSync(cohortRoot,{withFileTypes:true})){
   if(!entry.isDirectory()||!/^([a-z0-9][a-z0-9-]{0,63})$/.test(entry.name)||cohorts.has(entry.name))continue;
   const root=path.join(cohortRoot,entry.name),file=path.join(root,'ml-bank.json');if(!fs.existsSync(file))continue;
   const b=JSON.parse(fs.readFileSync(file,'utf8')),statePath=path.join(root,'ml-shadow-state.json');
   requireSourceMetadata(b);
   const state=fs.existsSync(statePath)?JSON.parse(fs.readFileSync(statePath,'utf8')):{schemaVersion:1,bankHash:digest(b),sourceSequence:0,sourceHash:null,wallets:{},histories:{},predictions:{}};
   cohorts.set(entry.name,{root,bank:b,state,status:'running'});
  }
 }
 function publish(status,reason){
  const now=new Date().toISOString(),extra=[];
  for(const [name,cohort]of cohorts){const r={...shadowReport(cohort.state,cohort.bank,now),status:status==='stopped'?'stopped':cohort.status,reason:cohort.reason};atomicJson(path.join(cohort.root,'ml-stats.json'),r);extra.push({name,report:r});}
  atomicJson(path.join(dir,'ml-stats.json'),combineReports({...shadowReport(s,bank,now),status,reason},extra));
 }
 const stopped=()=>fs.existsSync(path.join(dir,'ML_STOP'))||fs.existsSync(path.join(dir,'STOP'))||fs.existsSync(path.join(workspace,'STOP'));
 do{
  if(stopped())break;
  try{
   const records=verifySource(fs.readFileSync(path.join(dir,'journal.jsonl'),'utf8'));
   discover();
   if(!records){if(++partialReads>=4)throw Error('Persistent incomplete source journal');publish('awaiting_committed_capture');}
   else{partialReads=0;s=advanceShadow(s,records,bank);atomicJson(stateFile,s);
    for(const cohort of cohorts.values()){if(cohort.status==='failed')continue;try{cohort.state=advanceShadow(cohort.state,records,cohort.bank);atomicJson(path.join(cohort.root,'ml-shadow-state.json'),cohort.state);}catch{cohort.status='failed';cohort.reason='Cohort verification failed';}}
    publish('running');}
  }catch(e){publish('failed',e.message);failed=true;break;}
  if(!once)for(let n=0;n<75;n++){if(stopped())break;await new Promise(r=>setTimeout(r,200));}
 }while(!once);
 if(!failed&&!once)publish('stopped');
 if(failed)process.exitCode=1;
}
