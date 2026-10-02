import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {digest,normalizeBook} from '../live-paper/model.mjs';
import {instantNs} from '../feed/contract.mjs';
import {atomicJson,lock} from '../live-paper/runtime.mjs';
import {validateUniverse} from '../live-paper/breadth.mjs';
import {measureExecutionPerformance} from './performance.mjs';
import {createBookPublicationState,applyBookPublication} from '../live-paper/book-publication.mjs';

const time=x=>typeof x==='string'&&Number.isFinite(Date.parse(x));
// Projection only. It cannot read credentials or dispatch an account request.
export function createPerformanceCapture(universe){
 const scope=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId])),books=new Map(),seen=new Map(),invalidated=new Map();
 const invalidate=(id,at)=>{books.delete(id);invalidated.set(id,Math.max(invalidated.get(id)||0,Date.parse(at)));};
 let sequence=0,previous='',sourceAt=null;
 const consume=record=>{
  const {hash,...body}=record;
  if(body.sequence!==sequence+1||body.previous!==previous||hash!==digest(body))throw Error('SOURCE_INTEGRITY_FAILURE');
  const e=body.event,at=e?.clockAt||body.at;
  if(!time(at)||sourceAt&&Date.parse(at)<Date.parse(sourceAt))throw Error('SOURCE_CLOCK_INVALID');
  if(e.type==='account_read_outage')for(const id of scope.values())invalidate(id,at);
  if(e.type==='cycle'){
   const deferred=new Set(),acceptedThisCycle=new Set();
   const rows=[...(e.sourceSnapshots||[]),...(e.supplementalDepth||[])].map((r,index)=>({r,index,t:r.receivedAt||r.at}));
   rows.sort((a,b)=>(time(a.t)?Date.parse(a.t):Infinity)-(time(b.t)?Date.parse(b.t):Infinity)||a.index-b.index);
   for(const {r,t} of rows){
    const marketId=r.marketId||/^\/markets\/([1-9]\d*)\/orderbook\?/.exec(r.endpoint||'')?.[1],exchangeId=scope.get(marketId);
    if(!exchangeId)continue;
    if(!time(t)||Date.parse(t)>Date.parse(at)){deferred.add(exchangeId);continue;}
    if(r.skipped){
     // The collector records raw depth before the paper engine excludes real
     // account inventory. That exact paper-only marker does not erase a book
     // which this cycle already validated for attributed official valuation.
     if(r.skipped==='Exchange has real account holdings or open orders'&&e.accountExclusions?.includes(exchangeId)&&acceptedThisCycle.has(exchangeId)&&books.has(exchangeId)&&Date.parse(t)>=Date.parse(books.get(exchangeId).receivedAt))continue;
     invalidate(exchangeId,t);continue;
    }
    try{
     if(r.endpoint!==`/markets/${marketId}/orderbook?tournamentId=${universe.tournamentId}&depth=20`)throw Error();
     const book={...normalizeBook(r.data,marketId,universe.tournamentId,r.receivedAt,r.requestStartedAt,20000,new Set(),'authoritative_receipt_v1'),tournamentId:universe.tournamentId};
     if(book.exchangeId!==exchangeId||Date.parse(book.requestStartedAt)<(invalidated.get(exchangeId)||0))throw Error();
     const old=seen.get(exchangeId);
     if(old&&(Date.parse(book.requestStartedAt)<Date.parse(old.requestStartedAt)||Date.parse(book.receivedAt)<=Date.parse(old.receivedAt)||BigInt(book.sequence)<BigInt(old.sequence)||instantNs(book.sourceAt)<instantNs(old.sourceAt)||book.sequence===old.sequence&&(book.versionKey!==old.versionKey||digest([book.bids,book.asks])!==digest([old.bids,old.asks]))))throw Error();
     seen.set(exchangeId,book);books.set(exchangeId,book);acceptedThisCycle.add(exchangeId);
    }catch{invalidate(exchangeId,t);}
   }
   for(const id of deferred)invalidate(id,at);
  }
  sequence=body.sequence;previous=hash;sourceAt=at;
 };
 return {consume,status:()=>({sourceSequence:sequence,sourceHash:previous,sourceAt,books:[...books.values()]})};
}

