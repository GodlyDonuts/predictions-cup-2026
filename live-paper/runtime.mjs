import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {retryAfterMs} from '../bot/api.mjs';

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
    const fd=fs.openSync(this.file,'a',0o600);try{fs.writeSync(fd,JSON.stringify({...body,hash})+'\n');fs.fsyncSync(fd);}finally{fs.closeSync(fd);}
    this.sequence=body.sequence;this.previous=hash;this.last=body;
  }
}

export function lock(file){fs.mkdirSync(path.dirname(file),{recursive:true});const fd=fs.openSync(file,'wx',0o600);fs.writeSync(fd,String(process.pid));return()=>{fs.closeSync(fd);fs.unlinkSync(file);};}

export class ReadClient {
  #key;
  constructor(key,{interval=6000,fetchFn=fetch,signal,now=Date.now,sleepFn=ms=>new Promise(r=>setTimeout(r,ms)),onAdmission=()=>{}}={}){
    if(interval<5000)throw Error('Read budget requires >=5 second pacing');this.#key=key;this.interval=interval;this.fetch=fetchFn;this.signal=signal;this.now=now;this.sleep=sleepFn;this.next=0;this.cooldown=0;this.requests=0;this.onAdmission=onAdmission;this.tail=Promise.resolve();
  }
  async pause(ms){const until=this.now()+ms;while(this.now()<until){if(this.signal?.aborted)throw Error('Stopped');await this.sleep(Math.min(200,until-this.now()));}}
  request(endpoint){const result=this.tail.then(()=>this.perform(endpoint));this.tail=result.catch(()=>{});return result;}
  async perform(endpoint){
    const patterns=[/^\/tournaments\/midterm-elections$/, /^\/tournaments\/midterm-elections\/portfolio\/positions$/, /^\/orders\?status=open&limit=200&tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55(?:&cursor=[A-Za-z0-9%._~-]+)?$/, /^\/markets\/\d+\/orderbook\?tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55&depth=20$/];
    if(!patterns.some(p=>p.test(endpoint)))throw Error('Endpoint outside paper GET allowlist');
    await this.pause(Math.max(0,Math.max(this.next,this.cooldown)-this.now()));
    if(this.signal?.aborted)throw Error('Stopped');
    this.next=this.now()+this.interval;this.requests++;this.onAdmission(this.next);
    const requestStartedAt=new Date(this.now()).toISOString();
    let response;
    try{response=await this.fetch('https://sig.thesuper.market/api/v1'+endpoint,{method:'GET',redirect:'error',signal:this.signal?AbortSignal.any([this.signal,AbortSignal.timeout(20000)]):AbortSignal.timeout(20000),headers:{Authorization:'Bearer '+this.#key}});}catch{const err=Error(this.signal?.aborted?'Stopped':'Market API connection unavailable');err.recoverable=!this.signal?.aborted;throw err;}
    if(!response.ok){if(response.status===429||response.status===503){this.cooldown=this.now()+Math.max(response.status===429?60000:10000,retryAfterMs(response.headers.get('Retry-After'),this.now()));this.onAdmission(Math.max(this.next,this.cooldown));}const err=Error('Market API HTTP '+response.status);err.status=response.status;err.recoverable=[429,500,502,503,504].includes(response.status);throw err;}
    let data;try{data=await response.json();}catch{throw Error('Invalid market API JSON');}
    if(this.signal?.aborted)throw Error('Stopped');
    return{endpoint,requestStartedAt,receivedAt:new Date(this.now()).toISOString(),data};
  }
  async orders(){const out=[];let cursor;const seen=new Set();do{const r=await this.request('/orders?status=open&limit=200&tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55'+(cursor?'&cursor='+encodeURIComponent(cursor):''));if(!Array.isArray(r.data.data))throw Error('Invalid open orders response');out.push(...r.data.data);const pg=r.data.pagination;cursor=pg?.hasMore?pg.nextCursor:null;if(pg?.hasMore&&(!cursor||seen.has(cursor)))throw Error('Incomplete order pagination');seen.add(cursor);}while(cursor);return out;}
  close(){this.#key='';}
}
