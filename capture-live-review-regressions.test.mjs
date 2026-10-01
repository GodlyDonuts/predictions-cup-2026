// PR #8 independent offline regression artifact.
// Reviewed base: 19ad3f2be04438faad04d283116397f6a33ab200.
// Run from repository root: node --test capture-live-review-regressions.test.mjs
// All FIVE assertions intentionally FAIL on that reviewed base and describe
// integration defects in capture-live/. The original 95 tests pass there.
// These are not newly failing feed/paper/ML baseline tests.
// Node built-ins only; every SDK, fetch response and credential is invented.
// No credentials are read, no authenticated traffic is sent, and no orders exist.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {BookTransport} from './capture-live/transport.mjs';
import {capturePilot} from './capture-live/pilot.mjs';
import {Capture} from './feed/capture.mjs';
import {config,snapshotPath} from './feed/contract.mjs';
const scope=config({tournamentId:'00000000-0000-0000-0000-000000000001',marketId:1,exchangeIds:[2]});
function fixture({ackDisconnect=false}={}){
 let now=Date.parse('2026-10-01T22:00:00Z');const calls=[],channels=[];
 const budget={sharedAuthority:true,admit:async()=>{},cooldown:async()=>{}};
 const createClient=()=>({realtime:{setAuth:async()=>{},disconnect:()=>{}},removeChannel:async()=>{},channel:()=>{const c={handlers:{},on(_,filter,fn){this.handlers[filter.event]=fn;return this;},subscribe(fn){this.status=fn;queueMicrotask(()=>{fn('SUBSCRIBED');if(ackDisconnect)fn('CHANNEL_ERROR');});return this;}};channels.push(c);return c;}});
 const snapshot=()=>({contexts:[{type:'tournament',tournament:{id:scope.tournamentId},orderbook:{exchanges:[{exchangeId:2,asOf:{sequence:1,at:new Date(now).toISOString()},bids:[{price:.4,quantity:10}],asks:[{price:.6,quantity:10}]}]}}]});
 const fetchFn=async(url,opts)=>{calls.push({url,opts});return {ok:true,headers:new Headers(),json:async()=>url.endsWith('/realtime/token')?{token:'invented-jwt',anonKey:'invented-anon',supabaseUrl:'https://project.supabase.co',expiresAt:new Date(now+10800000).toISOString()}:snapshot()};};
 return {options:{scope,key:'invented-test-key',createClient,budget,tokenSetupApproved:true,fetchFn,now:()=>now,timeoutMs:100},calls,channels,snapshot,sleep:async ms=>{now+=ms;}};
}
test('P2: disconnect immediately after ACK cannot produce ready recovery without reconnect',async()=>{
 const f=fixture({ackDisconnect:true});const rows=[];
 const result=await capturePilot({scope,transport:new BookTransport(f.options),capture:{append:r=>rows.push(r)},durationMs:1000,now:f.options.now,sleep:f.sleep});
 console.log('ACK_ERROR_RESULT',JSON.stringify({connections:result.connections,recoveries:result.collector.metrics.recoveries,states:rows.filter(r=>['disconnected','recovery_finished'].includes(r.kind))}));
 assert.equal(rows.some(r=>r.kind==='recovery_finished'&&r.booksReady),false,'closed connection was declared ready');
});
test('P2: actual durable Capture preserves the token request count under a safe key',async()=>{
 const f=fixture();const dir=fs.mkdtempSync(path.join(os.tmpdir(),'pr8-review-'));const file=path.join(dir,'capture.jsonl');const capture=new Capture(file);
 let report;try{report=await capturePilot({scope,transport:new BookTransport(f.options),capture,durationMs:1000,now:f.options.now,sleep:f.sleep});}finally{capture.close();}
 const saved=JSON.parse(fs.readFileSync(file,'utf8').trim().split('\n').at(-1));
 console.log('PERSISTED_COUNTS',JSON.stringify({returned:report.restCounts,persisted:saved.report.restCounts}));
 assert.equal(Object.values(saved.report.restCounts).reduce((a,b)=>a+b,0),2,'durable summary omits the bootstrap POST request');
});
test('P2: admission that resolves after close must never start a network request',async()=>{
 const f=fixture();let release;f.options.budget.admit=()=>new Promise(r=>{release=r;});const transport=new BookTransport(f.options);
 const request=transport.readSnapshot({method:'GET',path:snapshotPath(scope)});await transport.close();release();await request.catch(()=>{});
 console.log('POST_CLOSE_REQUESTS',JSON.stringify(f.calls.map(c=>({url:c.url,authorization:c.opts.headers.Authorization}))));
 assert.equal(f.calls.length,0,'request proceeded after close');
});
test('P3: a normally completed real-time window is duration_complete',async()=>{
 const transport={counts:{},subscribe:async()=>{},readSnapshot:async()=>fixture().snapshot(),refreshDue:()=>false,close:async()=>{}};
 const report=await capturePilot({scope,transport,durationMs:30});
 console.log('WALLCLOCK_OUTCOME',report.outcome);
 assert.equal(report.outcome,'duration_complete');
});
test('P2: capture sink failure in event callback stops rather than escaping the callback',async()=>{
 const f=fixture();let failure=false;const capture={append:r=>{if(failure&&r.kind==='event')throw new Error('DISK_FULL');}};
 const transport=new BookTransport(f.options);let callbackError;
 let injected=false;
 const result=await capturePilot({scope,transport,capture,durationMs:1000,now:f.options.now,sleep:async ms=>{
  if(!injected){injected=true;failure=true;try{f.channels[0].handlers.market_batch({payload:{delivery:{model:'best-effort-authoritative-resync',correlationId:'x',revision:1,previousRevision:0,sourceSequenceFrom:null,sourceSequenceThrough:null},books:[],trades:[],bookDirty:[],marketSettled:[]}});}catch(e){callbackError=e.message;}}
  await f.sleep(ms);
 }});
 console.log('SINK_FAILURE',JSON.stringify({callbackError,outcome:result.outcome}));
 assert.equal(callbackError,undefined,'write failure escaped asynchronous SDK callback');assert.equal(result.outcome,'blocked');
});
