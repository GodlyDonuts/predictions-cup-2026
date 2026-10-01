import test from 'node:test';
import assert from 'node:assert/strict';
import {BookTransport} from './transport.mjs';
import {capturePilot} from './pilot.mjs';
import {config,snapshotPath} from '../feed/contract.mjs';
const scope=config({tournamentId:'00000000-0000-0000-0000-000000000001',marketId:1,exchangeIds:[2]});
function fixture(){
 let now=Date.parse('2026-10-01T22:00:00Z');const calls=[],admissions=[],cooldowns=[],channels=[];let createCount=0;
 const budget={sharedAuthority:true,admit:async x=>admissions.push(x),cooldown:async x=>cooldowns.push(x)};
 const createClient=(url,key,options)=>{createCount++;assert.equal(options.auth.persistSession,false);return {
  realtime:{setAuth:async()=>{},disconnect:()=>{}},removeChannel:async c=>c.status?.('CLOSED'),
  channel:(topic,cfg)=>{assert.equal(cfg.config.private,true);const c={handlers:{},on(_,filter,fn){this.handlers[filter.event]=fn;return this;},subscribe(fn){this.status=fn;queueMicrotask(()=>fn('SUBSCRIBED'));return this;}};channels.push(c);return c;}
 };};
 const snapshot=()=>({contexts:[{type:'tournament',tournament:{id:scope.tournamentId},orderbook:{exchanges:[{exchangeId:2,asOf:{sequence:1,at:new Date(now).toISOString()},bids:[{price:.4,quantity:10}],asks:[{price:.6,quantity:10}]}]}}]});
 const fetchFn=async(url,opts)=>{calls.push({url,opts});return {ok:true,headers:new Headers(),json:async()=>url.endsWith('/realtime/token')?{token:'secret-jwt',anonKey:'public-key',supabaseUrl:'https://project.supabase.co',expiresAt:new Date(now+10800000).toISOString()}:snapshot()};};
 const options={scope,key:'private-test-key',createClient,budget,tokenSetupApproved:true,fetchFn,now:()=>now,timeoutMs:100};
 return {options,calls,admissions,cooldowns,channels,snapshot,setNow:v=>{now=v;},getNow:()=>now,created:()=>createCount};
}
test('requires reviewed token setup and real shared budget interface before any traffic',()=>{
 const f=fixture();assert.throws(()=>new BookTransport({...f.options,tokenSetupApproved:false}),/TOKEN_SETUP/);
 assert.throws(()=>new BookTransport({...f.options,budget:{}}),/SHARED_ACCOUNT/);assert.equal(f.calls.length,0);
});
test('subscribes only scoped private market topic and admits only token/snapshot routes',async()=>{
 const f=fixture();const t=new BookTransport(f.options);const events=[];
 await assert.rejects(t.subscribe({topic:'user:any',onEvent:()=>{}}),/TOPIC_NOT_ALLOWED/);
 await t.subscribe({topic:scope.topic,onEvent:(...e)=>events.push(e)});
 f.channels[0].handlers.market_batch({payload:{ok:true}});assert.equal(events.length,1);
 await t.readSnapshot({method:'GET',path:snapshotPath(scope)});
 for(const [method,path] of [['POST','/orders'],['DELETE','/orders/1'],['GET','/account']])await assert.rejects(t.request(method,path),/ENDPOINT_NOT_ALLOWED/);
 assert.deepEqual(f.admissions.map(x=>x.method),['POST','GET']);assert.equal(f.calls[0].opts.redirect,'error');
 assert.equal(f.calls[0].url,'https://sig.thesuper.market/api/v1/realtime/token');
 assert.ok(!JSON.stringify(t).includes('private-test-key'));await t.close();
});
test('token expiry renews authentication and ignores old-channel callbacks',async()=>{
 const f=fixture();const t=new BookTransport(f.options);let n=0,d=0;t.onDisconnect=()=>d++;
 await t.subscribe({topic:scope.topic,onEvent:()=>n++});const old=f.channels[0];
 f.setNow(f.getNow()+10800000-30000);assert.equal(t.refreshDue(),true);
 await t.subscribe({topic:scope.topic,onEvent:()=>n++});
 old.handlers.market_batch({payload:{}});old.status('CHANNEL_ERROR');
 assert.equal(n,0);assert.equal(d,0);assert.equal(t.counts.tokenWrites,2);await t.close();
});
test('malformed bootstrap destination and expired token fail closed without subscription',async()=>{
 for(const patch of [{supabaseUrl:'http://evil.example'},{expiresAt:'2000-01-01T00:00:00Z'}]){
  const f=fixture();f.options.fetchFn=async()=>({ok:true,json:async()=>({token:'x',anonKey:'a',supabaseUrl:'https://project.supabase.co',expiresAt:'2026-10-02T01:00:00Z',...patch})});
  const t=new BookTransport(f.options);await assert.rejects(t.subscribe({topic:scope.topic,onEvent:()=>{}}));assert.equal(f.channels.length,0);await t.close();
 }
});
test('429 shares cooldown without leaking headers/token or retrying placement',async()=>{
 const f=fixture();f.options.fetchFn=async()=>({ok:false,status:429,headers:new Headers({'Retry-After':'120'})});
 const t=new BookTransport(f.options);await assert.rejects(t.bootstrap(),/RECOVERY_UNAVAILABLE/);
 assert.equal(f.cooldowns[0].retryAfterMs,120000);assert.equal(f.admissions.length,1);await t.close();
});
test('STOP before pilot touches neither transport nor capture',async()=>{
 let touched=0;await assert.rejects(capturePilot({scope,transport:{subscribe:()=>touched++},capture:{append:()=>touched++},shouldStop:()=>true}),/STOPPED/);assert.equal(touched,0);
});
test('bounded pilot recovers book, emits no fictional fills and closes',async()=>{
 const f=fixture();const t=new BookTransport(f.options);const rows=[];
 const report=await capturePilot({scope,transport:t,capture:{append:r=>rows.push(r)},durationMs:1000,now:f.options.now,sleep:async ms=>f.setNow(f.getNow()+ms)});
 assert.equal(report.outcome,'duration_complete');assert.equal(report.collector.coverage.versionedExchanges,1);
 assert.equal(report.orderWrites,0);assert.equal(report.tradeTapeComplete,false);assert.ok(rows.some(r=>r.kind==='snapshot'));
 assert.ok(!JSON.stringify(rows).includes('secret-jwt'));assert.ok(!JSON.stringify(rows).includes('private-test-key'));
 assert.equal(rows.at(-1).kind,'summary');assert.equal(t.closed,true);
});

