import fs from 'node:fs';import path from 'node:path';import {createHash,randomUUID} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {decimalMicros,payloadHash,canonicalPayload} from './core.mjs';
import {projectCanonicalFifo} from './canonical-fifo.mjs';
const directory=path.dirname(fileURLToPath(import.meta.url));
const need=(x,c)=>{if(!x)throw Error(c);};
const id=x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)&&Number.isSafeInteger(Number(x));
const token=x=>typeof x==='string'&&/^[A-Za-z0-9_.:-]{1,160}$/.test(x)&&!['__proto__','constructor','prototype'].includes(x);
const stamp=x=>typeof x==='string'&&Number.isFinite(Date.parse(x))&&new Date(x).toISOString()===x;
const plain=x=>x!==null&&typeof x==='object'&&!Array.isArray(x)&&Object.getPrototypeOf(x)===Object.prototype;
const keys=(x,allowed)=>plain(x)&&Object.keys(x).every(k=>allowed.includes(k));
const sha=x=>createHash('sha256').update(x).digest('hex');
const tick=x=>{need(typeof x==='string','CANONICAL_EXACT_LIMIT');const n=decimalMicros(x);need(n>=5000n&&n<=995000n&&n%5000n===0n,'CANONICAL_EXACT_LIMIT');return n;};
const sources=()=>Object.fromEntries(['canonical-order-journal.mjs','canonical-fifo.mjs','side-accounting.mjs','api-tick-price.mjs','core.mjs'].map(x=>[x,sha(fs.readFileSync(path.join(directory,x)))]));
function prepare(input){
 need(keys(input,['intentId','idempotencyKey','binding','wallet'])&&token(input.intentId)&&token(input.idempotencyKey),'CANONICAL_PREPARATION');
 const b=input.binding,w=input.wallet;
 need(keys(b,['ownerId','exchangeId','marketId','tournamentId','side','action','quantity','limit','createdAt','expirationDate'])
  &&token(b.ownerId)&&id(b.exchangeId)&&id(b.marketId)&&typeof b.tournamentId==='string'&&b.tournamentId.length>0
  &&['yes','no'].includes(b.side)&&['buy','sell'].includes(b.action)&&Number.isSafeInteger(b.quantity)&&b.quantity>0&&b.quantity<=100
  &&stamp(b.createdAt)&&stamp(b.expirationDate)&&Date.parse(b.expirationDate)>Date.parse(b.createdAt)
  &&Date.parse(b.expirationDate)-Date.parse(b.createdAt)<=120000,'CANONICAL_BINDING');tick(b.limit);
 need(keys(w,['ownerId','exchangeId','marketId','tournamentId','cash','realizedPnl','accountSignedQuantity','lots','appliedOrderIds'])
  &&w.ownerId===b.ownerId&&w.exchangeId===b.exchangeId&&w.marketId===b.marketId&&w.tournamentId===b.tournamentId
  &&typeof w.cash==='string'&&typeof w.realizedPnl==='string'&&Number.isSafeInteger(w.accountSignedQuantity)
  &&Array.isArray(w.lots)&&w.lots.length<=10000,'CANONICAL_BASELINE');decimalMicros(w.cash);decimalMicros(w.realizedPnl.replace(/^-/,''));
 let qty=0,old=null,clock=-Infinity;const lots=new Set();for(const l of w.lots){
  need(keys(l,['lotId','side','quantity','entryPrice','openedAt'])&&token(l.lotId)&&!lots.has(l.lotId)
   &&['yes','no'].includes(l.side)&&Number.isSafeInteger(l.quantity)&&l.quantity>0&&stamp(l.openedAt)
   &&Date.parse(l.openedAt)>=clock&&Date.parse(l.openedAt)<=Date.parse(b.createdAt),'CANONICAL_BASELINE_LOT');
  old??=l.side;need(old===l.side,'CANONICAL_MIXED_LOTS');lots.add(l.lotId);tick(l.entryPrice);clock=Date.parse(l.openedAt);qty+=(l.side==='yes'?1:-1)*l.quantity;
 }
 need(Number.isSafeInteger(qty)&&qty===w.accountSignedQuantity,'CANONICAL_UNATTRIBUTED_BASELINE');
 const applied=w.appliedOrderIds??[];need(Array.isArray(applied)&&applied.length<=10000&&applied.every(id)&&new Set(applied).size===applied.length,'CANONICAL_BASELINE_HISTORY');
 const body={idempotencyKey:input.idempotencyKey,exchangeId:b.exchangeId,side:b.side,action:b.action,quantity:b.quantity,price:Number(b.limit),expirationDate:b.expirationDate,tournamentId:b.tournamentId};
 return {...structuredClone(input),requestBody:body,requestHash:payloadHash(body),baselineHash:payloadHash(w),orderId:null,phase:'prepared',attempts:0,evidence:null,projection:null,executionAuthority:false,authoritativeReconciliation:false};
}
function advance(state,kind,payload){
 if(kind==='prepare'){need(state===null,'CANONICAL_ALREADY_PREPARED');return prepare(payload);}
 need(state!==null,'CANONICAL_NOT_PREPARED');const next=structuredClone(state);
 if(kind==='dispatching'){need(state.phase==='prepared'&&payload===null,'CANONICAL_DISPATCH_PHASE');next.phase='dispatching';next.attempts++;return next;}
 if(kind==='retry'){need(state.phase==='uncertain'&&keys(payload,['requestHash','idempotencyKey'])&&payload.requestHash===state.requestHash&&payload.idempotencyKey===state.idempotencyKey,'CANONICAL_RETRY_CHANGED_OR_PHASE');next.phase='dispatching';next.attempts++;return next;}
 if(kind==='uncertain'){need(['dispatching','uncertain'].includes(state.phase)&&keys(payload,['reasonCode'])&&/^[A-Z][A-Z0-9_]{0,80}$/.test(payload.reasonCode),'CANONICAL_UNCERTAIN_PHASE');next.phase='uncertain';next.reasonCode=payload.reasonCode;return next;}
 if(kind==='bind'){
  need(['dispatching','uncertain'].includes(state.phase)&&keys(payload,['orderId'])&&id(payload.orderId)
   &&!(state.wallet.appliedOrderIds??[]).includes(payload.orderId),'CANONICAL_ORDER_BINDING');next.orderId=payload.orderId;next.phase='bound';return next;
 }
 need(kind==='observe'&&['bound','observed','terminal'].includes(state.phase),'CANONICAL_OBSERVATION_PHASE');
 need(keys(payload,['ack','rows','source','readAt'])&&stamp(payload.readAt)
  &&keys(payload.ack,['orderId','exchangeId','side','action','quantity','price','all','quantityTraded','remainingQuantity','open','totalCost','terminalReasonCode'])
  &&Array.isArray(payload.rows)&&payload.rows.every(f=>keys(f,['id','orderId','exchangeId','marketId','tournamentId','side','quantity','price','filledAt'])),'CANONICAL_OBSERVATION_SHAPE');
 const prior=state.evidence;
 if(prior){
  need(payload.source===prior.source&&Date.parse(payload.readAt)>=Date.parse(prior.readAt),'CANONICAL_OBSERVATION_CLOCK_OR_SOURCE');
  const immutable=['orderId','exchangeId','side','action','quantity','price'];
  need(immutable.every(k=>canonicalPayload(payload.ack[k])===canonicalPayload(prior.ack[k])),'CANONICAL_ACK_CHANGED');
  need(payload.ack.quantityTraded>=prior.ack.quantityTraded&&payload.ack.remainingQuantity<=prior.ack.remainingQuantity,'CANONICAL_FILL_REGRESSION');
  for(const old of prior.rows)need(payload.rows.some(f=>canonicalPayload(f)===canonicalPayload(old)),'CANONICAL_FILL_HISTORY_CHANGED');
  if(state.phase==='terminal')need(payloadHash({...payload,readAt:prior.readAt})===payloadHash(prior),'CANONICAL_TERMINAL_CHANGED');
 }
 const projected=projectCanonicalFifo({wallet:state.wallet,binding:{...state.binding,orderId:state.orderId},...payload});
 next.evidence=structuredClone(payload);next.projection=projected;next.phase=payload.ack.open?'observed':'terminal';return next;
}

