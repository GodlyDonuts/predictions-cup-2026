import test from 'node:test';
import assert from 'node:assert/strict';
import {Api,AdmissionQueue,retryAfterMs} from './api.mjs';
test('read-only client rejects placement, cancellation, and credential redirects',async()=>{
 const api=new Api('https://sig.thesuper.market/api/v1','test-key',0);
 await assert.rejects(api.request('/orders','POST',{}),/read-only/);
 await assert.rejects(api.request('/orders/1','DELETE'),/read-only/);
 assert.throws(()=>new Api('https://example.com','test-key'),/authorized SIG host/);
});
test('pagination reads all pages and refuses a truncated cursor contract',async()=>{
 const api=new Api('https://sig.thesuper.market/api/v1','test-key',0);const calls=[];
 api.request=async path=>{calls.push(path);return calls.length===1?{data:[1],pagination:{hasMore:true,nextCursor:'a/b'}}:{data:[2],pagination:{hasMore:false}};};
 assert.deepEqual(await api.pages('/markets?limit=100'),[1,2]);
 assert.equal(calls[1],'/markets?limit=100&cursor=a%2Fb');
 api.request=async()=>({data:[],pagination:{hasMore:true,nextCursor:null}});
 await assert.rejects(api.pages('/markets?limit=100'),/Incomplete/);
});

// Deterministic clock: sleeps resolve only when the test advances time.
function fakeClock(start=0){
 let time=start;const pending=[];
 return {now:()=>time,sleepFn:ms=>new Promise(resolve=>pending.push({at:time+ms,resolve})),
  async advance(ms){time+=ms;for(let i=pending.length-1;i>=0;i--)if(pending[i].at<=time)pending.splice(i,1)[0].resolve();await this.flush();},
  async flush(){for(let i=0;i<30;i++)await Promise.resolve();}};
}
test('cooldown preserves serialized spacing for concurrent pending admissions',async()=>{
 const clock=fakeClock();const q=new AdmissionQueue(30,clock);q.pause(100);
 const times=[];const jobs=Array.from({length:4},()=>q.admit().then(()=>times.push(clock.now())));
 await clock.flush();await clock.advance(99);assert.deepEqual(times,[]);
 await clock.advance(1);assert.deepEqual(times,[100]);
 await clock.advance(30);await clock.advance(30);await clock.advance(30);
 await Promise.all(jobs);assert.deepEqual(times,[100,130,160,190]);
});
test('extended cooldown is rechecked before admission',async()=>{
 const clock=fakeClock();const q=new AdmissionQueue(30,clock);q.pause(100);
 const times=[];const job=q.admit().then(()=>times.push(clock.now()));
 await clock.flush();await clock.advance(50);q.pause(100);
 await clock.advance(50);assert.deepEqual(times,[]);
 await clock.advance(50);await job;assert.deepEqual(times,[150]);
});
test('Retry-After handles seconds, HTTP dates, expired dates and invalid values',()=>{
 const now=Date.parse('2026-10-01T20:00:00Z');
 assert.equal(retryAfterMs('1.5',now,99),1500);
 assert.equal(retryAfterMs('Thu, 01 Oct 2026 20:00:02 GMT',now,99),2000);
 assert.equal(retryAfterMs('Thu, 01 Oct 2026 19:59:59 GMT',now,99),0);
 for(const h of [null,'','garbage','Infinity','1e309','-1'])assert.equal(retryAfterMs(h,now,99),99);
});
test('HTTP-date 503 retry shares spaced admission with concurrent reads',async()=>{
 const start=Date.parse('2026-10-01T20:00:00Z');const clock=fakeClock(start);const times=[];
 const api=new Api('https://sig.thesuper.market/api/v1','test-key',30,{...clock,fetchFn:async()=>{
  times.push(clock.now()-start);const first=times.length===1;
  return {ok:!first,status:first?503:200,headers:new Headers(first?{'Retry-After':'Thu, 01 Oct 2026 20:00:02 GMT'}:{}),json:async()=>first?{error:{code:'BUSY'}}:{ok:true}};
 }});
 const jobs=[api.request('/a'),api.request('/b'),api.request('/c')];
 await clock.flush();assert.deepEqual(times,[0]);
 await clock.advance(30);assert.deepEqual(times,[0]);
 await clock.advance(1970);assert.deepEqual(times,[0,2000]);
 await clock.advance(30);await clock.advance(30);await Promise.all(jobs);
 assert.deepEqual(times,[0,2000,2030,2060]);
});

