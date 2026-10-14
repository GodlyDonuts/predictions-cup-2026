import fs from 'node:fs';import path from 'node:path';import readline from 'node:readline';
import {createHash} from 'node:crypto';import {pathToFileURL} from 'node:url';
import {createMoloch,advanceMoloch,reportMoloch,assertMoloch} from './engine.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const verifiedTails=new WeakMap();
const byteHash=bytes=>createHash('sha256').update(bytes).digest('hex');
function prefixHash(file,bytes){const fd=fs.openSync(file,'r'),hash=createHash('sha256');try{let offset=0;while(offset<bytes){const chunk=Buffer.alloc(Math.min(bytes-offset,1048576)),n=fs.readSync(fd,chunk,0,chunk.length,offset);if(n!==chunk.length)throw Error('MOLOCH_VERIFIED_PARENT_SOURCE_SHORT');hash.update(chunk);offset+=n;}return hash.digest('hex');}finally{fs.closeSync(fd);}}
export function reuseCompletedTail(parent){
 const proof=verifiedTails.get(parent);if(!proof||parent.directory!==proof.directory||payloadHash(parent.snapshot)!==proof.snapshotHash)throw Error('MOLOCH_UNVERIFIED_PARENT');
 for(const file of proof.files){
  if(file.absent){if(fs.existsSync(file.path))throw Error('MOLOCH_VERIFIED_PARENT_CHANGED');continue;}
  const hash=file.prefix?prefixHash(file.path,file.bytes):byteHash(fs.readFileSync(file.path));
  if(hash!==file.hash)throw Error('MOLOCH_VERIFIED_PARENT_CHANGED');
 }
 return parent;
}

