import fs from 'node:fs';import path from 'node:path';import {spawn} from 'node:child_process';import {randomUUID} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {sha,verifyNativeDipResult} from './native-dip-proof-v1.mjs';
const need=(v,c)=>{if(!v)throw Error('NATIVE_DIP_'+c);},read=f=>JSON.parse(fs.readFileSync(f)),here=path.dirname(fileURLToPath(import.meta.url));
export const nativeDipHostFiles=['live-paper/native-dip-host-v1.mjs','live-paper/native-dip-proof-v1.mjs','execution/ledger.mjs'];
export function nativeDipHostIdentity(workspace=path.dirname(here)){return Object.fromEntries(nativeDipHostFiles.map(f=>[f,sha(fs.readFileSync(path.join(workspace,f)))]));}
export function sealedNativePrefix(file){const fd=fs.openSync(file,'r');try{const s=fs.fstatSync(fd),size=s.size,n=Math.min(size,65536),b=Buffer.alloc(n);need(fs.readSync(fd,b,0,n,size-n)===n,'SOURCE_READ');const at=b.lastIndexOf(10);need(at>=0,'SOURCE_BOUNDARY');return{bytes:size-n+at+1,dev:s.dev,ino:s.ino};}finally{fs.closeSync(fd);}}
export async function hashNativePrefix(file,end,prior,signal){
 const {createHash}=await import('node:crypto'),whole=createHash('sha256'),old=createHash('sha256');let n=0;
 for await(const b of fs.createReadStream(file,{end:end-1})){need(!signal?.aborted,'STOPPED');whole.update(b);if(n<prior)old.update(b.subarray(0,Math.min(b.length,prior-n)));n+=b.length;}
 need(n===end&&prior<=end,'SOURCE_TRUNCATED');return{hash:whole.digest('hex'),priorHash:old.digest('hex')};
}
export async function runNativeDipChild(exe,args,signal){
 need(!signal?.aborted,'STOPPED');await new Promise((resolve,reject)=>{
  const child=spawn(exe,args,{windowsHide:true,stdio:['ignore','ignore','pipe']});let done=false,bytes=0;
  const abort=()=>child.kill(),finish=e=>{if(done)return;done=true;signal?.removeEventListener('abort',abort);e?reject(e):resolve();};signal?.addEventListener('abort',abort,{once:true});
  child.stderr.on('data',b=>{bytes+=b.length;if(bytes>4096)child.kill();});
  child.once('error',()=>finish(Error('NATIVE_DIP_CHILD_UNAVAILABLE')));child.once('close',code=>finish(signal?.aborted?Error('NATIVE_DIP_STOPPED'):code!==0?Error('NATIVE_DIP_CHILD_FAILED'):null));
 });
}
export function openNativeDipHost({runtimeDir,universe,signal,canOperate=()=>true,now=Date.now}){
 const activeFile=path.join(runtimeDir,'native-dip-active.json');if(!fs.existsSync(activeFile))return null;
 const activeBytes=fs.readFileSync(activeFile),active=JSON.parse(activeBytes),activeHash=sha(activeBytes);
 need(active.schemaVersion===1&&active.mode==='frozen_native_dip_paper_host_v1'&&/^[a-zA-Z0-9_-]{1,100}$/.test(active.cohort)&&active.intervalMs>=60000&&active.intervalMs<=900000&&Number.isSafeInteger(active.intervalMs)&&Number.isSafeInteger(active.workers)&&active.workers>=1&&active.workers<=32,'ACTIVE');
 const dir=path.join(runtimeDir,'research-cohorts',active.cohort),bindingFile=path.join(dir,'native-clock-binding.json'),declarationFile=path.join(dir,'declaration.json'),binding=read(bindingFile),declaration=read(declarationFile);
 need(sha(fs.readFileSync(bindingFile))===active.bindingHash&&sha(fs.readFileSync(declarationFile))===binding.declarationHash&&binding.cohort===active.cohort,'FROZEN');
 const identity=nativeDipHostIdentity();need(payloadHash(identity)===payloadHash(active.hostSourceHashes),'HOST_IDENTITY');
 for(const [name,expected] of Object.entries(binding.sourceHashes)){need(name.startsWith('backend-rust/')&&!name.includes('..'),'SOURCE_SCOPE');need(sha(fs.readFileSync(path.join(runtimeDir,'native-sources',binding.engineHash,name.slice(13))))===expected,'ENGINE_SOURCE');}
 const build=path.join(runtimeDir,'native-builds',binding.binaryHash),exe=path.join(build,'baal-backend.exe'),library=path.join(build,'libunwind.dll'),journal=path.join(runtimeDir,'journal.jsonl'),statusFile=path.join(runtimeDir,'native-dip-status.json'),pointerFile=path.join(dir,'native-host-latest.json');
 const validateBinding=()=>{need(sha(fs.readFileSync(activeFile))===activeHash&&sha(fs.readFileSync(bindingFile))===active.bindingHash&&sha(fs.readFileSync(declarationFile))===binding.declarationHash&&sha(fs.readFileSync(exe))===binding.binaryHash&&sha(fs.readFileSync(library))===binding.runtimeLibraryHash,'IDENTITY_CHANGED');};validateBinding();
 const outputPath=name=>{need(typeof name==='string'&&/^[a-f0-9-]{36}\.json$/.test(name),'OUTPUT_PATH');return path.join(dir,name);};
 function restore(pointer){const file=outputPath(pointer.output);need(pointer.activeHash===activeHash&&sha(fs.readFileSync(file))===pointer.outputHash,'OUTPUT_HASH');const receipt=read(file+'.receipt.json');need(receipt.binaryHash===binding.binaryHash&&receipt.engineHash===binding.engineHash&&receipt.declarationFileHash===binding.declarationHash&&receipt.outputHash===pointer.outputHash&&receipt.competitionOrders===0,'RECEIPT');const result=read(file);need(receipt.sourceHash===result.capture.source_hash&&receipt.sourceBytes===result.capture.source_bytes,'RECEIPT_SOURCE');return{result,projection:verifyNativeDipResult({result,declaration,binding,universe})};}
 let current=restore(fs.existsSync(pointerFile)?read(pointerFile):{...active.seed,activeHash}),closed=false,busy=null,nextAt=0,lastError=null,lastWallMs=null,verifiedAt=null;
 const stopControl=new AbortController(),runSignal=stopControl.signal,parentAbort=()=>stopControl.abort();signal?.addEventListener('abort',parentAbort,{once:true});if(signal?.aborted)parentAbort();
 const permitted=()=>!closed&&!signal?.aborted&&canOperate()===true;
 function status(){return{...current.projection,activeHash,status:closed?'stopped':lastError?'degraded':busy?'replaying':'running',replayInProgress:!!busy,verifiedAt,lastError,lastWallMs,sourceLagMs:Math.max(0,now()-Date.parse(current.projection.through)),riskSourceStale:now()-Date.parse(current.projection.through)>active.intervalMs*2,heldRiskRetained:true,additionalPersistentProcesses:0};}
 function publish(){if(permitted())atomicJson(statusFile,status());}
 async function replay(){
  const started=performance.now();validateBinding();const prefix=sealedNativePrefix(journal);need(prefix.bytes>=current.result.capture.source_bytes,'SOURCE_REGRESSION');
  const hashes=await hashNativePrefix(journal,prefix.bytes,current.result.capture.source_bytes,runSignal);need(hashes.priorHash===current.result.capture.source_hash,'SOURCE_HISTORY_CHANGED');
  if(prefix.bytes===current.result.capture.source_bytes){verifiedAt=new Date(now()).toISOString();lastError=null;return;}
  need(permitted(),'STOPPED');const output=randomUUID()+'.json',file=outputPath(output);
  await runNativeDipChild(exe,['evaluate','--journal',journal,'--declaration',declarationFile,'--end-byte',String(prefix.bytes),'--expected-sha',hashes.hash,'--workers',String(active.workers),'--out',file],runSignal);
  need(permitted(),'STOPPED');validateBinding();const stat=fs.statSync(journal);need(stat.dev===prefix.dev&&stat.ino===prefix.ino&&stat.size>=prefix.bytes,'SOURCE_REPLACED');
  const bytes=fs.readFileSync(file);need(bytes.length<=32*1024*1024,'OUTPUT_SIZE');const result=JSON.parse(bytes);need(result.capture.source_hash===hashes.hash&&result.capture.source_bytes===prefix.bytes,'PREFIX_BINDING');
  const projection=verifyNativeDipResult({result,declaration,binding,universe,previous:current.result}),outputHash=sha(bytes),at=new Date(now()).toISOString();
  fs.writeFileSync(file+'.receipt.json',JSON.stringify({at,sourceHash:hashes.hash,sourceBytes:prefix.bytes,engineHash:binding.engineHash,declarationFileHash:binding.declarationHash,binaryHash:binding.binaryHash,runtimeLibraryHash:binding.runtimeLibraryHash,outputHash,networkRequests:0,competitionOrders:0},null,2)+'\n',{flag:'wx',mode:0o600});
  need(permitted(),'STOPPED');atomicJson(pointerFile,{schemaVersion:1,at,activeHash,output,outputHash});current={result,projection};verifiedAt=at;lastError=null;lastWallMs=performance.now()-started;
 }
 function kick(){if(!permitted()||busy||now()<nextAt)return busy;nextAt=now()+active.intervalMs;busy=replay().catch(e=>{lastError=/^NATIVE_DIP_[A-Z_]+$/.test(e.message)?e.message:'NATIVE_DIP_UNAVAILABLE';}).finally(()=>{busy=null;try{publish();}catch{lastError='NATIVE_DIP_STATUS_UNAVAILABLE';}});return busy;}
 publish();return{status,kick,close:async()=>{closed=true;stopControl.abort();signal?.removeEventListener('abort',parentAbort);await busy;}};
}
