import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {createBookPublicationState,applyBookPublication} from './book-publication.mjs';
import {createPairedPaper as createPairedPackage,warmPairedPaper as warmPairedPackage,advancePairedPaper as advancePairedPackage,pairedPaperReport as pairedPackageReport,pairedPaperHash as pairedPackageHash,PAIRED_PAPER_POLICY as PAIRED_PACKAGE_POLICY,PAIRED_PAPER_VARIANTS as PAIRED_PACKAGE_VARIANTS,PAIRED_PAPER_CONFIRMATION as PUBLIC_CONFIRMATION_POLICY} from './paired-package-paper-v3.mjs';
import {atomicPaperJson} from './paper-persistence.mjs';
const HOST_VERSION='session-aware-paired-v4';
const hostHash=()=>createHash('sha256').update(fs.readFileSync(fileURLToPath(import.meta.url))).digest('hex');
const workspace=path.dirname(path.dirname(fileURLToPath(import.meta.url))),MAX=8*1024*1024,need=(x,m)=>{if(!x)throw Error(m);};
const hash=x=>createHash('sha256').update(x).digest('hex'),read=p=>JSON.parse(fs.readFileSync(p)),name=x=>typeof x==='string'&&/^[a-z][a-z0-9-]{0,63}$/.test(x)&&!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(x);
export function pairedPackageCodeIdentity(){return Object.fromEntries(['live-paper/paired-package-watch-v3.mjs','live-paper/paired-package-paper-v3.mjs','live-paper/book-publication.mjs','live-paper/paper-persistence.mjs','live-paper/model.mjs','paper/fixed.mjs','feed/contract.mjs'].map(p=>[p,hash(fs.readFileSync(path.join(workspace,p)))]));}
function durable(file,bytes,flags){const f=fs.openSync(file,flags,0o600);try{let n=0;while(n<bytes.length){const written=fs.writeSync(f,bytes,n,bytes.length-n);need(written>0,'PAIRED_PACKAGE_WRITE');n+=written;}fs.fsyncSync(f);}finally{fs.closeSync(f);}}
function* rows(file,{start=0,size=fs.statSync(file).size,partial=false}={}){const f=fs.openSync(file,'r');let pos=start,offset=start,tail=Buffer.alloc(0);
 try{while(pos<size){const b=Buffer.alloc(Math.min(1024*1024,size-pos)),n=fs.readSync(f,b,0,b.length,pos);need(n>0,'PAIRED_PACKAGE_READ');pos+=n;const data=Buffer.concat([tail,b.subarray(0,n)]);let begin=0,end;
  while((end=data.indexOf(10,begin))>=0){const raw=data.subarray(begin,end+1);need(raw.length>1&&raw.length<=MAX,'PAIRED_PACKAGE_RECORD');offset+=raw.length;yield {row:JSON.parse(raw),raw,offset};begin=end+1;}tail=data.subarray(begin);need(tail.length<MAX,'PAIRED_PACKAGE_RECORD');}need(partial||tail.length===0,'PAIRED_PACKAGE_INCOMPLETE_JOURNAL');
 }finally{fs.closeSync(f);}}
function prefixHash(file,n){const h=createHash('sha256'),f=fs.openSync(file,'r');let offset=0;try{while(offset<n){const b=Buffer.alloc(Math.min(n-offset,1024*1024)),count=fs.readSync(f,b,0,b.length,offset);need(count>0,'PAIRED_PACKAGE_TRUNCATED');h.update(b.subarray(0,count));offset+=count;}return h.digest('hex');}finally{fs.closeSync(f);}}
function boundary(file,offset){const n=Math.min(4096,offset),b=Buffer.alloc(n),f=fs.openSync(file,'r');try{need(fs.readSync(f,b,0,n,offset-n)===n,'PAIRED_PACKAGE_TRUNCATED');return hash(b);}finally{fs.closeSync(f);}}
export function pairedPackageEvent(record,eventId,availableAt){
 const b=record.payload.book;return {kind:'book',eventId,tournamentId:record.tournamentId,marketId:b.marketId,exchangeId:b.exchangeId,
  origin:'rest_observation',sourceKind:'verified_public_rest_book_publication',publicationSequence:record.sequence,publicationHash:record.hash,publicationSessionId:record.sessionId,
  requestStartedAt:b.requestStartedAt,quoteReceivedAt:b.receivedAt,confirmedAt:record.at,availableAt,nextExpiryAt:null,expiryKnown:false,
  decisionEligible:true,continuity:{ready:true,continuous:true,evidence:'verified_authoritative_snapshot_not_continuous_trade_tape'},book:structuredClone(b)};
}

