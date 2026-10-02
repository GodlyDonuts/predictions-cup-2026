import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {retryAfterMs} from '../feed/runtime.mjs';
import {allowedHistoryEndpoint} from './history.mjs';
import {networkInterval,networkWriteInterval} from './transport-network-budget.mjs';

export function exclusionSet(positions,orders){
  if(!Array.isArray(positions)||!Array.isArray(orders))throw Error('Incomplete account reconciliation');
  const ids=new Set();
  const validate=row=>{
    if(!row||typeof row!=='object'||Array.isArray(row)||typeof row.exchangeId!=='string'||!/^([1-9]\d*)$/.test(row.exchangeId)||!Number.isSafeInteger(Number(row.exchangeId))||!Number.isSafeInteger(row.quantity))throw Error('Unmappable account exposure');
  };
  // Tournament positions are signed: YES is positive, NO is negative.
  // Either side owns this exchange's depth; a valid zero position is flat.
  for(const row of positions){validate(row);if(row.quantity!==0)ids.add(row.exchangeId);}
  // Order quantity is unsigned and strictly positive, irrespective of side.
  for(const row of orders){validate(row);if(row.quantity<=0)throw Error('Unmappable account exposure');ids.add(row.exchangeId);}
  return ids;
}

export function atomicJson(file,value){const tmp=file+'.tmp';fs.writeFileSync(tmp,JSON.stringify(value,null,2),{mode:0o600});fs.renameSync(tmp,file);}
export class Journal {
  constructor(file){this.file=file;this.sequence=0;this.previous='';this.last=null;
    if(fs.existsSync(file)){
      const raw=fs.readFileSync(file,'utf8');if(raw&&!raw.endsWith('\n'))throw Error('Incomplete paper journal: manual recovery required');
      for(const line of raw.split('\n').filter(Boolean)){
        const r=JSON.parse(line),{hash,...body}=r;
        if(body.sequence!==this.sequence+1||body.previous!==this.previous||hash!==this.hash(body))throw Error('Journal integrity failure');
        this.sequence=body.sequence;this.previous=hash;this.last=body;
      }
    }
  }
  hash(body){return createHash('sha256').update(JSON.stringify(body)).digest('hex');}
  commit(state,event){const body={sequence:this.sequence+1,previous:this.previous,at:new Date().toISOString(),event,state};const hash=this.hash(body);
    const fd=fs.openSync(this.file,'a',0o600);try{const bytes=Buffer.from(JSON.stringify({...body,hash})+'\n');let offset=0;while(offset<bytes.length){const written=fs.writeSync(fd,bytes,offset,bytes.length-offset);if(written<=0)throw Error('Incomplete journal write');offset+=written;}fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
    this.sequence=body.sequence;this.previous=hash;this.last=body;
  }
}

export function lock(file){fs.mkdirSync(path.dirname(file),{recursive:true});const fd=fs.openSync(file,'wx',0o600);fs.writeSync(fd,String(process.pid));return()=>{fs.closeSync(fd);fs.unlinkSync(file);};}

export class ReadClient {
  #key;
  constructor(key,{interval=6000,networkBudget=null,fetchFn=fetch,signal,now=Date.now,sleepFn=ms=>new Promise(r=>setTimeout(r,ms)),onAdmission=()=>{},onTiming=()=>{},authorizeDispatch}={}){
    this.#key=key;this.interval=networkInterval(interval,networkBudget);this.writeInterval=networkWriteInterval(interval,networkBudget);this.completionPacing=networkBudget!==null;this.fetch=fetchFn;this.signal=signal;this.now=now;this.sleep=sleepFn;this.next=0;this.cooldown=0;this.requests=0;this.onAdmission=onAdmission;this.onTiming=onTiming;this.tail=Promise.resolve();this.authorizeDispatch=authorizeDispatch;
  }
  async pause(ms){const until=this.now()+ms;while(this.now()<until){if(this.signal?.aborted)throw Error('Stopped');await this.sleep(Math.min(200,until-this.now()));}}
  request(endpoint){const queuedAt=this.now();const result=this.tail.then(()=>this.perform(endpoint,queuedAt));this.tail=result.catch(()=>{});return result;}
  dispatch(spec){
    const safe={method:spec.method,endpoint:spec.endpoint,body:spec.body===undefined?undefined:structuredClone(spec.body),bodyJson:spec.bodyJson,beforeDispatch:spec.beforeDispatch,beforeNetwork:spec.beforeNetwork};
    if(!this.authorizeDispatch||this.authorizeDispatch(safe)!==true)throw Error('Execution dispatch is not authorized');
    const queuedAt=this.now(),result=this.tail.then(()=>this.perform(safe.endpoint,queuedAt,safe));this.tail=result.catch(()=>{});return result;
  }
  async perform(endpoint,queuedAt=this.now(),dispatch){
    const executingAt=this.now();
    const interval=dispatch&&dispatch.method!=='GET'?this.writeInterval:this.interval;
    const patterns=[/^\/tournaments\/midterm-elections$/, /^\/tournaments\/midterm-elections\/portfolio\/fills\?limit=100$/, /^\/tournaments\/midterm-elections\/portfolio\/pnl\?period=day$/, /^\/orders\?status=all&limit=200&tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55$/, /^\/tournaments\/midterm-elections\/portfolio\/positions$/, /^\/orders\?status=open&limit=200&tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55(?:&cursor=[A-Za-z0-9%._~-]+)?$/, /^\/markets\/\d+\/orderbook\?tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55&depth=20$/];
    const bulk=/^\/exchanges\/prices\?ids=([1-9]\d*(?:,[1-9]\d*)*)&tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55$/.exec(endpoint);
    const ids=bulk?.[1].split(',');
    const allowedBulk=ids&&ids.length<=100&&new Set(ids).size===ids.length&&ids.every(x=>Number.isSafeInteger(Number(x)));
    if(dispatch){if(!this.authorizeDispatch||this.authorizeDispatch(dispatch)!==true)throw Error('Execution dispatch is not authorized');}
    else if(!patterns.some(p=>p.test(endpoint))&&!allowedBulk&&!allowedHistoryEndpoint(endpoint))throw Error('Endpoint outside paper GET allowlist');
    const kind=endpoint.includes('/fills')?'fills':endpoint.includes('/portfolio/pnl?')?'pnl':endpoint.includes('/price-history?')?'history':endpoint.startsWith('/markets/')?(endpoint.includes('/orderbook?')?'book':'market'):endpoint.startsWith('/orders')?'orders':endpoint.startsWith('/exchanges/prices?')?'prices':endpoint.endsWith('/portfolio/positions')?'positions':'tournament';
    const event={schemaVersion:1,kind,marketId:kind==='book'?/^\/markets\/(\d+)\//.exec(endpoint)[1]:null,queuedAt:new Date(queuedAt).toISOString(),admittedAt:null,finishedAt:null,queueWaitMs:Math.max(0,executingAt-queuedAt),admissionWaitMs:null,headersMs:null,bodyMs:null,serviceMs:null,totalMs:null,statusCode:null,outcome:'blocked'};
    let admittedAt=null,headersAt=null;
    try{
      if(this.manualReview)throw Error('Rate cooldown requires manual review');
      await this.pause(Math.max(0,Math.max(this.next,this.cooldown)-this.now()));
      if(this.signal?.aborted){event.outcome='aborted';throw Error('Stopped');}
      if(dispatch){
        if(this.authorizeDispatch(dispatch)!==true)throw Error('Execution dispatch is not authorized');
        if(dispatch.method!=='GET'&&typeof dispatch.beforeDispatch!=='function')throw Error('Execution admission guard required');
        if(dispatch.beforeDispatch)await dispatch.beforeDispatch();
        if(this.signal?.aborted)throw Error('Stopped');
      }
      this.next=this.now()+interval;this.requests++;this.onAdmission(this.next);
      if(dispatch?.beforeNetwork)await dispatch.beforeNetwork();
      if(this.signal?.aborted)throw Error('Stopped');
      admittedAt=this.now();event.admittedAt=new Date(admittedAt).toISOString();event.admissionWaitMs=Math.max(0,admittedAt-executingAt);
      const requestStartedAt=event.admittedAt;let response;
      try{response=await this.fetch('https://sig.thesuper.market/api/v1'+endpoint,{method:dispatch?.method||'GET',redirect:'error',signal:this.signal?AbortSignal.any([this.signal,AbortSignal.timeout(20000)]):AbortSignal.timeout(20000),headers:{Authorization:'Bearer '+this.#key,...(dispatch?.body!==undefined?{'Content-Type':'application/json'}:{})},...(dispatch?.body!==undefined?{body:dispatch.bodyJson??JSON.stringify(dispatch.body)}:{})});}catch{event.outcome=this.signal?.aborted?'aborted':'connection_error';const err=Error(this.signal?.aborted?'Stopped':'Market API connection unavailable');err.recoverable=!this.signal?.aborted;throw err;}
      headersAt=this.now();event.headersMs=Math.max(0,headersAt-admittedAt);event.statusCode=response.status??200;
      if(!response.ok){event.outcome='http_error';if(response.status===429||response.status===503){const delay=Math.max(response.status===429?60000:10000,retryAfterMs(response.headers.get('Retry-After'),this.now()));if(delay===Number.MAX_SAFE_INTEGER||delay>8640000000000000-this.now()){this.manualReview=true;this.onAdmission(null);throw Error('Rate cooldown requires manual review');}this.cooldown=this.now()+delay;this.onAdmission(Math.max(this.next,this.cooldown));}let apiCode;try{const failure=await response.json();const code=failure?.code||failure?.error?.code;if(['REQUEST_IN_FLIGHT','ORDER_STATUS_UNKNOWN','IDEMPOTENCY_KEY_REUSED','IDEMPOTENCY_CONFLICT','IDEMPOTENCY_KEY_CONFLICT','ORDER_NOT_FOUND','INSUFFICIENT_BALANCE','INVALID_ORDER'].includes(code))apiCode=code;}catch{}const err=Error('Market API HTTP '+response.status);err.status=response.status;err.apiCode=apiCode;err.retryAfterMs=Math.max(0,this.cooldown-this.now(),apiCode==='REQUEST_IN_FLIGHT'?90000:0);err.recoverable=[429,500,502,503,504].includes(response.status);throw err;}
      let data;try{data=await response.json();}catch{event.outcome=this.signal?.aborted?'aborted':'body_error';throw Error('Invalid market API JSON');}finally{event.bodyMs=Math.max(0,this.now()-headersAt);}
      if(this.signal?.aborted){event.outcome='aborted';throw Error('Stopped');}
      event.outcome='success';return{endpoint,requestStartedAt,receivedAt:new Date(this.now()).toISOString(),data,...(dispatch?{status:response.status??200,retryAfterMs:0}:{})};
    }catch(e){if(this.signal?.aborted)event.outcome='aborted';throw e;}finally{
      const finished=this.now();event.finishedAt=new Date(finished).toISOString();event.totalMs=Math.max(0,finished-queuedAt);if(admittedAt!==null){event.serviceMs=Math.max(0,finished-admittedAt);if(headersAt===null)event.headersMs=event.serviceMs;}else event.admissionWaitMs=Math.max(0,finished-executingAt);
      // Faster coordinated mode reserves before fetch and again after completion.
      // This prevents a slow durable admission from creating a catch-up burst.
      if(this.completionPacing&&admittedAt!==null&&!this.manualReview){this.next=Math.max(this.next,finished+interval,this.cooldown);this.onAdmission(this.next);}
      try{this.onTiming(event);}catch{this.manualReview=true;this.onAdmission(null);throw Error('Read timing recording failed');}
    }
  }
  async orders(){const out=[];let cursor;const seen=new Set();do{const r=await this.request('/orders?status=open&limit=200&tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55'+(cursor?'&cursor='+encodeURIComponent(cursor):''));if(!Array.isArray(r.data.data))throw Error('Invalid open orders response');exclusionSet([],r.data.data);out.push(...r.data.data);const pg=r.data.pagination;if(!pg||typeof pg.hasMore!=='boolean')throw Error('Incomplete order pagination');cursor=pg.hasMore?pg.nextCursor:null;if(pg.hasMore&&(typeof cursor!=='string'||!cursor||seen.has(cursor)))throw Error('Incomplete order pagination');seen.add(cursor);}while(cursor);return out;}
  close(){this.#key='';}
}
