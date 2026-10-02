import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {digest,assertState} from './model.mjs';
import {emptyEntryComparisonState,ENTRY_COMPARISON_MARKETS} from './entry-comparison.mjs';
import {entryComparisonCodeIdentity} from './entry-comparison-watch.mjs';

const CHUNK=1024*1024,MAX_RECORD=16*1024*1024,HEARTBEAT_MS=60000,PROOFS=16;
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const object=x=>x!==null&&typeof x==='object'&&!Array.isArray(x);
const hex=x=>typeof x==='string'&&/^[a-f0-9]{64}$/.test(x);
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const clock=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;
const cohortName=x=>typeof x==='string'&&/^[a-z][a-z0-9-]{0,63}$/.test(x)&&!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(x);
const sameCode=(a,b)=>object(a)&&object(b)&&Object.keys(a).length===Object.keys(b).length&&Object.entries(a).every(([k,v])=>hex(v)&&Object.hasOwn(b,k)&&b[k]===v);
class RiskError extends Error{}
const need=(ok,code)=>{if(!ok)throw new RiskError(code);};
function boundedJson(file,max){const stat=fs.statSync(file);need(stat.isFile()&&stat.size>0&&stat.size<=max,'INVALID_IDENTITY_FILE');const bytes=fs.readFileSync(file);need(bytes.length<=max,'INVALID_IDENTITY_FILE');return {value:JSON.parse(bytes.toString('utf8')),hash:sha(bytes)};}

