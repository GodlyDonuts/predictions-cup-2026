import {createHash,randomUUID} from 'node:crypto';
import {parseNumberLexemes} from './exact-money-receipts.mjs';
import {payloadHash} from './core.mjs';

const sha=raw=>createHash('sha256').update(raw).digest('hex');
const stamp=n=>new Date(n).toISOString();
const need=(ok,code)=>{if(!ok)throw Object.assign(Error(code),{code});};
// This is a decoder around the sole collector's existing fetch. It adds no
// client, admission, credential, persistence, timer or execution authority.
export function createScopedRestReceiptsV3({fetchFn,scope,now=Date.now,onFetchStart=()=>{},capacity=64,maximumBytes=16*1024*1024}){
 need(typeof fetchFn==='function'&&typeof now==='function'&&typeof onFetchStart==='function','REST_RECEIPT_DEPENDENCIES');
 need(typeof scope?.tournamentId==='string'&&/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(scope?.tournamentSlug),'REST_RECEIPT_SCOPE');
 need(Number.isSafeInteger(capacity)&&capacity>0&&capacity<=64&&Number.isSafeInteger(maximumBytes)&&maximumBytes>0&&maximumBytes<=64*1024*1024,'REST_RECEIPT_BOUNDS');
 const tournamentId=scope.tournamentId,tournamentSlug=scope.tournamentSlug;
 const marketIds=new Set(scope.marketIds||[]);
 need(Array.isArray(scope.marketIds||[])&&marketIds.size<1001&&[...marketIds].every(id=>typeof id==='string'&&/^[1-9]\d*$/.test(id)),'REST_RECEIPT_MARKET_SCOPE');
 let closed=false,records=[],bytes=0,started=0,captured=0;
 function endpointFor(url,method){
  const u=new URL(url);if(u.origin!=='https://sig.thesuper.market'||!u.pathname.startsWith('/api/v1/'))return null;
  const p=u.pathname.slice('/api/v1'.length),endpoint=p+u.search;
  if(method==='POST'&&p==='/orders'&&!u.search)return endpoint;
  if(method==='DELETE'&&/^\/orders\/[1-9]\d*$/.test(p)&&!u.search)return endpoint;
  if(method!=='GET')return null;
  // Explicit opt-in captures only the one owned-market research job. These
  // routes add no authorization; the shared dispatcher still needs a permit.
  if(scope.ownedAllMarketId==='384'&&marketIds.has('384')&&!u.hash){
   if(p==='/portfolio/collateral'&&u.searchParams.get('tournamentId')===tournamentId&&[...u.searchParams.keys()].join('|')==='tournamentId')return endpoint;
   if(p==='/relationships/graph'&&u.searchParams.get('marketId')==='384'&&u.searchParams.get('depth')==='4'&&u.searchParams.get('tournamentId')===tournamentId&&[...u.searchParams.keys()].sort().join('|')==='depth|marketId|tournamentId')return endpoint;
  }
  if((p==='/tournaments/'+tournamentSlug||p==='/tournaments/'+tournamentSlug+'/portfolio/positions')&&!u.search)return endpoint;
  if(p==='/orders'&&u.searchParams.get('tournamentId')===tournamentId)return endpoint;
  if(/^\/orders\/[1-9]\d*(?:\/fills)?$/.test(p))return endpoint;
  const nodes=/^\/markets\/([1-9]\d*)\/nodes$/.exec(p);
  if(nodes&&marketIds.has(nodes[1])&&!u.hash&&u.searchParams.get('tournamentId')===tournamentId&&[...u.searchParams.keys()].join('|')==='tournamentId')return endpoint;
  const book=/^\/markets\/([1-9]\d*)\/orderbook$/.exec(p);
  if(book&&marketIds.has(book[1])&&u.searchParams.get('tournamentId')===tournamentId&&u.searchParams.get('depth')==='20'&&[...u.searchParams.keys()].sort().join('|')==='depth|tournamentId')return endpoint;
  return null;
 }
 function save(record){
  need(!closed,'REST_RECEIPT_CLOSED');
  const size=Buffer.byteLength(record.bodyText);need(size<=maximumBytes,'REST_RECEIPT_BODY_BOUND');
  while(records.length>=capacity||bytes+size>maximumBytes){bytes-=records.shift().size;}
  records.push({...record,size});bytes+=size;captured++;
 }
 async function fetch(url,options={}){
  need(!closed,'REST_RECEIPT_CLOSED');
  const method=(options.method||'GET').toUpperCase(),endpoint=endpointFor(url,method);
  if(endpoint===null)return fetchFn(url,options);
  let requestBodyHash=null;
  if(method==='POST'){
   need(typeof options.body==='string','REST_RECEIPT_REQUEST_BODY');
   const body=parseNumberLexemes(options.body).data;need(body.tournamentId===tournamentId,'REST_RECEIPT_REQUEST_SCOPE');requestBodyHash=payloadHash(body);
  }
  const meta={fetchInvocationId:randomUUID(),method,endpoint,requestBodyHash,requestStartedAt:stamp(now())};
  // Call the actual supplied fetch BEFORE reporting an invocation. The durable
  // logical intent must already exist; this callback cannot authorize a write.
  let pending;
  try{pending=Promise.resolve(fetchFn(url,options)).then(response=>({response}),()=>({failed:true}));}
  catch{pending=Promise.resolve({failed:true});}
  started++;
  try{await onFetchStart(Object.freeze({...meta}));}
  catch{
   const outcome=await pending;try{await outcome.response?.body?.cancel();}catch{}
   throw Object.assign(Error('REST_RECEIPT_START_RECORDING_FAILED'),{code:'REST_RECEIPT_START_RECORDING_FAILED',fatal:true,durableRecordingFailed:true,mayHaveExecuted:method!=='GET',requestStartedAt:meta.requestStartedAt,fetchInvocationId:meta.fetchInvocationId});
  }
  const outcome=await pending;
  if(outcome.failed)throw Object.assign(Error('REST_RECEIPT_FETCH_UNAVAILABLE'),{code:'REST_RECEIPT_FETCH_UNAVAILABLE',mayHaveExecuted:method!=='GET',requestStartedAt:meta.requestStartedAt,fetchInvocationId:meta.fetchInvocationId});
  const response=outcome.response;if(response===null||typeof response!=='object')return response;
  let consumed=false;
  const consume=async()=>{
   need(!closed,'REST_RECEIPT_CLOSED');need(!consumed,'REST_RECEIPT_BODY_USED');consumed=true;
   // Count decoded-response bytes before building a complete text value. A
   // text-only mock cannot provide a hard bound and supplies no raw evidence.
   need(typeof response.body?.getReader==='function','REST_RECEIPT_STREAM_REQUIRED');
   const reader=response.body.getReader(),limit=Math.min(maximumBytes,8*1024*1024),decoder=new TextDecoder(),parts=[];let count=0;
   try{while(true){const {value,done}=await reader.read();if(done)break;need(!closed,'REST_RECEIPT_CLOSED');need(value instanceof Uint8Array,'REST_RECEIPT_BODY_CHUNK');count+=value.byteLength;if(count>limit){await reader.cancel();throw Object.assign(Error('REST_RECEIPT_BODY_BOUND'),{code:'REST_RECEIPT_BODY_BOUND'});}parts.push(decoder.decode(value,{stream:true}));}parts.push(decoder.decode());}
   catch(e){try{await reader.cancel();}catch{}throw e;}finally{reader.releaseLock();}
   const bodyText=parts.join('');
   const parsed=parseNumberLexemes(bodyText),record={...meta,status:response.status??200,receivedAt:stamp(now()),bodyText,sourceBodyHash:sha(bodyText),dataHash:payloadHash(parsed.data)};
   need(Date.parse(record.receivedAt)>=Date.parse(record.requestStartedAt),'REST_RECEIPT_CLOCK_ROLLBACK');
   save(record);return{bodyText,data:parsed.data};
  };
  return new Proxy(response,{get(target,key){
   if(key==='json')return async()=> (await consume()).data;
   if(key==='text')return async()=> (await consume()).bodyText;
   const value=Reflect.get(target,key,target);return typeof value==='function'?value.bind(target):value;
  }});
 }
 function receiptOf(response,{method='GET',requestBodyHash=null}={}){
  need(!closed&&typeof response?.endpoint==='string'&&response.data!==undefined,'REST_RECEIPT_RESPONSE');
  const begin=Date.parse(response.requestStartedAt),end=Date.parse(response.receivedAt);
  need(Number.isFinite(begin)&&Number.isFinite(end)&&begin<=end,'REST_RECEIPT_RESPONSE_CLOCK');
  const dataHash=payloadHash(response.data),matches=records.filter(r=>r.method===method&&r.endpoint===response.endpoint&&r.status===(response.status??200)&&r.requestBodyHash===requestBodyHash&&r.dataHash===dataHash&&Date.parse(r.requestStartedAt)>=begin&&Date.parse(r.receivedAt)<=end);
  need(matches.length===1,'REST_RECEIPT_RESPONSE_UNBOUND');
  const {size,dataHash:ignored,...record}=matches[0];return Object.freeze({...record});
 }
 return{fetch,receiptOf,stats:()=>({records:records.length,bytes,started,captured,closed,executionAuthority:false,additionalClients:0}),close(){records=[];bytes=0;closed=true;}};
}
