import test from 'node:test';
import assert from 'node:assert/strict';
import {extractRows,chronologicalSplit,fitAdverse,predictAdverse,evaluateAdverse,quoteFeatures} from './adverse-model.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const at=n=>new Date(Date.parse('2026-10-14T00:00:00Z')+n*1000).toISOString();
const book=(n,sequence,bid=.4,ask=.42,quantity=100)=>({exchangeId:'2',asOf:{at:at(n),sequence},bids:[{price:bid,quantity}],asks:[{price:ask,quantity:200}]});
function fixture({gap=false,stale=false}={}){
 const events=[{n:0,books:[book(0,1)]},{n:3,books:[book(3,2,.4,.42,1000000)]},{n:6,gap,books:[book(6,11,.45,.47)],trades:[{id:'t',sequence:10,marketId:'1',exchangeId:'2',tournamentId:'cup',price:.4,quantity:500,executedAt:at(4)}]},{n:stale?50:40,books:[book(stale?35:40,12,.39,.41)]}];
 let previous='';return events.map((e,i)=>{const event={kind:'broadcast',source:'official_sig_realtime',marketId:'1',event:'market_batch',receivedAt:at(e.n),gap:e.gap??false,payload:{books:e.books,trades:e.trades??[],resyncRequired:false}},body={sequence:i+1,previous,event},row={...body,hash:payloadHash(body)};previous=row.hash;return row;});
}
test('features predate activation and exclude newer pre-match and post-match books',()=>{
 const samples=extractRows(fixture()).samples;assert.equal(samples.length,1);const s=samples[0];assert.equal(s.quoteAt,at(0));assert.equal(s.features[3],Math.log1p(100));assert.equal(s.features[0],4);assert.equal(s.label,1);assert.equal(s.delta,-.01);assert.ok(Date.parse(s.executedAt)-Date.parse(s.quoteAt)>=3000);
});
test('gaps, stale future marks and edited chains cannot produce training evidence',()=>{
 assert.equal(extractRows(fixture({gap:true})).samples.length,0);assert.equal(extractRows(fixture({stale:true})).samples.length,0);const rows=fixture();rows[1].event.receivedAt=at(9);assert.throws(()=>extractRows(rows),/CHAIN/);
});
test('a fresh but late book cannot stretch a thirty-second label into a longer horizon',()=>{
 const rows=fixture(),last=rows.at(-1);last.event.receivedAt=at(60);last.event.payload.books=[book(60,12,.39,.41)];const {hash,...body}=last;last.hash=payloadHash(body);
 const result=extractRows(rows);assert.equal(result.samples.length,0);assert.equal(result.excludedMarks,1);
});
test('training excludes labels unavailable by cutoff and enforces an evaluation embargo',()=>{
 const rows=[{quoteAt:at(0),labelAt:at(5)},{quoteAt:at(1),labelAt:at(11)},{quoteAt:at(12),labelAt:at(50)},{quoteAt:at(20),labelAt:at(55)}];const split=chronologicalSplit(rows,at(10),at(20));assert.deepEqual(split.train,[rows[0]]);assert.deepEqual(split.test,[rows[3]]);assert.throws(()=>chronologicalSplit(rows,at(20),at(10)),/SPLIT/);
});
test('normalization is training-only and fitted risk distinguishes labeled feature groups',()=>{
 const rows=Array.from({length:40},(_,i)=>({features:[1,i%2?1:-1,.4,4,5,0],label:i%2,delta:i%2?-.01:0})),model=fitAdverse(rows),before=payloadHash(model);
 assert.ok(predictAdverse(model,rows[1].features)>predictAdverse(model,rows[0].features));assert.ok(evaluateAdverse(model,rows).brier<evaluateAdverse(model,rows).baselineBrier);evaluateAdverse(model,[{features:[9999,9999,9999,9999,9999,9999],label:0,delta:0}]);assert.equal(payloadHash(model),before);assert.equal(model.mean[0],1);assert.equal(model.scale[0],1);
});
test('NO features use complementary entry price and side-relative queues',()=>{
 const snapshot={book:book(0,1),receivedAt:at(1)},yes=quoteFeatures(snapshot,'YES'),no=quoteFeatures(snapshot,'NO');assert.ok(Math.abs(no[2]-.58)<1e-12);assert.equal(no[1],-yes[1]);assert.equal(no[3],yes[4]);assert.equal(no[5],1);
});
