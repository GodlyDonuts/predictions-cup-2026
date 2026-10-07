import fs from 'node:fs';import {validateTiming} from './read-timing.mjs';import {atomicJson} from '../execution/ledger.mjs';
const causes=new Map([['Invalid timing schema','TIMING_SCHEMA'],['Invalid timing clock','TIMING_CLOCK'],['Invalid timing admission','TIMING_ADMISSION'],['Invalid timing duration','TIMING_DURATION'],['Invalid timing status','TIMING_STATUS'],['Invalid timing instrumentation','TIMING_INSTRUMENTATION'],['Invalid event-loop duration','TIMING_EVENT_LOOP'],['Invalid event-loop quantiles','TIMING_QUANTILES']]);
export function appendReadTimingV1({file,diagnosticFile,event,filesystem=fs,pause=ms=>Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,ms)}){
 let fd=null,written=0;try{
  validateTiming(event);const bytes=Buffer.from(JSON.stringify(event)+'\n');
  // Retry OPEN only: a failed/partial write is never appended twice.
  for(let attempt=0;attempt<4;attempt++){try{fd=filesystem.openSync(file,'a',0o600);break;}catch(e){if(!['EBUSY','EACCES','EPERM'].includes(e.code)||attempt===3)throw e;pause(20);}}
  while(written<bytes.length){const n=filesystem.writeSync(fd,bytes,written,bytes.length-written);if(!Number.isInteger(n)||n<1||n>bytes.length-written)throw Error('TIMING_SHORT_WRITE');written+=n;}
  filesystem.fsyncSync(fd);
 }catch(error){
  const code=causes.get(error.message)??(['EBUSY','EACCES','EPERM','EIO','ENOSPC'].includes(error.code)?error.code:error.message==='TIMING_SHORT_WRITE'?'TIMING_SHORT_WRITE':'TIMING_PERSISTENCE_FAILED'),safe={code,bytesWritten:written,schemaVersion:Number.isSafeInteger(event?.schemaVersion)?event.schemaVersion:null,outcome:['success','blocked','connection_error','http_error','body_error','aborted'].includes(event?.outcome)?event.outcome:null,statusCode:Number.isInteger(event?.statusCode)?event.statusCode:null,eventLoopSamples:Number.isSafeInteger(event?.eventLoopSamples)?event.eventLoopSamples:null,eventLoopP99Ms:Number.isFinite(event?.eventLoopP99Ms)?event.eventLoopP99Ms:null,eventLoopMaxMs:Number.isFinite(event?.eventLoopMaxMs)?event.eventLoopMaxMs:null};
  try{atomicJson(diagnosticFile,{schemaVersion:1,at:new Date().toISOString(),...safe});}catch{}
  throw Object.assign(Error(code),{code,fatal:true,durableRecordingFailed:true});
 }finally{if(fd!==null)filesystem.closeSync(fd);}
}
