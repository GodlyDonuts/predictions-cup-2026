import fs from 'node:fs';import path from 'node:path';import {randomUUID} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {sha} from './native-dip-proof-v1.mjs';import {sealedNativePrefix,runNativeDipChild} from './native-dip-host-v1.mjs';import {verifyNativeQuoteResult,nativeQuoteReadFocus} from './native-quote-proof-v1.mjs';
const need=(v,c)=>{if(!v)throw Error('NATIVE_QUOTE_'+c);},read=f=>JSON.parse(fs.readFileSync(f)),here=path.dirname(fileURLToPath(import.meta.url));
export const nativeQuoteHostFiles=['live-paper/native-quote-host-v1.mjs','live-paper/native-quote-proof-v1.mjs','live-paper/native-dip-host-v1.mjs','live-paper/native-dip-proof-v1.mjs','live-paper/account-eligibility-publication-v1.mjs','live-paper/runtime.mjs','execution/ledger.mjs'];
export function nativeQuoteHostIdentity(workspace=path.dirname(here)){return Object.fromEntries(nativeQuoteHostFiles.map(f=>[f,sha(fs.readFileSync(path.join(workspace,f)))]));}
export function quoteSourcePrefix(file){const stat=fs.statSync(file);return stat.size?sealedNativePrefix(file):{bytes:0,dev:stat.dev,ino:stat.ino};}
export function openNativeQuoteHost({runtimeDir,universe,signal,canOperate=()=>true,now=Date.now}){
 const activeFile=path.join(runtimeDir,'native-quote-active.json');if(!fs.existsSync(activeFile))return null;
 const activeBytes=fs.readFileSync(activeFile),active=JSON.parse(activeBytes),activeHash=sha(activeBytes);
 need(active.schemaVersion===1&&active.mode==='frozen_native_quote_history_host_v1'&&/^[A-Za-z0-9_-]{1,100}$/.test(active.cohort)&&Number.isSafeInteger(active.intervalMs)&&active.intervalMs>=30000&&active.intervalMs<=900000&&Number.isSafeInteger(active.workers)&&active.workers>=1&&active.workers<=32,'ACTIVE');
 const dir=path.join(runtimeDir,'research-cohorts',active.cohort),bindingFile=path.join(dir,'binding.json'),declarationFile=path.join(dir,'declaration.json'),binding=read(bindingFile),declaration=read(declarationFile),pointerFile=path.join(dir,'host-latest.json'),statusFile=path.join(runtimeDir,'native-quote-status.json');
 need(sha(fs.readFileSync(bindingFile))===active.bindingHash&&sha(fs.readFileSync(declarationFile))===binding.declarationHash&&binding.cohort===active.cohort&&payloadHash(nativeQuoteHostIdentity())===payloadHash(active.hostSourceHashes),'FROZEN');
 for(const[name,hash]of Object.entries(binding.sourceHashes)){need(name.startsWith('backend-rust/')&&!name.includes('..'),'SOURCE_PATH');need(sha(fs.readFileSync(path.join(runtimeDir,'native-sources',binding.engineHash,name.slice(13))))===hash,'SOURCE_IDENTITY');}
 const build=path.join(runtimeDir,'native-builds',binding.binaryHash),exe=path.join(build,'baal-backend.exe'),library=path.join(build,'libunwind.dll');
 const journals={books:path.join(runtimeDir,'public-books.jsonl'),quotes:path.join(runtimeDir,'market-prices.jsonl'),eligibility:path.join(runtimeDir,'account-eligibility.jsonl')};
 const validate=()=>need(sha(fs.readFileSync(activeFile))===activeHash&&sha(fs.readFileSync(bindingFile))===active.bindingHash&&sha(fs.readFileSync(declarationFile))===binding.declarationHash&&sha(fs.readFileSync(exe))===binding.binaryHash&&sha(fs.readFileSync(library))===binding.runtimeLibraryHash,'IDENTITY_CHANGED');validate();
 const outPath=name=>{need(typeof name==='string'&&(name==='seed.json'||/^[a-f0-9-]{36}\.json$/.test(name)),'OUTPUT_PATH');return path.join(dir,name);};
 const pointer=fs.existsSync(pointerFile)?read(pointerFile):{...active.seed,activeHash},file=outPath(pointer.output),bytes=fs.readFileSync(file),receipt=read(file+'.receipt.json');
 need(pointer.activeHash===activeHash&&sha(bytes)===pointer.outputHash&&receipt.outputHash===pointer.outputHash&&receipt.binaryHash===binding.binaryHash&&receipt.engineHash===binding.engineHash&&receipt.declarationHash===binding.declarationHash&&receipt.competitionOrders===0,'RESTORE');
 let current={result:JSON.parse(bytes)};need(payloadHash(receipt.sources)===payloadHash(current.result.capture.sources),'RESTORE_SOURCE');current.projection=verifyNativeQuoteResult({result:current.result,declaration,binding,universe});
 let closed=false,busy=null,nextAt=0,lastError=null,lastWallMs=null,verifiedAt=null;const control=new AbortController(),abort=()=>control.abort();signal?.addEventListener('abort',abort,{once:true});if(signal?.aborted)abort();
 const permitted=()=>!closed&&!signal?.aborted&&canOperate()===true;
 function status(){return{...current.projection,...nativeQuoteReadFocus(current.projection,now()),activeHash,status:closed?'stopped':lastError?'degraded':busy?'replaying':'running',lastError,lastWallMs,verifiedAt,replayInProgress:!!busy,sourceLagMs:Math.max(0,now()-Date.parse(current.projection.through)),heldRiskRetained:true,additionalPersistentProcesses:0};}
 function publish(){if(permitted())atomicJson(statusFile,status());}
 async function replay(){const start=performance.now();validate();const prefixes=Object.fromEntries(Object.entries(journals).map(([k,v])=>[k,quoteSourcePrefix(v)])),sources={};
  for(const[name,journal]of Object.entries(journals)){const prior=current.result.capture.sources[name],dev=declaration.development[name];need(prefixes[name].bytes>=prior.bytes,'SOURCE_REGRESSION');
   const checkpoints=[dev];if(prior.bytes!==dev.bytes)checkpoints.push({bytes:prior.bytes,hash:prior.hash});else need(prior.hash===dev.hash,'SOURCE_HISTORY');sources[name]={path:journal,bytes:prefixes[name].bytes,hash:null,checkpoints};}
  const output=randomUUID()+'.json',out=outPath(output),sourcesFile=out+'.sources.json';fs.writeFileSync(sourcesFile,JSON.stringify(sources),{flag:'wx',mode:0o600});need(permitted(),'STOPPED');
  await runNativeDipChild(exe,['quote-replay','--declaration',declarationFile,'--sources',sourcesFile,'--workers',String(active.workers),'--out',out],control.signal);need(permitted(),'STOPPED');validate();
  for(const[name,journal]of Object.entries(journals)){const s=fs.statSync(journal);need(s.dev===prefixes[name].dev&&s.ino===prefixes[name].ino&&s.size>=prefixes[name].bytes,'SOURCE_REPLACED');}
  const bytes=fs.readFileSync(out);need(bytes.length<=32*1024*1024,'OUTPUT_SIZE');const result=JSON.parse(bytes);for(const name of Object.keys(journals))need(result.capture.sources[name].bytes===prefixes[name].bytes,'PREFIX_BINDING');
  const projection=verifyNativeQuoteResult({result,declaration,binding,universe,previous:current.result}),outputHash=sha(bytes),at=new Date(now()).toISOString();
  fs.writeFileSync(out+'.receipt.json',JSON.stringify({at,binaryHash:binding.binaryHash,engineHash:binding.engineHash,declarationHash:binding.declarationHash,sources:result.capture.sources,outputHash,networkRequests:0,competitionOrders:0},null,2)+'\n',{flag:'wx',mode:0o600});need(permitted(),'STOPPED');
  atomicJson(pointerFile,{schemaVersion:1,activeHash,at,output,outputHash});current={result,projection};lastWallMs=performance.now()-start;verifiedAt=at;lastError=null;
 }
 function kick(){if(!permitted()||busy||now()<nextAt)return busy;nextAt=now()+active.intervalMs;busy=replay().catch(e=>{lastError=/^NATIVE_QUOTE_[A-Z_]+$/.test(e.message)?e.message:'NATIVE_QUOTE_REPLAY_UNAVAILABLE';}).finally(()=>{busy=null;try{publish();}catch{lastError='NATIVE_QUOTE_STATUS_UNAVAILABLE';}});return busy;}
 publish();return{status,kick,close:async()=>{closed=true;control.abort();signal?.removeEventListener('abort',abort);await busy;}};
}
