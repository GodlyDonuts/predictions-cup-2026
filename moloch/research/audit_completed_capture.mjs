// Original Node-number semantics and producer hashes, before Python decoding.
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {payloadHash,canonicalJson} from '../../execution/ledger.mjs';

const [input,manifestFile,output,lower,upper]=process.argv.slice(2);
const lo=Number(lower),hi=Number(upper);
if(!Number.isSafeInteger(lo)||!Number.isSafeInteger(hi)||lo<0||lo>=hi)throw Error('EXPLICIT_RECEIPT_BOUNDS_REQUIRED');
const m=JSON.parse(fs.readFileSync(manifestFile));
const p=path.resolve(input),before=fs.statSync(p);
const sha=f=>createHash('sha256').update(fs.readFileSync(f)).digest('hex');
if(before.size!==m.files['events.jsonl'].bytes||sha(p)!==m.files['events.jsonl'].sha256)throw Error('ORIGINAL_FORWARD_BYTES_CHANGED');
const tail=fs.openSync(p,'r'),b=Buffer.alloc(1);
fs.readSync(tail,b,0,1,before.size-1);fs.closeSync(tail);
if(b[0]!==10)throw Error('COMPLETE_CAPTURE_NEWLINE_REQUIRED');
let sequence=0,previous='',lastPublic=-1,lastAny=-1,metadataClockRegressions=0;
const counts={},bounds={},h=createHash('sha256'),fd=fs.openSync(output,'wx');
try{
  for await(const line of readline.createInterface({input:fs.createReadStream(p),crlfDelay:Infinity})){
    if(Buffer.byteLength(line)>16*1024*1024)throw Error('FORWARD_ROW_BOUND');
    const row=JSON.parse(line),{hash,...body}=row;
    if(Object.keys(row).sort().join(',')!=='event,hash,previous,sequence'||body.sequence!==sequence+1||body.previous!==previous||hash!==payloadHash(body))throw Error('EXACT_FORWARD_PRODUCER_CHAIN_REQUIRED');
    const e=body.event,at=Date.parse(e.receivedAt);
    if(!['rest_reference','subscribed','broadcast'].includes(e.kind))throw Error('EXPLICIT_PUBLIC_CAPTURE_KIND_REQUIRED');
    if(!Number.isSafeInteger(at)||at<lo||at>=hi)throw Error('BOUNDED_ORIGINAL_RECEIPT_CLOCK_REQUIRED');
    if(e.kind!=='rest_reference'){
      if(at<lastPublic)throw Error('PUBLIC_STREAM_RECEIPT_CLOCK_REGRESSION');
      lastPublic=at;
    }else if(at<lastAny)metadataClockRegressions++;
    lastAny=Math.max(lastAny,at);
    counts[e.kind]=(counts[e.kind]??0)+1;bounds[e.kind]??=[at,at];
    bounds[e.kind][0]=Math.min(bounds[e.kind][0],at);bounds[e.kind][1]=Math.max(bounds[e.kind][1],at);
    sequence=body.sequence;previous=hash;
    const canonical=canonicalJson(row)+'\n';fs.writeSync(fd,canonical);h.update(canonical);
  }
  fs.fsyncSync(fd);
}finally{fs.closeSync(fd);}
const after=fs.statSync(p);
if(before.size!==after.size||before.mtimeMs!==after.mtimeMs||sha(p)!==m.files['events.jsonl'].sha256)throw Error('SOURCE_CHANGED_DURING_AUDIT');
const result={classification:'verified_original_forward_chain_transport_not_market_semantic_admission',
  sourceOriginalSha256:m.files['events.jsonl'].sha256,canonicalTransportSha256:h.digest('hex'),
  originalArchiveSha256:m.archiveSha256,rows:sequence,terminalProducerHash:previous,counts,bounds,
  metadataClockRegressions,publicStreamClockMonotonic:true,producerHashPreserved:true,
  sourceScriptSha256:sha(fileURLToPath(import.meta.url)),ledgerSourceSha256:sha(fileURLToPath(new URL('../../execution/ledger.mjs',import.meta.url))),
  receiptBoundsMs:[lo,hi],trainingAdmitted:false,independentTestUsed:false,productionChanged:false,accountRequests:0};
fs.writeFileSync(path.join(path.dirname(output),'RESULT.json'),JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({rows:sequence,counts,metadataClockRegressions}));
