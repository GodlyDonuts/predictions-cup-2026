import fs from 'node:fs';import {createHash} from 'node:crypto';
export function* journalLines(file,{chunkBytes=1048576,maxRecordBytes=16777216}={}){
 if(!Number.isSafeInteger(chunkBytes)||chunkBytes<1||chunkBytes>1048576||!Number.isSafeInteger(maxRecordBytes)||maxRecordBytes<chunkBytes)throw Error('INVALID_JOURNAL_READ_LIMIT');
 const fd=fs.openSync(file,'r'),size=fs.fstatSync(fd).size;let pos=0,tail=Buffer.alloc(0);
 try{while(pos<size){const b=Buffer.alloc(Math.min(chunkBytes,size-pos)),n=fs.readSync(fd,b,0,b.length,pos);if(n<1)throw Error('JOURNAL_READ_TRUNCATED');pos+=n;const data=Buffer.concat([tail,b.subarray(0,n)]);let begin=0,end;while((end=data.indexOf(10,begin))>=0){if(end-begin>maxRecordBytes)throw Error('JOURNAL_RECORD_TOO_LARGE');yield data.subarray(begin,end).toString('utf8');begin=end+1;}tail=data.subarray(begin);if(tail.length>maxRecordBytes)throw Error('JOURNAL_RECORD_TOO_LARGE');}if(tail.length){const e=Error('Incomplete paper journal: manual recovery required');e.code='JOURNAL_INCOMPLETE_TAIL';throw e;}}
 finally{fs.closeSync(fd);}
}
// Verify original complete bytes and hashes first, then omit unused historical
// state bodies from strategy inputs. Prefix hashes remain byte-for-byte identical.
export function readVerifiedSource(file,options={}){
 const rows=[],prefixHashes=[],prefix=createHash('sha256');let previous='',clock=-Infinity;
 try{for(const line of journalLines(file,options)){const r=JSON.parse(line),{hash,...body}=r;if(r.sequence!==rows.length+1||r.previous!==previous||hash!==createHash('sha256').update(JSON.stringify(body)).digest('hex'))throw Error('Source integrity failure');const next=Date.parse(r.event?.clockAt||r.at);if(!Number.isFinite(next)||next<clock)throw Error('Source clock rollback');clock=next;prefix.update(line+'\n');prefixHashes.push(prefix.copy().digest('hex'));previous=hash;const {state,...input}=r;rows.push(input);}}
 catch(e){if(e.code==='JOURNAL_INCOMPLETE_TAIL')return null;throw e;}
 if(!rows.length)return null;Object.defineProperty(rows,'prefixHashes',{value:prefixHashes});return rows;
}
