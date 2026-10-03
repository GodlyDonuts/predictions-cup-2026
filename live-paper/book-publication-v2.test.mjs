import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import * as original from './book-publication.mjs';import * as fast from './book-publication-v2.mjs';
const u={tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',exchanges:[{marketId:'381',exchangeId:'1070',option:'YES'},{marketId:'382',exchangeId:'1071',option:'YES'}]};
const response=(marketId,exchangeId,seq,at)=>({endpoint:'/markets/'+marketId+'/orderbook?tournamentId='+u.tournamentId+'&depth=20',status:200,
 requestStartedAt:new Date(Date.parse(at)-100).toISOString(),receivedAt:at,data:{marketId,contexts:[{type:'tournament',tournament:{id:u.tournamentId},orderbook:{exchanges:[{exchangeId,option:'YES',asOf:{sequence:seq,at},bids:[{price:.4,quantity:1000}],asks:[{price:.42,quantity:500}]}]}}]}});
test('separate faster publisher emits byte-identical records and immutable states across books/boundaries/restart',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'public-books-v2-'));let time=Date.parse('2026-10-03T22:00:00.000Z');const files=['old','fast'].map(n=>path.join(dir,n));
 const writers=[original,fast].map((m,i)=>m.createBookPublication({file:files[i],universe:u,sessionId:'same-session',now:()=>time}));
 try{for(let i=1;i<=100;i++){time+=200;const j=i%2,b=response(u.exchanges[j].marketId,u.exchanges[j].exchangeId,i,new Date(time).toISOString());for(const w of writers)assert.equal(w.publishBook({marketId:b.data.marketId,response:b}),true);
  if(i%25===0){time+=10;for(const w of writers)w.invalidate({marketId:i%50===0?null:'381',reason:'MANUAL_INVALIDATION'});}
  assert.deepEqual(writers[1].status(),writers[0].status());
 }
 const prior=writers[1].status();assert.throws(()=>prior.scope.exchanges.push({}));assert.throws(()=>prior.books['1070:YES'].bids[0].quantity=1);
 for(const w of writers)w.close('NORMAL_SHUTDOWN');assert.equal(fs.readFileSync(files[0],'utf8'),fs.readFileSync(files[1],'utf8'));
 time+=1000;const reopened=[original,fast].map((m,i)=>m.createBookPublication({file:files[i],universe:u,sessionId:'same-next-session',now:()=>time}));
 try{assert.deepEqual(reopened[1].status(),reopened[0].status());assert.equal(fs.readFileSync(files[0],'utf8'),fs.readFileSync(files[1],'utf8'));}finally{for(const w of reopened)w.close('STOP');}
 }finally{for(const w of writers)w.close('STOP');fs.rmSync(dir,{recursive:true,force:true});}
});
test('caller state and records remain unmodified; rollback/version conflicts and corrupted chain still fail',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'public-books-v2-parity-'));let time=Date.parse('2026-10-03T22:00:00.000Z');const file=path.join(dir,'source');const w=original.createBookPublication({file,universe:u,sessionId:'source-session',now:()=>time});
 try{time+=200;w.publishBook({marketId:'381',response:response('381','1070',1,new Date(time).toISOString())});w.close('STOP');
  const rows=fs.readFileSync(file,'utf8').trim().split('\n').map(JSON.parse);let a=original.createBookPublicationState(u),b=fast.createBookPublicationState(u);
  for(const r of rows){const saved=JSON.stringify(b),input=JSON.stringify(r);a=original.applyBookPublication(a,r);const next=fast.applyBookPublication(b,r);assert.equal(JSON.stringify(b),saved);assert.equal(JSON.stringify(r),input);b=next;assert.deepEqual(b,a);}
  const bad=structuredClone(rows[1]);bad.hash='a'.repeat(64);for(const m of [original,fast])assert.throws(()=>m.applyBookPublication(m.applyBookPublication(m.createBookPublicationState(u),rows[0]),bad),/CHAIN_INVALID/);
 }finally{w.close('STOP');fs.rmSync(dir,{recursive:true,force:true});}
});
