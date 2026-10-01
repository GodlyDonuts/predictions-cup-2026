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
export class Api {
  constructor(base,key,interval=1500) {
    if (base !== 'https://sig.thesuper.market/api/v1') throw new Error('API destination must be the authorized SIG host');
    this.base=base;this.key=key;this.interval=interval;this.next=0;this.pausedUntil=0;
  }
  async request(path, method='GET', body) {
    if (method !== 'GET') throw new Error('Research client is read-only: order placement and cancellation are disabled');
    // Sequential request pacing leaves space in the account's shared 100-read budget.
    for (let attempt=0;attempt<4;attempt++) {
      const slot=Math.max(this.next,Date.now());this.next=slot+this.interval;
      await sleep(Math.max(0,slot-Date.now()));
      while(Date.now()<this.pausedUntil)await sleep(this.pausedUntil-Date.now());
      let response;
      try {
        response=await fetch(this.base+path,{method,redirect:'error',signal:AbortSignal.timeout(20000),
          headers:{Authorization:`Bearer ${this.key}`,'Content-Type':'application/json'},body:body ? JSON.stringify(body):undefined});
      } catch {
        if (attempt===3) throw new Error('API connection unavailable');
        await sleep(1000*2**attempt);continue;
      }
      const data=await response.json();
      if (response.ok) return data;
      const code=data.error?.code ?? 'UNKNOWN_ERROR';
      const delay=Number(response.headers.get('Retry-After'))*1000;
      if (response.status===429 || response.status===503) {
        if(attempt===3) throw new ApiError(response.status,code,delay);
        const backoff=Math.max(delay,response.status===429?60000:1000*2**attempt);
        this.pausedUntil=Math.max(this.pausedUntil,Date.now()+backoff);
        await sleep(backoff);continue;
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
