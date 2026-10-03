import path from 'node:path';
import {Journal} from './transport-runtime.mjs';
import {digest} from './model.mjs';
import {instantNs} from '../feed/contract.mjs';

const check=(ok,code='TRADE_TAPE_INVALID')=>{if(!ok)throw Error(code);};
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const clock=x=>typeof x==='string'&&Number.isFinite(Date.parse(x));
const cursor=x=>typeof x==='string'&&x.length>0&&x.length<=4096;
export function tradeTapeEndpoint(spec){
 check(id(spec.exchangeId)&&id(spec.marketId)&&/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(spec.tournamentId));
 check(clock(spec.from)&&clock(spec.to)&&Date.parse(spec.from)<Date.parse(spec.to));
 check(spec.cursor===null||cursor(spec.cursor));
 return '/exchanges/'+spec.exchangeId+'/trades?tournamentId='+spec.tournamentId+'&from='+encodeURIComponent(spec.from)+'&to='+encodeURIComponent(spec.to)+'&limit=200'+(spec.cursor===null?'':'&cursor='+encodeURIComponent(spec.cursor));
}
export function normalizeTradeTape(response,spec){
 check(response.endpoint===tradeTapeEndpoint(spec)&&response.status===200,'TRADE_TAPE_SCOPE_INVALID');
 const started=Date.parse(response.requestStartedAt),received=Date.parse(response.receivedAt),from=Date.parse(spec.from),to=Date.parse(spec.to),d=response.data;
 check(clock(response.requestStartedAt)&&clock(response.receivedAt)&&started>=to&&received>=started&&received-started<=20000,'TRADE_TAPE_CLOCK_INVALID');
 check(d?.exchangeId===spec.exchangeId&&d.marketId===spec.marketId&&clock(d.from)&&clock(d.to)&&Date.parse(d.from)===from&&Date.parse(d.to)===to,'TRADE_TAPE_SCOPE_INVALID');
 check(Array.isArray(d.data)&&d.data.length<=200&&d.pagination?.limit===200&&typeof d.pagination.hasMore==='boolean');
 check(d.pagination.hasMore?cursor(d.pagination.nextCursor)&&d.pagination.nextCursor!==spec.cursor:d.pagination.nextCursor===null,'TRADE_TAPE_CURSOR_INVALID');
 check(typeof d.coverage?.complete==='boolean'&&(d.coverage.projectedThroughSequence===null||Number.isSafeInteger(d.coverage.projectedThroughSequence)&&d.coverage.projectedThroughSequence>=0));
 let previous=null;const seen=new Set(),trades=d.data.map(t=>{
  check(t&&typeof t.id==='string'&&t.id.length>0&&t.id.length<=256&&clock(t.createdAt)&&Number.isSafeInteger(t.size)&&t.size>0&&['YES','NO'].includes(t.side)&&Number.isFinite(t.volume)&&t.volume>=0);
  check(t.price===null||Number.isFinite(t.price)&&t.price>=0&&t.price<=1);
  const at=instantNs(t.createdAt);check(at>=BigInt(from)*1000000n&&at<BigInt(to)*1000000n&&(previous===null||at<=previous),'TRADE_TAPE_TRADE_CLOCK_INVALID');previous=at;
  // Legacy IDs and engine sequences are separate namespaces. Never dedup on ID alone.
  const key=digest([spec.exchangeId,t.id,t.createdAt,t.side]);check(!seen.has(key),'TRADE_TAPE_DUPLICATE_IN_PAGE');seen.add(key);
  return {key,id:t.id,createdAt:t.createdAt,yesPrice:t.price===null?null:String(t.price),size:t.size,executedSpace:t.side,reportedVolume:t.volume,aggressorSide:null};
 });
 return {schemaVersion:1,source:'official_anonymous_exchange_trades',exchangeId:spec.exchangeId,marketId:spec.marketId,tournamentId:spec.tournamentId,
  from:spec.from,to:spec.to,requestStartedAt:response.requestStartedAt,receivedAt:response.receivedAt,requestCursor:spec.cursor,
  pagination:structuredClone(d.pagination),coverage:structuredClone(d.coverage),trades,responseHash:digest({endpoint:response.endpoint,data:d}),
  firstAvailableAt:response.receivedAt,completeTapeProven:false,counterfactualFillsProven:false};
}
export function selectAuxiliaryReads(historyBudget,cycle){
 check(Number.isSafeInteger(historyBudget)&&historyBudget>=0&&historyBudget<=2&&Number.isSafeInteger(cycle)&&cycle>=0);
 const tapeReads=historyBudget>0&&cycle%2===0?1:0;
 return {historyReads:historyBudget-tapeReads,tapeReads,totalReads:historyBudget};
}
export function createTradeTape({runtimeDir,universe,now=Date.now,canOperate=()=>true}){
 const scope={tournamentId:universe.tournamentId,exchanges:universe.exchanges.map(({marketId,exchangeId})=>({marketId,exchangeId}))},scopeHash=digest(scope);
 const journal=new Journal(path.join(runtimeDir,'anonymous-trades.jsonl'));
 let state=journal.last?.state||{schemaVersion:1,scopeHash,pages:0,tradesCaptured:0,duplicates:0,markets:{},lastAt:null},pending=null;
 check(state.schemaVersion===1&&state.scopeHash===scopeHash,'TRADE_TAPE_RESTART_SCOPE_INVALID');
 function stats(){return {status:'running',schemaVersion:1,pages:state.pages,tradesCaptured:state.tradesCaptured,duplicates:state.duplicates,lastAt:state.lastAt,
  journalSequence:journal.sequence,journalHash:journal.previous,markets:Object.values(state.markets).map(m=>({marketId:m.marketId,exchangeId:m.exchangeId,pages:m.pages,tradesCaptured:m.tradesCaptured,
   lastReceiptAt:m.lastReceiptAt,latestTradeAt:m.latestTradeAt,windowFrom:m.window?.from||null,windowTo:m.window?.to||null,paginationPending:!!m.window?.cursor,
   projectedThroughSequence:m.projectedThroughSequence,reportedCoverageComplete:m.reportedCoverageComplete})),executionAuthority:false,networkClientsAdded:0,completeTapeProven:false,counterfactualFillsProven:false};}
 function authorizes(spec){return !!pending&&canOperate()&&spec.method==='GET'&&spec.endpoint===pending.endpoint&&spec.body===undefined&&spec.bodyJson===undefined;}
 async function collect(dispatch,marketIds){
  check(!pending,'TRADE_TAPE_CONCURRENT_REQUEST');if(!canOperate())return stats();
  const candidates=[...new Set(marketIds)].map(m=>scope.exchanges.find(x=>x.marketId===m)).filter(Boolean);check(candidates.length>0,'TRADE_TAPE_NO_SCOPE');
  candidates.sort((a,b)=>(Date.parse(state.markets[a.exchangeId]?.lastReceiptAt||'1970-01-01T00:00:00.000Z')-Date.parse(state.markets[b.exchangeId]?.lastReceiptAt||'1970-01-01T00:00:00.000Z'))||Number(a.marketId)-Number(b.marketId));
  const chosen=candidates[0],prior=state.markets[chosen.exchangeId],to=new Date(Math.floor(now()/1000)*1000).toISOString();
  const window=prior?.window||{from:new Date(prior?.completedTo?Math.max(0,Date.parse(prior.completedTo)-60000):Date.parse(to)-1800000).toISOString(),to,cursor:null,cursors:[]};
  const spec={...chosen,tournamentId:scope.tournamentId,from:window.from,to:window.to,cursor:window.cursor};pending={...spec,endpoint:tradeTapeEndpoint(spec)};
  try{
   const response=await dispatch({method:'GET',endpoint:pending.endpoint}),page=normalizeTradeTape(response,spec);
   if(!canOperate())return stats();
   const next=structuredClone(state),m=next.markets[chosen.exchangeId]||{...chosen,pages:0,tradesCaptured:0,seen:{},latestTradeAt:null};
   check(page.pagination.nextCursor===null||!window.cursors.includes(page.pagination.nextCursor),'TRADE_TAPE_CURSOR_CYCLE');
   // Retain all identities needed by an unfinished page window, and one-minute reporting overlap thereafter.
   for(const [k,v] of Object.entries(m.seen))if(Date.parse(v.at)<Date.parse(window.from))delete m.seen[k];
   const novel=[];for(const t of page.trades){const fingerprint=digest(t),known=m.seen[t.key];
    if(known){check(known.hash===fingerprint,'TRADE_TAPE_IDENTITY_CONFLICT');next.duplicates++;}
    else{m.seen[t.key]={at:t.createdAt,hash:fingerprint};novel.push(t);m.tradesCaptured++;next.tradesCaptured++;if(!m.latestTradeAt||instantNs(t.createdAt)>instantNs(m.latestTradeAt))m.latestTradeAt=t.createdAt;}
   }
   check(Object.keys(m.seen).length<=100000,'TRADE_TAPE_WINDOW_CAPACITY');
   m.pages++;m.lastReceiptAt=page.receivedAt;m.projectedThroughSequence=page.coverage.projectedThroughSequence;m.reportedCoverageComplete=page.coverage.complete;
   m.window=page.pagination.hasMore?{...window,cursor:page.pagination.nextCursor,cursors:[...window.cursors,page.pagination.nextCursor]}:null;
   if(!page.pagination.hasMore)m.completedTo=window.to;
   next.markets[chosen.exchangeId]=m;next.pages++;next.lastAt=page.receivedAt;
   // The immutable event holds only novel public trades; state holds resumable pagination/deduplication.
   try{journal.commit(next,{type:'anonymous_trade_page',...page,trades:novel,receivedCount:page.trades.length,duplicateCount:page.trades.length-novel.length});}
   catch{const e=Error('TRADE_TAPE_RECORDING_FAILED');e.durableRecordingFailed=true;throw e;}
   state=next;return stats();
  }finally{pending=null;}
 }
 return {collect,authorizes,status:stats};
}