export function bookExportEvents(document){
  const {exportHash,...body}=document;
  if(exportHash!==payloadHash(body)||!Array.isArray(document.rows))throw Error('MOLOCH_EXPORT_HASH');
  let previous=null,sequence=null;const events=[];
  for(const row of document.rows){const {hash,...record}=row;if(hash!==payloadHash(record)||(previous!==null&&row.previousHash!==previous)||(sequence!==null&&row.sequence!==sequence+1))throw Error('MOLOCH_RECORD_CHAIN');previous=hash;sequence=row.sequence;
    if(row.kind==='book')events.push({type:'book',receivedAt:row.payload.book.receivedAt,book:row.payload.book});
    else if(row.kind==='invalidation'||row.kind==='session_stop')throw Error('MOLOCH_EXPORT_BOUNDARY_REQUIRES_EXPLICIT_GAP');
  }
  return events;
}
export function replayEvents(events,policy={}){
  const state=createMoloch(policy);for(const event of events)advanceMoloch(state,event);
  return{state,report:reportMoloch(state)};
}
export function journalDecoder(){let previous='',sequence=0;return row=>{const {hash,...body}=row;if(hash!==payloadHash(body)||body.sequence!==sequence+1||body.previous!==previous)throw Error('MOLOCH_SOURCE_JOURNAL_CHAIN');sequence++;previous=hash;return row.event;};}
export function entryPolicyTransition(state,ids){
 if(!Array.isArray(ids)||!ids.length)throw Error('MOLOCH_TAIL_ENTRY_TRANSITION');
 const {policy,policyHash,...financial}=state,next=createMoloch({...policy,entryExchangeIds:ids}).policy;
 state.policy=next;state.policyHash=payloadHash(next);const {policy:p,policyHash:h,...after}=state;if(payloadHash(financial)!==payloadHash(after))throw Error('MOLOCH_TAIL_FINANCIAL_TRANSITION');
 return{entryExchangeIds:ids,priorPolicyHash:policyHash,nextPolicyHash:state.policyHash};
}
export function drainPolicyTransition(state){const {policy,policyHash,...before}=state;state.policy=createMoloch({...policy,entriesEnabled:false}).policy;state.policyHash=payloadHash(state.policy);const {policy:p,policyHash:h,...after}=state;if(payloadHash(before)!==payloadHash(after))throw Error('MOLOCH_TAIL_DRAIN_FINANCIAL');return{priorPolicyHash:policyHash,nextPolicyHash:state.policyHash,entriesEnabled:false};}
export function flatEntryTransition(state,executionAge=false,exitPolicy=null,entryPolicy=null){assertMoloch(state);if(state.orders.some(q=>!q.retired)||Object.values(state.positions).some(p=>[...p.YES,...p.NO].some(l=>l.quantity>0)))throw Error('MOLOCH_TAIL_REENTRY_REQUIRES_FLAT');if(exitPolicy!==null&&(!exitPolicy||typeof exitPolicy!=='object'||Array.isArray(exitPolicy)||!Object.keys(exitPolicy).length||Object.keys(exitPolicy).some(k=>!['inventoryTakerAfterMs','inventoryMinimumGain','maximumInventoryAgeMs','inventoryExitMode'].includes(k))))throw Error('MOLOCH_TAIL_EXIT_POLICY_SCOPE');if(entryPolicy!==null&&(!entryPolicy||typeof entryPolicy!=='object'||Array.isArray(entryPolicy)||Object.keys(entryPolicy).length!==1||!Object.hasOwn(entryPolicy,'insideTicks')||![0,1].includes(entryPolicy.insideTicks)))throw Error('MOLOCH_TAIL_ENTRY_POLICY_SCOPE');const {policy,policyHash,...before}=state;state.policy=createMoloch({...policy,...exitPolicy,...entryPolicy,entriesEnabled:true,...(executionAge?{inventoryAgeClock:'execution'}:{})}).policy;state.policyHash=payloadHash(state.policy);const {policy:p,policyHash:h,...after}=state;if(payloadHash(before)!==payloadHash(after))throw Error('MOLOCH_TAIL_REENTRY_FINANCIAL');return{priorPolicyHash:policyHash,nextPolicyHash:state.policyHash,entriesEnabled:true,inventoryAgeClock:state.policy.inventoryAgeClock??null,...(exitPolicy?{exitPolicy:structuredClone(exitPolicy)}:{}),...(entryPolicy?{entryPolicy:structuredClone(entryPolicy)}:{})};}
export function completedTail(directory,visited=new Set()){
 const dir=fs.realpathSync(directory);if(visited.has(dir)||visited.size>=64)throw Error('MOLOCH_TAIL_CONTINUATION_CYCLE');visited.add(dir);
 const snapshotBytes=fs.readFileSync(path.join(dir,'state.json')),reportBytes=fs.readFileSync(path.join(dir,'report.json'));
 const snapshot=JSON.parse(snapshotBytes),{hash,...body}=snapshot,report=JSON.parse(reportBytes);
 if(hash!==payloadHash(body)||report.mode!=='local_paper_tail'||report.eventJournalHash!==body.lastHash||report.source.bytes!==body.source.consumedBytes||report.source.sha256!==body.source.consumedSha256)throw Error('MOLOCH_TAIL_TERMINAL_PROOF');
 if(!Number.isSafeInteger(body.source.consumedBytes)||body.source.consumedBytes<0)throw Error('MOLOCH_TAIL_SOURCE_LENGTH');
 let state,ancestorFiles=[];
 if(body.continuation){const parent=completedTail(body.continuation.parentDirectory,visited);ancestorFiles=verifiedTails.get(parent).files;if(parent.snapshot.hash!==body.continuation.parentSnapshotHash)throw Error('MOLOCH_TAIL_PARENT_HASH');state=structuredClone(parent.snapshot.state);if(body.continuation.entryPolicyTransition){const expected=body.continuation.entryPolicyTransition,actual=entryPolicyTransition(state,expected.entryExchangeIds);if(payloadHash(actual)!==payloadHash(expected))throw Error('MOLOCH_TAIL_ENTRY_TRANSITION_PROOF');}if(body.continuation.drainPolicyTransition){const actual=drainPolicyTransition(state);if(payloadHash(actual)!==payloadHash(body.continuation.drainPolicyTransition))throw Error('MOLOCH_TAIL_DRAIN_TRANSITION_PROOF');}}
 else state=createMoloch(snapshot.state.policy);
 if(body.continuation?.flatEntryTransition){const expected=body.continuation.flatEntryTransition,actual=flatEntryTransition(state,expected.inventoryAgeClock==='execution',expected.exitPolicy??null,expected.entryPolicy??null);if(payloadHash(actual)!==payloadHash(expected))throw Error('MOLOCH_TAIL_REENTRY_PROOF');}
 const source=fs.openSync(body.source.path,'r'),digest=createHash('sha256');let remaining=body.source.consumedBytes,offset=0;
 try{while(remaining){const b=Buffer.alloc(Math.min(remaining,1024*1024)),n=fs.readSync(source,b,0,b.length,offset);if(n!==b.length)throw Error('MOLOCH_TAIL_SOURCE_SHORT');digest.update(b);remaining-=n;offset+=n;}}finally{fs.closeSync(source);}
 if(digest.digest('hex')!==body.source.consumedSha256)throw Error('MOLOCH_TAIL_SOURCE_CHANGED');
 let previous='',sequence=0;const journalExists=fs.existsSync(path.join(dir,'events.jsonl')),journalBytes=journalExists?fs.readFileSync(path.join(dir,'events.jsonl')):Buffer.alloc(0),raw=journalBytes.toString('utf8');
 if(raw&&!raw.endsWith('\n'))throw Error('MOLOCH_TAIL_PARTIAL_JOURNAL');
 for(const line of raw.trim().split(/\r?\n/).filter(Boolean)){const row=JSON.parse(line),{hash:h,...b}=row;if(h!==payloadHash(b)||b.previous!==previous||b.sequence!==sequence+1)throw Error('MOLOCH_TAIL_JOURNAL_CHAIN');advanceMoloch(state,b.event);if(payloadHash(state)!==b.stateHash)throw Error('MOLOCH_TAIL_REPLAY_STATE');previous=h;sequence++;}
 if(sequence!==body.sequence||previous!==body.lastHash||payloadHash(state)!==payloadHash(snapshot.state))throw Error('MOLOCH_TAIL_REPLAY_TERMINAL');assertMoloch(state);
 const result={directory:dir,snapshot};
 verifiedTails.set(result,{directory:dir,snapshotHash:payloadHash(snapshot),files:[...ancestorFiles,
  {path:path.join(dir,'state.json'),hash:byteHash(snapshotBytes)},
  {path:path.join(dir,'report.json'),hash:byteHash(reportBytes)},
  journalExists?{path:path.join(dir,'events.jsonl'),hash:byteHash(journalBytes)}:{path:path.join(dir,'events.jsonl'),absent:true},
  {path:body.source.path,prefix:true,bytes:body.source.consumedBytes,hash:body.source.consumedSha256}]});
 return result;
}
function options(args){const o={format:'ndjson',follow:false};for(let i=0;i<args.length;i++){const k=args[i];if(k==='--follow')o.follow=true;else if(k==='--drain')o.drain=true;else if(k==='--resume-entries')o.resumeEntries=true;else if(k==='--execution-age')o.executionAge=true;else if(['--input','--output','--format','--policy','--seconds','--resume','--entry-exchanges','--exit-policy','--entry-policy'].includes(k)){if(!args[i+1])throw Error('Missing '+k);o[k.slice(2)]=args[++i];}else throw Error('Unknown option '+k);}if(!o.input||!o.output)throw Error('Usage: node moloch/run.mjs --input FILE --output NEW_DIRECTORY [--format book-export] [--policy FILE] [--follow --seconds 60]');return o;}
export async function run(args,{processingClock=null,verifiedParent=null}={}){
  if(processingClock!==null&&typeof processingClock!=='function')throw Error('MOLOCH_PROCESSING_CLOCK');
  const o=options(args),input=path.resolve(o.input),out=path.resolve(o.output);
  if(fs.existsSync(out))throw Error('MOLOCH_OUTPUT_EXISTS: preserve previous run; use a new directory');
  if(!['ndjson','journal','book-export'].includes(o.format)||(o.follow&&!['ndjson','journal'].includes(o.format)))throw Error('MOLOCH_FORMAT');
  const seconds=o.seconds===undefined?null:Number(o.seconds);if(seconds!==null&&(!Number.isFinite(seconds)||seconds<=0))throw Error('MOLOCH_DURATION');
  if(o.resume&&(!o.follow||o.format!=='journal'||o.policy))throw Error('MOLOCH_TAIL_RESUME_SCOPE');
  if(verifiedParent&&(!o.resume||fs.realpathSync(o.resume)!==verifiedParent.directory))throw Error('MOLOCH_VERIFIED_PARENT_SCOPE');
  const parent=o.resume?(verifiedParent?reuseCompletedTail(verifiedParent):completedTail(o.resume)):null,continuation=parent?{parentDirectory:parent.directory,parentSnapshotHash:parent.snapshot.hash,newPaperCapital:'0.000000'}:null;
  const policy=o.policy?JSON.parse(fs.readFileSync(o.policy,'utf8')):{},state=parent?structuredClone(parent.snapshot.state):createMoloch(policy);
  if(o['entry-exchanges']){if(!parent)throw Error('MOLOCH_TAIL_ENTRY_REQUIRES_RESUME');continuation.entryPolicyTransition=entryPolicyTransition(state,o['entry-exchanges'].split(','));}
  if(o.drain){if(!parent)throw Error('MOLOCH_TAIL_DRAIN_REQUIRES_RESUME');continuation.drainPolicyTransition=drainPolicyTransition(state);}
  if((o.executionAge||o['exit-policy']||o['entry-policy'])&&!o.resumeEntries||o.resumeEntries&&(o.drain||!parent))throw Error('MOLOCH_TAIL_REENTRY_SCOPE');
  if(o.resumeEntries)continuation.flatEntryTransition=flatEntryTransition(state,o.executionAge===true,o['exit-policy']?JSON.parse(fs.readFileSync(o['exit-policy'],'utf8')):null,o['entry-policy']?JSON.parse(fs.readFileSync(o['entry-policy'],'utf8')):null);
  fs.mkdirSync(out,{recursive:true});const startedAt=new Date().toISOString(),source=fs.statSync(input),sourceHash=createHash('sha256');
  const journal=path.join(out,'events.jsonl');let previous='',count=0,bytes=0,preCutoffEventsSkipped=0;
  const decode=o.format==='journal'?journalDecoder():row=>row;
  const save=()=>{const body={state,source:{path:input,dev:source.dev,ino:source.ino,consumedBytes:bytes,consumedSha256:sourceHash.copy().digest('hex')},sequence:count,lastHash:previous,...(continuation?{continuation}:{})};const temp=path.join(out,'state.tmp');fs.writeFileSync(temp,JSON.stringify({...body,hash:payloadHash(body)}));fs.renameSync(temp,path.join(out,'state.json'));};
  const accept=event=>{
    // Validate on a copy, so a failed event cannot partially alter the durable wallet.
    const candidate=structuredClone(state);advanceMoloch(candidate,event);
    const body={sequence:count+1,previous,event,stateHash:payloadHash(candidate)};const row={...body,hash:payloadHash(body)};
    fs.appendFileSync(journal,JSON.stringify(row)+'\n');count++;previous=row.hash;Object.assign(state,candidate);
  };
  try{
    if(o.format==='book-export'){
      const raw=fs.readFileSync(input);sourceHash.update(raw);bytes=raw.length;
      for(const event of bookExportEvents(JSON.parse(raw)))accept(event);
    }else if(!o.follow){
      const stream=fs.createReadStream(input);stream.on('data',b=>{sourceHash.update(b);bytes+=b.length;});
      for await(const line of readline.createInterface({input:stream,crlfDelay:Infinity}))if(line.trim())accept(decode(JSON.parse(line)));
    }else{
      let offset=0,parsedBytes=0,pending=Buffer.alloc(0);const deadline=seconds===null?Infinity:Date.now()+seconds*1000;
      const fd=fs.openSync(input,'r');
      try{while(Date.now()<deadline&&!fs.existsSync(path.join(out,'STOP'))){
        const st=fs.statSync(input);if(st.dev!==source.dev||st.ino!==source.ino||st.size<offset)throw Error('MOLOCH_SOURCE_REPLACED');
        if(st.size>offset){const n=Math.min(st.size-offset,1024*1024),chunk=Buffer.alloc(n),read=fs.readSync(fd,chunk,0,n,offset);if(read!==n)throw Error('MOLOCH_SOURCE_SHORT_READ');offset+=n;sourceHash.update(chunk);bytes+=n;pending=Buffer.concat([pending,chunk]);
          let newline;while((newline=pending.indexOf(10))>=0){parsedBytes+=newline+1;const line=pending.subarray(0,newline).toString('utf8');pending=Buffer.from(pending.subarray(newline+1));if(line.trim()){const event=decode(JSON.parse(line));const alreadyConsumed=parent&&input===parent.snapshot.source.path&&parsedBytes<=parent.snapshot.source.consumedBytes;if(alreadyConsumed||!parent&&Date.parse(event.receivedAt)<Date.parse(startedAt)){preCutoffEventsSkipped++;continue;}const processedAt=processingClock?await processingClock(event,count+1):new Date().toISOString();accept({...event,processedAt});}}
          if(pending.length>1024*1024)throw Error('MOLOCH_EVENT_TOO_LARGE');save();
        }else await new Promise(resolve=>setTimeout(resolve,250));
      }}finally{fs.closeSync(fd);}
      if(pending.length)throw Error('MOLOCH_PARTIAL_FINAL_EVENT');
    }
    save();const report={...reportMoloch(state),mode:o.follow?'local_paper_tail':'historical_paper_replay',startedAt,
      source:{path:input,sha256:sourceHash.digest('hex'),bytes,format:o.format},eventJournalHash:previous,
      externalSourceProvenanceVerified:o.format==='book-export'?'bounded_export_hash_and_record_chain_only':o.format==='journal'?'source_journal_hash_chain_only':false,
      prospectiveCutoff:o.follow?startedAt:null,preCutoffEventsSkipped,continuation,
      sourcePrefixNotIndependentlyReproduced:true,liveConnectionVerified:false};
    fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));return report;
  }catch(error){save();fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({at:new Date().toISOString(),error:error.message,events:count}));throw error;}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href)run(process.argv.slice(2)).catch(e=>{console.error(e.message);process.exitCode=1;});
