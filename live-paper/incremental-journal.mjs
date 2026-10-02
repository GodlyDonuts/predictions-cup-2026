import fs from 'node:fs';import {createHash} from 'node:crypto';import {journalLines} from './journal-lines.mjs';
const digest=x=>createHash('sha256').update(x).digest('hex');
const same=(a,b)=>a.dev===b.dev&&a.ino===b.ino&&a.birthtimeMs===b.birthtimeMs;
export function createVerifiedSourceReader(file,{chunkBytes=1048576,maxRecordBytes=16777216}={}){
 const rows=[],prefixHashes=[];Object.defineProperty(rows,'prefixHashes',{value:prefixHashes});let identity=null,offset=0,previous='',clock=-Infinity,prefix=createHash('sha256'),head=null,boundary=null;
 const metrics={polls:0,fullVerifications:0,appendVerifications:0,bytesRead:0,guardBytesRead:0,partialTails:0,verifiedRecords:0,verifiedBytes:0};
 function guard(start,n){const b=Buffer.alloc(n),fd=fs.openSync(file,'r');try{if(fs.readSync(fd,b,0,n,start)!==n)throw Error('SOURCE_GUARD_READ');metrics.guardBytesRead+=n;return digest(b);}finally{fs.closeSync(fd);}}
 function read(){metrics.polls++;const stat=fs.statSync(file);if(identity&&(!same(stat,identity)||stat.size<offset))throw Error('SOURCE_REPLACED_OR_TRUNCATED');
  if(identity&&offset){if(guard(0,Math.min(offset,4096))!==head||guard(Math.max(0,offset-32768),Math.min(offset,32768))!==boundary)throw Error('SOURCE_VERIFIED_BOUNDARY_CHANGED');}
  if(stat.size===offset)return rows.length?rows:null;
  // Verify into temporary state. A partial tail or bad append commits nothing.
  const appended=[],hashes=[],nextPrefix=prefix.copy();let nextPrevious=previous,nextClock=clock,consumed=0;
  try{for(const line of journalLines(file,{chunkBytes,maxRecordBytes,startByte:offset,endByte:stat.size,onRead:n=>{metrics.bytesRead+=n;}})){
   const r=JSON.parse(line),{hash,...body}=r;if(r.sequence!==rows.length+appended.length+1||r.previous!==nextPrevious||hash!==digest(JSON.stringify(body)))throw Error('Source integrity failure');const at=Date.parse(r.event?.clockAt||r.at);if(!Number.isFinite(at)||at<nextClock)throw Error('Source clock rollback');
   nextClock=at;nextPrevious=hash;nextPrefix.update(line+'\n');hashes.push(nextPrefix.copy().digest('hex'));consumed+=Buffer.byteLength(line)+1;const {state,...input}=r;appended.push(input);
  }}catch(e){if(e.code==='JOURNAL_INCOMPLETE_TAIL'){metrics.partialTails++;return null;}throw e;}
  if(offset+consumed!==stat.size)throw Error('SOURCE_BYTE_ACCOUNTING');const after=fs.statSync(file);if(!same(stat,after)||after.size<stat.size)throw Error('SOURCE_REPLACED_DURING_READ');
  const nextHead=guard(0,Math.min(stat.size,4096)),nextBoundary=guard(Math.max(0,stat.size-32768),Math.min(stat.size,32768));
  for(const row of appended)rows.push(row);for(const hash of hashes)prefixHashes.push(hash);if(identity)metrics.appendVerifications++;else metrics.fullVerifications++;
  identity=stat;offset=stat.size;previous=nextPrevious;clock=nextClock;prefix=nextPrefix;head=nextHead;boundary=nextBoundary;metrics.verifiedRecords=rows.length;metrics.verifiedBytes=offset;return rows.length?rows:null;
 }
 return {read,metrics:()=>({...metrics,sourceSequence:rows.length,sourceHash:previous,bytePrefixHash:prefix.copy().digest('hex'),trustBoundary:'full_chain_on_restart_then_append_identity_head_and_last32KiB_guards; arbitrary_old_middle_rewrites_require_restart_verification'})};
}
const readers=new Map();export function readIncrementalSource(file){let reader=readers.get(file);if(!reader){reader=createVerifiedSourceReader(file);readers.set(file,reader);}return reader.read();}
export function incrementalSourceMetrics(file){return readers.get(file)?.metrics()||null;}
