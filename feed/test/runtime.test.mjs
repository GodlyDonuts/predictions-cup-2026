import test from 'node:test';
import assert from 'node:assert/strict';
import {MarketCollector} from '../collector.mjs';
import {OfflineRuntime,AccountReadBudget,DisabledTransport,RecoveryError,backoff,retryAfterMs,interruptibleWait} from '../runtime.mjs';
import {scope,start,book,batch,snapshot} from './helpers.mjs';
function setup(options={}) {
 const calls=[];const c=new MarketCollector(scope,{now:()=>start});
 const adapter={mode:'offline',subscribe:async o=>{calls.push('subscribe');adapter.onEvent=o.onEvent;},readSnapshot:async o=>{calls.push(o);return snapshot();},close:async()=>{calls.push('close');},...options.adapter};
 const budget=new AccountReadBudget({accountScope:'synthetic-account',admit:async o=>{calls.push('admit');},cooldown:async o=>{calls.push({cooldown:o.retryAfterMs});},...options.budget});
 const rt=new OfflineRuntime({collector:c,adapter,budget,enabled:true,sleep:async()=>{},random:()=>1,...options,collector:c,adapter,budget});return {c,adapter,budget,rt,calls};
}
test('default disabled transport, flags and missing central callback prevent traffic',async()=>{
 const {c,adapter,budget,calls}=setup();const r=new OfflineRuntime({collector:c,adapter,budget});await assert.rejects(r.connect(),/LIVE_TRANSPORT_DISABLED/);assert.equal(calls.length,0);
 await assert.rejects(new DisabledTransport().subscribe(),/DISABLED/);assert.throws(()=>new AccountReadBudget({}),/SHARED_BUDGET/);
 const x=new OfflineRuntime({collector:c,adapter,enabled:true});c.subscribed();await assert.rejects(x.recover(),/SHARED_BUDGET/);
});
test('subscribes before one shared GET admission and snapshot; no write API exists',async()=>{
 const {rt,calls,c}=setup();assert.equal(await rt.connect(),true);assert.equal(calls[0],'subscribe');assert.equal(calls[1],'admit');assert.equal(calls[2].method,'GET');assert.match(calls[2].path,/tournamentId=.*&depth=200/);assert.equal(c.state,'ready');assert.equal('placeOrder' in rt,false);
});
test('subscribed-window frames buffered and applied before REST recovery',async()=>{
 const {rt,c}=setup({adapter:{mode:'offline',subscribe:async({onEvent})=>onEvent('market_batch',batch(1,0,[book(701,110),book(702,110)])),readSnapshot:async()=>snapshot([book(701,110),book(702,110)]),close:async()=>{}}});
 assert.ok(await rt.connect());assert.equal(c.revision,1);assert.equal(c.books.get('701').book.asOf.sequence,110);
});
test('429 Retry-After shared, waited, and every retry consumes account budget',async()=>{
 let n=0;const {rt,calls}=setup({adapter:{mode:'offline',subscribe:async()=>{},readSnapshot:async()=>{if(!n++)throw new RecoveryError({status:429,retryAfter:'70'});return snapshot();},close:async()=>{}}});
 assert.equal(await rt.connect(),true);assert.equal(calls.filter(x=>x==='admit').length,2);assert.deepEqual(calls.find(x=>x?.cooldown),{cooldown:70000});
});
test('server Retry-After exceeding bound stops, never truncated early',async()=>{
 const {rt,calls,c}=setup({adapter:{mode:'offline',subscribe:async()=>{},readSnapshot:async()=>{throw new RecoveryError({status:503,retryAfter:'9999'});},close:async()=>{}}});
 assert.equal(await rt.connect(),false);assert.equal(calls.filter(x=>x==='admit').length,1);assert.equal(c.state,'disconnected');assert.deepEqual(calls.find(x=>x?.cooldown),{cooldown:9999000});
});
test('nonretryable auth error never blindly retries',async()=>{
 const {rt,calls}=setup({adapter:{mode:'offline',subscribe:async()=>{},readSnapshot:async()=>{throw new RecoveryError({status:403});},close:async()=>{}}});
 assert.equal(await rt.connect(),false);assert.equal(calls.filter(x=>x==='admit').length,1);
});
test('bounded backoff and max attempts stop failing recovery',async()=>{
 let n=0;const {rt}=setup({maxAttempts:3,adapter:{mode:'offline',subscribe:async()=>{},readSnapshot:async()=>{n++;throw new Error('Bearer secret');},close:async()=>{}}});assert.equal(await rt.connect(),false);assert.equal(n,3);
 assert.equal(backoff(99,{random:()=>1}),30000);assert.equal(backoff(0,{random:()=>0}),125);
});
test('disconnect during failed recovery cannot recover without reconnect',async()=>{
 const s=setup({maxAttempts:2});s.adapter.readSnapshot=async()=>{s.c.disconnected();throw new RecoveryError();};assert.equal(await s.rt.connect(),false);assert.equal(s.c.state,'disconnected');assert.equal(s.calls.filter(x=>x==='admit').length,1);
});
test('disconnected snapshot success is discarded',async()=>{const s=setup();s.adapter.readSnapshot=async()=>{s.c.disconnected();return snapshot();};assert.equal(await s.rt.connect(),false);assert.equal(s.c.state,'disconnected');});
test('aborted before startup/STOP causes no adapter or budget traffic',async()=>{
 const s=setup({shouldStop:()=>true});await assert.rejects(s.rt.connect(),/STOPPED/);assert.equal(s.calls.length,0);
 const a=setup();const ac=new AbortController();ac.abort();await assert.rejects(a.rt.connect({signal:ac.signal}),/STOPPED/);assert.equal(a.calls.length,0);
});
test('abort interrupts queued budget wait and closes subscription',async()=>{
 const s=setup({budget:{admit:async()=>new Promise(()=>{})},requestTimeoutMs:1000});const ac=new AbortController();const work=s.rt.connect({signal:ac.signal});setTimeout(()=>ac.abort(),10);await assert.rejects(work,/STOPPED/);assert.equal(s.c.state,'stopped');assert.ok(s.calls.includes('close'));assert.equal(s.calls.filter(x=>typeof x==='object'&&x.method).length,0);
});
test('STOP callback interrupts outstanding request without waiting timeout',async()=>{
 let stopped=false;const s=setup({shouldStop:()=>stopped,adapter:{mode:'offline',subscribe:async()=>{},readSnapshot:async()=>new Promise(()=>{}),close:async()=>{}},requestTimeoutMs:1000});const w=s.rt.connect();setTimeout(()=>stopped=true,10);await assert.rejects(w,/STOPPED/);assert.equal(s.c.state,'stopped');
});
test('stop aborts runtime-owned work; uncooperative close bounded',async()=>{
 const s=setup({adapter:{mode:'offline',subscribe:async()=>{},readSnapshot:async()=>new Promise(()=>{}),close:async()=>new Promise(()=>{})},requestTimeoutMs:30});
 const w=s.rt.connect();const rejection=assert.rejects(w,/STOPPED/);await new Promise(r=>setTimeout(r,5));await s.rt.stop();await rejection;assert.equal(s.c.state,'stopped');
});
test('request timeout fails closed and connection closed',async()=>{const s=setup({requestTimeoutMs:15});s.adapter.readSnapshot=async()=>new Promise(()=>{});assert.equal(await s.rt.connect(),false);assert.equal(s.c.state,'disconnected');assert.ok(s.calls.includes('close'));});
test('reconnect is bounded and fresh REST remains required',async()=>{
 let n=0;const s=setup();s.adapter.subscribe=async()=>{if(++n<3)throw new Error('offline');};assert.ok(await s.rt.connect({reason:'RECONNECT'}));assert.equal(n,3);assert.equal(s.c.metrics.reconnects,1);assert.equal(s.calls.filter(x=>x==='admit').length,1);
});
test('interruptible wait honors signals and invalid values',async()=>{
 const ac=new AbortController();ac.abort();await assert.rejects(interruptibleWait(10000,{signal:ac.signal}),/STOPPED/);await assert.rejects(interruptibleWait(NaN));
 assert.equal(retryAfterMs('Thu, 01 Oct 2026 21:00:01 GMT',start),1000);assert.equal(retryAfterMs('1.5',start),1500);assert.equal(retryAfterMs('garbage',start),0);
});
test('later direct recovery abort also stops and closes the existing subscription',async()=>{
 const s=setup({requestTimeoutMs:1000});assert.ok(await s.rt.connect());s.c.invalidate('STALE');s.adapter.readSnapshot=async()=>new Promise(()=>{});const ac=new AbortController();const w=s.rt.recover({signal:ac.signal});setTimeout(()=>ac.abort(),10);await assert.rejects(w,/STOPPED/);assert.equal(s.c.state,'stopped');assert.ok(s.calls.includes('close'));
});
