import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {digest} from './model.mjs';import {canonicalJson} from '../execution/ledger.mjs';import {validateUniverse} from './breadth.mjs';
const need=ok=>{if(!ok)throw Error('INVALID_BULK_READ_FOCUS');};
const hash=x=>createHash('sha256').update(canonicalJson(x)).digest('hex');
export const bulkFocusCodeHash=()=>createHash('sha256').update(fs.readFileSync(new URL('./executable-edge-observe.mjs',import.meta.url))).update(fs.readFileSync(new URL('./bulk-dip-read-focus.mjs',import.meta.url))).digest('hex');
export function projectBulkReadFocus(stats,lock,observerCohort){
 const body={schemaVersion:1,mode:'bulk_dip_read_focus_only',observerCohort,pid:lock.pid,lockId:lock.id,codeHash:bulkFocusCodeHash(),statsHash:hash(stats),networkRequests:0,competitionOrders:0};
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
export function readBulkReadFocus(runtimeDir,universe,now=Date.now()){
 try{
  const active=JSON.parse(fs.readFileSync(path.join(runtimeDir,'bulk-dip-active.json')));need(/^[a-z][a-z0-9-]{1,63}$/.test(active.cohort));
  const dir=path.join(runtimeDir,'bulk-dip-cohorts',active.cohort),bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json'))),stats=JSON.parse(fs.readFileSync(path.join(dir,'stats.json'))),lock=JSON.parse(fs.readFileSync(path.join(dir,'worker.lock'))),projection=JSON.parse(fs.readFileSync(path.join(dir,'read-focus.json'))),{projectionHash,...body}=projection;
  need(/^[a-z][a-z0-9-]{1,63}$/.test(body.observerCohort));
  if(['STOP','BULK_DIP_STOP'].some(n=>fs.existsSync(path.join(runtimeDir,n)))||fs.existsSync(path.join(dir,'STOP'))||fs.existsSync(path.join(runtimeDir,'executable-edge-cohorts',body.observerCohort,'OBSERVER_STOP')))return null;
  need(body.schemaVersion===1&&body.mode==='bulk_dip_read_focus_only'&&body.networkRequests===0&&body.competitionOrders===0&&body.codeHash===bulkFocusCodeHash()&&projectionHash===hash(body)&&body.statsHash===hash(stats)&&body.pid===lock.pid&&body.lockId===lock.id&&Number.isSafeInteger(lock.pid)&&lock.pid>0&&typeof lock.id==='string');process.kill(lock.pid,0);
  const canonical=validateUniverse(bank.universe,universe.tournamentId);need(canonical.hash===universe.hash&&digest(canonical.exchanges)===digest(universe.exchanges));
  need(digest(bank)===active.bankHash&&stats.bankHash===active.bankHash&&stats.cohort===active.cohort&&stats.mode==='market_wide_bulk_reference_paper'&&stats.failed===false&&stats.sourceClosed===false&&stats.competitionOrders===0&&stats.networkRequests===0&&now>=Date.parse(stats.updatedAt)&&now-Date.parse(stats.updatedAt)<=20000);
  // Bind the projection to the original durable checkpoint, never an invented wallet.
  const file=path.join(dir,'journal.jsonl'),size=fs.statSync(file).size,fd=fs.openSync(file,'r');let bytes;try{bytes=Buffer.alloc(Math.min(size,65536));fs.readSync(fd,bytes,0,bytes.length,size-bytes.length);}finally{fs.closeSync(fd);}
  need(bytes.length&&bytes.at(-1)===10);const record=JSON.parse(bytes.toString().split('\n').at(-2)),{hash:journalHash,...checkpoint}=record;
  need(digest(checkpoint)===journalHash&&journalHash===stats.journalHash&&checkpoint.sequence===stats.journalSequence&&checkpoint.stateHash===stats.stateHash&&checkpoint.bankHash===stats.bankHash);
  const markets=anchorFocusMarkets(stats,universe),seen=new Set(markets.map(x=>x.marketId)),members=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId]));need(Array.isArray(stats.bulkReference?.candidates));for(const candidate of stats.bulkReference.candidates){const expiry=Date.parse(candidate.expiresAt),available=Date.parse(candidate.availableAt);need(members.get(candidate.marketId)===candidate.exchangeId&&candidate.referenceKind==="bulk_quoted_bid_not_executable"&&candidate.depthVerified===false&&Number.isFinite(available)&&available<=Date.parse(stats.updatedAt)&&expiry===available+bank.bulkPolicy.candidateLifetimeMs);if(expiry>=now&&!seen.has(candidate.marketId)){seen.add(candidate.marketId);markets.push({marketId:candidate.marketId,phase:"depth_required"});}}return {bankHash:stats.bankHash,markets};
 }catch{return null;}
}

export function mergePaperReadFocus(...foci){const rank={held:0,exit_pending:0,entry_pending:1,depth_required:2},markets=new Map();for(const focus of foci)for(const market of focus?.markets||[]){const old=markets.get(market.marketId);if(!old||rank[market.phase]<rank[old.phase])markets.set(market.marketId,market);}return {markets:[...markets.values()]};}
