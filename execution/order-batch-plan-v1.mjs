import {createHash} from 'node:crypto';

const MAX_ORDERS=50;
const MAX_QUANTITY=2_147_483_647;
const KEY_PREFIX='baal-batch-v1-';

function fail(code){throw new TypeError(code);}
function isRecord(value){return value!==null&&typeof value==='object'&&!Array.isArray(value);}
function sha256(text){return createHash('sha256').update(text,'utf8').digest('hex');}
function deepFreeze(value){
 if(value&&typeof value==='object'&&!Object.isFrozen(value)){
  for(const child of Object.values(value))deepFreeze(child);
  Object.freeze(value);
 }
 return value;
}
function stableJson(value){
 if(Array.isArray(value))return`[${value.map(stableJson).join(',')}]`;
 if(isRecord(value))return`{${Object.keys(value).sort().map(key=>`${JSON.stringify(key)}:${stableJson(value[key])}`).join(',')}}`;
 return JSON.stringify(value);
}
function retryAfterSeconds(response){
 const headers=response?.headers;
 let value;
 if(headers&&typeof headers.get==='function')value=headers.get('retry-after')??headers.get('Retry-After');
 else if(isRecord(headers)){
  const entry=Object.entries(headers).find(([key])=>key.toLowerCase()==='retry-after');
  value=entry?.[1];
 }
 if(typeof value==='number'&&Number.isSafeInteger(value)&&value>=0)return value;
 if(typeof value==='string'&&/^\d+$/.test(value.trim())){
  const parsed=Number(value.trim());
  if(Number.isSafeInteger(parsed))return parsed;
 }
 return null;
}
function retryMetadata(response,minimumSeconds=null){
 const headerSeconds=retryAfterSeconds(response);
 return{
  retryAfterSeconds:headerSeconds===null?minimumSeconds:Math.max(headerSeconds,minimumSeconds??0),
  retryAfterSource:headerSeconds!==null?'response-header':minimumSeconds!==null?'documented-minimum':'backoff-policy-required',
  retryBackoffRequired:true,
 };
}
function isDateTime(value){
 const match=/^(\d{4})-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])T(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d+)?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/.exec(value);
 if(!match||!Number.isFinite(Date.parse(value)))return false;
 const year=Number(match[1]),month=Number(match[2]),day=Number(match[3]);
 const leap=year%4===0&&(year%100!==0||year%400===0);
 const days=[31,leap?29:28,31,30,31,30,31,31,30,31,30,31];
 return day<=days[month-1];
}

function validatedOrder(intent,tournamentId,nowMs){
 if(!isRecord(intent)||typeof intent.intentId!=='string'||!intent.intentId.trim())fail('INTENT_ID');
 if(intent.independentlyEligible!==true)fail('NOT_INDEPENDENTLY_ELIGIBLE');
 const order=intent.order;
 if(!isRecord(order))fail('ORDER');
 const allowed=new Set(['exchangeId','side','action','quantity','price','expirationDate','tournamentId']);
 if(Object.keys(order).some(key=>!allowed.has(key)))fail('ORDER_FIELD');
 if(typeof order.exchangeId!=='string'||!/^\d+$/.test(order.exchangeId))fail('EXCHANGE_ID');
 if(!['yes','no'].includes(order.side))fail('SIDE');
 if(!['buy','sell'].includes(order.action))fail('ACTION');
 if(!Number.isSafeInteger(order.quantity)||order.quantity<1||order.quantity>MAX_QUANTITY)fail('QUANTITY');
 if(order.price!==undefined){
  if(typeof order.price!=='number'||!Number.isFinite(order.price)||order.price<0||order.price>1)fail('PRICE');
  if(order.price===1&&order.action==='buy'||order.price===0&&order.action==='sell'){
   if(order.expirationDate!==undefined)fail('MARKET_ORDER_EXPIRATION');
  }else if(order.price<0.005||order.price>0.995||Math.abs(order.price*200-Math.round(order.price*200))>1e-8)fail('PRICE_TICK');
 }
 if(order.expirationDate!==undefined){
  // Omitting price also requests a market order; expiry is supported only for limits.
  if(order.price===undefined)fail('MARKET_ORDER_EXPIRATION');
  if(typeof order.expirationDate!=='string'||!isDateTime(order.expirationDate))fail('EXPIRATION_DATE');
  if(Date.parse(order.expirationDate)<=nowMs)fail('EXPIRATION_NOT_FUTURE');
 }
 if(order.tournamentId!==undefined&&order.tournamentId!==tournamentId)fail('TOURNAMENT_SCOPE');
 const resolved={exchangeId:order.exchangeId,side:order.side,action:order.action,quantity:order.quantity};
 if(order.price!==undefined)resolved.price=order.price;
 if(order.expirationDate!==undefined)resolved.expirationDate=order.expirationDate;
 // Make the originally selected tournament explicit so retries cannot drift
 // when an organization changes its default tournament.
 resolved.tournamentId=tournamentId;
 return{intentId:intent.intentId,order:resolved};
}

