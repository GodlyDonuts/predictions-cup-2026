import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {journalLines} from './journal-lines.mjs';

const verified=new WeakSet();
const hash=x=>createHash('sha256').update(x).digest('hex');
const freeze=x=>{if(x&&typeof x==='object'&&!Object.isFrozen(x)){for(const v of Object.values(x))freeze(v);Object.freeze(x);}return x;};
export const isVerifiedTrainingSource=x=>!!x&&typeof x==='object'&&verified.has(x);
function seal(records,prefixHashes,prefixBytes,metadata){
 freeze(metadata);
 Object.defineProperty(records,'prefixHashes',{value:Object.freeze(prefixHashes)});
 Object.freeze(records);
 const source=Object.freeze({records,prefixBytes:Object.freeze(prefixBytes),...metadata,
  sourceHash:prefixHashes.at(-1),sourceRecords:records.length,
  sourceBytes:prefixBytes.at(-1),finalEventClock:records.at(-1).event?.clockAt||records.at(-1).at});
 verified.add(source);return source;
}

// Verify every original record, including its historical state, before dropping
// unused state bodies. This is an immutable in-process source, not a rewritten
// journal: row and prefix hashes still identify the original captured bytes.
export function readTrainingSource(file,{endByte=null,onProgress=()=>{},...limits}={}){
 const initial=fs.statSync(file),end=endByte??initial.size;
 const records=[],prefixHashes=[],prefixBytes=[],prefix=createHash('sha256');
 let previous='',clock=-Infinity,bytes=0;
 for(const line of journalLines(file,{...limits,endByte:end})){
  const r=JSON.parse(line),{hash:recordHash,...body}=r;
  if(r.sequence!==records.length+1||r.previous!==previous||recordHash!==hash(JSON.stringify(body)))throw Error('Source integrity failure');
  const next=Date.parse(r.event?.clockAt||r.at);
  if(!Number.isFinite(next)||next<clock)throw Error('Source clock rollback');
  clock=next;previous=recordHash;prefix.update(line+'\n');bytes+=Buffer.byteLength(line+'\n');
  prefixHashes.push(prefix.copy().digest('hex'));prefixBytes.push(bytes);
  // replayJournal uses historical skip timestamps when an explicit receipt is
  // absent. Preserve that small fallback to keep legacy replay exactly equal.
  const {state,...input}=r;
  if(state?.skips)input.state={skips:state.skips};
  records.push(freeze(input));onProgress({bytes,records:records.length,endBytes:end});
 }
 if(!records.length||bytes!==end)throw Error('Incomplete capture journal');
 const final=fs.statSync(file);
 if(final.ino!==initial.ino||final.birthtimeMs!==initial.birthtimeMs||final.size<end)throw Error('Training source replaced or truncated');
 return seal(records,prefixHashes,prefixBytes,{format:'verified_original_journal_v1',fileIdentity:{ino:initial.ino,birthtimeMs:initial.birthtimeMs},snapshotBytes:end});
}

export function trainingSourcePrefix(source,count){
 if(!isVerifiedTrainingSource(source)||!Number.isSafeInteger(count)||count<1||count>source.records.length)throw Error('Invalid verified training prefix');
 return seal(source.records.slice(0,count),source.records.prefixHashes.slice(0,count),source.prefixBytes.slice(0,count),
  {format:source.format,fileIdentity:source.fileIdentity,snapshotBytes:source.prefixBytes[count-1]});
}
