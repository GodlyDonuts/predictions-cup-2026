import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {digest} from './model.mjs';import {canonicalJson} from '../execution/ledger.mjs';import {validateUniverse} from './breadth.mjs';
const need=ok=>{if(!ok)throw Error('INVALID_ANCHOR_READ_FOCUS');};
const hash=x=>createHash('sha256').update(canonicalJson(x)).digest('hex');
export const anchorFocusCodeHash=()=>createHash('sha256').update(fs.readFileSync(new URL('./executable-edge-observe.mjs',import.meta.url))).update(fs.readFileSync(new URL('./bid-anchor-read-focus.mjs',import.meta.url))).digest('hex');
export function projectAnchorReadFocus(stats,lock,observerCohort){
 const body={schemaVersion:1,mode:'bid_anchor_read_focus_only',observerCohort,pid:lock.pid,lockId:lock.id,codeHash:anchorFocusCodeHash(),statsHash:hash(stats),networkRequests:0,competitionOrders:0};
 return {...body,projectionHash:hash(body)};
}
export function anchorFocusMarkets(stats,universe){
 const valid=new Set(universe.exchanges.map(x=>x.marketId)),byInstrument=new Map(universe.exchanges.map(x=>[x.exchangeId+':YES',x.marketId])),markets=new Map();
 need(stats.wallets&&Object.keys(stats.wallets).length===2&&stats.wallets['1000']&&stats.wallets['3000']);
 for(const wallet of Object.values(stats.wallets)){
  need(Array.isArray(wallet.positions)&&Array.isArray(wallet.pending));
  for(const position of wallet.positions){const marketId=byInstrument.get(position.instrument);need(valid.has(marketId)&&Number.isSafeInteger(position.quantity)&&position.quantity>0);markets.set(marketId,'held');}
  // The frozen report omits pending map keys. Held instruments cover all sell
  // intents; buy candidates are the causal signals at their actual creation
  // receipt. Coincident receipts may include more than one paper candidate.
  for(const intent of wallet.pending){need(['buy','sell'].includes(intent.action));if(intent.action==='sell'){need(wallet.positions.length>0);continue;}
   const candidates=stats.signals.filter(s=>s.actualAvailableAt===intent.createdAt);need(candidates.length>0);
   for(const signal of candidates){need(valid.has(signal.marketId));if(!markets.has(signal.marketId))markets.set(signal.marketId,'entry_pending');}
  }
 }
 return [...markets].map(([marketId,phase])=>({marketId,phase})).sort((a,b)=>Number(a.phase==='entry_pending')-Number(b.phase==='entry_pending')||Number(a.marketId)-Number(b.marketId));
}
export function readAnchorReadFocus(runtimeDir,universe,now=Date.now()){
 try{
  const active=JSON.parse(fs.readFileSync(path.join(runtimeDir,'bid-anchor-live-active.json')));need(/^[a-z][a-z0-9-]{1,63}$/.test(active.cohort));
  const dir=path.join(runtimeDir,'bid-anchor-live-cohorts',active.cohort),bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json'))),stats=JSON.parse(fs.readFileSync(path.join(dir,'stats.json'))),lock=JSON.parse(fs.readFileSync(path.join(dir,'worker.lock'))),projection=JSON.parse(fs.readFileSync(path.join(dir,'read-focus.json'))),{projectionHash,...body}=projection;
  need(/^[a-z][a-z0-9-]{1,63}$/.test(body.observerCohort));
  if(['STOP','BID_ANCHOR_LIVE_STOP'].some(n=>fs.existsSync(path.join(runtimeDir,n)))||fs.existsSync(path.join(dir,'STOP'))||fs.existsSync(path.join(runtimeDir,'executable-edge-cohorts',body.observerCohort,'OBSERVER_STOP')))return null;
  need(body.schemaVersion===1&&body.mode==='bid_anchor_read_focus_only'&&body.networkRequests===0&&body.competitionOrders===0&&body.codeHash===anchorFocusCodeHash()&&projectionHash===hash(body)&&body.statsHash===hash(stats)&&body.pid===lock.pid&&body.lockId===lock.id&&Number.isSafeInteger(lock.pid)&&lock.pid>0&&typeof lock.id==='string');process.kill(lock.pid,0);
  const canonical=validateUniverse(bank.universe,universe.tournamentId);need(canonical.hash===universe.hash&&digest(canonical.exchanges)===digest(universe.exchanges));
  need(digest(bank)===active.bankHash&&stats.bankHash===active.bankHash&&stats.cohort===active.cohort&&stats.mode==='bid_anchor_actual_receipt_paper'&&stats.failed===false&&stats.sourceClosed===false&&stats.competitionOrders===0&&stats.networkRequests===0&&now>=Date.parse(stats.updatedAt)&&now-Date.parse(stats.updatedAt)<=20000);
  // Bind the projection to the original durable checkpoint, never an invented wallet.
  const file=path.join(dir,'journal.jsonl'),size=fs.statSync(file).size,fd=fs.openSync(file,'r');let bytes;try{bytes=Buffer.alloc(Math.min(size,65536));fs.readSync(fd,bytes,0,bytes.length,size-bytes.length);}finally{fs.closeSync(fd);}
  need(bytes.length&&bytes.at(-1)===10);const record=JSON.parse(bytes.toString().split('\n').at(-2)),{hash:journalHash,...checkpoint}=record;
  need(digest(checkpoint)===journalHash&&journalHash===stats.journalHash&&checkpoint.sequence===stats.journalSequence&&checkpoint.stateHash===stats.stateHash&&checkpoint.bankHash===stats.bankHash);
  return {bankHash:stats.bankHash,markets:anchorFocusMarkets(stats,universe)};
 }catch{return null;}
}
export function prioritizeAnchorReads(plan,focus,{excludedMarketIds=[],lastAttempts={}}={}){
 if(!focus?.markets?.length||plan.comparisonSampling||plan.basketMeasurement?.marketIds?.length||plan.paperPackageFocus?.marketIds?.length)return plan;
 const excluded=new Set(excludedMarketIds),eligible=focus.markets.filter(x=>!excluded.has(x.marketId)).sort((a,b)=>Number(a.phase==='entry_pending')-Number(b.phase==='entry_pending')||(lastAttempts[a.marketId]||0)-(lastAttempts[b.marketId]||0)||Number(a.marketId)-Number(b.marketId)),reads=plan.reads.map(x=>({...x})),observed=[];let replacements=0;
 for(const market of eligible){if(reads.some(x=>x.marketId===market.marketId)){observed.push(market.marketId);continue;}if(replacements===2)break;
  const index=reads.findIndex(x=>['discovery','comparison','edge_observation'].includes(x.reason)&&!eligible.some(y=>y.marketId===x.marketId));if(index<0)continue;
  reads[index]={marketId:market.marketId,path:'supplementalDepth',reason:'bid_anchor_paper_risk'};replacements++;observed.push(market.marketId);
 }
 if(!observed.length)return plan;const present=new Set(reads.map(x=>x.marketId));
 return {...plan,reads,entryMarketIds:plan.entryMarketIds.filter(id=>present.has(id)&&!observed.includes(id)),bidAnchorReadFocus:{marketIds:observed,replacedSlots:replacements,requestsAdded:0,executionAuthority:false}};
}
