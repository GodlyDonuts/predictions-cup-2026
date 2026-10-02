import fs from 'node:fs';
import path from 'node:path';
import {createHash,randomUUID} from 'node:crypto';

const VERSION=1;
const identity=/^[A-Za-z0-9_.:-]{1,160}$/;
const orderFields=new Set(['idempotencyKey','exchangeId','side','action','quantity','price','expirationDate','tournamentId']);
const attributionFields=new Set(['ownerId','runId','strategyId','decisionId','policyId','marketId','exchangeId','parentIntentId','orderId','signalId','modelHash']);
const outcomeFields=new Set(['status','orderId','reasonCode','retryAllowed','earliestRetryAt']);
const semanticUncertainty=/semantic_mismatch|missing_order_id|null_order_id|conflict|canonical_order_mismatch|unattributable_order_id/i;
const clone=value=>structuredClone(value);
function fail(code,message){const error=Error(message);error.code=code;throw error;}
function object(value){return value!==null&&typeof value==='object'&&!Array.isArray(value)&&Object.getPrototypeOf(value)===Object.prototype;}
function id(value,label){if(typeof value!=='string'||!identity.test(value)||['__proto__','constructor','prototype'].includes(value))fail('INVALID_INPUT','Invalid '+label);return value;}
function exactKeys(value,allowed,label){if(!object(value)||Object.keys(value).some(k=>!allowed.has(k)))fail('INVALID_INPUT','Invalid '+label);}
function instant(value){if(typeof value!=='string'||!Number.isFinite(Date.parse(value))||new Date(value).toISOString()!==value)fail('INVALID_INPUT','Invalid timestamp');return value;}

export function canonicalJson(value){
 function normalize(v){
  if(v===null||typeof v==='string'||typeof v==='boolean')return v;
  if(typeof v==='number'&&Number.isFinite(v))return v;
  if(Array.isArray(v))return v.map(normalize);
  if(object(v)){const out={};for(const key of Object.keys(v).sort())Object.defineProperty(out,key,{value:normalize(v[key]),enumerable:true});return out;}
  fail('INVALID_INPUT','Canonical JSON requires finite JSON values');
 }
 return JSON.stringify(normalize(value));
}
export const payloadHash=body=>createHash('sha256').update(canonicalJson(body)).digest('hex');
const hash=value=>createHash('sha256').update(canonicalJson(value)).digest('hex');
function writeAll(fd,text){const bytes=Buffer.from(text);let offset=0;while(offset<bytes.length){const written=fs.writeSync(fd,bytes,offset,bytes.length-offset);if(written<1)fail('DURABILITY_FAILURE','Incomplete durable write');offset+=written;}}
function syncDirectory(dir){
 let fd;try{fd=fs.openSync(dir,'r');fs.fsyncSync(fd);}catch(error){if(process.platform!=='win32'||!['EACCES','EPERM','EISDIR','EINVAL','EBADF','ENOTSUP'].includes(error.code))throw error;}finally{if(fd!==undefined)fs.closeSync(fd);}
}

// Projections are replaceable caches. The append-only journal is authoritative.
export function atomicJson(file,value){
 const temporary=file+'.'+process.pid+'.'+randomUUID()+'.tmp';let fd;
 try{fd=fs.openSync(temporary,'wx',0o600);writeAll(fd,canonicalJson(value)+'\n');fs.fsyncSync(fd);fs.closeSync(fd);fd=undefined;fs.renameSync(temporary,file);syncDirectory(path.dirname(file));}
 catch(error){if(fd!==undefined)fs.closeSync(fd);try{fs.unlinkSync(temporary);}catch(cleanup){if(cleanup.code!=='ENOENT')error.cleanupFailed=true;}throw error;}
}

export function acquireOwnerLease({dir,ownerId,runId,scope,now=Date.now}){
 id(ownerId,'owner');id(runId,'run');id(scope,'scope');
 fs.mkdirSync(dir,{recursive:true,mode:0o700});
 const file=path.join(dir,'execution-owner.lock'),record={schemaVersion:VERSION,ownerId,runId,scope,pid:process.pid,token:randomUUID(),createdAt:new Date(now()).toISOString()};
 let fd;
 try{fd=fs.openSync(file,'wx',0o600);}catch(error){if(error.code==='EEXIST')fail('OWNER_LOCKED','Execution owner lock already exists; explicit recovery is required');throw error;}
 try{writeAll(fd,canonicalJson(record)+'\n');fs.fsyncSync(fd);fs.closeSync(fd);fd=undefined;syncDirectory(dir);}catch(error){if(fd!==undefined)fs.closeSync(fd);try{fs.unlinkSync(file);}catch{}throw error;}
 let released=false;
 function assertOwnership(){
  if(released)fail('OWNER_RELEASED','Execution owner lease has been released');
  let current;try{current=JSON.parse(fs.readFileSync(file,'utf8'));}catch{fail('OWNER_CHANGED','Execution owner lease is missing or invalid');}
  if(canonicalJson(current)!==canonicalJson(record))fail('OWNER_CHANGED','Execution owner lease identity changed');
 }
 return {file,identity:clone(record),assertOwnership,release(){if(released)return;assertOwnership();fs.unlinkSync(file);syncDirectory(dir);released=true;}};
}