/**
 * Build an inert, deterministic POST /orders/batch request plan.
 * batchId must be unique for a logical operation and must be retained on retry.
 */
export function planOrderBatchV1({batchId,tournamentId,intents}={}){
 if(typeof batchId!=='string'||!batchId.trim())fail('BATCH_ID');
 if(typeof tournamentId!=='string'||!tournamentId.trim())fail('TOURNAMENT_ID');
 if(!Array.isArray(intents)||intents.length<1||intents.length>MAX_ORDERS)fail('ORDER_COUNT');
 const seen=new Set();
 const nowMs=Date.now();
 const planned=intents.map(intent=>{
  const row=validatedOrder(intent,tournamentId,nowMs);
  if(seen.has(row.intentId))fail('DUPLICATE_INTENT_ID');
  seen.add(row.intentId);
  return row;
 });
 const orders=planned.map(row=>row.order);
 const logicalIdentity=stableJson({batchId,tournamentId,intentIds:planned.map(x=>x.intentId),orders});
 const idempotencyKey=KEY_PREFIX+sha256(logicalIdentity);
 const body={idempotencyKey,orders};
 const bodyJson=JSON.stringify(body);
 return deepFreeze({schemaVersion:1,batchId,tournamentId,intentIds:planned.map(x=>x.intentId),idempotencyKey,body,bodyJson,payloadHash:sha256(bodyJson),orderCount:orders.length});
}

/** Return the exact frozen request to reuse for an idempotent retry. */
export function retryOrderBatchV1(plan){
 assertOrderBatchPlanV1(plan);
 return deepFreeze(JSON.parse(plan.bodyJson));
}

export function assertOrderBatchPlanV1(plan){
 if(!isRecord(plan)||plan.schemaVersion!==1||typeof plan.batchId!=='string'||!plan.batchId.trim()||typeof plan.tournamentId!=='string'||!plan.tournamentId.trim()||typeof plan.bodyJson!=='string'||typeof plan.payloadHash!=='string'||typeof plan.idempotencyKey!=='string'||!Array.isArray(plan.intentIds))fail('PLAN');
 if(sha256(plan.bodyJson)!==plan.payloadHash)fail('PLAN_HASH');
 let body;try{body=JSON.parse(plan.bodyJson);}catch{fail('PLAN_JSON');}
 if(!isRecord(body)||body.idempotencyKey!==plan.idempotencyKey||!Array.isArray(body.orders)||body.orders.length!==plan.orderCount||body.orders.length<1||body.orders.length>MAX_ORDERS)fail('PLAN_BODY');
 if(!isRecord(plan.body)||JSON.stringify(plan.body)!==plan.bodyJson)fail('PLAN_BODY_VIEW');
 if(Object.keys(body).sort().join(',')!=='idempotencyKey,orders')fail('PLAN_BODY_FIELDS');
 if(JSON.stringify(body)!==plan.bodyJson)fail('PLAN_NOT_CANONICAL');
 if(plan.intentIds.length!==body.orders.length||plan.intentIds.some(id=>typeof id!=='string'||!id.trim())||new Set(plan.intentIds).size!==plan.intentIds.length)fail('PLAN_INTENTS');
 if(body.orders.some(order=>!isRecord(order)||order.tournamentId!==plan.tournamentId))fail('PLAN_TOURNAMENT_SCOPE');
 const identity=stableJson({batchId:plan.batchId,tournamentId:plan.tournamentId,intentIds:plan.intentIds,orders:body.orders});
 if(plan.idempotencyKey!==KEY_PREFIX+sha256(identity))fail('PLAN_IDENTITY');
 return true;
}

function invalid(reason){return{schemaVersion:1,classification:'malformed_response',retryMode:'blocked',retryIndexes:[],missingIndexes:[],results:[],reason};}

/**
 * Classify one HTTP result without treating best-effort batches as atomic.
 * `response` is {status, headers?, body}; this function performs no I/O and schedules no retry.
 * Retryable outcomes expose Retry-After when present; null means the caller must apply its
 * coordinated backoff policy before retrying. REQUEST_IN_FLIGHT carries the documented 90s minimum.
 */