// Owns one accounting journal only. The account owner lease and network permit
// remain the caller's responsibility; this class has no network capability.
export function openCanonicalOrderJournal({dir,readOnly=false,canCommit=()=>true}={}){
 need(typeof dir==='string'&&path.isAbsolute(dir)&&typeof canCommit==='function','CANONICAL_JOURNAL_DIRECTORY');
 const identity=sources(),file=path.join(dir,'canonical-order.jsonl'),lock=path.join(dir,'canonical-order.lock');let lockFd,lockStat,lockBytes,failed=false,closed=false;
 if(!readOnly){need(canCommit(),'CANONICAL_JOURNAL_STOP');fs.mkdirSync(dir,{recursive:true,mode:0o700});lockFd=fs.openSync(lock,'wx',0o600);lockBytes=Buffer.from(JSON.stringify({pid:process.pid,token:randomUUID()}));try{fs.writeFileSync(lockFd,lockBytes);fs.fsyncSync(lockFd);lockStat=fs.fstatSync(lockFd);}catch(error){fs.closeSync(lockFd);lockFd=undefined;throw error;}}
 const release=()=>{if(lockFd!==undefined){fs.closeSync(lockFd);lockFd=undefined;const s=fs.statSync(lock);need(s.ino===lockStat.ino&&s.birthtimeMs===lockStat.birthtimeMs&&fs.readFileSync(lock).equals(lockBytes),'CANONICAL_LOCK_CHANGED');fs.unlinkSync(lock);}};
 try{
  if(!fs.existsSync(file)){need(!readOnly,'CANONICAL_JOURNAL_MISSING');fs.closeSync(fs.openSync(file,'wx',0o600));}
  const bytes=fs.readFileSync(file);need(bytes.length===0||bytes.at(-1)===10,'CANONICAL_JOURNAL_PARTIAL');
  let state=null,sequence=0,previous=null;
  for(const line of bytes.toString('utf8').split('\n').filter(Boolean)){
   const row=JSON.parse(line),{hash,...body}=row;
   need(keys(body,['schemaVersion','sequence','previous','codeIdentity','kind','payload','stateHash'])&&body.schemaVersion===1&&body.sequence===++sequence&&body.previous===previous&&payloadHash(body)===hash&&payloadHash(body.codeIdentity)===payloadHash(identity),'CANONICAL_JOURNAL_CHAIN_OR_CODE');
   state=advance(state,body.kind,body.payload);need(payloadHash(state)===body.stateHash,'CANONICAL_JOURNAL_REPLAY');previous=hash;
  }
  let size=bytes.length,bytesHash=sha(bytes);const stat=fs.statSync(file);
  const live=()=>{
   need(!readOnly&&!closed&&!failed,'CANONICAL_JOURNAL_UNWRITABLE');need(canCommit(),'CANONICAL_JOURNAL_STOP');
   const current=fs.statSync(file),priorBytes=fs.readFileSync(file),ls=fs.statSync(lock);
   need(ls.ino===lockStat.ino&&ls.birthtimeMs===lockStat.birthtimeMs&&fs.readFileSync(lock).equals(lockBytes),'CANONICAL_LOCK_CHANGED');
   need(current.ino===stat.ino&&current.birthtimeMs===stat.birthtimeMs&&current.size===size&&sha(priorBytes)===bytesHash&&payloadHash(sources())===payloadHash(identity),'CANONICAL_JOURNAL_CHANGED');return priorBytes;
  };
  const commit=(kind,payload)=>{
   const priorBytes=live();
   const next=advance(state,kind,payload),body={schemaVersion:1,sequence:sequence+1,previous,codeIdentity:identity,kind,payload:structuredClone(payload),stateHash:payloadHash(next)},hash=payloadHash(body),data=Buffer.from(canonicalPayload({...body,hash})+'\n');
   need(size+data.length<=64*1024*1024,'CANONICAL_JOURNAL_TOO_LARGE');need(canCommit(),'CANONICAL_JOURNAL_STOP');
   const expectedHash=sha(Buffer.concat([priorBytes,data])),fd=fs.openSync(file,'a');try{let offset=0;while(offset<data.length){const n=fs.writeSync(fd,data,offset,data.length-offset);need(n>0,'CANONICAL_JOURNAL_WRITE');offset+=n;}fs.fsyncSync(fd);
    const after=fs.statSync(file);need(after.ino===stat.ino&&after.birthtimeMs===stat.birthtimeMs&&after.size===size+data.length&&sha(fs.readFileSync(file))===expectedHash,'CANONICAL_JOURNAL_CHANGED');need(canCommit(),'CANONICAL_JOURNAL_STOP');
   }catch(error){failed=true;throw error;}finally{fs.closeSync(fd);}
   size+=data.length;bytesHash=expectedHash;state=next;sequence++;previous=hash;return structuredClone(state);
  };
  return {
   prepare:input=>{live();if(state){need(payloadHash(prepare(input))===payloadHash(prepare({intentId:state.intentId,idempotencyKey:state.idempotencyKey,binding:state.binding,wallet:state.wallet})),'CANONICAL_PREPARATION_CHANGED');return structuredClone(state);}return commit('prepare',input);},
   markDispatch:()=>commit('dispatching',null),uncertain:reasonCode=>commit('uncertain',{reasonCode}),retry:proof=>commit('retry',proof),
   bind:orderId=>{live();if(state?.orderId){need(state.orderId===orderId,'CANONICAL_ORDER_ID_CHANGED');return structuredClone(state);}return commit('bind',{orderId});},
   observe:input=>{live();if(state?.evidence&&payloadHash(state.evidence)===payloadHash(input))return structuredClone(state);return commit('observe',input);},
   status:()=>({sequence,hash:previous,state:structuredClone(state),readOnly,closed,failed,unconfirmedPersistence:failed,executionAuthority:false}),
   close:()=>{if(!closed){closed=true;release();}},
  };
 }catch(error){release();throw error;}
}
