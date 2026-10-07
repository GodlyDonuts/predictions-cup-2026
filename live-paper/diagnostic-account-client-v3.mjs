import fs from 'node:fs';import path from 'node:path';
import {CoordinatedAccountClientV2} from './coordinated-account-client-v2.mjs';import {atomicJson} from '../execution/ledger.mjs';
const codes=new Set(['EIO','ENOSPC','EACCES','EPERM','ENETUNREACH','EHOSTUNREACH','UND_ERR_BODY_TIMEOUT','ECONNRESET','ECONNREFUSED','ETIMEDOUT','ENOTFOUND','EAI_AGAIN','UND_ERR_CONNECT_TIMEOUT','UND_ERR_HEADERS_TIMEOUT','UND_ERR_SOCKET','ABORT_ERR','REST_RECEIPT_START_RECORDING_FAILED','REST_RECEIPT_FETCH_UNAVAILABLE','REST_RECEIPT_BEFORE_FETCH_GUARD','REST_RECEIPT_FINAL_FENCE_EXPIRED']);
export function safeFetchFailureV1(error){
 const candidates=[error?.code,error?.cause?.code,...(Array.isArray(error?.cause?.errors)?error.cause.errors.slice(0,8).map(e=>e?.code):[])],code=candidates.find(c=>codes.has(c))??'UNCLASSIFIED_FETCH_FAILURE';
 return{code,name:['AbortError','TimeoutError','TypeError'].includes(error?.name)?error.name:'Error',durableRecordingFailed:error?.durableRecordingFailed===true||['EIO','ENOSPC','EACCES','EPERM'].includes(error?.code),mayHaveExecuted:error?.mayHaveExecuted===true};
}
// Reuses the same queue, credentials, limiter and fetch. This is not a new client.
export class DiagnosticAccountClientV3 extends CoordinatedAccountClientV2{
 constructor(key,{diagnosticFile,...options}){
  if(!path.isAbsolute(diagnosticFile))throw Error('DIAGNOSTIC_PATH');
  super(key,options);this.diagnosticFile=diagnosticFile;this.lastFault=null;this.failures=[];
  const original=this.fetch;this.fetch=async(url,request)=>{try{return await original(url,request);}catch(error){
   this.lastFault=safeFetchFailureV1(error);this.failures.push({at:new Date(this.now()).toISOString(),method:request?.method==='GET'?'GET':'WRITE',...this.lastFault});this.failures=this.failures.slice(-20);
   try{atomicJson(this.diagnosticFile,{schemaVersion:1,failures:this.failures});}catch{this.lastFault={code:'DIAGNOSTIC_PERSISTENCE_FAILED',name:'Error',durableRecordingFailed:true,mayHaveExecuted:this.lastFault.mayHaveExecuted};}throw error;
  }};
 }
 async perform(...args){
  this.lastFault=null;try{return await super.perform(...args);}catch(error){
   if(this.lastFault?.durableRecordingFailed)throw Object.assign(Error(this.lastFault.code),{code:this.lastFault.code,fatal:true,durableRecordingFailed:true,mayHaveExecuted:this.lastFault.mayHaveExecuted});
   throw error;
  }
 }
}