function normalizeIntent(input){
 exactKeys(input,new Set(['intentId','idempotencyKey','method','path','body','bodyJson','payloadHash','attribution']),'intent');
 const intentId=id(input.intentId,'intent ID'),idempotencyKey=id(input.idempotencyKey,'idempotency key');
 if(input.method!=='POST'||input.path!=='/orders')fail('INVALID_INPUT','Only order-placement intents belong in this ledger');
 let body=input.body;
 if(input.bodyJson!==undefined){if(typeof input.bodyJson!=='string')fail('INVALID_INPUT','Invalid frozen order body');let parsed;try{parsed=JSON.parse(input.bodyJson);}catch{fail('INVALID_INPUT','Invalid frozen order body');}if(body!==undefined&&canonicalJson(body)!==canonicalJson(parsed))fail('INTENT_CONFLICT','Frozen body differs from order terms');body=parsed;}
 exactKeys(body,orderFields,'order terms');
 for(const value of Object.values(body))if(value!==null&&!['string','number','boolean'].includes(typeof value)||typeof value==='number'&&!Number.isFinite(value))fail('INVALID_INPUT','Order terms must be scalar JSON values');
 if(body.idempotencyKey!==idempotencyKey)fail('INTENT_CONFLICT','Body and intent idempotency keys differ');
 const bodyJson=canonicalJson(body),digest=payloadHash(body);
 if(input.bodyJson!==undefined&&input.bodyJson!==bodyJson)fail('INTENT_CONFLICT','Frozen order body must use canonical JSON');
 if(input.payloadHash!==undefined&&input.payloadHash!==digest)fail('INTENT_CONFLICT','Order payload hash differs');
 const attribution=input.attribution??{};exactKeys(attribution,attributionFields,'intent attribution');for(const [key,value]of Object.entries(attribution))id(value,key);
 return {intentId,idempotencyKey,method:'POST',path:'/orders',bodyJson,payloadHash:digest,attribution:clone(attribution)};
}
function normalizeOutcome(input){
 exactKeys(input,outcomeFields,'intent outcome');
 if(!['uncertain','acknowledged','reconciled','closed'].includes(input.status))fail('INVALID_INPUT','Invalid intent outcome status');
 const out={status:input.status};
 if(input.orderId!==undefined&&input.orderId!==null)out.orderId=id(input.orderId,'order ID');
 if(input.reasonCode!==undefined){if(typeof input.reasonCode!=='string'||!/^[A-Za-z][A-Za-z0-9_]{0,79}$/.test(input.reasonCode))fail('INVALID_INPUT','Invalid outcome reason code');out.reasonCode=input.reasonCode;}
 if(input.retryAllowed!==undefined&&typeof input.retryAllowed!=='boolean')fail('INVALID_INPUT','Invalid retry permission');
 if(input.status==='uncertain'){
  out.retryAllowed=input.retryAllowed===true;
  if(semanticUncertainty.test(out.reasonCode||'')&&out.retryAllowed)fail('INVALID_INPUT','Semantic uncertainty cannot authorize retry');
  if(input.earliestRetryAt!==undefined)out.earliestRetryAt=instant(input.earliestRetryAt);
 }else if(input.retryAllowed!==undefined||input.earliestRetryAt!==undefined)fail('INVALID_INPUT','Only uncertain outcomes may authorize retries');
 return out;
}
const empty=()=>({schemaVersion:VERSION,sequence:0,lastHash:'',scope:null,intents:{}});
function immutable(intent){return {intentId:intent.intentId,idempotencyKey:intent.idempotencyKey,method:intent.method,path:intent.path,bodyJson:intent.bodyJson,payloadHash:intent.payloadHash,attribution:intent.attribution};}
function applyEvent(state,event,at){
 if(!object(event))fail('JOURNAL_CORRUPT','Invalid intent journal event');
 if(event.type==='prepared'){
  exactKeys(event,new Set(['type','intent']),'prepared event');
  const intent=normalizeIntent(event.intent);
  if(JSON.parse(intent.bodyJson).tournamentId!==state.scope)fail('SCOPE_MISMATCH','Prepared order belongs to another execution scope');
  if(state.intents[intent.intentId]||Object.values(state.intents).some(x=>x.idempotencyKey===intent.idempotencyKey))fail('INTENT_CONFLICT','Duplicate intent or idempotency key in journal');
  state.intents[intent.intentId]={...intent,body:JSON.parse(intent.bodyJson),status:'prepared',attempts:0,preparedAt:at,updatedAt:at,outcome:null};return;
 }
 if(!['dispatching','outcome'].includes(event.type))fail('JOURNAL_CORRUPT','Unknown intent journal event');
 id(event.intentId,'intent ID');const intent=state.intents[event.intentId];if(!intent)fail('JOURNAL_CORRUPT','Intent event precedes preparation');
 if(event.type==='dispatching'){
  exactKeys(event,new Set(['type','intentId','attempt']),'dispatch event');
  if(intent.orderId||intent.retryBlocked||!['prepared','uncertain'].includes(intent.status)||intent.status==='uncertain'&&(!intent.outcome?.retryAllowed||intent.outcome.earliestRetryAt&&Date.parse(at)<Date.parse(intent.outcome.earliestRetryAt)))fail('INVALID_TRANSITION','Intent requires reconciliation before dispatch');
  if(event.attempt!==intent.attempts+1)fail('JOURNAL_CORRUPT','Invalid dispatch attempt sequence');
  intent.status='dispatching';intent.attempts=event.attempt;intent.updatedAt=at;intent.lastDispatchAt=at;return;
 }
 exactKeys(event,new Set(['type','intentId','outcome']),'outcome event');const outcome=normalizeOutcome(event.outcome);
 const transitions={prepared:['closed'],dispatching:['uncertain','acknowledged','reconciled','closed'],uncertain:['uncertain','acknowledged','reconciled','closed'],acknowledged:['uncertain','reconciled','closed'],reconciled:['reconciled','closed'],closed:[]};
 if(!transitions[intent.status].includes(outcome.status))fail('INVALID_TRANSITION','Invalid intent status transition');
 if((intent.orderId||intent.retryBlocked)&&outcome.retryAllowed)fail('INVALID_TRANSITION','Attributed or semantically uncertain orders cannot authorize another placement');
 if(intent.orderId&&outcome.orderId&&intent.orderId!==outcome.orderId)fail('INTENT_CONFLICT','Official order attribution changed');
 if(['acknowledged','reconciled'].includes(outcome.status)&&!outcome.orderId&&!intent.orderId)fail('INVALID_INPUT','Acknowledged or reconciled intent requires an order ID');
 if(outcome.orderId)intent.orderId=outcome.orderId;
 if(outcome.status==='uncertain'&&semanticUncertainty.test(outcome.reasonCode||''))intent.retryBlocked=true;
 intent.status=outcome.status;intent.outcome=outcome;intent.updatedAt=at;
}

