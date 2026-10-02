import fs from 'node:fs';
import path from 'node:path';
import {createHash,randomUUID} from 'node:crypto';

const delays=[100,200,400],deadlineMs=1000;
const phases=new Set(['serialize','open','write','fsync','close','guard','rename','read_bank','read_state','read_source','verify_source','advance','state','report','stats','lock','release','host']);
const codes=new Set(['EBUSY','EPERM','EACCES','ENOSPC','EDQUOT','EIO','EROFS','ENOENT','EEXIST','EMFILE','ENFILE','EINVAL','EXDEV','EISDIR','ENOTDIR','EINTR','EBADF']);
const basename=x=>typeof x==='string'&&/^[a-z][a-z0-9.-]{0,200}$/.test(x)?x:null;
const integer=x=>Number.isSafeInteger(x)&&x>=0?x:null;
export class PaperPersistenceError extends Error {
 constructor(details){super('Paper checkpoint persistence failed');this.name='PaperPersistenceError';this.details=Object.freeze({...details});}
}

// Only a complete, fsynced sibling is renamed. Retries reuse those exact bytes.
// No target unlink, copy fallback, or failed-temp cleanup is permitted here.
// This protects against ordinary interruption; directory/power-loss durability
// is not claimed, and a blocked OS call itself cannot have a JS time bound.
export async function atomicPaperJson(file,value,{io=fs,platform=process.platform,sleep=ms=>new Promise(r=>setTimeout(r,ms)),clock=()=>performance.now(),canCommit=()=>true}={}){
 if(!path.isAbsolute(file)||!basename(path.basename(file))||!file.endsWith('.json'))throw new PaperPersistenceError({phase:'open',osCode:'INVALID_TARGET',attempts:0});
 const temporary=file+'.'+process.pid+'.'+randomUUID()+'.tmp';
 let phase='serialize',fd,created=false,bytes,payloadHash=null,attempts=0;
 try{
  const json=JSON.stringify(value,null,2);if(typeof json!=='string')throw Error();
  bytes=Buffer.from(json);payloadHash=createHash('sha256').update(bytes).digest('hex');
  phase='open';fd=io.openSync(temporary,'wx',0o600);created=true;
  phase='write';let offset=0;
  while(offset<bytes.length){const n=io.writeSync(fd,bytes,offset,bytes.length-offset);if(!Number.isInteger(n)||n<=0||n>bytes.length-offset)throw Error();offset+=n;}
  phase='fsync';io.fsyncSync(fd);phase='close';io.closeSync(fd);fd=undefined;
  const started=clock();
  for(;;){
   phase='guard';if(canCommit()!==true)throw Error();
   phase='rename';attempts++;
   try{io.renameSync(temporary,file);return {attempts,payloadHash,bytes:bytes.length};}
   catch(error){
    const transient=error?.code==='EBUSY'||(platform==='win32'&&error?.code==='EPERM');
    const delay=delays[attempts-1];
    if(!transient||delay===undefined||clock()-started+delay>deadlineMs)throw error;
    await sleep(delay);
    if(clock()-started>=deadlineMs)throw error;
   }
  }
 }catch(error){
  if(fd!==undefined){try{io.closeSync(fd);}catch{}}
  throw new PaperPersistenceError({phase,osCode:codes.has(error?.code)?error.code:phase==='guard'?'COMMIT_BLOCKED':'UNKNOWN',attempts,
   target:path.basename(file),temporary:created?path.basename(temporary):null,payloadHash,bytes:bytes?.length??null});
 }
}

// Never expose an exception message, stack, path, payload, or arbitrary code.
export function paperFault(error,{cohort,phase='host',at,bankHash=null,durableSourceSequence=null,attemptedSourceSequence=null}={}){
 const d=error instanceof PaperPersistenceError?error.details:{};
 return {schemaVersion:1,type:'paper_worker_fault',cohort:['dip','dip-scale','dip-responsive'].includes(cohort)?cohort:null,
  at:typeof at==='string'&&Number.isFinite(Date.parse(at))?new Date(at).toISOString():null,
  phase:phases.has(d.phase)?d.phase:phases.has(phase)?phase:'host',osCode:codes.has(d.osCode)?d.osCode:codes.has(error?.code)?error.code:d.osCode==='COMMIT_BLOCKED'?'COMMIT_BLOCKED':'UNKNOWN',
  attempts:integer(d.attempts),target:basename(d.target),temporary:basename(d.temporary),bytes:integer(d.bytes),
  payloadHash:typeof d.payloadHash==='string'&&/^[a-f0-9]{64}$/.test(d.payloadHash)?d.payloadHash:null,
  bankHash:typeof bankHash==='string'&&/^[a-f0-9]{64}$/.test(bankHash)?bankHash:null,
  durableSourceSequence:integer(durableSourceSequence),attemptedSourceSequence:integer(attemptedSourceSequence)};
}

export async function persistPaperFault(runtimeDir,fault){
 const dir=path.join(runtimeDir,'paper-faults');fs.mkdirSync(dir,{recursive:true,mode:0o700});
 const name=(fault.cohort||'paper')+'-'+Date.now()+'-'+randomUUID()+'.json';
 await atomicPaperJson(path.join(dir,name),fault);return name;
}
