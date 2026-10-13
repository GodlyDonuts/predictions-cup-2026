import test from 'node:test';import assert from 'node:assert/strict';import {StreamEvidence,capture} from './realtime/capture.mjs';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {EventEmitter} from 'node:events';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55',at='2026-10-13T22:00:00Z';
const payload=(revision,previousRevision)=>({secretField:'not persisted',delivery:{model:'best-effort-authoritative-resync',correlationId:'not persisted',revision,previousRevision,sourceSequenceFrom:1,sourceSequenceThrough:1},books:[{exchangeId:2,asOf:{sequence:1,at},nextExpiryAt:null,bids:[{price:.4,quantity:100}],asks:[{price:.42,quantity:100}]}],trades:[{id:'1',sequence:1,marketId:'1',exchangeId:'2',tournamentId:tid,price:.42,quantity:10,executedAt:at}],bookDirty:[],marketSettled:[]});
test('stream evidence allowlists public fields, deduplicates revisions and flags gaps without financial claims',()=>{
  const rows=[],s=new StreamEvidence([{marketId:'1',exchangeId:'2'}],tid,r=>rows.push(r));s.batch('1','market_batch',payload(1,0),'2026-10-13T22:00:01Z');s.batch('1','market_batch',payload(1,0));s.batch('1','market_batch',payload(4,3));
  assert.equal(s.metrics.trades,1);assert.equal(s.metrics.duplicateBatches,1);assert.equal(s.metrics.gaps,1);assert.equal(rows.at(-1).requiresRestResync,true);assert.equal(JSON.stringify(rows).includes('not persisted'),false);assert.equal(s.metrics.latencyMaxMs,1000);
});
test('compact oversized batches and mismatched tournament never silently become executable evidence',()=>{
  const rows=[],s=new StreamEvidence([{marketId:'1',exchangeId:'2'}],tid,r=>rows.push(r));s.batch('1','market_batch',{delivery:payload(1,0).delivery,resyncRequired:true});assert.equal(rows[0].requiresRestResync,true);assert.equal(s.metrics.trades,0);
  const bad=payload(2,1);bad.trades[0].tournamentId='aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';s.batch('1','market_batch',bad);assert.equal(s.metrics.schemaErrors,1);assert.equal(rows.at(-1).code,'INVALID_SCHEMA');
});
test('aggregate topic demultiplexes market revisions and records unknown catalog markets without using them',()=>{
  const rows=[],s=new StreamEvidence([{marketId:1,exchangeId:2},{marketId:3,exchangeId:4}],tid,r=>rows.push(r));const second=payload(1,0);second.books[0].exchangeId=4;second.trades[0].marketId='3';second.trades[0].exchangeId='4';
  s.aggregate({markets:[{marketId:'1',batch:payload(1,0)},{marketId:'3',batch:second},{marketId:'99',batch:{private:'not saved'}}]},'2026-10-13T22:00:01Z');
  assert.equal(s.metrics.trades,2);assert.equal(s.revisions.get('1'),1);assert.equal(s.revisions.get('3'),1);assert.equal(s.metrics.unknownMarkets,1);assert.equal(rows.at(-1).kind,'unknown_market');assert.equal(JSON.stringify(rows).includes('not saved'),false);
});
test('REST and realtime share documented engine trade IDs in capture deduplication',()=>{
  const rows=[],s=new StreamEvidence([{marketId:'1',exchangeId:'2'}],tid,r=>rows.push(r));
  s.rest({source:'official_sig_api_v1',status:200,kind:'trades',tournamentId:tid,marketId:'1',exchangeId:'2',requestStartedAt:at,receivedAt:'2026-10-13T22:00:01Z',endpoint:'/exchanges/2/trades?tournamentId='+tid,data:{marketId:'1',exchangeId:'2',data:[{id:'1',price:.42,size:10,createdAt:at}],pagination:{hasMore:true,nextCursor:'next'}}});
  s.batch('1','market_batch',payload(1,0));assert.equal(s.metrics.trades,0);assert.equal(s.metrics.duplicateTrades,1);
});
test('capture drains final REST receipt on shutdown and never persists its auth packet',async()=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-stream-test-')),out=path.join(dir,'stream'),input=new EventEmitter();input.close=()=>input.emit('close');
  const receipt={source:'official_sig_api_v1',status:200,kind:'book',marketId:'1',exchangeId:'2',tournamentId:tid,receivedAt:at,endpoint:'/markets/1/orderbook?tournamentId='+tid,data:{contexts:[{type:'tournament',tournament:{id:tid},orderbook:{exchanges:[{exchangeId:2,asOf:{sequence:1,at},bids:[{price:.4,quantity:100}],asks:[{price:.42,quantity:100}]}]}}]}};
  const channel={on(){return this;},subscribe(callback){callback('SUBSCRIBED');setImmediate(()=>{fs.writeFileSync(path.join(dir,'receipts.jsonl'),JSON.stringify(receipt)+'\n');input.emit('line','{"stop":true}');});return this;}};
  const client={realtime:{async setAuth(){},disconnect(){}},channel(){return channel;},async removeAllChannels(){}};
  const work=capture(input,()=>client);
  setImmediate(()=>input.emit('line',JSON.stringify({auth:{token:'DO_NOT_PERSIST_AUTH',anonKey:'DO_NOT_PERSIST_ANON',supabaseUrl:'https://example.invalid',expiresAt:new Date(Date.now()+3600000).toISOString()},outputDirectory:out,tournamentId:tid,pairs:[{marketId:'1',exchangeId:'2'}]})));
  try{await work;const stats=JSON.parse(fs.readFileSync(path.join(out,'stats.json')));assert.equal(stats.status,'completed');assert.equal(stats.metrics.restBooks,1);for(const name of fs.readdirSync(out)){const data=fs.readFileSync(path.join(out,name),'utf8');assert.equal(data.includes('DO_NOT_PERSIST_'),false);}}
  finally{const resolved=fs.realpathSync(dir),relative=path.relative(fs.realpathSync(os.tmpdir()),resolved);assert.match(relative,/^moloch-stream-test-[^\\/]+$/);fs.rmSync(resolved,{recursive:true,force:true});}
});
