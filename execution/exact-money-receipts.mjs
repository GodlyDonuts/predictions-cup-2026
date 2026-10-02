import fs from 'node:fs';import {createHash} from 'node:crypto';
import {decimalMicros,formatMicros,payloadHash} from './core.mjs';
const need=(x,c)=>{if(!x)throw Error(c);};
const sha=x=>createHash('sha256').update(x).digest('hex');

// Parse JSON without losing number lexemes. Reject duplicate keys, including
// escaped duplicates, so a receipt cannot bind a different value than the API.
export function parseNumberLexemes(raw){
 need(typeof raw==='string'&&Buffer.byteLength(raw)<=8*1024*1024,'EXACT_JSON_SIZE');
 let at=0;const numbers=new Map(),space=()=>{while(/[\t\n\r ]/.test(raw[at]||'!'))at++;};
 const string=()=>{need(raw[at]==='"','EXACT_JSON_STRING');const start=at++;while(at<raw.length){const c=raw[at++];if(c==='\\'){at++;continue;}if(c==='"')return JSON.parse(raw.slice(start,at));}throw Error('EXACT_JSON_STRING');};
 const value=(path,depth)=>{need(depth<=64,'EXACT_JSON_DEPTH');space();const c=raw[at];
  if(c==='{'){at++;space();const out={},seen=new Set();if(raw[at]==='}'){at++;return out;}while(true){space();const key=string();need(!seen.has(key),'EXACT_JSON_DUPLICATE_KEY');seen.add(key);space();need(raw[at++]===':','EXACT_JSON_COLON');Object.defineProperty(out,key,{value:value([...path,key],depth+1),enumerable:true,writable:true,configurable:true});space();const end=raw[at++];if(end==='}')return out;need(end===',','EXACT_JSON_OBJECT');}}
  if(c==='['){at++;space();const out=[];if(raw[at]===']'){at++;return out;}while(true){need(out.length<200000,'EXACT_JSON_ARRAY_SIZE');out.push(value([...path,out.length],depth+1));space();const end=raw[at++];if(end===']')return out;need(end===',','EXACT_JSON_ARRAY');}}
  if(c==='"')return string();for(const [word,v]of [['true',true],['false',false],['null',null]])if(raw.startsWith(word,at)){at+=word.length;return v;}
  const m=/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/.exec(raw.slice(at));need(m,'EXACT_JSON_VALUE');at+=m[0].length;const n=Number(m[0]);need(Number.isFinite(n),'EXACT_JSON_NUMBER');numbers.set(JSON.stringify(path),m[0]);return n;
 };
 const data=value([],0);space();need(at===raw.length,'EXACT_JSON_TRAILING');return {data,numbers};
}
const decimal=(numbers,path)=>{
 const lexeme=numbers.get(JSON.stringify(path));if(lexeme===undefined)return {available:false,reason:'numeric_source_missing'};
 try{return {available:true,lexeme,decimal:formatMicros(decimalMicros(lexeme))};}
 catch{return {available:false,reason:'non_exact_nonnegative_micro_decimal'};}
};

// Adapter around the existing fetch, not a second client or admission system.
// Only the decoder changes. Native numeric data still goes to the existing core.
export function exactMoneyFetch(fetchFn,{tournamentId,tournamentSlug,onReceipt,now=Date.now}){
 need(typeof fetchFn==='function'&&typeof onReceipt==='function'&&typeof now==='function'
  &&typeof tournamentId==='string'&&/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(tournamentSlug),'EXACT_MONEY_DEPENDENCIES');
 return async(url,options={})=>{
  const response=await fetchFn(url,options),u=new URL(url),method=options.method||'GET';
  const cash=method==='GET'&&u.pathname===`/api/v1/tournaments/${tournamentSlug}`;
  const order=method==='POST'&&u.pathname==='/api/v1/orders';
  if(u.origin!=='https://sig.thesuper.market'||u.search||!response.ok||(!cash&&!order))return response;
  // Test/mock or unsupported bodies cannot manufacture decimal evidence.
  if(typeof response.text!=='function')return response;
  return new Proxy(response,{get(target,key){if(key!=='json'){const v=Reflect.get(target,key,target);return typeof v==='function'?v.bind(target):v;}
   return async()=>{const raw=await target.text(),{data,numbers}=parseNumberLexemes(raw),sourceBodyHash=sha(raw),receivedAt=new Date(now()).toISOString();let receipt;
    if(cash){need(data.id===tournamentId,'EXACT_CASH_SCOPE_MISMATCH');receipt={kind:'tournament_cash',tournamentId,joined:data.isPendingEnrolment===false,fields:{myBalance:decimal(numbers,['myBalance'])}};}
    else{
     let request;try{request=JSON.parse(options.body);}catch{throw Error('EXACT_ORDER_REQUEST');}
     need(request.tournamentId===tournamentId&&typeof request.exchangeId==='string','EXACT_ORDER_SCOPE_MISMATCH');
     // The canonical leg is one placement result, never a guessed aggregate.
     const matched=data&&data.exchangeId===request.exchangeId&&Number.isSafeInteger(data.orderId)&&data.orderId>0;
     // Preserve the returned body even when attribution fails: the execution
     // core must retain a returned ID and reconcile its uncertain submission.
     receipt={kind:'order_ack_principal',tournamentId,matchedIdentity:!!matched,exchangeId:matched?data.exchangeId:null,orderId:matched?String(data.orderId):null,canonicalSide:data?.side??null,canonicalAction:data?.action??null,requestHash:sha(options.body),fields:{totalCost:matched?decimal(numbers,['totalCost']):{available:false,reason:'ack_identity_unbound'}}};
    }
    await onReceipt({schemaVersion:1,...receipt,method,endpoint:u.pathname.slice('/api/v1'.length),receivedAt,sourceBodyHash,executionAuthority:false});return data;
   };
  }});
 };
}

export function createMoneyReceiptWriter(file,{canCommit=()=>true}={}){
 need(typeof file==='string'&&typeof canCommit==='function','EXACT_RECEIPT_WRITER');let sequence=0,previous=null,bytes=0;
 if(fs.existsSync(file)){const raw=fs.readFileSync(file,'utf8');need(!raw||raw.endsWith('\n'),'EXACT_RECEIPT_PARTIAL');for(const line of raw.trimEnd().split('\n').filter(Boolean)){const row=JSON.parse(line),{hash,...body}=row;need(body.sequence===sequence+1&&body.previous===previous&&payloadHash(body)===hash,'EXACT_RECEIPT_CHAIN');sequence++;previous=hash;}bytes=Buffer.byteLength(raw);}
 else{const f=fs.openSync(file,'wx',0o600);fs.closeSync(f);}
 const identity=fs.statSync(file);
 return receipt=>{need(canCommit(),'EXACT_RECEIPT_STOPPED');const current=fs.statSync(file);need(current.ino===identity.ino&&current.birthtimeMs===identity.birthtimeMs&&current.size===bytes,'EXACT_RECEIPT_CHANGED');const body={sequence:sequence+1,previous,receipt},hash=payloadHash(body),data=Buffer.from(JSON.stringify({...body,hash})+'\n'),fd=fs.openSync(file,'a',0o600);
  try{let offset=0;while(offset<data.length){const n=fs.writeSync(fd,data,offset,data.length-offset);need(n>0,'EXACT_RECEIPT_WRITE');offset+=n;}fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
  bytes+=data.length;sequence++;previous=hash;return {sequence,hash};
 };
}