export function openPerformanceTail(file,capture){
 let offset=0,partial=Buffer.alloc(0);const identity=fs.statSync(file);
 return ()=>{
  const stat=fs.statSync(file);
  if(stat.size<offset||stat.ino!==identity.ino||stat.birthtimeMs!==identity.birthtimeMs)throw Error('SOURCE_REPLACED_OR_TRUNCATED');
  const length=Math.min(stat.size-offset,4*1024*1024);if(!length)return false;
  const fd=fs.openSync(file,'r'),chunk=Buffer.alloc(length);let n;
  try{n=fs.readSync(fd,chunk,0,length,offset);}finally{fs.closeSync(fd);}
  offset+=n;const bytes=Buffer.concat([partial,chunk.subarray(0,n)]),end=bytes.lastIndexOf(10);
  if(end>=0)for(const line of bytes.subarray(0,end).toString('utf8').split('\n')){if(!line)throw Error('SOURCE_EMPTY_RECORD');capture.consume(JSON.parse(line));}
  partial=bytes.subarray(end+1);if(partial.length>8*1024*1024)throw Error('SOURCE_OVERSIZED_RECORD');
  return offset<stat.size;
 };
}

// Once the immediate stream is present it owns the entire book projection.
// Missing, invalidated, or closed-session books must never fall back to older
// cycle snapshots. Original journal identity remains independently verified.
export function createPerformanceBookSource(universe,legacy){
 let state=null;
 return {
  usePublication(){state??=createBookPublicationState(universe);},
  consume(record){if(!state)throw Error('PUBLICATION_NOT_ATTACHED');state=applyBookPublication(state,record);},
  status(){return state?{books:Object.values(state.books),bookSource:'immediate_publication',publicationSequence:state.sequence,publicationHash:state.hash,publicationAt:state.lastAt,publicationSessionId:state.sessionId,publicationClosed:state.closed}:{books:legacy.status().books,bookSource:'cycle_journal',publicationSequence:null,publicationHash:null,publicationAt:null,publicationSessionId:null,publicationClosed:null};},
 };
}

async function main(){
 const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url))),runtime=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
 const stop=()=>[path.join(workspace,'STOP'),path.join(runtime,'STOP'),path.join(runtime,'PERFORMANCE_STOP')].some(p=>fs.existsSync(p));
 if(stop())throw Error('STOP_EXISTS');
 const release=lock(path.join(runtime,'performance-watch.lock'));let ending=false,capture,bookSource;
 process.on('SIGINT',()=>ending=true);process.on('SIGTERM',()=>ending=true);
 const reportFile=path.join(runtime,'performance-stats.json');
 const publish=(status,report={})=>{const source=capture?.status(),{books,...publication}=bookSource?.status()||{};atomicJson(reportFile,{...report,...publication,mode:'official_execution_performance',status,pid:process.pid,updatedAt:new Date().toISOString(),sourceSequence:source?.sourceSequence??null,sourceHash:source?.sourceHash??null,sourceAt:source?.sourceAt??null,networkRequests:0,ordersSubmitted:0});};
 try{
  const config=JSON.parse(fs.readFileSync(path.join(workspace,'live-paper/config.json'))),universe=validateUniverse(JSON.parse(fs.readFileSync(path.join(workspace,'live-paper/price-universe.json'))),config.tournamentId);
  capture=createPerformanceCapture(universe);const poll=openPerformanceTail(path.join(runtime,'journal.jsonl'),capture);
  bookSource=createPerformanceBookSource(universe,capture);let pollPublication;
  const publicationFile=path.join(runtime,'public-books.jsonl');
  publish('initializing');
  while(!ending&&!stop()){
   while(poll()){if(ending||stop())break;}
   if(ending||stop())break;
   if(!pollPublication&&fs.existsSync(publicationFile)){bookSource.usePublication();pollPublication=openPerformanceTail(publicationFile,bookSource);}
   if(pollPublication)while(pollPublication()){if(ending||stop())break;}
   if(ending||stop())break;
   try{
    const execution=JSON.parse(fs.readFileSync(path.join(runtime,'competition-stats.json'))),now=Date.now();
    if(execution.mode!=='official_quant_execution'||execution.owner!=='Codex'||execution.addedFees!=='0.000000'||!time(execution.updatedAt)||now<Date.parse(execution.updatedAt)||now-Date.parse(execution.updatedAt)>60000)throw Error('EXECUTION_SNAPSHOT_UNAVAILABLE');
    const report=measureExecutionPerformance({tournamentId:config.tournamentId,positions:execution.positions,fills:execution.fills,books:bookSource.status().books,now});
    publish('running',{...report,runId:execution.runId,executionUpdatedAt:execution.updatedAt,executionHash:digest(execution),executionLedgerHash:digest({runId:execution.runId,positions:execution.positions,fills:execution.fills})});
   }catch{publish('unavailable',{reason:'EXECUTION_EVIDENCE_UNAVAILABLE'});}
   if(process.argv.includes('--once'))break;
   for(let i=0;i<20&&!ending&&!stop();i++)await new Promise(r=>setTimeout(r,100));
  }
  if(!process.argv.includes('--once'))publish('stopped');
 }catch{publish('failed',{reason:'CAPTURE_INTEGRITY_OR_RUNTIME_FAILURE'});process.exitCode=1;}
 finally{release();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))await main();
