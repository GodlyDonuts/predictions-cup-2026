import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {digest,initialState,normalizeBook,stepForecast,report,invalidate,assertState} from './model.mjs';
import {atomicJson,lock} from './runtime.mjs';
import {features,predict} from './ml-bank.mjs';
import {amount,fee,TICK} from '../paper/fixed.mjs';

export function verifySource(raw){
 if(!raw||!raw.endsWith('\n'))return null;
 let previous='',clock=-Infinity;const records=[];
 for(const [i,line]of raw.trimEnd().split('\n').entries()){
  let r;try{r=JSON.parse(line);}catch{throw Error('Source JSON invalid');}
  const {hash,...body}=r;
  if(r.sequence!==i+1||r.previous!==previous||hash!==digest(body))throw Error('Source integrity failure');
  const next=Date.parse(r.event?.clockAt||r.at);if(!Number.isFinite(next)||next<clock)throw Error('Source clock rollback');clock=next;
  previous=hash;records.push(r);
 }
 return records;
}

export function advanceShadow(input,records,bank){
 const state=structuredClone(input),c=bank.config;
 if(state.bankHash!==digest(bank))throw Error('Frozen model bank changed');
 if(state.sourceSequence>records.length||(state.sourceSequence&&records[state.sourceSequence-1].hash!==state.sourceHash))throw Error('Source prefix changed on resume');
 for(const r of records){
  if(r.sequence<=state.sourceSequence)continue;
  const e=r.event;state.sourceSequence=r.sequence;state.sourceHash=r.hash;state.captureThrough=e.clockAt||r.at;
  if(Date.parse(state.captureThrough)<=Date.parse(bank.trainingCaptureThrough))continue;
  if(e.type==='account_read_outage'){for(const wallet of Object.values(state.wallets))invalidate(wallet,null,state.captureThrough,'account_outage');state.histories={};continue;}
  if(e.type!=='cycle')continue;
  for(const snap of e.sourceSnapshots){
   if(snap.skipped){for(const wallet of Object.values(state.wallets))invalidate(wallet,snap.marketId,snap.at||state.captureThrough,snap.skipped);state.histories={};continue;}
   const match=/^\/markets\/(\d+)\/orderbook\?/.exec(snap.endpoint||'');if(!match||!c.marketIds.includes(match[1])||Date.parse(snap.requestStartedAt)<=Date.parse(bank.trainingCaptureThrough))continue;
   let b;try{b=normalizeBook(snap.data,match[1],c.tournamentId,snap.receivedAt,snap.requestStartedAt,c.maxDataAgeMs,new Set(e.accountExclusions));}catch{state.histories={};for(const w of Object.values(state.wallets))invalidate(w,match[1],snap.receivedAt,'invalid_book');continue;}
   let history=state.histories[b.instrument]||[];
   if(history.length&&Date.parse(b.receivedAt)-Date.parse(history.at(-1).at)>120000)history=[];
   const x=features(b,history),mid=x[11];let advanced=false;
   for(const model of bank.selected)for(const margin of [.005,.01,.015]){
    const id=model.id+'-edge'+margin;
    const w=state.wallets[id]??initialState(c,id,bank.trainingCaptureThrough);
    const same=w.books[b.instrument]?.versionKey===b.versionKey;
    const exitValue=history.length>=6?mid+predict(model,x):null;
    const ask=amount(b.asks[0].price)+TICK*BigInt(c.slippageTicks),entryCost=Number(ask+fee(ask,1,{feeBps:c.feeBps,feePerShare:'0.000000'}))/1e6;
    const netEdge=exitValue===null?null:exitValue-entryCost;
    state.signalMetrics??={};const metrics=state.signalMetrics[id]??={observations:0,warmup:0,invalidForecast:0,edgeBelowMargin:0,edgePass:0,zeroTopAskCapacity:0};
    if(!same){metrics.observations++;if(exitValue===null)metrics.warmup++;else if(!Number.isFinite(exitValue)||exitValue<0||exitValue>1)metrics.invalidForecast++;else if(netEdge<margin)metrics.edgeBelowMargin++;else metrics.edgePass++;if(Math.floor(b.asks[0].quantity*c.depthFractionBps/10000)<1)metrics.zeroTopAskCapacity++;}
    state.wallets[id]=stepForecast(w,b,c,{exitValue,horizonMs:model.horizon*1000,minimumEdge:margin});
    state.predictions[id]={at:b.receivedAt,exitValue,netEdge,entryCost,modelRmse:model.rmse,margin,instrument:b.instrument,warmupRemaining:Math.max(0,6-history.length)};
    if(!same)advanced=true;
   }
   if(advanced)state.histories[b.instrument]=[...history,{at:b.receivedAt,mid}].slice(-30);
  }
 }
 return state;
}
export function shadowReport(s,bank,now){
 const candidates=Object.entries(s.wallets).map(([id,w])=>{assertState(w,bank.config);const r=report(w,bank.config,now).strategies.mean_reversion;return {id,cash:r.cash,realizedPnl:r.realizedPnl,referencePnl:r.referencePnl,liquidationPnl:r.liquidationPnl,intents:r.intents,fills:r.fills,closedTrades:r.closedTrades,fees:r.fees,positions:r.positions.map(p=>({instrument:p.instrument,quantity:p.quantity,unsoldShares:p.unsoldShares})),prediction:s.predictions[id],signalMetrics:s.signalMetrics?.[id]||null};});
 return {mode:'prospective_ml_paper',updatedAt:now,bankHash:s.bankHash,trainingCaptureThrough:bank.trainingCaptureThrough,captureThrough:s.captureThrough,modelsAttempted:bank.candidatesAttempted,modelsTrained:bank.modelsTrained,modelRejections:bank.rejected.length,candidateWallets:candidates.length,sourceSequence:s.sourceSequence,candidates,competitionOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:bank.limitations};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1]){
 const dir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url)));
 const release=lock(path.join(dir,'ml-shadow.lock'));process.on('exit',release);
 const bank=JSON.parse(fs.readFileSync(path.join(dir,'ml-bank.json'),'utf8'));
 const stateFile=path.join(dir,'ml-shadow-state.json');let s=fs.existsSync(stateFile)?JSON.parse(fs.readFileSync(stateFile,'utf8')):{schemaVersion:1,bankHash:digest(bank),sourceSequence:0,sourceHash:null,wallets:{},histories:{},predictions:{}};
 const once=process.argv.includes('--once');let failed=false,partialReads=0;
 const stopped=()=>fs.existsSync(path.join(dir,'ML_STOP'))||fs.existsSync(path.join(dir,'STOP'))||fs.existsSync(path.join(workspace,'STOP'));
 do{
  if(stopped())break;
  try{
   const records=verifySource(fs.readFileSync(path.join(dir,'journal.jsonl'),'utf8'));
   if(!records){if(++partialReads>=4)throw Error('Persistent incomplete source journal');atomicJson(path.join(dir,'ml-stats.json'),{...shadowReport(s,bank,new Date().toISOString()),status:'awaiting_committed_capture'});}
   else{partialReads=0;s=advanceShadow(s,records,bank);atomicJson(stateFile,s);atomicJson(path.join(dir,'ml-stats.json'),{...shadowReport(s,bank,new Date().toISOString()),status:'running'});}
  }catch(e){atomicJson(path.join(dir,'ml-stats.json'),{...shadowReport(s,bank,new Date().toISOString()),status:'failed',reason:e.message});failed=true;break;}
  if(!once)for(let n=0;n<75;n++){if(stopped())break;await new Promise(r=>setTimeout(r,200));}
 }while(!once);
 if(!failed&&!once)atomicJson(path.join(dir,'ml-stats.json'),{...shadowReport(s,bank,new Date().toISOString()),status:'stopped'});
 if(failed)process.exitCode=1;
}
