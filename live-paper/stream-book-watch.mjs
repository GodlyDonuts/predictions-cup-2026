import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {createStreamBookAdapter,advanceStreamBookAdapter,streamBookReport,streamDigest} from './stream-book-adapter.mjs';
import {atomicPaperJson} from './paper-persistence.mjs';
const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url))),MAX=8*1024*1024;
const hash=x=>createHash('sha256').update(x).digest('hex'),read=p=>JSON.parse(fs.readFileSync(p,'utf8')),need=(x,m)=>{if(!x)throw Error(m);};
const validName=x=>typeof x==='string'&&/^[a-z][a-z0-9-]{0,63}$/.test(x)&&!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(x);
export function streamBookCodeIdentity(){
 const files=['live-paper/stream-book-watch.mjs','live-paper/stream-book-adapter.mjs','live-paper/passive-capture.mjs','live-paper/passive-observer.mjs','feed/contract.mjs','paper/fixed.mjs','live-paper/paper-persistence.mjs'];
 return Object.fromEntries(files.map(p=>[p,hash(fs.readFileSync(path.join(workspace,p)))]));
}
function durable(file,bytes,flags){const fd=fs.openSync(file,flags,0o600);try{let i=0;while(i<bytes.length){const n=fs.writeSync(fd,bytes,i,bytes.length-i);need(n>0,'STREAM_WRITE_FAILED');i+=n;}fs.fsyncSync(fd);}finally{fs.closeSync(fd);}}
function* rows(file,{size=fs.statSync(file).size,partial=false}={}){
 const fd=fs.openSync(file,'r');let pos=0,offset=0,tail=Buffer.alloc(0);
 try{while(pos<size){const b=Buffer.alloc(Math.min(1024*1024,size-pos)),n=fs.readSync(fd,b,0,b.length,pos);need(n>0,'STREAM_READ_FAILED');pos+=n;const bytes=Buffer.concat([tail,b.subarray(0,n)]);let start=0,end;
  while((end=bytes.indexOf(10,start))>=0){const raw=bytes.subarray(start,end+1);need(raw.length>1&&raw.length<=MAX,'STREAM_RECORD_SIZE');offset+=raw.length;yield {raw,row:JSON.parse(raw.toString('utf8')),offset};start=end+1;}tail=bytes.subarray(start);need(tail.length<=MAX,'STREAM_RECORD_SIZE');}
  need(partial||tail.length===0,'STREAM_JOURNAL_INCOMPLETE');
 }finally{fs.closeSync(fd);}
}
export function openStreamBookWatch({runtimeDir,cohort,initialize=false,now=Date.now,workspaceDir=workspace,canOperate=()=>true}){
 need(path.isAbsolute(runtimeDir)&&validName(cohort),'INVALID_STREAM_DIRECTORY');
 const dir=path.join(runtimeDir,'stream-book-cohorts',cohort),lockFile=path.join(runtimeDir,'stream-book-gateway.lock'),bankFile=path.join(dir,'bank.json'),journalFile=path.join(dir,'journal.jsonl');
 const stopped=()=>!canOperate()||[path.join(workspaceDir,'STOP'),path.join(runtimeDir,'STOP'),path.join(runtimeDir,'STREAM_BOOK_STOP'),path.join(dir,'STOP')].some(p=>fs.existsSync(p));
 need(!stopped(),'STREAM_STOPPED');fs.mkdirSync(dir,{recursive:true,mode:0o700});
 const lock=fs.openSync(lockFile,'wx',0o600),newClaim=fs.fstatSync(lock);let closed=false,failed=false;
 try{fs.writeFileSync(lock,String(process.pid));fs.fsyncSync(lock);}catch(e){fs.closeSync(lock);try{const claim=fs.statSync(lockFile);if(claim.ino===newClaim.ino&&claim.birthtimeMs===newClaim.birthtimeMs)fs.unlinkSync(lockFile);}catch{}throw e;}
 const owned=()=>{try{return !closed&&fs.readFileSync(lockFile,'utf8')===String(process.pid);}catch{return false;}},close=()=>{if(closed)return;const ours=owned();closed=true;fs.closeSync(lock);if(ours)fs.unlinkSync(lockFile);};
 try{
  const codeIdentity=streamBookCodeIdentity(),session=read(path.join(runtimeDir,'realtime-session.json'));
  need(typeof session.file==='string'&&/^realtime-[A-Za-z0-9._-]+\.jsonl$/.test(session.file),'STREAM_SOURCE_NAME');
  const sourceFile=path.join(runtimeDir,session.file),sourceStat=fs.statSync(sourceFile);let bank;
  if(initialize){
   need(!fs.existsSync(bankFile)&&!fs.existsSync(journalFile),'STREAM_COHORT_EXISTS');let records=0,prefixBytes=0;const prefixHash=createHash('sha256');
   for(const x of rows(sourceFile,{size:sourceStat.size,partial:true})){need(x.row.captureIndex===records++,'STREAM_CAPTURE_INDEX');prefixHash.update(x.raw);prefixBytes=x.offset;}
   need(records>0,'STREAM_SOURCE_EMPTY');const sourceHash=prefixHash.digest('hex');
   bank={schemaVersion:1,mode:'verified_stream_book_input',cohort,cutoff:new Date(now()).toISOString(),sourcePrefixRecords:records,sourcePrefixBytes:prefixBytes,sourceHash,
    sourceFile:session.file,sessionId:hash(session.file).slice(0,32),scope:session.scope,codeIdentity};
  }else bank=read(bankFile);
  need(bank.cohort===cohort&&bank.sourceFile===session.file&&streamDigest(bank.scope)===streamDigest(session.scope)&&streamDigest(bank.codeIdentity)===streamDigest(codeIdentity),'STREAM_FROZEN_IDENTITY');
  let state=createStreamBookAdapter(bank),offset=0,journalSequence=0,journalHash=streamDigest(bank),journalBytes=0;
  const journal=initialize?null:rows(journalFile),first=journal?.next();
  if(!initialize)need(!first.done&&first.value.row.kind==='prefix','STREAM_PREFIX_CHECKPOINT_MISSING');
  let nextSaved=null,prefixComplete=false;const prefixHash=createHash('sha256');
  function verifyCheckpoint(saved,expected){const {hash:actual,...body}=saved.row;need(actual===streamDigest(body)&&body.sequence===journalSequence+1&&body.previous===journalHash&&body.bankHash===streamDigest(bank),'STREAM_JOURNAL_CHAIN');
   for(const [k,v]of Object.entries(expected))need(streamDigest(body[k])===streamDigest(v),'STREAM_REPLAY_MISMATCH');journalSequence++;journalHash=actual;journalBytes=saved.offset;}
  try{for(const x of rows(sourceFile,{size:sourceStat.size,partial:true})){
   if(x.row.captureIndex<bank.sourcePrefixRecords){prefixHash.update(x.raw);state=advanceStreamBookAdapter(state,x.row,bank,{availableAt:bank.cutoff}).state;offset=x.offset;
    if(x.row.captureIndex===bank.sourcePrefixRecords-1){need(offset===bank.sourcePrefixBytes&&prefixHash.digest('hex')===bank.sourceHash,'STREAM_PREFIX_CHANGED');prefixComplete=true;
     if(!initialize){verifyCheckpoint(first.value,{kind:'prefix',stateHash:streamDigest(state)});nextSaved=journal.next();}}
    continue;
   }
   if(initialize||nextSaved.done)break;
   const saved=nextSaved.value.row;need(saved.kind==='capture'&&saved.sourceLineHash===hash(x.raw),'STREAM_SOURCE_CHANGED');
   const out=advanceStreamBookAdapter(state,x.row,bank,{availableAt:saved.availableAt});
   verifyCheckpoint(nextSaved.value,{kind:'capture',sourceCaptureIndex:x.row.captureIndex,events:out.events,stateHash:streamDigest(out.state)});state=out.state;offset=x.offset;nextSaved=journal.next();
  }}finally{journal?.return();}
  need(prefixComplete&&(initialize||nextSaved.done),'STREAM_REPLAY_INCOMPLETE');
  const live=()=>need(owned()&&!stopped(),'STREAM_STOPPED');
  need(streamDigest(codeIdentity)===streamDigest(streamBookCodeIdentity()),'STREAM_CODE_CHANGED');live();
  const append=(body)=>{live();need(fs.statSync(journalFile).size===journalBytes,'STREAM_JOURNAL_CHANGED');const record={sequence:journalSequence+1,previous:journalHash,bankHash:streamDigest(bank),...body},h=streamDigest(record),bytes=Buffer.from(JSON.stringify({...record,hash:h})+'\n');
   durable(journalFile,bytes,'a');journalSequence++;journalHash=h;journalBytes+=bytes.length;};
  if(initialize){durable(bankFile,Buffer.from(JSON.stringify(bank,null,2)+'\n'),'wx');durable(journalFile,Buffer.alloc(0),'wx');append({kind:'prefix',stateHash:streamDigest(state)});}
  const bankHash=hash(fs.readFileSync(bankFile)),headLength=Math.min(offset,4096);
  const sliceHash=(start,n)=>{const fd=fs.openSync(sourceFile,'r'),b=Buffer.alloc(n);try{need(fs.readSync(fd,b,0,n,start)===n,'STREAM_SOURCE_CHANGED');}finally{fs.closeSync(fd);}return hash(b);};
  const headHash=sliceHash(0,headLength);let boundaryLength=Math.min(offset,4096),boundaryHash=sliceHash(offset-boundaryLength,boundaryLength);
  const current=()=>{const s=read(path.join(runtimeDir,'realtime-session.json'));return s.file===bank.sourceFile&&streamDigest(s.scope)===streamDigest(bank.scope)&&!state.capture.sourceEnded;};
  const consume=()=>{need(!failed&&!closed,'STREAM_FAILED_CLOSED');try{live();need(current(),'STREAM_SESSION_ENDED');
   need(hash(fs.readFileSync(bankFile))===bankHash&&streamDigest(codeIdentity)===streamDigest(streamBookCodeIdentity()),'STREAM_CODE_CHANGED');
   const stat=fs.statSync(sourceFile);need(stat.ino===sourceStat.ino&&stat.birthtimeMs===sourceStat.birthtimeMs&&stat.size>=offset,'STREAM_SOURCE_REPLACED');
   need(sliceHash(0,headLength)===headHash&&sliceHash(offset-boundaryLength,boundaryLength)===boundaryHash,'STREAM_SOURCE_CHANGED');
   if(stat.size===offset)return 0;
   const fd=fs.openSync(sourceFile,'r'),b=Buffer.alloc(Math.min(MAX,stat.size-offset));let n;try{n=fs.readSync(fd,b,0,b.length,offset);}finally{fs.closeSync(fd);}need(n>0,'STREAM_READ_FAILED');
   let start=0,end,count=0;while((end=b.indexOf(10,start))>=0&&end<n){const raw=b.subarray(start,end+1),row=JSON.parse(raw.toString('utf8')),availableAt=new Date(now()).toISOString(),out=advanceStreamBookAdapter(state,row,bank,{availableAt});
    append({kind:'capture',sourceCaptureIndex:row.captureIndex,sourceLineHash:hash(raw),availableAt,events:out.events,stateHash:streamDigest(out.state)});
    state=out.state;offset+=raw.length;start=end+1;count++;}
   need(n-start<MAX,'STREAM_RECORD_SIZE');boundaryLength=Math.min(offset,4096);boundaryHash=sliceHash(offset-boundaryLength,boundaryLength);return count;
  }catch(e){failed=true;throw e;}};
  return {dir,bank,consume,current,stopped,owned,close,status:()=>({...streamBookReport(state,bank,new Date(now()).toISOString()),cohort,sourceFile:bank.sourceFile,sourceBytes:offset,journalSequence,journalHash,pid:process.pid})};
 }catch(e){try{close();}catch{}throw e;}
}
export async function runStreamBookWatch({runtimeDir,cohort,initialize=false,once=false,now=Date.now,sleep=ms=>new Promise(r=>setTimeout(r,ms)),workspaceDir=workspace}){
 let ending=false,watch;const end=()=>ending=true;process.on('SIGINT',end);process.on('SIGTERM',end);
 const publish=async status=>{const report={...watch.status(),status};if(status!=='running')report.markets=report.markets.map(m=>({...m,ready:false}));await atomicPaperJson(path.join(watch.dir,'stats.json'),report,{canCommit:()=>watch.owned()&&(status!=='running'||!watch.stopped())});};
 try{watch=openStreamBookWatch({runtimeDir,cohort,initialize,now,workspaceDir,canOperate:()=>!ending});
  do{if(watch.stopped()||!watch.current())break;watch.consume();await publish('running');if(once)break;await sleep(250);}while(true);
  if(watch.stopped()||!watch.current())await publish(watch.stopped()?'stopped':'source_session_ended');
 }catch{if(watch)try{await publish(watch.stopped()?'stopped':'failed');}catch{}throw Error('STREAM_GATEWAY_FAILED_CLOSED');}
 finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);watch?.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const i=process.argv.indexOf('--name');try{await runStreamBookWatch({runtimeDir:process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),cohort:i<0?null:process.argv[i+1],initialize:process.argv.includes('--init'),once:process.argv.includes('--once')});}
 catch{process.stderr.write('Stream gateway failed closed; inspect restricted local state\n');process.exitCode=1;}
}