test('pre-aborted subscription sends no authenticated request',async()=>{
 const f=fixture();const t=new BookTransport(f.options);const ac=new AbortController();ac.abort();
 await assert.rejects(t.subscribe({topic:scope.topic,signal:ac.signal,onEvent:()=>{}}),/STOPPED/);
 assert.equal(f.calls.length,0);await t.close();
});
test('bootstrap does not follow redirects or expose secret server errors',async()=>{
 const f=fixture();f.options.fetchFn=async()=>({ok:false,status:302,headers:new Headers(),json:async()=>({token:'secret-server-value'})});
 const t=new BookTransport(f.options);await assert.rejects(t.bootstrap(),e=>e.message==='RECOVERY_UNAVAILABLE');assert.equal(f.created(),0);await t.close();
});
test('lost acknowledgments are bounded and old channels are removed',async()=>{
 const f=fixture();let removed=0;f.options.timeoutMs=10;
 f.options.createClient=()=>({realtime:{setAuth:async()=>{},disconnect:()=>{}},channel:()=>({on(){return this;},subscribe(){}}),removeChannel:async()=>removed++});
 const t=new BookTransport(f.options);await assert.rejects(t.subscribe({topic:scope.topic,onEvent:()=>{}}),/SUBSCRIPTION_FAILED/);assert.equal(removed,1);await t.close();
});
test('pilot stops on hard deadline even when injected subscription never resolves',async()=>{
 let closed=0;const transport={counts:{},subscribe:()=>new Promise(()=>{}),close:async()=>closed++};
 const report=await capturePilot({scope,transport,durationMs:20});assert.equal(report.outcome,'stopped');assert.equal(closed,1);
});
test('invalid snapshot retries stop at recovery bound and never fabricate readiness',async()=>{
 const f=fixture();const t=new BookTransport({...f.options,fetchFn:async(url)=>({ok:true,json:async()=>url.endsWith('/realtime/token')?{token:'t',anonKey:'a',supabaseUrl:'https://project.supabase.co',expiresAt:new Date(f.getNow()+10800000).toISOString()}:{contexts:[]}})});
 const report=await capturePilot({scope,transport:t,durationMs:10000,maxRecoveries:2,now:f.options.now,sleep:async ms=>f.setNow(f.getNow()+ms)});
 assert.equal(report.outcome,'blocked');assert.equal(report.collector.coverage.booksReady,false);assert.equal(report.restCounts.reads,2);
});

test('socket failure reconnects once and repeats authoritative snapshot',async()=>{
 const f=fixture();const original=f.options.fetchFn;let gets=0;
 f.options.fetchFn=async(url,opts)=>{const response=await original(url,opts);if(!url.endsWith('/realtime/token')&&++gets===1)queueMicrotask(()=>f.channels[0].status('CHANNEL_ERROR'));return response;};
 const t=new BookTransport(f.options);
 const report=await capturePilot({scope,transport:t,durationMs:3000,now:f.options.now,sleep:async ms=>f.setNow(f.getNow()+ms)});
 assert.equal(report.connections,2);assert.equal(report.restCounts.reads,2);assert.equal(report.collector.metrics.reconnects,1);
});
test('events before acknowledgment retain original callback receive time',async()=>{
 const f=fixture();const rows=[];const originalTime=f.getNow();
 const batch={delivery:{model:'best-effort-authoritative-resync',correlationId:'synthetic',revision:1,previousRevision:0,sourceSequenceFrom:null,sourceSequenceThrough:null},books:[],trades:[],bookDirty:[],marketSettled:[]};
 const transport={counts:{},refreshDue:()=>false,subscribe:async({onEvent})=>{onEvent('market_batch',batch);f.setNow(f.getNow()+100);},readSnapshot:async()=>f.snapshot(),close:async()=>{}};
 await capturePilot({scope,transport,capture:{append:r=>rows.push(r)},durationMs:1000,now:f.options.now,sleep:async ms=>f.setNow(f.getNow()+ms)});
 const row=rows.find(r=>r.kind==='event');assert.equal(row.receivedAt,new Date(originalTime).toISOString());
 assert.ok(Date.parse(row.observedAt)>Date.parse(row.receivedAt));
});
