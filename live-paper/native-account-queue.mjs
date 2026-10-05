import fs from 'node:fs';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {createHash,randomUUID} from 'node:crypto';

const hash=value=>createHash('sha256').update(value).digest('hex');
const need=(ok,code)=>{if(!ok)throw queueError(code);};
const stable=value=>JSON.stringify(value,(_key,v)=>v&&typeof v==='object'&&!Array.isArray(v)?Object.fromEntries(Object.keys(v).sort().map(k=>[k,v[k]])):v);
const identity=value=>typeof value==='string'&&/^[A-Za-z0-9._:-]{1,160}$/.test(value);
const freeze=value=>{if(value&&typeof value==='object'){for(const item of Object.values(value))freeze(item);Object.freeze(value);}return value;};
function queueError(code,unknown=false){const e=Error('NATIVE_ACCOUNT_QUEUE_'+code);e.code=code;e.mayHaveExecuted=unknown;e.reconciliationRequired=unknown;return e;}

// A trusted local deployment supplies immutable binary hashes. This module never
// reads credentials, opens HTTP sockets or enables an execution activation.
export function verifyNativeAccountQueueBinding(binding){
 need(binding&&Object.keys(binding).every(k=>['executable','binaryHash','dependencies'].includes(k))&&typeof binding.executable==='string'&&path.isAbsolute(binding.executable)&&/^[a-f0-9]{64}$/.test(binding.binaryHash),'BINARY_BINDING');
 const dependencies=binding.dependencies??[];need(Array.isArray(dependencies)&&dependencies.length<=4,'DEPENDENCY_BINDING');
 for(const item of [{path:binding.executable,sha256:binding.binaryHash},...dependencies]){
  need(item&&Object.keys(item).every(k=>['path','sha256'].includes(k))&&typeof item.path==='string'&&path.isAbsolute(item.path)&&/^[a-f0-9]{64}$/.test(item.sha256),'DEPENDENCY_BINDING');
  let stat,bytes;try{stat=fs.lstatSync(item.path);bytes=fs.readFileSync(item.path);}catch{throw queueError('BINARY_UNAVAILABLE');}need(stat.isFile()&&!stat.isSymbolicLink()&&hash(bytes)===item.sha256,'BINARY_HASH_MISMATCH');
 }
 return binding.executable;
}

export function nativeQueueIntentBinding(scope,spec){
 need(identity(scope)&&identity(spec?.runId)&&identity(spec?.intentId)&&identity(spec?.idempotencyKey),'INTENT_IDENTITY');
 need(['POST','DELETE'].includes(spec.method)&&typeof spec.endpoint==='string'&&spec.endpoint.startsWith('/')&&!/[\r\n]/.test(spec.endpoint),'INTENT_OPERATION');
 let bodyJson=spec.bodyJson??(spec.body===undefined?'':JSON.stringify(spec.body));
 need(typeof bodyJson==='string'&&Buffer.byteLength(bodyJson)<=65536,'INTENT_BODY');
 if(spec.body!==undefined){let decoded;try{decoded=JSON.parse(bodyJson);}catch{throw queueError('INTENT_BODY');}need(stable(decoded)===stable(spec.body),'BODY_ENCODING_MISMATCH');}
 if(spec.method==='POST')need(spec.body?.idempotencyKey===spec.idempotencyKey,'IDEMPOTENCY_KEY_MISMATCH');
 return Object.freeze({intent:spec.runId+':'+spec.intentId,key:spec.idempotencyKey,payload_hash:hash(stable({scope,method:spec.method,endpoint:spec.endpoint,bodyJson}))});
}

/**
 * Migration adapter, initially exercised only against offline directories.
 * One child belongs to the existing account owner; it is not a second HTTP client.
 * canWrite must check the real STOP, owner and bounded activation; validators
 * must check the actual scoped lease and durable intent, not evidence booleans.
 * Validators may be repeated and must be idempotent. beforeDispatch/beforeNetwork
 * are each called once per attempted operation. Existing API allowlists, response
 * semantics, contract validation and order attribution remain the caller's job.
 * perform must forward its AbortSignal into the existing HTTP operation, process
 * the complete response/body, and never retry internally. Result/error classifiers
 * are trusted constructor callbacks: a write success without one remains unknown.
 * The adapter logs no request bodies, stdout, stderr, headers or raw account data.
 * Binary and DLL hashes are checked at startup and every RPC; deployment must also
 * keep their directory immutable. Local pinning is not a distributed account lock.
 */
