import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {createStreamDip,warmStreamDip,advanceStreamDip,streamDipReport,streamDipHash,STREAM_DIP_POLICY} from './stream-dip.mjs';
import {replayStreamDip} from './stream-dip-replay-v2.mjs';
import {advanceStreamBookAdapter,streamDigest} from './stream-book-adapter-v2.mjs';
import {streamBookCodeIdentity} from './stream-book-watch-v2.mjs';
import {atomicPaperJson} from './paper-persistence.mjs';
const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url))),MAX=8*1024*1024;
const hash=x=>createHash('sha256').update(x).digest('hex'),read=p=>JSON.parse(fs.readFileSync(p,'utf8')),need=(x,m)=>{if(!x)throw Error(m);};
const name=x=>typeof x==='string'&&/^[a-z][a-z0-9-]{0,63}$/.test(x)&&!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(x);
export function streamDipCodeIdentity(){return {...streamBookCodeIdentity(),...Object.fromEntries(['live-paper/stream-dip.mjs','live-paper/stream-dip-replay-v2.mjs','live-paper/stream-dip-watch-v2.mjs'].map(p=>[p,hash(fs.readFileSync(path.join(workspace,p)))]))};}
function durable(file,bytes,flags){const fd=fs.openSync(file,flags,0o600);try{let i=0;while(i<bytes.length){const n=fs.writeSync(fd,bytes,i,bytes.length-i);need(n>0,'STREAM_DIP_WRITE');i+=n;}fs.fsyncSync(fd);}finally{fs.closeSync(fd);}}
function* rows(file,{start=0,size=fs.statSync(file).size,partial=false}={}){const fd=fs.openSync(file,'r');let pos=start,offset=start,tail=Buffer.alloc(0);
 try{while(pos<size){const b=Buffer.alloc(Math.min(1024*1024,size-pos)),n=fs.readSync(fd,b,0,b.length,pos);need(n>0,'STREAM_DIP_READ');pos+=n;
  const data=Buffer.concat([tail,b.subarray(0,n)]);let begin=0,end;while((end=data.indexOf(10,begin))>=0){const raw=data.subarray(begin,end+1);need(raw.length<=MAX&&raw.length>1,'STREAM_DIP_RECORD_SIZE');offset+=raw.length;yield {raw,row:JSON.parse(raw),offset};begin=end+1;}
  tail=data.subarray(begin);need(tail.length<MAX,'STREAM_DIP_RECORD_SIZE');}need(partial||tail.length===0,'STREAM_DIP_INCOMPLETE_JOURNAL');
 }finally{fs.closeSync(fd);}}
function prefix(file,size){const b=Buffer.alloc(size),fd=fs.openSync(file,'r');try{let i=0;while(i<size){const n=fs.readSync(fd,b,i,size-i,i);need(n>0,'STREAM_DIP_TRUNCATED');i+=n;}}finally{fs.closeSync(fd);}return b;}
const sameFile=(file,identity,minSize)=>{const s=fs.statSync(file);need(s.ino===identity.ino&&s.birthtimeMs===identity.birthtimeMs&&s.size>=minSize,'STREAM_DIP_SOURCE_REPLACED');return s;};