export function replayJournal(raw,{scope}={}){
 if(typeof raw!=='string')fail('INVALID_INPUT','Journal must be text');
 if(raw&&!raw.endsWith('\n'))fail('JOURNAL_INCOMPLETE','Intent journal has an incomplete crash tail; preserve it for explicit recovery');
 const state=empty();if(scope!==undefined)id(scope,'scope');
 for(const line of raw?raw.slice(0,-1).split('\n'):[]){
  let record;try{record=JSON.parse(line);}catch{fail('JOURNAL_CORRUPT','Invalid intent journal record');}
  exactKeys(record,new Set(['schemaVersion','sequence','previous','at','ownerId','runId','scope','event','hash']),'journal record');
  const {hash:recordHash,...body}=record;
  if(body.schemaVersion!==VERSION||body.sequence!==state.sequence+1||body.previous!==state.lastHash||recordHash!==hash(body))fail('JOURNAL_CORRUPT','Intent journal sequence or hash mismatch');
  instant(body.at);id(body.ownerId,'owner');id(body.runId,'run');id(body.scope,'scope');
  if(scope!==undefined&&body.scope!==scope||state.scope!==null&&body.scope!==state.scope)fail('SCOPE_MISMATCH','Intent journal belongs to another execution scope');
  state.scope=body.scope;applyEvent(state,body.event,body.at);state.sequence=body.sequence;state.lastHash=recordHash;
 }
 if(state.scope===null&&scope!==undefined)state.scope=scope;
 return state;
}

