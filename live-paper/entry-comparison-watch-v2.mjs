import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {digest} from './model.mjs';
import {journalLines} from './journal-lines.mjs';
import {entryComparisonCodeIdentity} from './entry-comparison-watch.mjs';
import {Journal,lock} from './transport-runtime.mjs';
import {atomicJson} from '../execution/ledger.mjs';
import {createEntryComparison,emptyEntryComparisonState,advanceEntryComparison,entryComparisonReport} from './entry-comparison.mjs';

const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const need=(ok,message)=>{if(!ok)throw Error(message);};
const read=file=>JSON.parse(fs.readFileSync(file,'utf8'));
const nameOk=x=>typeof x==='string'&&/^[a-z][a-z0-9-]{0,63}$/.test(x)&&!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(x);

function createBankFile(file,bank){
 const fd=fs.openSync(file,'wx',0o600),bytes=Buffer.from(JSON.stringify(bank,null,2)+'\n');
 try{let offset=0;while(offset<bytes.length){const n=fs.writeSync(fd,bytes,offset,bytes.length-offset);need(n>0,'BANK_WRITE_FAILED');offset+=n;}fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
}

// Append-only local files are the trust boundary. Head/boundary guards detect
// replacement and ordinary rewrites; arbitrary edits to an old middle prefix
// during an append require the full verification performed on restart.
// This worker owns only its named local comparison files. It never reads a key,
// sends requests, changes collection priority, or mutates another paper cohort.
export function openEntryComparison({runtimeDir,cohort,initialize=false,now=Date.now,codeIdentity=entryComparisonCodeIdentity(),canOperate=()=>true}={}){
 need(path.isAbsolute(runtimeDir)&&nameOk(cohort),'INVALID_COMPARISON_DIRECTORY');
 need(!initialize,'STREAMING_SUCCESSOR_REQUIRES_EXISTING_FROZEN_BANK');
 need(typeof canOperate==='function'&&canOperate()===true,'COMPARISON_STOPPED');
 const dir=path.join(runtimeDir,'entry-comparisons',cohort),bankFile=path.join(dir,'bank.json'),journalFile=path.join(dir,'journal.jsonl');
 const sourceFile=path.join(runtimeDir,'journal.jsonl'),sourceStat=fs.statSync(sourceFile);
 const fd=fs.openSync(sourceFile,'r'),tailLength=Math.min(sourceStat.size,16*1024*1024),tail=Buffer.alloc(tailLength);
 try{need(fs.readSync(fd,tail,0,tailLength,sourceStat.size-tailLength)===tailLength,'SOURCE_READ_FAILED');}finally{fs.closeSync(fd);}
 const tailEnd=tail.lastIndexOf(10);need(tailEnd>=0,'SOURCE_PREFIX_EMPTY_OR_OVERSIZED_TAIL');
 const end=sourceStat.size-tailLength+tailEnd;
 need(fs.existsSync(bankFile)&&fs.existsSync(journalFile),'COMPARISON_ACCOUNTING_MISSING');
 const bank=read(bankFile);
 need(digest(bank.codeIdentity)===digest(codeIdentity),'FROZEN_COMPARISON_CODE_CHANGED');
 let state=emptyEntryComparisonState(bank),compared=0,savedSequence=0,savedHash='',lastSavedSource=0,savedClock=-Infinity;
 const saved=journalLines(journalFile)[Symbol.iterator]();
 const nextSaved=()=>{const n=saved.next();if(n.done)return null;const row=JSON.parse(n.value),{hash:h,...body}=row;
  need(row.sequence===++savedSequence&&row.previous===savedHash&&digest(body)===h,'COMPARISON_JOURNAL_INVALID');
  const at=Date.parse(row.event?.clockAt||row.at);need(Number.isFinite(at)&&at>=savedClock,'COMPARISON_JOURNAL_CLOCK_REGRESSION');savedClock=at;
  need(Number.isSafeInteger(row.state?.sourceSequence)&&row.state.sourceSequence>lastSavedSource,'COMPARISON_SOURCE_SEQUENCE_INVALID');
  need(row.event?.sourceSequence===row.state.sourceSequence&&row.event?.sourceHash===row.state.lastRecordHash,'COMPARISON_CHECKPOINT_BINDING_INVALID');
  savedHash=h;lastSavedSource=row.state.sourceSequence;return row;};
 const prefix=createHash('sha256');let checkpoint,prefixVerified=false;
 try{
  checkpoint=nextSaved();need(checkpoint!==null,'COMPARISON_JOURNAL_EMPTY');
  for(const line of journalLines(sourceFile,{endByte:end+1})){
   const record=JSON.parse(line);prefix.update(line+'\n');state=advanceEntryComparison(state,record,bank);
   if(record.sequence===bank.sourceRecords){need(prefix.copy().digest('hex')===bank.sourceHash,'FROZEN_SOURCE_PREFIX_CHANGED');prefixVerified=true;}
   if(checkpoint&&checkpoint.state.sourceSequence===record.sequence){need(digest(checkpoint.state)===digest(state),'COMPARISON_REPLAY_MISMATCH');compared++;checkpoint=nextSaved();}
  }
  need(prefixVerified&&checkpoint===null&&compared===savedSequence,'COMPARISON_REPLAY_INCOMPLETE');
 }finally{saved.return();}
 fs.mkdirSync(dir,{recursive:true,mode:0o700});
 if(initialize)createBankFile(bankFile,bank);
 const journal=new Journal(journalFile);
 need(canOperate()===true,'COMPARISON_STOPPED');
 if(!journal.last||journal.last.state.sourceSequence!==state.sourceSequence)journal.commit(state,{type:'source_advanced',sourceSequence:state.sourceSequence,sourceHash:state.lastRecordHash,cutoff:bank.cutoff});
 let offset=end+1,partial=Buffer.alloc(0),processedBytes=offset,failed=false;
 const guard=(at,length)=>{const fd=fs.openSync(sourceFile,'r'),b=Buffer.alloc(length);try{need(fs.readSync(fd,b,0,length,at)===length,'SOURCE_CHANGED');}finally{fs.closeSync(fd);}return hash(b);};
 const head={length:Math.min(4096,offset)},headHash=guard(0,head.length);
 let boundaryLength=Math.min(4096,offset),boundaryHash=guard(offset-boundaryLength,boundaryLength);
 const bankBytesHash=hash(fs.readFileSync(bankFile));
 const consume=()=>{
  need(!failed,'COMPARISON_FAILED_CLOSED');need(canOperate()===true,'COMPARISON_STOPPED');
  try{
   need(digest(codeIdentity)===digest(entryComparisonCodeIdentity()),'FROZEN_COMPARISON_CODE_CHANGED');
   need(hash(fs.readFileSync(bankFile))===bankBytesHash,'FROZEN_COMPARISON_BANK_CHANGED');
   const stat=fs.statSync(sourceFile);need(stat.ino===sourceStat.ino&&stat.birthtimeMs===sourceStat.birthtimeMs&&stat.size>=offset,'SOURCE_REPLACED_OR_TRUNCATED');
   need(guard(0,head.length)===headHash&&guard(offset-boundaryLength,boundaryLength)===boundaryHash,'SOURCE_CHANGED');
   if(stat.size===offset)return 0;
   const fd=fs.openSync(sourceFile,'r'),bytes=Buffer.alloc(Math.min(stat.size-offset,4*1024*1024));let n;
   try{n=fs.readSync(fd,bytes,0,bytes.length,offset);}finally{fs.closeSync(fd);}
   need(n>0,'SOURCE_READ_FAILED');offset+=n;
   const combined=Buffer.concat([partial,bytes.subarray(0,n)]),last=combined.lastIndexOf(10);let count=0,next=state;
   if(last>=0){
    for(const line of combined.subarray(0,last).toString('utf8').split('\n')){need(line.length>0,'SOURCE_EMPTY_RECORD');next=advanceEntryComparison(next,JSON.parse(line),bank);count++;}
    processedBytes+=last+1;partial=combined.subarray(last+1);
   }else partial=combined;
   need(partial.length<=8*1024*1024,'SOURCE_OVERSIZED_RECORD');
   if(count){need(canOperate()===true,'COMPARISON_STOPPED');journal.commit(next,{type:'source_advanced',sourceSequence:next.sourceSequence,sourceHash:next.lastRecordHash});state=next;}
   boundaryLength=Math.min(4096,offset);boundaryHash=guard(offset-boundaryLength,boundaryLength);
   return count;
  }catch(error){failed=true;throw error;}
 };
 const status=()=>({...entryComparisonReport(state,bank,new Date(now()).toISOString()),cohort,codeIdentity,hostVersion:'entry-comparison-streaming-v2',verifiedOriginalCheckpoints:compared,maxMarkAgeMs:bank.config.maxDataAgeMs+bank.config.cycleIntervalMs*4,
  sourceBytesConsumed:processedBytes,sourceBytesRead:offset,incompleteTailBytes:partial.length,journalSequence:journal.sequence,
  journalHash:journal.previous,networkRequests:0,competitionOrders:0});
 return {dir,bank,consume,status,sourceState:()=>structuredClone(state)};
}

export async function runEntryComparison({runtimeDir,cohort,initialize=false,once=false,now=Date.now,workspaceDir=workspace}={}){
 need(path.isAbsolute(runtimeDir)&&nameOk(cohort),'INVALID_COMPARISON_DIRECTORY');
 const dir=path.join(runtimeDir,'entry-comparisons',cohort);
 const stopped=()=>[path.join(workspaceDir,'STOP'),path.join(runtimeDir,'STOP'),path.join(runtimeDir,'ENTRY_COMPARISON_STOP'),path.join(dir,'STOP')].some(p=>fs.existsSync(p));
 need(!stopped(),'STOP_EXISTS');fs.mkdirSync(dir,{recursive:true,mode:0o700});
 const release=lock(path.join(dir,'worker.lock'));let ending=false,watch;
 const end=()=>ending=true;process.on('SIGINT',end);process.on('SIGTERM',end);
 const publish=status=>atomicJson(path.join(dir,'stats.json'),{...watch.status(),status,pid:process.pid,updatedAt:new Date(now()).toISOString()});
 try{
  watch=openEntryComparison({runtimeDir,cohort,initialize,now,canOperate:()=>!ending&&!stopped()});
  do{if(ending||stopped())break;watch.consume();publish(watch.status().incompleteTailBytes?'awaiting_capture':'running');if(once)break;await new Promise(r=>setTimeout(r,2000));}while(!ending&&!stopped());
  if(ending||stopped())publish('stopped');return watch.status();
 }catch{if(ending||stopped()){if(watch)publish('stopped');else atomicJson(path.join(dir,'stats.json'),{status:'stopped',mode:'entry_comparison_paper',updatedAt:new Date(now()).toISOString(),reason:'OPERATOR_STOP_DURING_REPLAY',networkRequests:0,competitionOrders:0});return;}if(watch)publish('failed');else atomicJson(path.join(dir,'stats.json'),{status:'failed',mode:'entry_comparison_paper',updatedAt:new Date(now()).toISOString(),reason:'COMPARISON_FAILED_CLOSED',networkRequests:0,competitionOrders:0});throw Error('Entry comparison failed; inspect its local evidence');}
 finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);release();}
}

if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const index=process.argv.indexOf('--name'),cohort=index>=0?process.argv[index+1]:null;
 try{await runEntryComparison({runtimeDir:process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),cohort,initialize:process.argv.includes('--init'),once:process.argv.includes('--once')});}
 catch{process.stderr.write('Entry comparison failed; inspect local status\n');process.exitCode=1;}
}
