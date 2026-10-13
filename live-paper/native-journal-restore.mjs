import fs from 'node:fs';import path from 'node:path';import {randomUUID,createHash} from 'node:crypto';import {spawn} from 'node:child_process';
import {Journal} from './transport-runtime.mjs';import {payloadHash,atomicJson} from '../execution/ledger.mjs';
const need=(v,c)=>{if(!v)throw Error('NATIVE_JOURNAL_'+c);},sha=b=>createHash('sha256').update(b).digest('hex');
export const journalSourceFiles=['Cargo.toml','Cargo.lock','build.rs','src/main.rs','src/canonical.rs'];
export function journalEngineIdentity(sourceRoot){const h=createHash('sha256');for(const name of journalSourceFiles){const bytes=fs.readFileSync(path.join(sourceRoot,name)),n=Buffer.from(name),size=Buffer.alloc(8);size.writeBigUInt64LE(BigInt(n.length));h.update(size);h.update(n);size.writeBigUInt64LE(BigInt(bytes.length));h.update(size);h.update(bytes);}return h.digest('hex');}
export function nativeJournalBinding(runtimeDir,binding){
 need(path.isAbsolute(runtimeDir)&&binding?.schemaVersion===1&&binding.mode==='canonical_js_journal_restore_v1'&&['binaryHash','engineHash','runtimeLibraryHash'].every(k=>/^[a-f0-9]{64}$/.test(binding[k])),'BINDING');
 const root=path.join(runtimeDir,'journal-native-builds',binding.binaryHash),exe=path.join(root,'baal-journal-restore.exe');
 need(sha(fs.readFileSync(exe))===binding.binaryHash&&sha(fs.readFileSync(path.join(root,'libunwind.dll')))===binding.runtimeLibraryHash,'BINARY_CHANGED');return exe;
}
async function invoke(exe,args,signal){
 need(!signal?.aborted,'STOPPED');await new Promise((resolve,reject)=>{
  const child=spawn(exe,args,{windowsHide:true,stdio:['ignore','ignore','pipe']});let error='',done=false;
  const abort=()=>{child.kill();};signal?.addEventListener('abort',abort,{once:true});
  child.stderr.on('data',b=>{error+=b.toString();if(error.length>2048)child.kill();});
  const finish=(e)=>{if(done)return;done=true;signal?.removeEventListener('abort',abort);e?reject(e):resolve();};
  child.once('error',()=>finish(Error('NATIVE_JOURNAL_UNAVAILABLE')));
  child.once('close',code=>{if(signal?.aborted)finish(Error('NATIVE_JOURNAL_STOPPED'));else if(code!==0){const label=error.trim();finish(Error(/^JOURNAL_[A-Z_]+$/.test(label)?label:'NATIVE_JOURNAL_FAILED'));}else finish();});
 });
}
export async function readNativeJournal({runtimeDir,binding,file,endByte,expectedSha,receiptDir,signal}){
 need(path.isAbsolute(file)&&path.isAbsolute(receiptDir)&&Number.isSafeInteger(endByte)&&endByte>=0&&(expectedSha===undefined||/^[a-f0-9]{64}$/.test(expectedSha)),'PREFIX_BINDING');
 const executable=nativeJournalBinding(runtimeDir,binding);fs.mkdirSync(receiptDir,{recursive:true,mode:0o700});const out=path.join(receiptDir,randomUUID()+'.state.json');
 const args=['--journal',file,'--end-byte',String(endByte),'--out',out];if(expectedSha!==undefined)args.push('--expected-sha',expectedSha);
 await invoke(executable,args,signal);nativeJournalBinding(runtimeDir,binding);
 const bytes=fs.readFileSync(out),result=JSON.parse(bytes);
 need(result.schemaVersion===1&&result.mode===binding.mode&&result.engineHash===binding.engineHash&&result.sourceBytes===endByte&&/^[a-f0-9]{64}$/.test(result.sourceHash)&&(!expectedSha||result.sourceHash===expectedSha)&&Number.isSafeInteger(result.records)&&result.records>=0&&typeof result.verifyMs==='number'&&result.verifyMs>=0,'RESULT_BINDING');
 if(result.records===0)need(result.lastBody===null&&result.tailHash==='','EMPTY_STATE');
 else need(result.lastBody?.sequence===result.records&&sha(JSON.stringify(result.lastBody))===result.tailHash,'LAST_BODY_PARITY');
 atomicJson(out+'.receipt.json',{schemaVersion:1,at:new Date().toISOString(),mode:result.mode,engineHash:result.engineHash,binaryHash:binding.binaryHash,runtimeLibraryHash:binding.runtimeLibraryHash,sourceHash:result.sourceHash,sourceBytes:endByte,records:result.records,tailHash:result.tailHash,outputHash:sha(bytes),networkRequests:0,competitionOrders:0});
 return{...result,receiptFile:out+'.receipt.json'};
}
export async function sourcePrefixHash(file,endByte,signal){
 const h=createHash('sha256');let size=0;if(endByte===0)return h.digest('hex');
 for await(const b of fs.createReadStream(file,{end:endByte-1})){need(!signal?.aborted,'STOPPED');h.update(b);size+=b.length;}
 need(size===endByte,'PREFIX_TRUNCATED');return h.digest('hex');
}
export async function openNativeJournal({runtimeDir,binding,file,receiptDir,signal,canOperate=()=>true}){
 need(canOperate()===true&&!signal?.aborted,'STOPPED');nativeJournalBinding(runtimeDir,binding);
 if(!fs.existsSync(file))fs.writeFileSync(file,Buffer.alloc(0),{flag:'wx',mode:0o600});
 const stamp=s=>[s.dev,s.ino,s.size,s.mtimeNs,s.ctimeNs].map(String).join(':'),initial=fs.statSync(file,{bigint:true});
 const started=performance.now(),bytes=Number(initial.size),result=await readNativeJournal({runtimeDir,binding,file,endByte:bytes,receiptDir,signal});
 need(canOperate()===true&&!signal?.aborted&&stamp(fs.statSync(file,{bigint:true}))===stamp(initial),'STOP_OR_SOURCE_CHANGED');
 // Use the original append/hash interface after exact verified restoration.
 const journal=Object.create(Journal.prototype);Object.assign(journal,{file,sequence:result.records,previous:result.tailHash,last:result.lastBody});
 Object.defineProperty(journal,'nativeRestore',{value:Object.freeze({mode:result.mode,sourceBytes:bytes,sourceHash:result.sourceHash,sequence:result.records,tailHash:result.tailHash,lastBodyHash:result.lastBody?payloadHash(result.lastBody):null,engineHash:result.engineHash,binaryHash:binding.binaryHash,verifyMs:result.verifyMs,prefixHashMs:0,totalMs:performance.now()-started}),enumerable:false});return journal;
}
