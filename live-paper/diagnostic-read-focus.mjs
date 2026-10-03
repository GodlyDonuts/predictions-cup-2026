import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {amount,money,TICK,instant} from '../paper/fixed.mjs';
import {digest} from './model.mjs';import {canonicalJson} from '../execution/ledger.mjs';
import {validateUniverse} from './breadth.mjs';import {validatePriceObservation} from './market-price-publication.mjs';
import {createBookPublicationState} from './book-publication.mjs';import {diagnoseQuotedDip} from './bulk-dip-diagnostic.mjs';import {BID_ANCHOR_POLICY} from './bid-anchor.mjs';
export const DIAGNOSTIC_FOCUS_POLICY=Object.freeze({minimumGrossDiscount:'0.010000',lifetimeMs:60000,cooldownMs:900000,maximumAgeMs:20000,maximumMarkets:2});
const need=(ok,c)=>{if(!ok)throw Error('DIAGNOSTIC_FOCUS_'+c)},hash=x=>createHash('sha256').update(canonicalJson(x)).digest('hex'),decimal=n=>money(BigInt(Math.round(n*1e6)));
export const diagnosticFocusCodeHash=()=>createHash('sha256').update(fs.readFileSync(new URL('./diagnostic-read-focus.mjs',import.meta.url))).update(fs.readFileSync(new URL('./runner.mjs',import.meta.url))).digest('hex');
export function createDiagnosticFocusState(universe){const {hash:storedHash,...raw}=universe,u=validateUniverse(raw,raw.tournamentId);need(storedHash===undefined||storedHash===u.hash,'UNIVERSE_HASH');return {universe:u,bookScopeHash:createBookPublicationState(raw).scopeHash,book:{sequence:0,hash:'',at:null},price:{sequence:0,hash:'',at:null},closed:true,history:{},candidates:{},cooldowns:{},barriers:{global:null,markets:{}}};}
export function advanceDiagnosticFocus(s,kind,row){
 const now=instant(row.at);
 if(kind==='book'){
  const {hash:recordHash,...body}=row;
  need(body.sequence===s.book.sequence+1&&body.previousHash===s.book.hash&&hash(body)===recordHash&&body.tournamentId===s.universe.tournamentId&&body.scopeHash===s.bookScopeHash&&(!s.book.at||now>=instant(s.book.at)),'BOOK_CHAIN');
  need(['book','session_start','session_stop','invalidation'].includes(row.kind),'BOOK_KIND');s.book={sequence:row.sequence,hash:row.hash,at:row.at};
  if(row.kind==='book')return;
  const id=row.kind==='invalidation'?row.payload.marketId:null;
  need(id===null||s.universe.exchanges.some(x=>x.marketId===id),'BOUNDARY_SCOPE');
  if(id===null){s.history={};s.candidates={};s.barriers.global=row.at;}else{delete s.history[id];delete s.candidates[id];s.barriers.markets[id]=row.at;}
  if(row.kind==='session_start')s.closed=false;else if(row.kind==='session_stop')s.closed=true;return;
 }
 need(kind==='price','SOURCE_KIND');const {hash:recordHash,...body}=row,snapshot=body.event?.snapshot;
 need(body.sequence===s.price.sequence+1&&body.previous===s.price.hash&&digest(body)===recordHash&&body.state===null&&body.event?.type==='market_price_observation'&&(!s.price.at||now>=instant(s.price.at)),'PRICE_CHAIN');
 validatePriceObservation(snapshot,s.universe);need(instant(snapshot.receivedAt)<=now,'PRICE_CLOCK');s.price={sequence:row.sequence,hash:row.hash,at:row.at};
 const byExchange=new Map(s.universe.exchanges.map(x=>[x.exchangeId,x.marketId]));
 for(const id of snapshot.missingIds){const market=byExchange.get(id);delete s.history[market];delete s.candidates[market];s.barriers.markets[market]=row.at;}
 if(now-instant(snapshot.receivedAt)>DIAGNOSTIC_FOCUS_POLICY.maximumAgeMs){for(const q of snapshot.quotes){delete s.history[q.marketId];delete s.candidates[q.marketId];s.barriers.markets[q.marketId]=row.at;}return;}
 for(const q of snapshot.quotes){const id=q.marketId;
  if(s.closed||![s.barriers.global,s.barriers.markets[id]].every(at=>!at||instant(snapshot.requestStartedAt)>instant(at)))continue;
  if(q.option!=='YES'||q.crossed||q.bestBid===null||q.bestAsk===null||q.bestBid<=0||q.bestAsk>=1||q.bestAsk<=q.bestBid){delete s.history[id];delete s.candidates[id];s.barriers.markets[id]=row.at;continue;}
  const points=s.history[id]||[],gate=diagnoseQuotedDip(points,q,snapshot.receivedAt),prior=s.candidates[id],gross=gate.features?amount(gate.features.netConcession)+2n*TICK:null;
  delete s.candidates[id];
  if(gross!==null&&gross>=amount(DIAGNOSTIC_FOCUS_POLICY.minimumGrossDiscount)&&gate.failures.every(x=>x==='insufficient_concession')&&now>=(s.cooldowns[id]||0)){
   s.candidates[id]={marketId:id,exchangeId:q.exchangeId,availableAt:row.at,latestAvailableAt:row.at,receivedAt:snapshot.receivedAt,expiresAt:new Date(now+DIAGNOSTIC_FOCUS_POLICY.lifetimeMs).toISOString(),grossDiscount:money(gross),anchor:gate.features.anchor,originalAsk:decimal(q.bestAsk),sourceSequence:row.sequence,sourceHash:row.hash,depthVerified:false};
   s.cooldowns[id]=now+DIAGNOSTIC_FOCUS_POLICY.cooldownMs;
  }else if(prior&&now<=instant(prior.expiresAt)&&amount(decimal(q.bestAsk))-amount(decimal(q.bestBid))<=amount(BID_ANCHOR_POLICY.maximumSpread)&&amount(decimal(q.bestAsk))<=amount(prior.originalAsk)&&amount(prior.anchor)-amount(decimal(q.bestBid))<=amount(BID_ANCHOR_POLICY.maximumBidDrop))s.candidates[id]={...prior,latestAvailableAt:row.at,receivedAt:snapshot.receivedAt};
  const minute=Math.floor(instant(snapshot.receivedAt)/60000);if(!points.some(p=>p.minute===minute))points.push({minute,at:snapshot.receivedAt,bid:decimal(q.bestBid)});
  s.history[id]=points.filter(p=>instant(p.at)>=instant(snapshot.receivedAt)-BID_ANCHOR_POLICY.historyMs);
 }
}
export function projectDiagnosticFocus(s,lock,observerCohort,now=Date.now()){
 const candidates=s.closed?[]:Object.values(s.candidates).filter(c=>now>=instant(c.latestAvailableAt)&&now-instant(c.latestAvailableAt)<=DIAGNOSTIC_FOCUS_POLICY.maximumAgeMs&&now<=instant(c.expiresAt))
  .sort((a,b)=>Number(amount(b.grossDiscount)-amount(a.grossDiscount))||instant(a.availableAt)-instant(b.availableAt)).slice(0,DIAGNOSTIC_FOCUS_POLICY.maximumMarkets);
 const body={schemaVersion:1,mode:'diagnostic_depth_reads_only',policy:DIAGNOSTIC_FOCUS_POLICY,observerCohort,pid:lock.pid,lockId:lock.id,codeHash:diagnosticFocusCodeHash(),universeHash:s.universe.hash,at:new Date(now).toISOString(),sourceClosed:s.closed,price:s.price,book:s.book,candidates,networkRequests:0,competitionOrders:0,executionAuthority:false};
 return {...body,projectionHash:hash(body)};
}
export function openDiagnosticFocus(runtimeDir,universe,{deferBootstrap=false}={}){
 const state=createDiagnosticFocusState(universe),files={price:path.join(runtimeDir,'market-prices.jsonl'),book:path.join(runtimeDir,'public-books.jsonl')},tails={},bootstrap=[];
 for(const[k,file]of Object.entries(files)){const bytes=fs.readFileSync(file),end=bytes.lastIndexOf(10)+1,stat=fs.statSync(file);tails[k]={offset:bytes.length,partial:bytes.subarray(end),ino:stat.ino,birthtimeMs:stat.birthtimeMs};for(const line of bytes.subarray(0,end).toString().split('\n').filter(Boolean))bootstrap.push({kind:k,row:JSON.parse(line)});}
 const sort=list=>list.sort((a,b)=>instant(a.row.at)-instant(b.row.at)||Number(a.kind==='book')-Number(b.kind==='book')||a.row.sequence-b.row.sequence),apply=list=>{for(const item of sort(list))advanceDiagnosticFocus(state,item.kind,item.row);};
 let initialized=!deferBootstrap;if(initialized){apply(bootstrap);bootstrap.length=0;}else sort(bootstrap);
 return {async initialize({stopped=()=>false}={}){if(initialized)return;for(let i=0;i<bootstrap.length;i++){need(!stopped(),'STOPPED');const item=bootstrap[i];advanceDiagnosticFocus(state,item.kind,item.row);if(i%64===63)await new Promise(setImmediate);}need(!stopped(),'STOPPED');initialized=true;bootstrap.length=0;},
 consume(){need(initialized,'BOOTSTRAP_PENDING');const list=[];for(const[k,file]of Object.entries(files)){const tail=tails[k],stat=fs.statSync(file);need(stat.size>=tail.offset&&stat.ino===tail.ino&&stat.birthtimeMs===tail.birthtimeMs,'SOURCE_REPLACED');const length=stat.size-tail.offset;if(!length)continue;need(length<=4*1024*1024,'TAIL_BACKLOG');const bytes=Buffer.alloc(length),fd=fs.openSync(file,'r');try{need(fs.readSync(fd,bytes,0,length,tail.offset)===length,'TAIL_READ');}finally{fs.closeSync(fd);}tail.offset+=length;const raw=Buffer.concat([tail.partial,bytes]),end=raw.lastIndexOf(10)+1;tail.partial=raw.subarray(end);need(tail.partial.length<=1024*1024,'PARTIAL_SIZE');for(const line of raw.subarray(0,end).toString().split('\n').filter(Boolean))list.push({kind:k,row:JSON.parse(line)});}apply(list);},project:(lock,cohort,now)=>{need(initialized,'BOOTSTRAP_PENDING');return projectDiagnosticFocus(state,lock,cohort,now);}};
}
export function diagnosticDepthFocus(p,universe,{pid,lockId,now=Date.now()}={}){
 try{
  const {projectionHash,...body}=p;
  need(body.schemaVersion===1&&body.mode==='diagnostic_depth_reads_only'&&body.executionAuthority===false&&body.networkRequests===0&&body.competitionOrders===0&&body.sourceClosed===false&&body.pid===pid&&body.lockId===lockId&&hash(body)===projectionHash&&body.codeHash===diagnosticFocusCodeHash()&&body.universeHash===universe.hash&&digest(body.policy)===digest(DIAGNOSTIC_FOCUS_POLICY),'IDENTITY');
  need(now>=instant(body.at)&&now-instant(body.at)<=DIAGNOSTIC_FOCUS_POLICY.maximumAgeMs&&Array.isArray(body.candidates)&&body.candidates.length<=2,'AGE_OR_SIZE');
  const members=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId])),seen=new Set();
  for(const c of body.candidates){need(members.get(c.marketId)===c.exchangeId&&!seen.has(c.marketId)&&c.depthVerified===false&&instant(c.expiresAt)===instant(c.availableAt)+DIAGNOSTIC_FOCUS_POLICY.lifetimeMs&&instant(c.availableAt)<=instant(c.latestAvailableAt)&&instant(c.latestAvailableAt)<=instant(body.at)&&now-instant(c.latestAvailableAt)<=DIAGNOSTIC_FOCUS_POLICY.maximumAgeMs&&now<=instant(c.expiresAt)&&amount(c.grossDiscount)>=amount(DIAGNOSTIC_FOCUS_POLICY.minimumGrossDiscount)&&Number.isSafeInteger(c.sourceSequence)&&c.sourceSequence>0&&c.sourceSequence<=body.price.sequence&&/^[a-f0-9]{64}$/.test(c.sourceHash),'CANDIDATE');seen.add(c.marketId);}
  return {markets:body.candidates.map(c=>({marketId:c.marketId,phase:'depth_required'})),sourceSequence:body.price.sequence,executionAuthority:false};
 }catch{return null;}
}
