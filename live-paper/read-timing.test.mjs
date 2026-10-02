import test from 'node:test';
import assert from 'node:assert/strict';
import {ReadClient} from './runtime.mjs';
import {timingSummary,validateTiming,analyzeReadTimings} from './read-timing.mjs';
const route='/tournaments/midterm-elections';
test('read timings separate queue, admission, header and body waits without copying private input',async()=>{
 let now=0;const events=[],a=new ReadClient('SENSITIVE_KEY',{now:()=>now,sleepFn:async ms=>now+=ms,onTiming:e=>events.push(e),fetchFn:async()=>{now+=500;return {ok:true,status:200,json:async()=>{now+=1500;return {private:'SENSITIVE_BODY'};}};}});
 await Promise.all([a.request(route),a.request('/orders?status=open&limit=200&tournamentId=bda92870-621e-47b0-bc3c-3602c5c26f55&cursor=SENSITIVE_CURSOR')]);
 assert.deepEqual(events.map(e=>e.queueWaitMs),[0,2000]);assert.deepEqual(events.map(e=>e.admissionWaitMs),[0,4000]);assert.deepEqual(events.map(e=>e.headersMs),[500,500]);assert.deepEqual(events.map(e=>e.bodyMs),[1500,1500]);assert.deepEqual(events.map(e=>e.totalMs),[2000,8000]);for(const e of events)validateTiming(e);for(const secret of ['SENSITIVE_KEY','SENSITIVE_BODY','SENSITIVE_CURSOR'])assert.equal(JSON.stringify(events).includes(secret),false);
 const summary=timingSummary(events);assert.equal(summary.groups.orders.serviceMs.median,2000);const raw=events.map(e=>JSON.stringify(e)).join('\n')+'\n';assert.equal(analyzeReadTimings(raw+'{').partialTailExcluded,true);assert.throws(()=>validateTiming({...events[0],Authorization:'secret'}),/schema/);
});
test('failed header and body waits retain durations and safe outcome categories',async()=>{
 for(const phase of ['headers','body']){let now=0;const events=[];const a=new ReadClient('FAKE',{now:()=>now,onTiming:e=>events.push(e),fetchFn:async()=>{now+=1000;if(phase==='headers')throw Error('SENSITIVE_NETWORK_DETAIL');return {ok:true,status:200,json:async()=>{now+=19000;throw Error('SENSITIVE_BODY_DETAIL');}};}});await assert.rejects(a.request(route));assert.equal(events[0].outcome,phase==='headers'?'connection_error':'body_error');assert.equal(events[0].headersMs,1000);assert.equal(events[0].bodyMs,phase==='headers'?null:19000);assert.equal(events[0].serviceMs,phase==='headers'?1000:20000);assert.equal(JSON.stringify(events).includes('SENSITIVE'),false);}
});
test('timing preserves Retry-After and abort admission without an additional fetch',async()=>{
 let now=0,calls=0;const events=[],c=new AbortController(),a=new ReadClient('FAKE',{signal:c.signal,now:()=>now,sleepFn:async ms=>now+=ms,onTiming:e=>events.push(e),fetchFn:async()=>{calls++;now+=200;return calls===1?{ok:false,status:429,headers:new Headers({'Retry-After':'90'})}:{ok:true,status:200,json:async()=>({})};}});
 await assert.rejects(a.request(route));await a.request(route);assert.equal(events[0].outcome,'http_error');assert.equal(events[1].admissionWaitMs,90000);assert.equal(calls,2);c.abort();await assert.rejects(a.request(route));assert.equal(events[2].outcome,'aborted');assert.equal(events[2].admittedAt,null);assert.equal(calls,2);
});
test('timing publication failures fail closed with a persistent manual-review admission sentinel',async()=>{
 let calls=0;const admission=[],a=new ReadClient('FAKE',{onAdmission:n=>admission.push(n),onTiming:()=>{throw Error('SENSITIVE_OBSERVER_DETAIL');},fetchFn:async()=>{calls++;return {ok:true,status:200,json:async()=>({})};}});
 await assert.rejects(a.request(route),/Read timing recording failed/);assert.equal(a.manualReview,true);assert.equal(admission.at(-1),null);await assert.rejects(a.request(route));assert.equal(calls,1);
});
