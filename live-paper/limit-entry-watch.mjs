import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {digest} from './model.mjs';
import {atomicPaperJson} from './paper-persistence.mjs';
import {createLimitEntry,emptyLimitEntryState,advanceLimitEntry,limitEntryReport} from './limit-entry.mjs';

const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const CHUNK=1024*1024,MAX_RECORD=16*1024*1024;
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const need=(ok,code)=>{if(!ok)throw Error(code);};
const nameOk=x=>typeof x==='string'&&/^[a-z][a-z0-9-]{0,63}$/.test(x)&&!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(x);
const read=file=>JSON.parse(fs.readFileSync(file,'utf8'));

// Freeze the actual local import closure, including this host and persistence.
// Built-in modules do not depend on mutable workspace source files.
export function limitEntryCodeIdentity(){
 const found=new Map();
 function visit(file){
  const relative=path.relative(workspace,file).replaceAll('\\','/');
  need(relative&&!relative.startsWith('../')&&!path.isAbsolute(relative),'CODE_OUTSIDE_WORKSPACE');
  if(found.has(relative))return;
  const bytes=fs.readFileSync(file);found.set(relative,sha(bytes));
  const source=bytes.toString('utf8'),imports=source.matchAll(/(?:import|export)\s+(?:[^'";]+?\s+from\s+)?['"]([^'"]+)['"]/g);
  for(const [,specifier]of imports)if(specifier.startsWith('.'))visit(path.resolve(path.dirname(file),specifier));
 }
 visit(fileURLToPath(import.meta.url));
 return Object.fromEntries([...found].sort(([a],[b])=>a.localeCompare(b)));
}
function writeAll(fd,bytes){let offset=0;while(offset<bytes.length){const n=fs.writeSync(fd,bytes,offset,bytes.length-offset);need(Number.isInteger(n)&&n>0&&n<=bytes.length-offset,'DURABLE_WRITE_FAILED');offset+=n;}fs.fsyncSync(fd);}
function createFile(file,bytes){const fd=fs.openSync(file,'wx',0o600);try{writeAll(fd,bytes);}finally{fs.closeSync(fd);}}

// Read complete UTF-8 records with bounded memory. The snapshot size is fixed;
// an incomplete concurrent append is left for a subsequent incremental read.
function scan(file,{size=fs.statSync(file).size,allowPartial=false,onRow=()=>{}}={}){
 const fd=fs.openSync(file,'r'),prefix=createHash('sha256');let offset=0,completeBytes=0,partial=Buffer.alloc(0),sequence=0,previous='',lastAt=-Infinity;
 try{
  while(offset<size){
   const b=Buffer.alloc(Math.min(CHUNK,size-offset)),n=fs.readSync(fd,b,0,b.length,offset);need(n>0,'SOURCE_READ_FAILED');offset+=n;
   const all=Buffer.concat([partial,b.subarray(0,n)]);let start=0,end;
   while((end=all.indexOf(10,start))>=0){
    const raw=all.subarray(start,end+1);need(raw.length>1&&raw.length<=MAX_RECORD,'INVALID_RECORD_SIZE');
    const row=JSON.parse(raw.toString('utf8')),{hash,...body}=row,at=Date.parse(body.at);
    need(hash===digest(body)&&body.sequence===sequence+1&&body.previous===previous&&Number.isFinite(at)&&at>=lastAt,'SOURCE_CHAIN_INVALID');
    prefix.update(raw);completeBytes+=raw.length;sequence++;previous=hash;lastAt=at;
    onRow(row,{prefixHash:prefix.copy().digest('hex'),completeBytes});start=end+1;
   }
   partial=all.subarray(start);need(partial.length<=MAX_RECORD,'SOURCE_OVERSIZED_RECORD');
  }
 }finally{fs.closeSync(fd);}
 need(allowPartial||partial.length===0,'JOURNAL_INCOMPLETE');
 return {sequence,hash:previous,prefixHash:prefix.digest('hex'),completeBytes,lastAt,partialBytes:partial.length};
}

// Local append-only files are the trust boundary. Head/boundary guards detect
// replacement and ordinary rewrites; arbitrary old-middle edits during append
// require the full source/checkpoint replay performed on every restart.
// This process owns only limit-entry-cohorts/<name>. It has no API capability.
export function openLimitEntry({runtimeDir,cohort,initialize=false,now=Date.now,workspaceDir=workspace,canOperate=()=>true}={}){
 need(path.isAbsolute(runtimeDir)&&nameOk(cohort),'INVALID_LIMIT_ENTRY_DIRECTORY');
 const dir=path.join(runtimeDir,'limit-entry-cohorts',cohort),bankFile=path.join(dir,'bank.json'),journalFile=path.join(dir,'journal.jsonl'),sourceFile=path.join(runtimeDir,'journal.jsonl');
 const stopped=()=>canOperate()!==true||[path.join(workspaceDir,'STOP'),path.join(runtimeDir,'STOP'),path.join(runtimeDir,'LIMIT_ENTRY_STOP'),path.join(dir,'STOP')].some(p=>fs.existsSync(p));
 need(!stopped(),'LIMIT_ENTRY_STOPPED');fs.mkdirSync(dir,{recursive:true,mode:0o700});
 const lockFile=path.join(dir,'worker.lock'),lockFd=fs.openSync(lockFile,'wx',0o600);let closed=false,failed=false,createdLock;
 try{createdLock=fs.fstatSync(lockFd);writeAll(lockFd,Buffer.from(String(process.pid)));}catch(e){
  try{fs.closeSync(lockFd);}catch{}
  try{const current=fs.statSync(lockFile);if(createdLock&&current.ino===createdLock.ino&&current.birthtimeMs===createdLock.birthtimeMs)fs.unlinkSync(lockFile);}catch{}
  throw e;
 }
 const owned=()=>{try{return !closed&&fs.readFileSync(lockFile,'utf8')===String(process.pid);}catch{return false;}};
 const close=()=>{if(closed)return;const ours=owned();closed=true;try{fs.closeSync(lockFd);}finally{if(ours)fs.unlinkSync(lockFile);}};
 try{
  const codeIdentity=limitEntryCodeIdentity(),sourceStat=fs.statSync(sourceFile),sourceSize=sourceStat.size;
  let bank,state,journalSequence=0,journalHash='',processedBytes=0;
  const checkpoints=[],availability=[],newAvailability=[];
  function recordAvailability(list,sequence,at){const last=list.at(-1);if(last?.at===at&&last.through===sequence-1)last.through=sequence;else list.push({from:sequence,through:sequence,at});}
  if(initialize){
   need(!fs.existsSync(bankFile)&&!fs.existsSync(journalFile),'FROZEN_LIMIT_ENTRY_ALREADY_EXISTS');
   const prefix=scan(sourceFile,{size:sourceSize,allowPartial:true});need(prefix.sequence>0,'SOURCE_PREFIX_EMPTY');
   const base=read(path.join(runtimeDir,'dip-responsive-bank.json')),cutoff=new Date(now()).toISOString();
   need(prefix.lastAt<=Date.parse(cutoff),'FUTURE_SOURCE_PREFIX');
   bank=createLimitEntry({sourceRecords:prefix.sequence,sourceHash:prefix.prefixHash,cutoff,config:{...base.config,marketIds:['377','354','277','305']},universe:base.universe,identities:base.identities,policy:base.policy,codeIdentity}).bank;
  }else{
   need(fs.existsSync(bankFile)&&fs.existsSync(journalFile),'LIMIT_ENTRY_ACCOUNTING_MISSING');bank=read(bankFile);
   let prior=0;
   const saved=scan(journalFile,{onRow:row=>{
    need(Number.isSafeInteger(row.state?.sourceSequence)&&row.state.sourceSequence>prior&&Array.isArray(row.event?.availability),'INVALID_CHECKPOINT_SEQUENCE');
    let next=prior+1;
    for(const range of row.event.availability){const clock=Date.parse(range.at),previousClock=Date.parse(availability.at(-1)?.at||range.at);
     need(Number.isSafeInteger(range.from)&&range.from===next&&Number.isSafeInteger(range.through)&&range.through>=range.from&&range.through<=row.state.sourceSequence&&Number.isFinite(clock)&&new Date(clock).toISOString()===range.at&&clock>=previousClock&&clock<=Date.parse(row.at),'INVALID_HOST_AVAILABILITY');
     availability.push(range);next=range.through+1;
    }
    need(next===row.state.sourceSequence+1,'HOST_AVAILABILITY_INCOMPLETE');prior=row.state.sourceSequence;checkpoints.push({sourceSequence:prior,stateHash:digest(row.state)});
   }});
   need(saved.sequence>0,'LIMIT_ENTRY_ACCOUNTING_EMPTY');journalSequence=saved.sequence;journalHash=saved.hash;
  }
  need(digest(bank.codeIdentity)===digest(codeIdentity),'FROZEN_LIMIT_ENTRY_CODE_CHANGED');state=emptyLimitEntryState(bank);let compared=0,prefixVerified=false,availabilityIndex=0,lastAvailable=-Infinity;
  const replay=scan(sourceFile,{size:sourceSize,allowPartial:true,onRow:(row,meta)=>{
   if(row.sequence===bank.sourceRecords){need(meta.prefixHash===bank.sourceHash,'FROZEN_SOURCE_PREFIX_CHANGED');prefixVerified=true;}
   while(availability[availabilityIndex]&&availability[availabilityIndex].through<row.sequence)availabilityIndex++;
   const savedClock=availability[availabilityIndex],availableAt=savedClock&&savedClock.from<=row.sequence?savedClock.at:initialize&&row.sequence<=bank.sourceRecords?bank.cutoff:new Date(now()).toISOString();
   need(Date.parse(availableAt)>=lastAvailable,'HOST_CLOCK_ROLLBACK');lastAvailable=Date.parse(availableAt);
   if(!savedClock)recordAvailability(newAvailability,row.sequence,availableAt);
   state=advanceLimitEntry(state,row,bank,{availableAt});
   if(checkpoints[compared]?.sourceSequence===row.sequence){need(checkpoints[compared].stateHash===digest(state),'LIMIT_ENTRY_REPLAY_MISMATCH');compared++;}
  }});
  need(prefixVerified&&compared===checkpoints.length,'LIMIT_ENTRY_REPLAY_INCOMPLETE');processedBytes=replay.completeBytes;
  const live=()=>need(owned()&&!stopped(),'LIMIT_ENTRY_STOPPED');
  function append(next,type,available){live();const at=new Date(now()).toISOString();need(Date.parse(at)>=lastAvailable,'HOST_CLOCK_ROLLBACK');const body={sequence:journalSequence+1,previous:journalHash,at,event:{type,sourceSequence:next.sourceSequence,sourceHash:next.lastRecordHash,availability:available},state:next},hash=digest(body);
   const fd=fs.openSync(journalFile,'a',0o600);try{writeAll(fd,Buffer.from(JSON.stringify({...body,hash})+'\n'));}finally{fs.closeSync(fd);}journalSequence++;journalHash=hash;
  }
  live();need(digest(codeIdentity)===digest(limitEntryCodeIdentity()),'FROZEN_LIMIT_ENTRY_CODE_CHANGED');const currentSourceStat=fs.statSync(sourceFile);need(currentSourceStat.ino===sourceStat.ino&&currentSourceStat.birthtimeMs===sourceStat.birthtimeMs&&currentSourceStat.size>=sourceSize,'SOURCE_REPLACED_OR_TRUNCATED');
  if(initialize){createFile(bankFile,Buffer.from(JSON.stringify(bank,null,2)+'\n'));createFile(journalFile,Buffer.alloc(0));}
  if(initialize||checkpoints.at(-1)?.sourceSequence!==state.sourceSequence)append(state,initialize?'cohort_frozen':'source_advanced',newAvailability);
  const bankBytesHash=sha(fs.readFileSync(bankFile));let offset=processedBytes,partial=Buffer.alloc(0),lastSourceAt=replay.lastAt;
  const guard=(at,length)=>{const fd=fs.openSync(sourceFile,'r'),bytes=Buffer.alloc(length);try{need(fs.readSync(fd,bytes,0,length,at)===length,'SOURCE_CHANGED');}finally{fs.closeSync(fd);}return sha(bytes);};
  const headLength=Math.min(4096,offset),headHash=guard(0,headLength);let boundaryLength=Math.min(4096,offset),boundaryHash=guard(offset-boundaryLength,boundaryLength);
  const consume=()=>{
   need(!failed&&!closed,'LIMIT_ENTRY_FAILED_CLOSED');
   try{
    live();need(digest(codeIdentity)===digest(limitEntryCodeIdentity())&&sha(fs.readFileSync(bankFile))===bankBytesHash,'FROZEN_LIMIT_ENTRY_CODE_CHANGED');
    const stat=fs.statSync(sourceFile);need(stat.ino===sourceStat.ino&&stat.birthtimeMs===sourceStat.birthtimeMs&&stat.size>=offset,'SOURCE_REPLACED_OR_TRUNCATED');
    need(guard(0,headLength)===headHash&&guard(offset-boundaryLength,boundaryLength)===boundaryHash,'SOURCE_CHANGED');
    if(stat.size===offset)return 0;
    const fd=fs.openSync(sourceFile,'r'),bytes=Buffer.alloc(Math.min(CHUNK,stat.size-offset));let n;try{n=fs.readSync(fd,bytes,0,bytes.length,offset);}finally{fs.closeSync(fd);}need(n>0,'SOURCE_READ_FAILED');
    const combined=Buffer.concat([partial,bytes.subarray(0,n)]),available=[];let count=0,start=0,end,next=state,nextSourceAt=lastSourceAt;
    while((end=combined.indexOf(10,start))>=0){need(end+1-start>1&&end+1-start<=MAX_RECORD,'INVALID_RECORD_SIZE');const row=JSON.parse(combined.subarray(start,end).toString('utf8')),availableAt=new Date(now()).toISOString(),sourceAt=Date.parse(row.at);need(Number.isFinite(sourceAt)&&sourceAt>=nextSourceAt,'SOURCE_CLOCK_ROLLBACK');nextSourceAt=sourceAt;need(Date.parse(availableAt)>=lastAvailable,'HOST_CLOCK_ROLLBACK');lastAvailable=Date.parse(availableAt);next=advanceLimitEntry(next,row,bank,{availableAt});recordAvailability(available,row.sequence,availableAt);count++;start=end+1;}
    const pending=combined.subarray(start);need(pending.length<=MAX_RECORD,'SOURCE_OVERSIZED_RECORD');
    if(count){append(next,'source_advanced',available);state=next;processedBytes+=start;lastSourceAt=nextSourceAt;}
    offset+=n;partial=pending;boundaryLength=Math.min(4096,offset);boundaryHash=guard(offset-boundaryLength,boundaryLength);return count;
   }catch(e){failed=true;throw e;}
  };
  const status=()=>({...limitEntryReport(state,bank,new Date(now()).toISOString()),cohort,codeIdentity,sourceBytesConsumed:processedBytes,sourceBytesRead:offset,incompleteTailBytes:partial.length,journalSequence,journalHash,networkRequests:0,competitionOrders:0});
  return {dir,bank,consume,status,sourceState:()=>structuredClone(state),close,stopped,owned};
 }catch(e){try{close();}catch{}throw e;}
}

export async function runLimitEntry({runtimeDir,cohort,initialize=false,once=false,now=Date.now,workspaceDir=workspace,sleep=ms=>new Promise(r=>setTimeout(r,ms))}={}){
 let ending=false,watch;const end=()=>ending=true;process.on('SIGINT',end);process.on('SIGTERM',end);
 async function publish(status){await atomicPaperJson(path.join(watch.dir,'stats.json'),{...watch.status(),status,pid:process.pid,updatedAt:new Date(now()).toISOString()},{canCommit:()=>watch.owned()&&(status!=='running'||!watch.stopped())});}
 try{
  watch=openLimitEntry({runtimeDir,cohort,initialize,now,workspaceDir,canOperate:()=>!ending});
  do{if(watch.stopped())break;watch.consume();await publish(watch.status().incompleteTailBytes?'awaiting_capture':'running');if(once)break;await sleep(1000);}while(!watch.stopped());
  if(watch.stopped())await publish('stopped');return watch.status();
 }catch(e){if(watch){try{await publish(watch.stopped()?'stopped':'failed');}catch{}}throw Error('LIMIT_ENTRY_FAILED_CLOSED');}
 finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);if(watch)watch.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const index=process.argv.indexOf('--name'),cohort=index>=0?process.argv[index+1]:null;
 try{await runLimitEntry({runtimeDir:process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),cohort,initialize:process.argv.includes('--init'),once:process.argv.includes('--once')});}
 catch{process.stderr.write('Limit-entry worker failed; inspect local evidence\n');process.exitCode=1;}
}