export class IntentLedger {
 #state;#lease;#closed=false;#poisoned=false;#isStopped;#now;#identity;
 constructor({dir,ownerId,runId,scope,isStopped,now=Date.now}){
  if(typeof isStopped!=='function')fail('INVALID_INPUT','A STOP callback is required');
  const resolved=path.resolve(dir);Object.defineProperties(this,{dir:{value:resolved,enumerable:true},file:{value:path.join(resolved,'execution-journal.jsonl'),enumerable:true},projectionFile:{value:path.join(resolved,'execution-state.json'),enumerable:true}});
  this.#identity={ownerId,runId,scope};this.#isStopped=isStopped;this.#now=now;
  this.#lease=acquireOwnerLease({dir:this.dir,ownerId,runId,scope,now});
  try{let raw='';try{raw=fs.readFileSync(this.file,'utf8');}catch(error){if(error.code!=='ENOENT')throw error;}this.#state=replayJournal(raw,{scope});atomicJson(this.projectionFile,this.#state);}catch(error){this.#lease.release();throw error;}
 }
 #check(){if(this.#closed)fail('LEDGER_CLOSED','Intent ledger is closed');if(this.#poisoned)fail('DURABILITY_FAILURE','Intent ledger requires restart and reconciliation');this.#lease.assertOwnership();}
 #append(event){
  this.#check();const at=new Date(this.#now()).toISOString(),next=clone(this.#state);applyEvent(next,event,at);
  const body={schemaVersion:VERSION,sequence:this.#state.sequence+1,previous:this.#state.lastHash,at,...this.#identity,event},digest=hash(body);let fd;
  try{fd=fs.openSync(this.file,'a',0o600);writeAll(fd,canonicalJson({...body,hash:digest})+'\n');fs.fsyncSync(fd);fs.closeSync(fd);fd=undefined;syncDirectory(this.dir);next.sequence=body.sequence;next.lastHash=digest;next.scope=this.#identity.scope;this.#state=next;atomicJson(this.projectionFile,next);}
  catch(error){this.#poisoned=true;if(fd!==undefined)fs.closeSync(fd);throw error;}
 }
 prepareIntent(input){
  this.#check();const intent=normalizeIntent(input),existing=this.#state.intents[intent.intentId];
  if(existing){if(canonicalJson(immutable(existing))!==canonicalJson(intent))fail('INTENT_CONFLICT','Existing intent terms cannot change');return clone(existing);}
  if(Object.values(this.#state.intents).some(x=>x.idempotencyKey===intent.idempotencyKey))fail('INTENT_CONFLICT','Idempotency key already belongs to another intent');
  this.#append({type:'prepared',intent});return this.getIntent(intent.intentId);
 }
 markDispatching(intentId,{retry=false}={}){
  this.#check();if(this.#isStopped()!==false)fail('STOPPED','Execution STOP is active');const intent=this.#state.intents[id(intentId,'intent ID')];if(!intent)fail('UNKNOWN_INTENT','Intent must be durably prepared before dispatch');
  if(retry!==true&&intent.status!=='prepared'||retry===true&&intent.status!=='uncertain')fail('INVALID_TRANSITION','Retry must be explicit and preserve the original prepared intent');
  this.#append({type:'dispatching',intentId,attempt:intent.attempts+1});
  // A STOP observed during the durability barrier leaves an unresolved dispatch
  // record, which is safer than returning permission to send a request.
  if(this.#isStopped()!==false)fail('STOPPED','Execution STOP became active before dispatch');
  return this.getIntent(intentId);
 }
 recordOutcome(intentId,input){
  this.#check();id(intentId,'intent ID');const intent=this.#state.intents[intentId];if(!intent)fail('UNKNOWN_INTENT','Unknown prepared intent');const outcome=normalizeOutcome(input);
  if(intent.status===outcome.status&&canonicalJson(intent.outcome)===canonicalJson(outcome))return clone(intent);
  this.#append({type:'outcome',intentId,outcome});return this.getIntent(intentId);
 }
 getIntent(intentId){this.#check();id(intentId,'intent ID');return this.#state.intents[intentId]?clone(this.#state.intents[intentId]):null;}
 snapshot(){this.#check();return clone(this.#state);}
 close(){if(this.#closed)return;this.#lease.release();this.#closed=true;}
}
export const openLedger=options=>new IntentLedger(options);
