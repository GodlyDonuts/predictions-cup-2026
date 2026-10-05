import fs from 'node:fs';
import path from 'node:path';
import {randomUUID,createHash} from 'node:crypto';
import {spawn} from 'node:child_process';
import {nativeJournalBinding,sourcePrefixHash} from './native-journal-restore.mjs';
import {journalLines} from './journal-lines.mjs';
import {atomicJson} from '../execution/ledger.mjs';

const need=(ok,code)=>{if(!ok)throw Error('NATIVE_PROJECTION_'+code);};
const sha=b=>createHash('sha256').update(b).digest('hex');
// Verify every original byte/chain with Rust, but send only not-yet-consumed
// original records through JS. The checkpoint is an exact sequence/hash, not a
// trusted cache. This never modifies sources, wallets, banks or market depth.
export async function projectNativeSource({runtimeDir,binding,file,frontier,receiptDir,signal,canOperate=()=>true,endByte:requestedEndByte}){
 need(path.isAbsolute(file)&&path.isAbsolute(receiptDir)&&Number.isSafeInteger(frontier?.sequence)&&frontier.sequence>=0&&(frontier.sequence===0&&frontier.hash===''||frontier.sequence>0&&/^[a-f0-9]{64}$/.test(frontier.hash)),'BINDING');
 need(canOperate()===true&&!signal?.aborted,'STOPPED');
 const executable=nativeJournalBinding(runtimeDir,binding),initial=fs.statSync(file),endByte=requestedEndByte??initial.size;
 need(Number.isSafeInteger(endByte)&&endByte>=0&&endByte<=initial.size,'SOURCE_PREFIX');
 const expectedSha=await sourcePrefixHash(file,endByte,signal);
 fs.mkdirSync(receiptDir,{recursive:true,mode:0o700});const root=path.join(receiptDir,randomUUID()),out=root+'.json',suffix=root+'.suffix.jsonl';
 need(canOperate()===true&&!signal?.aborted,'STOPPED');
 await new Promise((resolve,reject)=>{
  const child=spawn(executable,['--journal',file,'--end-byte',String(endByte),'--expected-sha',expectedSha,'--out',out,'--after-sequence',String(frontier.sequence),'--after-hash',frontier.sequence?frontier.hash:'EMPTY','--suffix-out',suffix],{windowsHide:true,stdio:['ignore','ignore','pipe']});let error='',done=false;
  const abort=()=>child.kill();signal?.addEventListener('abort',abort,{once:true});
  const finish=e=>{if(done)return;done=true;signal?.removeEventListener('abort',abort);e?reject(e):resolve();};
  child.stderr.on('data',b=>{error+=b.toString();if(error.length>2048)child.kill();});child.once('error',()=>finish(Error('NATIVE_PROJECTION_BINARY_UNAVAILABLE')));
  child.once('close',code=>finish(signal?.aborted?Error('NATIVE_PROJECTION_STOPPED'):code===0?null:Error(/^JOURNAL_[A-Z_]+$/.test(error.trim())?error.trim():'NATIVE_PROJECTION_FAILED')));
 });
 nativeJournalBinding(runtimeDir,binding);const after=fs.statSync(file);
 need(canOperate()===true&&!signal?.aborted&&after.ino===initial.ino&&after.birthtimeMs===initial.birthtimeMs&&after.size>=endByte,'STOP_OR_SOURCE_REPLACED');
 const output=fs.readFileSync(out),result=JSON.parse(output),p=result.projection;
 need(result.schemaVersion===1&&result.mode==='canonical_js_journal_projection_v1'&&result.engineHash===binding.engineHash&&result.sourceBytes===endByte&&result.sourceHash===expectedSha&&Number.isSafeInteger(result.records)&&result.records>=frontier.sequence&&p?.afterSequence===frontier.sequence&&p.afterHash===frontier.hash&&p.records===result.records-frontier.sequence&&p.bytes===fs.statSync(suffix).size&&p.hash===await sourcePrefixHash(suffix,p.bytes,signal),'RESULT_BINDING');
 let sequence=frontier.sequence,previous=frontier.hash;
 for(const line of journalLines(suffix)){const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===previous&&sha(JSON.stringify(body))===hash,'SUFFIX_CHAIN');previous=hash;}
 need(sequence===result.records&&previous===result.tailHash,'SUFFIX_FRONTIER');
 atomicJson(out+'.receipt.json',{schemaVersion:1,at:new Date().toISOString(),mode:result.mode,sourceBytes:endByte,sourceHash:expectedSha,afterSequence:frontier.sequence,afterHash:frontier.hash,records:result.records,tailHash:result.tailHash,suffixBytes:p.bytes,suffixHash:p.hash,binaryHash:binding.binaryHash,engineHash:binding.engineHash,outputHash:sha(output),networkRequests:0,competitionOrders:0});
 return{...result,suffixFile:suffix,receiptFile:out+'.receipt.json'};
}