export function classifyOrderBatchResultV1(plan,response){
 try{assertOrderBatchPlanV1(plan);}catch(error){return invalid(error.message);}
 if(!isRecord(response)||!Number.isInteger(response.status)||response.status<100||response.status>599||!isRecord(response.body))return invalid('RESPONSE_SHAPE');
 const outer=response.status,body=response.body,n=plan.orderCount;
 if(!Array.isArray(body.results)){
  const code=body.error?.code;
  if(outer===409&&code==='REQUEST_IN_FLIGHT'){
   return{schemaVersion:1,classification:'request_retryable',retryMode:'same_batch_same_key',retryIndexes:Array.from({length:n},(_,i)=>i),missingIndexes:Array.from({length:n},(_,i)=>i),results:[],payloadHash:plan.payloadHash,idempotencyKey:plan.idempotencyKey,outerStatus:outer,code,...retryMetadata(response,90)};
  }
  if((outer===429&&code==='RATE_LIMITED')||(outer===502&&code==='ORDER_STATUS_UNKNOWN')||(outer===503&&['TX_CONFLICT','SERVICE_UNAVAILABLE'].includes(code))){
   return{schemaVersion:1,classification:'request_retryable',retryMode:'same_batch_same_key',retryIndexes:Array.from({length:n},(_,i)=>i),missingIndexes:Array.from({length:n},(_,i)=>i),results:[],payloadHash:plan.payloadHash,idempotencyKey:plan.idempotencyKey,outerStatus:outer,code,...retryMetadata(response,outer===429?60:null)};
  }
  if([409,429,502,503].includes(outer))return invalid('UNRECOGNIZED_RETRYABLE_ENVELOPE');
  if(outer>=200&&outer<300)return invalid('MISSING_RESULTS');
  return{schemaVersion:1,classification:'request_terminal_failure',retryMode:'none',retryIndexes:[],missingIndexes:Array.from({length:n},(_,i)=>i),results:[],payloadHash:plan.payloadHash,idempotencyKey:plan.idempotencyKey,outerStatus:outer,code:typeof code==='string'?code:null};
 }
 const seen=new Set(),items=[];
 for(const item of body.results){
  if(!isRecord(item)||!Number.isInteger(item.index)||item.index<0||item.index>=n)return invalid('RESULT_INDEX');
  if(seen.has(item.index))return invalid('DUPLICATE_RESULT_INDEX');
  seen.add(item.index);
  if(typeof item.ok!=='boolean'||!Number.isInteger(item.status)||!(item.status>=200&&item.status<300||item.status>=400&&item.status<=599)||!isRecord(item.data))return invalid('RESULT_SHAPE');
  if(item.ok!== (item.status>=200&&item.status<300))return invalid('RESULT_STATUS_MISMATCH');
  if(item.status===429&&item.data.code!=='RATE_LIMITED')return invalid('RESULT_RATE_LIMIT_CODE');
  if(item.status===502&&item.data.code!=='ORDER_STATUS_UNKNOWN')return invalid('RESULT_UNKNOWN_STATUS_CODE');
  const outcome=item.ok?'success':item.status===429||item.status>=500?'retryable_failure':'terminal_failure';
  items.push({index:item.index,intentId:plan.intentIds[item.index],outcome,status:item.status,data:item.data});
 }
 items.sort((a,b)=>a.index-b.index);
 const missingIndexes=Array.from({length:n},(_,i)=>i).filter(i=>!seen.has(i));
 const retryIndexes=items.filter(x=>x.outcome==='retryable_failure').map(x=>x.index);
 const hasSuccess=items.some(x=>x.outcome==='success'),hasFailure=items.some(x=>x.outcome!=='success');
 if(outer===200&&(!hasSuccess||hasFailure||missingIndexes.length))return invalid('HTTP_200_CONTRACT');
 if(outer===207&&(!hasSuccess||!hasFailure))return invalid('HTTP_207_CONTRACT');
 if(outer===422&&(hasSuccess||retryIndexes.length||missingIndexes.length))return invalid('HTTP_422_CONTRACT');
 if(outer===429&&!(items.length===n&&items.every(x=>x.status===429)))return invalid('HTTP_429_CONTRACT');
 if(outer===502&&!(items.length===n&&items.every(x=>x.status===502)))return invalid('HTTP_502_CONTRACT');
 if(outer===503&&hasSuccess&&missingIndexes.length===0)return invalid('HTTP_503_COMPLETED_RESULTS');
 if(outer===503&&!hasSuccess&&missingIndexes.length===0&&retryIndexes.length===0)return invalid('HTTP_503_TERMINAL_RESULTS');
 if(outer===503&&missingIndexes.length===0&&items.length===n&&items.every(x=>x.status===429))return invalid('HTTP_503_ALL_RATE_LIMITED');
 if(outer===503&&missingIndexes.length===0&&items.length===n&&items.every(x=>x.status===502))return invalid('HTTP_503_ALL_UNKNOWN');
 const unresolved=[...new Set([...retryIndexes,...(outer===503?missingIndexes:[])])].sort((a,b)=>a-b);
 if(missingIndexes.length&&outer!==503)return invalid('MISSING_ITEM_RESULTS');
 if(retryIndexes.length&&![207,429,502,503].includes(outer))return invalid('TRANSIENT_ITEM_OUTER_STATUS');
 if(outer===207&&retryIndexes.length===0&&missingIndexes.length===0&&items.every(x=>x.outcome==='success'))return invalid('HTTP_207_CONTRACT');
 const retryMode=unresolved.length||outer===429&&retryIndexes.length? 'same_batch_same_key':'none';
 const classification=retryMode!=='none'?'incomplete_retryable':missingIndexes.length?'incomplete_unknown':hasSuccess&&hasFailure?'completed_partial':hasSuccess?'completed_success':'completed_terminal_failure';
 return{schemaVersion:1,classification,retryMode,retryIndexes:unresolved,missingIndexes,results:items,payloadHash:plan.payloadHash,idempotencyKey:plan.idempotencyKey,outerStatus:outer,...(retryMode!=='none'?retryMetadata(response):{})};
}