// This is an offline reader of the single collector's normalized publication.
// No network client, credential reader or account-order method exists here.
export function openPairedPackageWatch({runtimeDir,cohort,initialize=false,readOnly=false,universe,identities={},now=Date.now,workspaceDir=workspace,canOperate=()=>true}){
 need(!initialize,'PAIRED_V4_REQUIRES_EXISTING_V3_BANK');
 need(path.isAbsolute(runtimeDir)&&name(cohort)&&(!initialize||!readOnly),'PAIRED_PACKAGE_DIRECTORY');
 const dir=path.join(runtimeDir,'paired-package-cohorts',cohort),bankFile=path.join(dir,'bank.json'),journalFile=path.join(dir,'journal.jsonl'),sourceFile=path.join(runtimeDir,'public-books.jsonl'),lockFile=path.join(runtimeDir,'paired-package.lock');
 const stopped=()=>!canOperate()||['STOP','PAIRED_PACKAGE_STOP'].some(x=>fs.existsSync(path.join(runtimeDir,x)))||fs.existsSync(path.join(workspaceDir,'STOP'))||fs.existsSync(path.join(dir,'STOP'));
 need(readOnly||!stopped(),'PAIRED_PACKAGE_STOPPED');if(!readOnly)fs.mkdirSync(dir,{recursive:true,mode:0o700});
 let fd=null,claim,closed=false,failed=false;const owned=()=>{try{return !closed&&(readOnly||fs.readFileSync(lockFile,'utf8')===String(process.pid));}catch{return false;}};
 const close=()=>{if(closed)return;const ours=owned();closed=true;if(fd!==null){fs.closeSync(fd);if(ours){const s=fs.statSync(lockFile);if(s.ino===claim.ino&&s.birthtimeMs===claim.birthtimeMs)fs.unlinkSync(lockFile);}}};
 try{
  if(!readOnly){fd=fs.openSync(lockFile,'wx',0o600);claim=fs.fstatSync(fd);fs.writeFileSync(fd,String(process.pid));fs.fsyncSync(fd);}
  let bank=initialize?null:read(bankFile);universe=bank?.universe||universe;need(universe?.exchanges?.length>0,'PAIRED_PACKAGE_UNIVERSE');
  const codeIdentity=pairedPackageCodeIdentity(),hostIdentity=hostHash(),sourceIdentity=fs.statSync(sourceFile);let publication=createBookPublicationState(universe),sourceOffset=0,warm=[];
  const sourcePrefixSize=bank?.sourcePrefixBytes??sourceIdentity.size;need(sourcePrefixSize>0&&sourcePrefixSize<=512*1024*1024,'PAIRED_PACKAGE_PREFIX');
  for(const x of rows(sourceFile,{size:sourcePrefixSize,partial:initialize})){publication=applyBookPublication(publication,x.row);sourceOffset=x.offset;
   if(x.row.kind==='book')warm.push(pairedPackageEvent(x.row,0,x.row.at));}
  need(!publication.closed&&sourceOffset>0,'PAIRED_PACKAGE_SOURCE_CLOSED');
  if(initialize){const markets=publication.scope.exchanges.filter(x=>['381','382'].includes(x.marketId)).map(x=>({...identities[x.marketId],marketId:x.marketId,exchangeId:x.exchangeId}));
   bank={schemaVersion:1,mode:'sequential_dual_no_paper',cohort,tournamentId:universe.tournamentId,universe:structuredClone(universe),markets,
    cutoff:new Date(now()).toISOString(),startEventId:0,sourceScopeHash:publication.scopeHash,sourceSessionId:publication.sessionId,
    sourcePrefixBytes:sourceOffset,sourcePrefixSequence:publication.sequence,sourcePrefixJournalHash:publication.hash,sourcePrefixHash:prefixHash(sourceFile,sourceOffset),
    sourceFile:'public-books.jsonl',policy:structuredClone(PAIRED_PACKAGE_POLICY),variants:structuredClone(PAIRED_PACKAGE_VARIANTS),confirmationPolicy:structuredClone(PUBLIC_CONFIRMATION_POLICY),codeIdentity};
   need(!fs.existsSync(bankFile)&&!fs.existsSync(journalFile),'PAIRED_PACKAGE_COHORT_EXISTS');
  }else need(sourceOffset===bank.sourcePrefixBytes&&prefixHash(sourceFile,sourceOffset)===bank.sourcePrefixHash&&publication.sequence===bank.sourcePrefixSequence
    &&publication.hash===bank.sourcePrefixJournalHash&&publication.scopeHash===bank.sourceScopeHash&&publication.sessionId===bank.sourceSessionId
    &&pairedPackageHash(codeIdentity)===pairedPackageHash(bank.codeIdentity),'PAIRED_PACKAGE_FROZEN_IDENTITY');
  let state=warmPairedPackage(createPairedPackage(bank),warm,bank),journalSequence=0,journalHash=pairedPackageHash(bank),journalBytes=0,ended=false;
  const eventsFor=(record,availableAt)=>{
   let events=[];if(record.kind==='book')events=[pairedPackageEvent(record,state.lastEventId+1,availableAt)];
   else if(record.kind==='invalidation'||record.kind==='session_stop'||record.kind==='session_start'){
    const id=record.kind==='invalidation'?record.payload.marketId:null;
    events=bank.markets.filter(m=>id===null||m.marketId===id).map((m,i)=>({kind:'invalidate',eventId:state.lastEventId+i+1,tournamentId:bank.tournamentId,marketId:m.marketId,exchangeId:m.exchangeId,confirmedAt:record.at,availableAt,reason:record.kind==='session_start'?'SESSION_START':record.payload.reason}));
   }return events;
  };
  // One consumer timestamp per batch; historical replay reuses exactly that time.
  // Reduce complete batches once: all evidence resolves older orders first, and
  // decisions use the final verified quote available at actual consumption.
  function reduceBatch(sourceRows,availableAt,allowDecisions,continuity=false){
   const events=[];let eventId=state.lastEventId;
   for(const x of sourceRows){const r=x.row;if(!continuity)need(r.sessionId===bank.sourceSessionId&&r.kind!=='session_start','PAIRED_PACKAGE_SESSION_CHANGED');publication=applyBookPublication(publication,r);sourceOffset=x.offset;
    const mapped=eventsFor(r,availableAt);for(const e of mapped)e.eventId=++eventId;events.push(...mapped);if(r.kind==='session_stop')ended=true;if(continuity&&r.kind==='session_start')ended=false;}
   state=advancePairedPackage(state,events,bank,{availableAt,evidenceThrough:publication.lastAt,allowDecisions:allowDecisions&&!ended});
  }
  const expected=()=>({sourceBytes:sourceOffset,sourceSequence:publication.sequence,sourceHash:publication.hash,sourceStateHash:pairedPackageHash(publication),evidenceThrough:publication.lastAt,stateHash:pairedPackageHash(state)});
  if(!initialize){const inputRows=rows(sourceFile,{start:sourceOffset,partial:true});try{for(const saved of rows(journalFile)){const {hash:h,...body}=saved.row;
    need(h===pairedPackageHash(body)&&body.sequence===journalSequence+1&&body.previous===journalHash&&body.bankHash===pairedPackageHash(bank),'PAIRED_PACKAGE_JOURNAL_CHAIN');
    if(body.kind==='genesis')need(journalSequence===0&&body.stateHash===pairedPackageHash(state),'PAIRED_PACKAGE_GENESIS');
    else{need(body.kind==='batch'&&body.sourceSequence>publication.sequence,'PAIRED_PACKAGE_CHECKPOINT');const pending=[];while(publication.sequence+pending.length<body.sourceSequence){const x=inputRows.next();need(!x.done,'PAIRED_PACKAGE_SOURCE_TRUNCATED');pending.push(x.value);}if(body.hostVersion!==undefined)need(body.hostVersion===HOST_VERSION&&body.hostHash===hostIdentity,'PAIRED_V4_HOST_IDENTITY_CHANGED');reduceBatch(pending,body.availableAt,body.allowDecisions,body.hostVersion===HOST_VERSION);
     for(const [k,v]of Object.entries(expected()))need(pairedPackageHash(body[k])===pairedPackageHash(v),'PAIRED_PACKAGE_REPLAY_MISMATCH');}
    journalSequence++;journalHash=h;journalBytes=saved.offset;
   }}finally{inputRows.return();}need(journalSequence>0,'PAIRED_PACKAGE_GENESIS_MISSING');}
  const live=()=>need(!readOnly&&owned()&&!stopped(),'PAIRED_PACKAGE_STOPPED_OR_READ_ONLY');
  const append=body=>{live();need(fs.statSync(journalFile).size===journalBytes,'PAIRED_PACKAGE_JOURNAL_CHANGED');const row={sequence:journalSequence+1,previous:journalHash,bankHash:pairedPackageHash(bank),...body},h=pairedPackageHash(row),bytes=Buffer.from(JSON.stringify({...row,hash:h})+'\n');durable(journalFile,bytes,'a');journalSequence++;journalHash=h;journalBytes+=bytes.length;};
  if(initialize){live();durable(bankFile,Buffer.from(JSON.stringify(bank,null,2)+'\n'),'wx');durable(journalFile,Buffer.alloc(0),'wx');append({kind:'genesis',stateHash:pairedPackageHash(state)});}
  const bankFileHash=hash(fs.readFileSync(bankFile));let sourceBoundary=boundary(sourceFile,sourceOffset);
  const current=()=>!publication.closed;
  const consume=()=>{need(!closed&&!failed,'PAIRED_PACKAGE_FAILED_CLOSED');const before={publication,state,sourceOffset,ended};let committed=false;
   try{live();const stat=fs.statSync(sourceFile);
    need(stat.ino===sourceIdentity.ino&&stat.birthtimeMs===sourceIdentity.birthtimeMs&&stat.size>=sourceOffset,'PAIRED_PACKAGE_SOURCE_REPLACED');
    need(boundary(sourceFile,sourceOffset)===sourceBoundary&&hash(fs.readFileSync(bankFile))===bankFileHash&&pairedPackageHash(pairedPackageCodeIdentity())===pairedPackageHash(codeIdentity)&&hostHash()===hostIdentity,'PAIRED_PACKAGE_IDENTITY_CHANGED');
    const pending=[...rows(sourceFile,{start:sourceOffset,size:stat.size,partial:true})];if(!pending.length)return 0;
    const availableAt=new Date(now()).toISOString();reduceBatch(pending,availableAt,true,true);append({kind:'batch',availableAt,allowDecisions:!ended,hostVersion:HOST_VERSION,hostHash:hostIdentity,...expected()});committed=true;sourceBoundary=boundary(sourceFile,sourceOffset);return pending.length;
   }catch(e){if(!committed)({publication,state,sourceOffset,ended}=before);failed=true;throw e;}
  };
  return {dir,bank,consume,owned,current,stopped,close,status:()=>({...pairedPackageReport(state,bank,new Date(now()).toISOString()),cohort,
   sourceBytes:sourceOffset,sourceSequence:publication.sequence,sourceHash:publication.hash,sourceStateHash:pairedPackageHash(publication),sourceSessionId:publication.sessionId,sourceClosed:publication.closed,hostVersion:HOST_VERSION,hostHash:hostIdentity,
   evidenceThrough:publication.lastAt,journalSequence,journalHash,stateHash:pairedPackageHash(state),pid:readOnly?null:process.pid,readOnly,networkRequests:0,runtimeWrites:readOnly?0:undefined})};
 }catch(e){try{close();}catch{}throw e;}
}
export async function runPairedPackageWatch({runtimeDir,cohort,initialize=false,universe,identities={},once=false,now=Date.now,sleep=ms=>new Promise(r=>setTimeout(r,ms)),workspaceDir=workspace}){
 let ending=false,watch;const end=()=>ending=true;process.on('SIGINT',end);process.on('SIGTERM',end);
 const publish=async status=>{const report={...watch.status(),status};if(status!=='running')for(const w of Object.values(report.wallets)){if(w.positions.length){w.totalPnl=null;w.liquidationPnl=null;}w.marksComplete=w.positions.length===0;for(const p of w.positions){p.markReady=false;p.liquidationValue=null;}}
  await atomicPaperJson(path.join(watch.dir,'stats.json'),report,{canCommit:()=>watch.owned()&&(status!=='running'||!watch.stopped())});};
 try{watch=openPairedPackageWatch({runtimeDir,cohort,initialize,universe,identities,now,workspaceDir,canOperate:()=>!ending});
  do{if(watch.stopped())break;watch.consume();await publish(watch.current()?'running':'awaiting_source');if(once)break;await sleep(250);}while(true);
  if(watch.stopped())await publish('stopped');
 }catch{if(watch)try{await publish(watch.stopped()?'stopped':'failed');}catch{}throw Error('PAIRED_PACKAGE_WORKER_FAILED_CLOSED');}
 finally{process.removeListener('SIGINT',end);process.removeListener('SIGTERM',end);watch?.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const args=process.argv.slice(2),value=k=>{const i=args.indexOf(k);return i<0?undefined:args[i+1];};
 try{const runtimeDir=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),initialize=args.includes('--init');let universe,identities={};
  if(initialize)universe=read(path.join(workspace,'live-paper/price-universe.json'));
  await runPairedPackageWatch({runtimeDir,cohort:value('--name'),initialize,universe,identities,once:args.includes('--once')});
 }catch{process.stderr.write('Public recovery worker failed closed; inspect restricted local state\n');process.exitCode=1;}
}
