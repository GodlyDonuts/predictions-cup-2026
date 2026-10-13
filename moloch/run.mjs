import fs from 'node:fs';import path from 'node:path';import readline from 'node:readline';
import {createHash} from 'node:crypto';import {pathToFileURL} from 'node:url';
import {createMoloch,advanceMoloch,reportMoloch} from './engine.mjs';
import {payloadHash} from '../execution/ledger.mjs';

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
function options(args){const o={format:'ndjson',follow:false};for(let i=0;i<args.length;i++){const k=args[i];if(k==='--follow')o.follow=true;else if(['--input','--output','--format','--policy','--seconds'].includes(k)){if(!args[i+1])throw Error('Missing '+k);o[k.slice(2)]=args[++i];}else throw Error('Unknown option '+k);}if(!o.input||!o.output)throw Error('Usage: node moloch/run.mjs --input FILE --output NEW_DIRECTORY [--format book-export] [--policy FILE] [--follow --seconds 60]');return o;}
export async function run(args){
  const o=options(args),input=path.resolve(o.input),out=path.resolve(o.output);
  if(fs.existsSync(out))throw Error('MOLOCH_OUTPUT_EXISTS: preserve previous run; use a new directory');
  if(!['ndjson','book-export'].includes(o.format)||(o.follow&&o.format!=='ndjson'))throw Error('MOLOCH_FORMAT');
  const seconds=o.seconds===undefined?null:Number(o.seconds);if(seconds!==null&&(!Number.isFinite(seconds)||seconds<=0))throw Error('MOLOCH_DURATION');
  const policy=o.policy?JSON.parse(fs.readFileSync(o.policy,'utf8')):{},state=createMoloch(policy);
  fs.mkdirSync(out,{recursive:true});const startedAt=new Date().toISOString(),source=fs.statSync(input),sourceHash=createHash('sha256');
  const journal=path.join(out,'events.jsonl');let previous='',count=0,bytes=0,preCutoffEventsSkipped=0;
  const save=()=>{const body={state,source:{path:input,dev:source.dev,ino:source.ino,consumedBytes:bytes,consumedSha256:sourceHash.copy().digest('hex')},sequence:count,lastHash:previous};const temp=path.join(out,'state.tmp');fs.writeFileSync(temp,JSON.stringify({...body,hash:payloadHash(body)}));fs.renameSync(temp,path.join(out,'state.json'));};
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
      for await(const line of readline.createInterface({input:stream,crlfDelay:Infinity}))if(line.trim())accept(JSON.parse(line));
    }else{
      let offset=0,pending=Buffer.alloc(0);const deadline=seconds===null?Infinity:Date.now()+seconds*1000;
      const fd=fs.openSync(input,'r');
      try{while(Date.now()<deadline&&!fs.existsSync(path.join(out,'STOP'))){
        const st=fs.statSync(input);if(st.dev!==source.dev||st.ino!==source.ino||st.size<offset)throw Error('MOLOCH_SOURCE_REPLACED');
        if(st.size>offset){const n=Math.min(st.size-offset,1024*1024),chunk=Buffer.alloc(n),read=fs.readSync(fd,chunk,0,n,offset);if(read!==n)throw Error('MOLOCH_SOURCE_SHORT_READ');offset+=n;sourceHash.update(chunk);bytes+=n;pending=Buffer.concat([pending,chunk]);
          let newline;while((newline=pending.indexOf(10))>=0){const line=pending.subarray(0,newline).toString('utf8');pending=Buffer.from(pending.subarray(newline+1));if(line.trim()){const event=JSON.parse(line);if(Date.parse(event.receivedAt)<Date.parse(startedAt)){preCutoffEventsSkipped++;continue;}accept({...event,processedAt:new Date().toISOString()});}}
          if(pending.length>1024*1024)throw Error('MOLOCH_EVENT_TOO_LARGE');save();
        }else await new Promise(resolve=>setTimeout(resolve,250));
      }}finally{fs.closeSync(fd);}
      if(pending.length)throw Error('MOLOCH_PARTIAL_FINAL_EVENT');
    }
    save();const report={...reportMoloch(state),mode:o.follow?'local_paper_tail':'historical_paper_replay',startedAt,
      source:{path:input,sha256:sourceHash.digest('hex'),bytes,format:o.format},eventJournalHash:previous,
      externalSourceProvenanceVerified:o.format==='book-export'?'bounded_export_hash_and_record_chain_only':false,
      prospectiveCutoff:o.follow?startedAt:null,preCutoffEventsSkipped,
      sourcePrefixNotIndependentlyReproduced:true,liveConnectionVerified:false};
    fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));return report;
  }catch(error){save();fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({at:new Date().toISOString(),error:error.message,events:count}));throw error;}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href)run(process.argv.slice(2)).catch(e=>{console.error(e.message);process.exitCode=1;});
