import {checkStop, retryAfterMs, RecoveryError} from '../feed/runtime.mjs';
import {snapshotPath, requireThat, config} from '../feed/contract.mjs';

const BASE='https://sig.thesuper.market/api/v1';
const EVENTS=['market_batch','book_dirty','market_settled'];
// One adapter per configured market. No account subscriptions or order methods.
export class BookTransport {
  #key; #session; #client; #channel;
  constructor({scope,key,createClient,budget,tokenSetupApproved=false,fetchFn=fetch,now=Date.now,timeoutMs=20000,onDisconnect=()=>{}}) {
    requireThat(tokenSetupApproved === true,'TOKEN_SETUP_REVIEW_REQUIRED');
    requireThat(typeof key==='string' && key.length>0 && typeof createClient==='function','INVALID_RUNTIME');
    requireThat(budget?.sharedAuthority===true && typeof budget.admit==='function' && typeof budget.cooldown==='function','SHARED_ACCOUNT_BUDGET_REQUIRED');
    requireThat(Number.isFinite(timeoutMs)&&timeoutMs>0&&timeoutMs<=30000,'INVALID_TIMEOUT');
    this.scope=config(scope);this.#key=key;this.createClient=createClient;this.budget=budget;
    this.fetch=fetchFn;this.now=now;this.timeoutMs=timeoutMs;this.onDisconnect=onDisconnect;
    this.mode='read_only_transport';this.counts={reads:0,tokenWrites:0};this.closed=false;
  }
  async request(method,path,{signal}={}) {
    checkStop(signal);requireThat(!this.closed,'TRANSPORT_CLOSED');
    requireThat((method==='POST'&&path==='/realtime/token')||(method==='GET'&&path===snapshotPath(this.scope)),'ENDPOINT_NOT_ALLOWED');
    await this.budget.admit({method,path,cost:1,signal});checkStop(signal);
    const bounded=AbortSignal.any([...(signal?[signal]:[]),AbortSignal.timeout(this.timeoutMs)]);
    if(method==='POST')this.counts.tokenWrites++;else this.counts.reads++;
    let response;
    try {response=await this.fetch(BASE+path,{method,redirect:'error',signal:bounded,headers:{Authorization:`Bearer ${this.#key}`,'Content-Type':'application/json'}});}
    catch {checkStop(bounded);throw new RecoveryError();}
    if(!response.ok) {
      const delay=Math.max(response.status===429?60000:0,retryAfterMs(response.headers.get('Retry-After'),this.now()));
      await this.budget.cooldown({retryAfterMs:delay,requiresManualReview:delay===Number.MAX_SAFE_INTEGER,signal});
      throw new RecoveryError({status:response.status});
    }
    try {return await response.json();}catch {throw new RecoveryError();}
  }
  async bootstrap({signal}={}) {
    const value=await this.request('POST','/realtime/token',{signal});checkStop(signal);
    // Destination is provided by the official credential bootstrap only. Never
    // send the SIG API key to it; the SDK receives only the scoped minted JWT.
    let url;try {url=new URL(value.supabaseUrl);}catch {throw new Error('INVALID_BOOTSTRAP');}
    requireThat(url.protocol==='https:'&&!url.username&&!url.password&&!url.search&&!url.hash,'INVALID_BOOTSTRAP');
    requireThat(typeof value.token==='string'&&value.token.length>0&&typeof value.anonKey==='string'&&value.anonKey.length>0,'INVALID_BOOTSTRAP');
    const expires=Date.parse(value.expiresAt);requireThat(Number.isFinite(expires)&&expires>this.now()+60000,'INVALID_TOKEN_EXPIRY');
    if(this.#session)requireThat(this.#session.origin===url.origin,'BOOTSTRAP_ORIGIN_CHANGED');
    this.#client?.realtime.disconnect();
    this.#session={origin:url.origin,expires};
    this.#client=this.createClient(url.origin,value.anonKey,{auth:{persistSession:false,autoRefreshToken:false,detectSessionInUrl:false}});
    await this.#client.realtime.setAuth(value.token);checkStop(signal);
  }
  refreshDue(){return !this.#session||this.#session.expires-this.now()<=60000;}
  async subscribe({topic,signal,onEvent}) {
    requireThat(topic===this.scope.topic,'TOPIC_NOT_ALLOWED');checkStop(signal);
    await this.disconnect();if(this.refreshDue()||!this.#client)await this.bootstrap({signal});
    checkStop(signal);let acknowledged=false;
    const channel=this.#client.channel(topic,{config:{private:true}});this.#channel=channel;
    for(const event of EVENTS)channel.on('broadcast',{event},({payload})=>{if(this.#channel===channel&&!this.closed&&!signal?.aborted)onEvent(event,payload);});
    await new Promise((resolve,reject)=>{
      let settled=false;
      const finish=error=>{if(settled)return;settled=true;clearTimeout(timer);signal?.removeEventListener('abort',aborted);error?reject(error):resolve();};
      const aborted=()=>finish(new Error('SUBSCRIPTION_ABORTED'));
      const timer=setTimeout(()=>finish(new Error('SUBSCRIPTION_TIMEOUT')),this.timeoutMs);
      signal?.addEventListener('abort',aborted,{once:true});
      if(signal?.aborted)return aborted();
      channel.subscribe(status=>{
        if(this.#channel!==channel)return;
        if(status==='SUBSCRIBED'){acknowledged=true;finish();}
        else if(['CHANNEL_ERROR','TIMED_OUT','CLOSED'].includes(status)){
          if(acknowledged)this.onDisconnect();else finish(new Error('SUBSCRIPTION_FAILED'));
        }
      });
    }).catch(async()=>{await this.disconnect();throw new Error('SUBSCRIPTION_FAILED');});
  }
  async readSnapshot({method,path,signal}) {
    requireThat(method==='GET'&&path===snapshotPath(this.scope),'ENDPOINT_NOT_ALLOWED');
    return this.request(method,path,{signal});
  }
  async disconnect() {
    const channel=this.#channel;this.#channel=null;
    let timer;
    try {if(channel&&this.#client)await Promise.race([this.#client.removeChannel(channel),new Promise(r=>{timer=setTimeout(r,1000);})]);}
    catch { /* Cleanup never exposes raw SDK diagnostics. */ }
    finally {clearTimeout(timer);}
  }
  async close(){this.closed=true;await this.disconnect();this.#client?.realtime.disconnect();this.#client=null;this.#key=null;}
}