// Local append-only journals are the trust boundary. This validates their hash
// chain and wallet accounting, not official orders or original-source replay.
// Every byte is streamed once. Head/boundary and same-size edit guards catch
// replacement and ordinary rewriting; restart rechecks the full chain. A rewrite
// solely in an old middle prefix concurrent with append needs that full rescan.
export function createComparisonRiskSource({runtimeDir,universe,now=Date.now}={}){
 need(typeof runtimeDir==='string'&&path.isAbsolute(runtimeDir)&&typeof now==='function','INVALID_RISK_SOURCE_OPTIONS');
 need(object(universe)&&typeof universe.tournamentId==='string'&&Array.isArray(universe.exchanges),'INVALID_RISK_UNIVERSE');
 const scope=structuredClone(universe),markets=new Map(),exchanges=new Set();
 for(const row of scope.exchanges){need(id(row.marketId)&&id(row.exchangeId)&&row.option==='YES'&&!markets.has(row.marketId)&&!exchanges.has(row.exchangeId),'INVALID_RISK_UNIVERSE');markets.set(row.marketId,row.exchangeId);exchanges.add(row.exchangeId);}
 const manifestFile=path.join(runtimeDir,'entry-comparison-active.json');
 let configured=false,closed=false,failed=false,reason='NOT_POLLED',manifestHash,bankFile,journalFile,statsFile,bank,bankHash,cohort;
 let lastNow=-Infinity,identity,prior,head,boundary,offset=0,consumed=0,bytesRead=0,partial=Buffer.alloc(0);
 let journalSequence=0,journalHash='',sourceSequence=0,sourceHash=null,lastJournalAt=-Infinity,lastSourceAt=-Infinity,held=[],pending=[];
 const proofs=new Map();
 const time=()=>{const at=now();need(Number.isSafeInteger(at)&&at>=0&&at>=lastNow,'INVALID_RISK_CLOCK');lastNow=at;return at;};
 const result=available=>({schemaVersion:1,available,configured,reason,cohort:cohort??null,bankHash:bankHash??null,
  codeIdentity:bank?structuredClone(bank.codeIdentity):null,sourceSequence,sourceHash,journalSequence,journalHash:journalHash||null,
  heldMarketIds:[...held],pendingMarketIds:[...pending],journalBytesRead:bytesRead,journalBytesConsumed:consumed,
  incompleteTailBytes:partial.length,retainedCheckpoints:proofs.size,eligibleForExecution:false,networkRequests:0,runtimeWrites:0});
 function bind(manifest){
  const m=manifest.value;need(object(m)&&Object.keys(m).length===3&&Object.keys(m).every(k=>['schemaVersion','cohort','bankHash'].includes(k))&&m.schemaVersion===1&&cohortName(m.cohort)&&hex(m.bankHash),'INVALID_COMPARISON_MANIFEST');
  cohort=m.cohort;manifestHash=manifest.hash;bankHash=m.bankHash;
  const dir=path.join(runtimeDir,'entry-comparisons',cohort);bankFile=path.join(dir,'bank.json');journalFile=path.join(dir,'journal.jsonl');statsFile=path.join(dir,'stats.json');
  bank=boundedJson(bankFile,4*1024*1024).value;
  need(digest(bank)===bankHash,'COMPARISON_BANK_CHANGED');
  need(sameCode(bank.codeIdentity,entryComparisonCodeIdentity()),'COMPARISON_CODE_CHANGED');
  emptyEntryComparisonState(bank); // Validate the frozen execution contract.
  need(clock(bank.cutoff)&&Date.parse(bank.cutoff)<=time()&&bank.config.tournamentId===scope.tournamentId&&bank.universe?.tournamentId===scope.tournamentId,'FOREIGN_COMPARISON_BANK');
  need(bank.universe.exchanges.length===scope.exchanges.length&&bank.universe.exchanges.every(x=>x.option==='YES'&&markets.get(x.marketId)===x.exchangeId),'FOREIGN_COMPARISON_UNIVERSE');
  need(ENTRY_COMPARISON_MARKETS.every(x=>markets.has(x)),'FOREIGN_COMPARISON_UNIVERSE');
 }
 function readSpan(start,length){const fd=fs.openSync(journalFile,'r'),bytes=Buffer.alloc(length);try{let n=0;while(n<length){const got=fs.readSync(fd,bytes,n,length-n,start+n);need(got>0,'COMPARISON_JOURNAL_CHANGED');n+=got;}}finally{fs.closeSync(fd);}return bytes;}
 function remember(stat){
  if(!head&&stat.size){const length=Math.min(4096,stat.size);head={start:0,length,hash:sha(readSpan(0,length))};}
  const length=Math.min(4096,offset);boundary=length?{start:offset-length,length,hash:sha(readSpan(offset-length,length))}:null;prior=stat;
 }
 function checkedState(state,row){
  need(object(state)&&state.schemaVersion===1&&state.bankHash===bankHash&&Number.isSafeInteger(state.sourceSequence)&&state.sourceSequence>=bank.sourceRecords&&state.sourceSequence>sourceSequence&&hex(state.lastRecordHash),'INVALID_COMPARISON_CHECKPOINT');
  need(clock(state.lastSourceAt)&&Date.parse(state.lastSourceAt)>=lastSourceAt&&Date.parse(state.lastSourceAt)<=Date.parse(row.at),'INVALID_COMPARISON_SOURCE_CLOCK');
  need(row.event?.sourceSequence===state.sourceSequence&&row.event?.sourceHash===state.lastRecordHash,'COMPARISON_EVENT_MISMATCH');
  need(object(state.strategies)&&Object.keys(state.strategies).length===2&&['historical_dip','shock_rebound'].every(k=>object(state.strategies[k])),'INVALID_COMPARISON_STRATEGIES');
  const nextHeld=new Set(),nextPending=new Set();
  for(const name of ['historical_dip','shock_rebound']){
   const wallet=state.strategies[name].wallet;need(object(wallet)&&object(wallet.books)&&clock(wallet.lastAt)&&Date.parse(wallet.lastAt)<=time(),'INVALID_COMPARISON_WALLET');
   assertState(wallet,bank.config);
   for(const [instrument,b] of Object.entries(wallet.books))need(object(b)&&ENTRY_COMPARISON_MARKETS.includes(b.marketId)&&markets.get(b.marketId)===b.exchangeId&&instrument===b.exchangeId+':YES'&&b.instrument===instrument&&clock(b.receivedAt)&&clock(b.requestStartedAt)&&Date.parse(b.requestStartedAt)<=Date.parse(b.receivedAt)&&Date.parse(b.receivedAt)<=time(),'FOREIGN_COMPARISON_BOOK');
   const strategy=wallet.strategies.mean_reversion;
   for(const [rows,target,isPending] of [[strategy.positions,nextHeld,false],[strategy.pending,nextPending,true]]){
    need(object(rows),'INVALID_COMPARISON_RISK');
    for(const [instrument,p] of Object.entries(rows)){
     const b=wallet.books[instrument];need(object(p)&&Number.isSafeInteger(p.quantity)&&p.quantity>0&&b,'INVALID_COMPARISON_RISK');
     if(isPending)need(['buy','sell'].includes(p.action)&&clock(p.createdAt)&&Date.parse(p.createdAt)<=time(),'INVALID_COMPARISON_PENDING');
     else need(clock(p.enteredAt)&&Date.parse(p.enteredAt)<=time(),'INVALID_COMPARISON_POSITION');
     target.add(b.marketId);
    }
   }
  }
  return {held:[...nextHeld].sort(),pending:[...nextPending].sort()};
 }
 function consumeRow(line){
  const text=line.toString('utf8');need(Buffer.from(text,'utf8').equals(line),'INVALID_COMPARISON_UTF8');
  const row=JSON.parse(text),{hash,...body}=row;
  need(Number.isSafeInteger(row.sequence)&&row.sequence===journalSequence+1&&row.previous===journalHash&&hex(hash)&&digest(body)===hash,'COMPARISON_CHAIN_MISMATCH');
  need(clock(row.at)&&Date.parse(row.at)>=lastJournalAt&&Date.parse(row.at)>=Date.parse(bank.cutoff)&&Date.parse(row.at)<=time(),'INVALID_COMPARISON_JOURNAL_CLOCK');
  const risk=checkedState(row.state,row);
  held=risk.held;pending=risk.pending;journalSequence=row.sequence;journalHash=hash;sourceSequence=row.state.sourceSequence;sourceHash=row.state.lastRecordHash;lastJournalAt=Date.parse(row.at);lastSourceAt=Date.parse(row.state.lastSourceAt);
  proofs.set(row.sequence,{journalHash:hash,sourceSequence,sourceHash});if(proofs.size>PROOFS)proofs.delete(proofs.keys().next().value);
 }
 function stream(){
  const stat=fs.statSync(journalFile);need(stat.isFile(),'INVALID_COMPARISON_JOURNAL');
  if(!identity)identity=stat;
  need(stat.dev===identity.dev&&stat.ino===identity.ino&&stat.birthtimeMs===identity.birthtimeMs,'COMPARISON_JOURNAL_REPLACED');
  if(prior){
   need(stat.size>=prior.size&&stat.size>=offset,'COMPARISON_JOURNAL_TRUNCATED');
   need(stat.size!==prior.size||(stat.mtimeMs===prior.mtimeMs&&stat.ctimeMs===prior.ctimeMs),'COMPARISON_JOURNAL_CHANGED');
   for(const span of [head,boundary])if(span)need(sha(readSpan(span.start,span.length))===span.hash,'COMPARISON_JOURNAL_CHANGED');
  }
  while(offset<stat.size){
   const bytes=readSpan(offset,Math.min(CHUNK,stat.size-offset));offset+=bytes.length;bytesRead+=bytes.length;partial=Buffer.concat([partial,bytes]);
   let end;while((end=partial.indexOf(10))>=0){need(end>0&&end+1<=MAX_RECORD,'INVALID_COMPARISON_RECORD_SIZE');consumeRow(partial.subarray(0,end));consumed+=end+1;partial=partial.subarray(end+1);}
   need(partial.length<MAX_RECORD,'INVALID_COMPARISON_RECORD_SIZE');
  }
  remember(stat);
 }
 function heartbeat(stats){
  if(!stats)return 'COMPARISON_WORKER_UNAVAILABLE';
  if(stats.cohort!==cohort||stats.bankHash!==bankHash||!sameCode(stats.codeIdentity,bank.codeIdentity))return 'COMPARISON_WORKER_IDENTITY_MISMATCH';
  const proof=proofs.get(stats.journalSequence);
  if(!proof||stats.journalHash!==proof.journalHash||stats.sourceSequence!==proof.sourceSequence||stats.sourceHash!==proof.sourceHash)return 'COMPARISON_WORKER_CHECKPOINT_UNVERIFIED';
  if(!clock(stats.updatedAt)||Date.parse(stats.updatedAt)>time()||time()-Date.parse(stats.updatedAt)>HEARTBEAT_MS)return 'COMPARISON_WORKER_STALE';
  if(stats.status!=='running')return 'COMPARISON_WORKER_NOT_RUNNING';
  return 'VERIFIED';
 }
 function poll(){
  if(closed){reason='CLOSED';return result(false);}if(failed)return result(false);
  try{
   time();
   if(!fs.existsSync(manifestFile)){need(!configured,'COMPARISON_MANIFEST_MISSING');reason='NOT_CONFIGURED';return result(true);}
   configured=true;const manifest=boundedJson(manifestFile,4096);
   if(!manifestHash)bind(manifest);else need(manifest.hash===manifestHash,'COMPARISON_MANIFEST_CHANGED');
   need(digest(boundedJson(bankFile,4*1024*1024).value)===bankHash,'COMPARISON_BANK_CHANGED');
   need(sameCode(bank.codeIdentity,entryComparisonCodeIdentity()),'COMPARISON_CODE_CHANGED');
   let stats;try{stats=boundedJson(statsFile,4*1024*1024).value;}catch{}
   stream();
   reason=partial.length?'COMPARISON_INCOMPLETE_TAIL':!journalSequence?'COMPARISON_EMPTY_JOURNAL':heartbeat(stats);
   return result(reason==='VERIFIED');
  }catch(error){failed=true;reason=error instanceof RiskError?error.message:'COMPARISON_EVIDENCE_UNAVAILABLE';return result(false);}
 }
 return Object.freeze({poll,close(){closed=true;partial=Buffer.alloc(0);}});
}