export function openStreamDipWatch({runtimeDir,cohort,inputCohort,initialize=false,readOnly=false,markets,now=Date.now,workspaceDir=workspace,canOperate=()=>true}){
 need(path.isAbsolute(runtimeDir)&&name(cohort),'STREAM_DIP_DIRECTORY');const dir=path.join(runtimeDir,'stream-dip-v2-cohorts',cohort),bankFile=path.join(dir,'bank.json'),journalFile=path.join(dir,'journal.jsonl'),lockFile=path.join(runtimeDir,'stream-dip.lock');
 let sourceRows=null;
 const stopped=()=>!canOperate()||['STOP','STREAM_DIP_V2_STOP'].some(x=>fs.existsSync(path.join(runtimeDir,x)))||fs.existsSync(path.join(workspaceDir,'STOP'))||fs.existsSync(path.join(dir,'STOP'));
 need(!readOnly||!initialize,'STREAM_DIP_READ_ONLY_INIT');if(!readOnly){need(!stopped(),'STREAM_DIP_V2_STOPPED');fs.mkdirSync(dir,{recursive:true,mode:0o700});}
 const fd=readOnly?null:fs.openSync(lockFile,'wx',0o600),claim=readOnly?null:fs.fstatSync(fd);let closed=false,failed=false;
 if(!readOnly)try{fs.writeFileSync(fd,String(process.pid));fs.fsyncSync(fd);}catch(e){fs.closeSync(fd);try{const s=fs.statSync(lockFile);if(s.ino===claim.ino&&s.birthtimeMs===claim.birthtimeMs)fs.unlinkSync(lockFile);}catch{}throw e;}
 const owned=()=>{try{return !closed&&(readOnly||fs.readFileSync(lockFile,'utf8')===String(process.pid));}catch{return false;}},close=()=>{if(closed)return;const ours=owned();closed=true;sourceRows?.return();if(!readOnly){fs.closeSync(fd);if(ours)fs.unlinkSync(lockFile);}};
 try{
  let bank=initialize?null:read(bankFile);inputCohort=bank?.inputCohort||inputCohort;need(name(inputCohort),'STREAM_DIP_INPUT_NAME');
  const inputDir=path.join(runtimeDir,'stream-book-v2-cohorts',inputCohort),inputBankFile=path.join(inputDir,'bank.json'),inputBank=read(inputBankFile),inputJournal=path.join(inputDir,'journal.jsonl');
  need(inputBank.cohort===inputCohort&&streamDigest(inputBank.codeIdentity)===streamDigest(streamBookCodeIdentity()),'STREAM_DIP_INPUT_IDENTITY');
  need(/^realtime-[A-Za-z0-9._-]+\.jsonl$/.test(inputBank.sourceFile),'STREAM_DIP_CAPTURE_NAME');const sourceFile=path.join(runtimeDir,inputBank.sourceFile),sourceIdentity=fs.statSync(sourceFile),inputIdentity=fs.statSync(inputJournal),codeIdentity=streamDipCodeIdentity();
  let records=[],inputBytes=0;const prefixSize=bank?.inputPrefixBytes??inputIdentity.size;
  need(prefixSize>0&&prefixSize<=512*1024*1024,'STREAM_DIP_PREFIX_SIZE');
  for(const x of rows(inputJournal,{size:prefixSize,partial:initialize})){records.push(x.row);inputBytes=x.offset;}
  need(records.length>0,'STREAM_DIP_INPUT_EMPTY');const inputPrefixHash=hash(prefix(inputJournal,inputBytes));
  if(bank)need(inputBytes===bank.inputPrefixBytes&&inputPrefixHash===bank.inputPrefixHash&&streamDipHash(codeIdentity)===streamDipHash(bank.codeIdentity),'STREAM_DIP_FROZEN_IDENTITY');
  // Verify actual raw capture, input checkpoints, every derived event and state.
  const verified=replayStreamDip({inputBank,source:prefix(sourceFile,sourceIdentity.size),checkpoints:records,markets:bank?.markets||markets||inputBank.scope.markets});
  let adapter=verified.verifiedInputState,sourceOffset=verified.report.sourceBytes,inputSequence=records.at(-1).sequence,inputHash=records.at(-1).hash,evidenceThrough=new Date(Number(BigInt(adapter.capture.lastObservedNs)/1000000n)).toISOString();
  if(initialize){bank={schemaVersion:1,mode:'stream_native_dip_paper',inputVersion:2,cohort,inputCohort,tournamentId:inputBank.scope.tournamentId,inputBankHash:streamDigest(inputBank),
   cutoff:new Date(now()).toISOString(),startEventId:adapter.lastEventId,markets:structuredClone(markets||inputBank.scope.markets),policy:structuredClone(STREAM_DIP_POLICY),
   inputPrefixBytes:inputBytes,inputPrefixHash,inputPrefixSequence:inputSequence,inputPrefixJournalHash:inputHash,sourceFile:inputBank.sourceFile,scope:structuredClone(inputBank.scope),
   sourcePrefixBytes:sourceOffset,sourcePrefixHash:hash(prefix(sourceFile,sourceOffset)),sourceHash:hash(prefix(sourceFile,sourceOffset)),codeIdentity};
   need(!fs.existsSync(bankFile)&&!fs.existsSync(journalFile),'STREAM_DIP_COHORT_EXISTS');
  }else need(bank.inputBankHash===streamDigest(inputBank)&&bank.sourcePrefixBytes===sourceOffset&&bank.sourcePrefixHash===hash(prefix(sourceFile,sourceOffset)),'STREAM_DIP_SOURCE_PREFIX');
  const warm=[...verified.warm,...records.flatMap(x=>(x.events||[]).filter(e=>e.kind==='book'))];let state=warmStreamDip(createStreamDip(bank),warm,bank),journalSequence=0,journalHash=streamDipHash(bank),journalBytes=0;
  const verifyCheckpoint=(saved,expected)=>{const {hash:h,...body}=saved.row;need(h===streamDipHash(body)&&body.sequence===journalSequence+1&&body.previous===journalHash&&body.bankHash===streamDipHash(bank),'STREAM_DIP_JOURNAL_CHAIN');
   for(const [k,v]of Object.entries(expected))need(streamDipHash(body[k])===streamDipHash(v),'STREAM_DIP_REPLAY_MISMATCH');journalSequence++;journalHash=h;journalBytes=saved.offset;};
  function advanceInput(saved){const {hash:h,...body}=saved.row;need(body.kind==='capture'&&h===streamDigest(body)&&body.bankHash===streamDigest(inputBank)&&body.sequence===inputSequence+1&&body.previous===inputHash,'STREAM_DIP_INPUT_CHAIN');
   sourceRows??=rows(sourceFile,{start:sourceOffset,partial:true});let x=sourceRows.next();if(x.done){sourceRows.return();sourceRows=rows(sourceFile,{start:sourceOffset,partial:true});x=sourceRows.next();}
   need(!x.done&&hash(x.value.raw)===body.sourceLineHash&&x.value.row.captureIndex===body.sourceCaptureIndex,'STREAM_DIP_CAPTURE_CHANGED');
   const out=advanceStreamBookAdapter(adapter,x.value.row,inputBank,{availableAt:body.availableAt});need(streamDigest(out.events)===streamDigest(body.events)&&streamDigest(out.state)===body.stateHash,'STREAM_DIP_INPUT_REPLAY');
   adapter=out.state;sourceOffset=x.value.offset;inputBytes=saved.offset;inputSequence=body.sequence;inputHash=h;
   evidenceThrough=new Date(Number(BigInt(adapter.capture.lastObservedNs)/1000000n)).toISOString();return out.events;}
  if(!initialize){const inputRows=rows(inputJournal,{start:inputBytes,partial:true});
   try{for(const saved of rows(journalFile)){if(saved.row.kind==='genesis'){need(journalSequence===0,'STREAM_DIP_GENESIS');verifyCheckpoint(saved,{kind:'genesis',stateHash:streamDipHash(state)});continue;}
    need(saved.row.kind==='batch'&&Number.isSafeInteger(saved.row.inputSequence)&&saved.row.inputSequence>inputSequence,'STREAM_DIP_CHECKPOINT_KIND');const events=[];
    while(inputSequence<saved.row.inputSequence){const next=inputRows.next();need(!next.done,'STREAM_DIP_INPUT_TRUNCATED');events.push(...advanceInput(next.value));}
    state=advanceStreamDip(state,events,bank,{availableAt:saved.row.availableAt,evidenceThrough,allowDecisions:saved.row.allowDecisions});
    verifyCheckpoint(saved,{kind:'batch',inputSequence,inputBytes,inputHash,evidenceThrough,stateHash:streamDipHash(state)});
   }}finally{inputRows.return();}need(journalSequence>0,'STREAM_DIP_GENESIS_MISSING');}
  const live=()=>{need(!readOnly,'STREAM_DIP_READ_ONLY');need(owned()&&!stopped(),'STREAM_DIP_V2_STOPPED');};
  const append=body=>{live();need(fs.statSync(journalFile).size===journalBytes,'STREAM_DIP_JOURNAL_CHANGED');const record={sequence:journalSequence+1,previous:journalHash,bankHash:streamDipHash(bank),...body},h=streamDipHash(record),bytes=Buffer.from(JSON.stringify({...record,hash:h})+'\n');
   durable(journalFile,bytes,'a');journalSequence++;journalHash=h;journalBytes+=bytes.length;};
  if(!readOnly)live();if(initialize){durable(bankFile,Buffer.from(JSON.stringify(bank,null,2)+'\n'),'wx');durable(journalFile,Buffer.alloc(0),'wx');append({kind:'genesis',stateHash:streamDipHash(state)});}
  const bankFileHash=hash(fs.readFileSync(bankFile)),inputBankFileHash=hash(fs.readFileSync(inputBankFile));
  let sourceBoundaryLength=Math.min(4096,sourceOffset),inputBoundaryLength=Math.min(4096,inputBytes);
  const sliceHash=(file,start,n)=>{const b=Buffer.alloc(n),f=fs.openSync(file,'r');try{need(fs.readSync(f,b,0,n,start)===n,'STREAM_DIP_TRUNCATED');}finally{fs.closeSync(f);}return hash(b);};
  let sourceBoundaryHash=sliceHash(sourceFile,sourceOffset-sourceBoundaryLength,sourceBoundaryLength),inputBoundaryHash=sliceHash(inputJournal,inputBytes-inputBoundaryLength,inputBoundaryLength);
  const current=()=>{try{const s=read(path.join(runtimeDir,'realtime-session.json'));return s.file===inputBank.sourceFile&&streamDigest(s.scope)===streamDigest(inputBank.scope)&&!adapter.capture.sourceEnded;}catch{return false;}};
  const healthy=()=>{try{const s=read(path.join(inputDir,'stats.json')),age=now()-Date.parse(s.updatedAt);process.kill(s.pid,0);return s.status==='running'&&s.bankHash===streamDigest(inputBank)&&s.cohort===inputCohort&&age>=0&&age<5000&&fs.readFileSync(path.join(runtimeDir,'stream-book-gateway.lock'),'utf8')===String(s.pid);}catch{return false;}};
  const consume=()=>{need(!closed&&!failed,'STREAM_DIP_FAILED_CLOSED');const before={adapter,sourceOffset,inputBytes,inputSequence,inputHash,evidenceThrough};let committed=false;try{live();need(current(),'STREAM_DIP_SESSION_ENDED');
   need(hash(fs.readFileSync(bankFile))===bankFileHash&&hash(fs.readFileSync(inputBankFile))===inputBankFileHash&&streamDipHash(streamDipCodeIdentity())===streamDipHash(codeIdentity),'STREAM_DIP_CODE_CHANGED');
   const inputStat=sameFile(inputJournal,inputIdentity,inputBytes);sameFile(sourceFile,sourceIdentity,sourceOffset);
   need(sliceHash(sourceFile,sourceOffset-sourceBoundaryLength,sourceBoundaryLength)===sourceBoundaryHash&&sliceHash(inputJournal,inputBytes-inputBoundaryLength,inputBoundaryLength)===inputBoundaryHash,'STREAM_DIP_SOURCE_CHANGED');
   const events=[];let count=0;for(const x of rows(inputJournal,{start:inputBytes,size:inputStat.size,partial:true})){events.push(...advanceInput(x));count++;}
   if(!count)return 0;const availableAt=new Date(now()).toISOString(),allowDecisions=healthy(),next=advanceStreamDip(state,events,bank,{availableAt,evidenceThrough,allowDecisions});
   append({kind:'batch',inputSequence,inputBytes,inputHash,evidenceThrough,availableAt,allowDecisions,stateHash:streamDipHash(next)});state=next;committed=true;
   sourceBoundaryLength=Math.min(4096,sourceOffset);inputBoundaryLength=Math.min(4096,inputBytes);sourceBoundaryHash=sliceHash(sourceFile,sourceOffset-sourceBoundaryLength,sourceBoundaryLength);inputBoundaryHash=sliceHash(inputJournal,inputBytes-inputBoundaryLength,inputBoundaryLength);return count;
  }catch(e){if(!committed){({adapter,sourceOffset,inputBytes,inputSequence,inputHash,evidenceThrough}=before);sourceRows?.return();sourceRows=null;}failed=true;throw e;}};
  return {dir,bank,consume,current,healthy,stopped,owned,close,status:()=>({...streamDipReport(state,bank,new Date(now()).toISOString()),cohort,inputCohort,inputSequence,inputBytes,inputHash,sourceBytes:sourceOffset,
   evidenceThrough,journalSequence,journalHash,stateHash:streamDipHash(state),inputHealthy:healthy(),pid:readOnly?null:process.pid,
   readOnly,networkRequests:0,runtimeWrites:readOnly?0:undefined})};
 }catch(e){try{close();}catch{}throw e;}
}
export async function runStreamDipWatch({runtimeDir,cohort,inputCohort,initialize=false,markets,once=false,now=Date.now,sleep=ms=>new Promise(r=>setTimeout(r,ms)),workspaceDir=workspace}){
 let ending=false,watch;const end=()=>ending=true;process.on('SIGINT',end);process.on('SIGTERM',end);
 const publish=async status=>{const report={...watch.status(),status};if(status!=='running')for(const w of Object.values(report.wallets)){if(w.positions.length)w.totalPnl=null;w.marksComplete=w.positions.length===0;for(const p of w.positions){p.markReady=false;p.liquidationValue=null;}}
  await atomicPaperJson(path.join(watch.dir,'stats.json'),report,{canCommit:()=>watch.owned()&&(status!=='running'||!watch.stopped())});};
 try{watch=openStreamDipWatch({runtimeDir,cohort,inputCohort,initialize,markets,now,workspaceDir,canOperate:()=>!ending});
  do{if(watch.stopped()||!watch.current())break;watch.consume();await publish(watch.healthy()?'running':'awaiting_verified_input');if(once)break;await sleep(250);}while(true);
  if(watch.stopped()||!watch.current())await publish(watch.stopped()?'stopped':'source_session_ended');
 }catch{if(watch)try{await publish(watch.stopped()?'stopped':'failed');}catch{}throw Error('STREAM_DIP_WORKER_FAILED_CLOSED');}
 finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);watch?.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),value=k=>{const i=args.indexOf(k);return i<0?undefined:args[i+1];};
 try{const runtimeDir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');let markets;
  if(args.includes('--init')){const old=read(path.join(runtimeDir,'limit-entry-cohorts/limit-pricecap-20261002/bank.json'));markets=old.config.marketIds.map(marketId=>({...old.identities[marketId],marketId,exchangeId:old.universe.exchanges.find(x=>x.marketId===marketId&&x.option==='YES').exchangeId}));}
  await runStreamDipWatch({runtimeDir,cohort:value('--name'),inputCohort:value('--input'),initialize:args.includes('--init'),markets,once:args.includes('--once')});
 }catch{process.stderr.write('Stream dip worker failed closed; inspect restricted local state\n');process.exitCode=1;}
}
