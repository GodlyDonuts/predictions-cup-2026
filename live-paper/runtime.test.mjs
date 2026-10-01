import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {Journal,ReadClient,lock} from './runtime.mjs';
test('durable journal restores committed state, verifies hash chain and rejects partial append',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-paper-test-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const file=path.join(dir,'j');const j=new Journal(file);j.commit({cash:'100'}, {type:'test'});j.commit({cash:'99'},{type:'test2'});const r=new Journal(file);assert.equal(r.last.state.cash,'99');assert.equal(r.sequence,2);fs.appendFileSync(file,'{');assert.throws(()=>new Journal(file),/Incomplete/);
});
test('single collector lock is exclusive and released explicitly',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-paper-test-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const p=path.join(dir,'lock');const release=lock(p);assert.throws(()=>lock(p));release();const release2=lock(p);release2();
});
test('GET transport allows fixed scope only and serializes concurrent requests',async()=>{
 let now=0;const calls=[];const a=new ReadClient('FAKE',{now:()=>now,sleepFn:async ms=>{now+=ms;},fetchFn:async(u,o)=>{calls.push({u,o,at:now});return{ok:true,json:async()=>({id:'x'})};}});
 await assert.rejects(()=>a.request('/orders'),/allowlist/);await assert.rejects(()=>a.request('https://example.com'),/allowlist/);
 await Promise.all([a.request('/tournaments/midterm-elections'),a.request('/tournaments/midterm-elections')]);assert.deepEqual(calls.map(x=>x.at),[0,6000]);assert.equal(calls[0].o.method,'GET');assert.equal(calls[0].o.redirect,'error');a.close();
});
test('429 extends shared read cooldown, no blind immediate retry',async()=>{
 let now=0,n=0;const times=[];const a=new ReadClient('FAKE',{now:()=>now,sleepFn:async ms=>now+=ms,fetchFn:async()=>{times.push(now);return++n===1?{ok:false,status:429,headers:new Headers({'Retry-After':'90'})}:{ok:true,json:async()=>({})};}});
 await assert.rejects(()=>a.request('/tournaments/midterm-elections'),/429/);await a.request('/tournaments/midterm-elections');assert.deepEqual(times,[0,90000]);
});
test('abort during admission or response never returns a paper observation',async()=>{
 const c=new AbortController();let fetched=false;const a=new ReadClient('FAKE',{signal:c.signal,fetchFn:async()=>{fetched=true;return{ok:true,json:async()=>{c.abort();return{};}};}});await assert.rejects(()=>a.request('/tournaments/midterm-elections'),/Stopped/);assert.equal(fetched,true);await assert.rejects(()=>a.request('/tournaments/midterm-elections'),/Stopped/);
});
