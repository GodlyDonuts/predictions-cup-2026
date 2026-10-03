import fs from 'node:fs';import path from 'node:path';import {digest} from './model.mjs';
import {validateUniverse} from './breadth.mjs';
export const EDGE_OBSERVATION_MS=420000;
const need=(ok)=>{if(!ok)throw Error('INVALID_EDGE_OBSERVATION');};
export function readEdgeObservationFocus(runtimeDir,universe,now=Date.now()){
 try{
  if(fs.existsSync(path.join(runtimeDir,'EXECUTABLE_EDGE_STOP')))return null;
  const manifest=JSON.parse(fs.readFileSync(path.join(runtimeDir,'executable-edge-active.json')));
  need(/^[a-z][a-z0-9-]{1,63}$/.test(manifest.cohort)&&/^[a-f0-9]{64}$/.test(manifest.bankHash));
  const dir=path.join(runtimeDir,'executable-edge-cohorts',manifest.cohort);if(fs.existsSync(path.join(dir,'STOP')))return null;
  const bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json'))),stats=JSON.parse(fs.readFileSync(path.join(dir,'stats.json')));
  need(digest(bank)===manifest.bankHash&&bank.universe.tournamentId===universe.tournamentId&&stats.bankHash===manifest.bankHash&&stats.cohort===manifest.cohort&&stats.mode==='execution_aware_online_paper'&&stats.failed===false&&stats.sourceClosed===false&&stats.networkRequests===0&&stats.competitionOrders===0);
  const lock=JSON.parse(fs.readFileSync(path.join(dir,'worker.lock')));
  need(Number.isSafeInteger(lock.pid)&&lock.pid>0&&typeof lock.id==='string'&&lock.id.length>0&&now>=Date.parse(stats.updatedAt)&&now-Date.parse(stats.updatedAt)<=20000);process.kill(lock.pid,0);
  const canonical=validateUniverse(bank.universe,universe.tournamentId);
  need(canonical.hash===universe.hash&&digest(canonical.exchanges)===digest(universe.exchanges));return {bankHash:manifest.bankHash};
 }catch{return null;}
}
// Fixed observation blocks change read allocation only, never execution leads.
// All protected risk, basket and comparison reservations outrank paper sampling.
export function planEdgeObservation(plan,state,universe,{focus,now,excludedMarketIds=[]}={}){
 if(!focus||plan.comparisonSampling||plan.basketMeasurement?.marketIds?.length||plan.paperPackageFocus?.marketIds?.length)return {plan,state};
 need(Number.isSafeInteger(now)&&now>=0&&/^[a-f0-9]{64}$/.test(focus.bankHash));
 const valid=new Set(universe.exchanges.map(x=>x.marketId)),excluded=new Set(excludedMarketIds);
 const slots=plan.reads.map((read,index)=>({read,index})).filter(({read})=>['discovery','comparison'].includes(read.reason)&&!excluded.has(read.marketId));
 if(slots.length<2)return {plan,state};
 const saved=state.edgeObservation;
 if(saved){need(/^[a-f0-9]{64}$/.test(saved.bankHash)&&Number.isSafeInteger(saved.startedAt)&&Number.isSafeInteger(saved.until)&&saved.startedAt<=now&&saved.until===saved.startedAt+EDGE_OBSERVATION_MS&&Array.isArray(saved.marketIds)&&saved.marketIds.length===2&&new Set(saved.marketIds).size===2&&saved.marketIds.every(id=>valid.has(id)));}
 let block=saved;
 if(!block||block.bankHash!==focus.bankHash||now>=block.until||block.marketIds.some(id=>excluded.has(id))){
  const choices=slots.filter(({read})=>read.reason==='discovery'&&valid.has(read.marketId));if(choices.length<2)return {plan,state};
  block={bankHash:focus.bankHash,startedAt:now,until:now+EDGE_OBSERVATION_MS,marketIds:choices.slice(0,2).map(({read})=>read.marketId)};
 }
 const present=new Set(plan.reads.map(x=>x.marketId)),missing=block.marketIds.filter(id=>!present.has(id)),replaceable=slots.filter(({read})=>!block.marketIds.includes(read.marketId)).sort((a,b)=>Number(a.read.reason!=='discovery')-Number(b.read.reason!=='discovery'));
 if(missing.length>replaceable.length)return {plan,state};
 const reads=plan.reads.map(x=>({...x}));for(let i=0;i<missing.length;i++)reads[replaceable[i].index]={marketId:missing[i],reason:'edge_observation',path:'supplementalDepth'};
 const observed=new Set(block.marketIds),admitted=new Set(reads.map(x=>x.marketId));
 return {state:{...state,edgeObservation:block},plan:{...plan,reads,entryMarketIds:plan.entryMarketIds.filter(id=>admitted.has(id)&&!observed.has(id)),edgeObservation:{...block,requestsAdded:0,executionAuthority:false}}};
}
