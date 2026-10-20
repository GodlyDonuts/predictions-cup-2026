import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { historyUrl,validate,recover } from './backfill_polymarket_history.mjs';
import { sha256 } from '../external-data/polymarket-client.mjs';
const start=1790812800,end=start+120;
async function cleanup(root){const resolved=path.resolve(root);assert.equal(path.dirname(resolved),path.resolve(os.tmpdir()));assert.ok(path.basename(resolved).startsWith('moloch-public-history-'));await fs.rm(resolved,{recursive:true,force:true});}
const response=(u,status=200)=>({status,headers:new Headers(),text:async()=>JSON.stringify({history:[{t:start,p:.56},{t:end,p:.57}]})});
function receipt(){const bodyText=JSON.stringify({history:[{t:start,p:.56},{t:end,p:.57}]});return {url:String(historyUrl('123',start,end)),status:200,requestStartedAt:'2026-10-20T00:00:00Z',receivedAt:'2026-10-20T00:00:01Z',bodyText,bodySha256:sha256(bodyText)};}
test('original samples and retrieval clocks stay distinct, upper boundary excluded',()=>{
  assert.deepEqual(validate(receipt(),'123',start,end),{records:2,insideHalfOpenRange:1});
  assert.throws(()=>historyUrl('../orders',start,end));assert.throws(()=>historyUrl('123',start,start+8*86400));
});
test('changed source body or scope cannot be silently replayed',()=>{
  assert.throws(()=>validate({...receipt(),bodySha256:'0'.repeat(64)},'123',start,end));
  assert.throws(()=>validate(receipt(),'124',start,end));
});
test('resumes immutable receipts without repeat network requests',async()=>{
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'moloch-public-history-'));let calls=0;
  try{
    const plan={assets:['123','124'],startTs:start,endTs:end};const options={fetcher:async u=>{calls++;return response(u);},admission:{reserve:async()=>{}},maximumRequests:1};
    await recover(root,plan,options);assert.equal(JSON.parse(await fs.readFile(path.join(root,'status.json'))).state,'bounded_incomplete');
    await recover(root,plan,options);assert.equal(calls,2);
    const s=JSON.parse(await fs.readFile(path.join(root,'status.json')));assert.equal(s.completedAssets,2);assert.equal(s.trainingAdmitted,false);
    await assert.rejects(recover(root,{...plan,endTs:end+1},options));
  }finally{await cleanup(root);}
});
test('denial is preserved and terminal without alternate identities or routes',async()=>{
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'moloch-public-history-'));let calls=0;
  try{
    await assert.rejects(recover(root,{assets:['123'],startTs:start,endTs:end},{fetcher:async u=>{calls++;return response(u,403);},admission:{reserve:async()=>{}}}),/HTTP_403/);
    assert.equal(calls,1);assert.ok((await fs.readdir(path.join(root,'pages'))).some(x=>x.includes('.error-')));
  }finally{await cleanup(root);}
});
test('retry budget includes throttled attempts and respects provider backoff',async()=>{
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'moloch-public-history-'));let calls=0,delay=0;
  try{
    await recover(root,{assets:['123'],startTs:start,endTs:end},{maximumRequests:1,fetcher:async u=>{calls++;const r=response(u,429);r.headers.set('retry-after','12');return r;},admission:{reserve:async()=>{},backoff:ms=>{delay=ms;}}});
    assert.equal(calls,1);assert.equal(delay,12000);assert.equal(JSON.parse(await fs.readFile(path.join(root,'status.json'))).state,'bounded_incomplete');
  }finally{await cleanup(root);}
});
