import {execFileSync} from 'node:child_process';
import {setTimeout as sleep} from 'node:timers/promises';

export function loadKey() {
  if (process.env.SIG_API_KEY) return process.env.SIG_API_KEY.trim();
  // Windows DPAPI binds the encrypted credential to this Windows account.
  const script = "$ErrorActionPreference='Stop'; $s=(Get-Content -Raw -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup\\api-key.dpapi')).Trim()|ConvertTo-SecureString; [Console]::Write(([pscredential]::new('api',$s)).GetNetworkCredential().Password)";
  return execFileSync('pwsh.exe',['-NoProfile','-NonInteractive','-Command',script],{encoding:'utf8',windowsHide:true}).trim();
}

export class ApiError extends Error {
  constructor(status, code, retryAfter) {super(`API ${status}: ${code}`); this.status=status;this.code=code;this.retryAfter=retryAfter;}
}
export function retryAfterMs(header, now, fallback=0) {
  const text=header?.trim();
  if (!text) return fallback;
  const seconds=/^\d+(\.\d+)?$/.test(text)?Number(text):NaN;
  const delay=Number.isFinite(seconds)?seconds*1000:(/GMT$/i.test(text)?Date.parse(text)-now:NaN);
  return Number.isFinite(delay)?Math.max(0,delay):fallback;
}

export class AdmissionQueue {
  constructor(interval, {now=Date.now, sleepFn=sleep}={}) {
    if(!Number.isFinite(interval)||interval<0)throw new Error('Invalid admission interval');
    this.interval=interval;this.now=now;this.sleep=sleepFn;this.next=0;this.pausedUntil=0;this.tail=Promise.resolve();
  }
  pause(ms) {
    if(!Number.isFinite(ms)||ms<0)throw new Error('Invalid cooldown');
    this.pausedUntil=Math.max(this.pausedUntil,this.now()+ms);
  }
  // Serialize admission only; network responses can still overlap.
  admit() {
    const result=this.tail.then(async()=>{
      while(true) {
        const delay=Math.max(this.next,this.pausedUntil)-this.now();
        if(delay<=0)break;
        await this.sleep(delay); // Recheck if another response extended the pause.
      }
      this.next=this.now()+this.interval;
    });
    this.tail=result.catch(()=>{});
    return result;
  }
}
export class Api {
  constructor(base,key,interval=1500,{now=Date.now,sleepFn=sleep,fetchFn=fetch}={}) {
    if (base !== 'https://sig.thesuper.market/api/v1') throw new Error('API destination must be the authorized SIG host');
    this.base=base;this.key=key;this.now=now;this.fetch=fetchFn;this.queue=new AdmissionQueue(interval,{now,sleepFn});
  }
  async request(path, method='GET', body) {
    if (method !== 'GET') throw new Error('Research client is read-only: order placement and cancellation are disabled');
    // Sequential request pacing leaves space in the account's shared 100-read budget.
    for (let attempt=0;attempt<4;attempt++) {
      await this.queue.admit();
      let response;
      try {
        response=await this.fetch(this.base+path,{method,redirect:'error',signal:AbortSignal.timeout(20000),
          headers:{Authorization:`Bearer ${this.key}`,'Content-Type':'application/json'},body:body ? JSON.stringify(body):undefined});
      } catch {
        if (attempt===3) throw new Error('API connection unavailable');
        this.queue.pause(1000*2**attempt);continue;
      }
      const data=await response.json();
      if (response.ok) return data;
      const code=data.error?.code ?? 'UNKNOWN_ERROR';
      const delay=retryAfterMs(response.headers.get('Retry-After'),this.now());
      if (response.status===429 || response.status===503) {
        const backoff=Math.max(delay,response.status===429?60000:1000*2**attempt);
        this.queue.pause(backoff);
        if(attempt===3) throw new ApiError(response.status,code,backoff);
        continue;
      }
      throw new ApiError(response.status,code,delay);
    }
  }
  async pages(path) {
    const out=[];let cursor;
    do {
      const page=await this.request(path+(cursor?'&cursor='+encodeURIComponent(cursor):''));
      out.push(...page.data);
      cursor=page.pagination?.hasMore?page.pagination.nextCursor:null;
      if(page.pagination?.hasMore&&!cursor)throw new Error('Incomplete API pagination');
    }while(cursor);
    return out;
  }
}