export class NativeAccountQueue {
 #child;#binding;#scope;#pending=null;#buffer='';#stderr='';#tail=Promise.resolve();#poison=null;#closing=false;#closed=false;#exit;#controller=new AbortController();#callbacks;#now;#sleep;#timeout;#operationTimeout;#next=0;#networkWrite=false;#signal;
 constructor({binding,directory,scope,canOperate,canWrite,validateLease,validatePermit,validateNetworkPermit,validateReconciliation,classifyResult,classifyError,now=Date.now,sleep=ms=>new Promise(r=>setTimeout(r,ms)),signal,rpcTimeoutMs=30000,operationTimeoutMs=20000}){
  need(typeof directory==='string'&&path.isAbsolute(directory)&&identity(scope)&&typeof canOperate==='function'&&Number.isSafeInteger(rpcTimeoutMs)&&rpcTimeoutMs>=100&&rpcTimeoutMs<=120000&&Number.isSafeInteger(operationTimeoutMs)&&operationTimeoutMs>=100&&operationTimeoutMs<=120000,'ADAPTER_CONFIGURATION');
  this.#binding=structuredClone(binding);const executable=verifyNativeAccountQueueBinding(this.#binding);this.#scope=scope;this.#callbacks={canOperate,canWrite,validateLease,validatePermit,validateNetworkPermit,validateReconciliation,classifyResult,classifyError};this.#now=now;this.#sleep=sleep;this.#signal=signal;this.#timeout=rpcTimeoutMs;this.#operationTimeout=operationTimeoutMs;
  this.#child=spawn(executable,['--offline-jsonl',directory,scope,randomUUID()],{windowsHide:true,stdio:['pipe','pipe','pipe']});
  this.#exit=new Promise(resolve=>this.#child.once('close',(code,termination)=>{if(!this.#closing||this.#pending)this.#fail('PROCESS_LOST');this.#closed=true;resolve({code,termination});}));
  this.#child.once('error',()=>this.#fail('PROCESS_UNAVAILABLE'));this.#child.stdin.on('error',()=>this.#fail('PROCESS_INPUT_LOST'));
  this.#child.stdout.on('data',b=>this.#output(b));this.#child.stderr.on('data',b=>{this.#stderr+=b.toString();if(this.#stderr.length>2048)this.#fail('PROCESS_DIAGNOSTIC_OVERFLOW');});
 }
 get status(){return {mode:'pinned_native_account_admission_adapter',closed:this.#closed,failed:!!this.#poison,executionAuthority:false,networkRequests:0};}
 #fail(code){if(this.#poison)return;this.#poison=queueError(code,this.#networkWrite);this.#controller.abort();if(this.#pending){clearTimeout(this.#pending.timer);this.#pending.reject(this.#poison);this.#pending=null;}this.#child.kill();}
 #output(chunk){this.#buffer+=chunk.toString();if(Buffer.byteLength(this.#buffer)>16*1024*1024){this.#fail('RESPONSE_TOO_LARGE');return;}
  let end;while((end=this.#buffer.indexOf('\n'))>=0){const line=this.#buffer.slice(0,end);this.#buffer=this.#buffer.slice(end+1);const pending=this.#pending;if(!pending){this.#fail('UNEXPECTED_RESPONSE');return;}let response;try{response=JSON.parse(line);}catch{this.#fail('INVALID_RESPONSE');return;}
   if(!response||typeof response.ok!=='boolean'||response.ok&&(!response.result||typeof response.result!=='object')||!response.ok&&!/^[A-Z0-9_]{1,90}$/.test(response.error)){this.#fail('INVALID_RESPONSE');return;}
   clearTimeout(pending.timer);this.#pending=null;response.ok?pending.resolve(response.result):pending.reject(queueError(response.error,this.#networkWrite));
  }
 }
 #clock(){const now=this.#now();need(Number.isSafeInteger(now)&&now>=0&&now<=8640000000000000,'CLOCK');return now;}
 async #rpc(command){if(this.#poison)throw this.#poison;need(!this.#closed&&!this.#pending,'PROCESS_NOT_READY');verifyNativeAccountQueueBinding(this.#binding);
  const now=this.#clock();return new Promise((resolve,reject)=>{const timer=setTimeout(()=>this.#fail('RESPONSE_TIMEOUT'),this.#timeout);this.#pending={resolve,reject,timer};this.#child.stdin.write(JSON.stringify({scope:this.#scope,now,...command})+'\n',error=>{if(error)this.#fail('PROCESS_INPUT_LOST');});});
 }
 #enqueue(work){need(!this.#closing&&!this.#closed,'CLOSED');const result=this.#tail.then(work);this.#tail=result.catch(()=>{});return result;}
 async #guard(write){if(this.#poison)throw this.#poison;need(!this.#signal?.aborted&&!this.#controller.signal.aborted&&await this.#callbacks.canOperate()===true,'STOPPED');if(write){need(typeof this.#callbacks.canWrite==='function'&&await this.#callbacks.canWrite()===true,'WRITE_STOPPED_OR_OWNER_LOST');need(await this.#callbacks.canOperate()===true,'STOPPED');}need(!this.#signal?.aborted&&!this.#controller.signal.aborted,'STOPPED');}
 async #wait(until,write){while(this.#clock()<until){await this.#guard(write);await this.#sleep(Math.min(200,until-this.#clock()));}await this.#guard(write);}
 async #fences(spec,authority,network){await this.#guard(true);const c=this.#callbacks;
  need(typeof c.validateLease==='function'&&await c.validateLease(authority.lease,spec)===true,'INVALID_EXECUTION_LEASE');
  const validate=network?c.validateNetworkPermit:c.validatePermit;need(typeof validate==='function'&&await validate(authority.permit,spec)===true,network?'INVALID_NETWORK_EXECUTION_PERMIT':'INVALID_EXECUTION_PERMIT');await this.#guard(true);
  return {authorized:true,lease_valid:true,permit_valid:true};
 }
 async #perform(perform,spec){const timeout=new AbortController(),timer=setTimeout(()=>timeout.abort(),this.#operationTimeout);const signal=AbortSignal.any([this.#controller.signal,timeout.signal,...(this.#signal?[this.#signal]:[])]);let abort;
  const interrupted=new Promise((_,reject)=>{abort=()=>reject(this.#poison??queueError(timeout.signal.aborted?'OPERATION_TIMEOUT':'STOPPED',spec.method!=='GET'));signal.addEventListener('abort',abort,{once:true});if(signal.aborted)abort();});
  try{return await Promise.race([Promise.resolve().then(()=>{if(signal.aborted)throw this.#poison??queueError('STOPPED',spec.method!=='GET');return perform(spec,signal);}),interrupted]);}finally{clearTimeout(timer);signal.removeEventListener('abort',abort);}
 }
 async #run(spec,authority,perform){
  const write=spec.method!=='GET';const binding=write?nativeQueueIntentBinding(this.#scope,spec):undefined;const request=randomUUID();let admitted=false,network=false,performed=false;let result,operationError;
  try{
   await this.#wait(this.#next,write);let flags=write?await this.#fences(spec,authority,false):{};
   if(spec.beforeDispatch)await spec.beforeDispatch();await this.#guard(write);
   while(true){const response=await this.#rpc({op:'admit',request,write,...(binding?{binding,...flags}:{})});
    need(typeof response.admitted==='boolean','ADMISSION_RESPONSE');if(response.admitted){admitted=true;this.#next=response.next;break;}
    need(Number.isSafeInteger(response.wait_until)&&response.wait_until>=this.#clock(),'WAIT_RESPONSE');await this.#wait(response.wait_until,write);flags=write?await this.#fences(spec,authority,false):{};
   }
   if(spec.beforeNetwork)await spec.beforeNetwork();const networkFlags=write?await this.#fences(spec,authority,true):{};
   if(write)this.#networkWrite=true;
   const allowed=await this.#rpc({op:'network',request,write,...(binding?{binding,...networkFlags}:{})});need(allowed.network_permitted===true,'NETWORK_RESPONSE');network=true;
   await this.#guard(write);performed=true;
   result=await this.#perform(perform,spec);
  }catch(error){operationError=error;}
  if(admitted&&!this.#poison){
   let classification;
   try{
    if(!network&&!this.#networkWrite)classification={outcome:'not_sent'};
    else if(!performed)classification={outcome:write?'unknown':'rejected'};
    else if(operationError)classification=typeof this.#callbacks.classifyError==='function'?await this.#callbacks.classifyError(operationError,spec):{outcome:write?'unknown':'rejected',status:operationError.status??0};
    else classification=typeof this.#callbacks.classifyResult==='function'?await this.#callbacks.classifyResult(result,spec):{outcome:write?'unknown':'success'};
    need(classification&&['success','rejected','unknown','not_sent'].includes(classification.outcome)&&(!network||classification.outcome!=='not_sent'),'OUTCOME_CLASSIFICATION');
    const status=classification.status??0;need(Number.isInteger(status)&&status>=0&&status<=599,'OUTCOME_STATUS');
    const complete=await this.#rpc({op:'complete',request,outcome:classification.outcome,status,retry_after:classification.retryAfter??'',api_code:classification.apiCode??''});this.#next=complete.next;
    if(write&&classification.outcome==='unknown')operationError=queueError('UNKNOWN_ACCEPTANCE_RECONCILE_FIRST',true);
    else if(classification.outcome==='rejected'&&!operationError){operationError=queueError('OPERATION_REJECTED');operationError.status=status;}
   }catch(error){this.#fail('COMPLETION_DURABILITY_UNCERTAIN');operationError=queueError(error.code??'COMPLETION_DURABILITY_UNCERTAIN',write&&network);}
  }
  this.#networkWrite=false;
  if(this.#poison)throw this.#poison;if(operationError)throw operationError;return result;
 }
 read({endpoint,perform,beforeNetwork}){need(typeof endpoint==='string'&&endpoint.startsWith('/')&&typeof perform==='function','READ_SPEC');return this.#enqueue(()=>this.#run(Object.freeze({method:'GET',endpoint,beforeNetwork}),null,perform));}
 write(input,authority,{perform,beforeDispatch,beforeNetwork}={}){need(typeof perform==='function'&&authority&&typeof authority.permit==='object'&&authority.lease,'WRITE_SPEC');
  const spec=freeze({...structuredClone(input),beforeDispatch,beforeNetwork});const ownedAuthority=freeze(structuredClone(authority));nativeQueueIntentBinding(this.#scope,spec);return this.#enqueue(()=>this.#run(spec,ownedAuthority,perform));
 }
 inspect(){return this.#enqueue(async()=>{await this.#guard(false);return this.#rpc({op:'status'});});}
 compact(){return this.#enqueue(async()=>{await this.#guard(false);return this.#rpc({op:'compact'});});}
 reconcile(spec,evidence){const binding=nativeQueueIntentBinding(this.#scope,structuredClone(spec));const snapshot=structuredClone(evidence);return this.#enqueue(async()=>{
  await this.#guard(true);need(typeof this.#callbacks.validateReconciliation==='function'&&await this.#callbacks.validateReconciliation(binding,snapshot)===true,'SCOPED_RECONCILIATION_REQUIRED');await this.#guard(true);
  return this.#rpc({op:'reconcile',binding,reconciled:true,lease_valid:true,permit_valid:true});
 });}
 async close({force=false}={}){if(this.#closed)return this.#exit;this.#closing=true;if(force)this.#fail('PROCESS_LOST');else{await this.#tail;if(!this.#poison)this.#child.stdin.end();}return this.#exit;}
}
