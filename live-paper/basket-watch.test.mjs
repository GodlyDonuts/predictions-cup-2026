import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {digest} from './model.mjs';import {verifySource} from './ml-shadow.mjs';
import {createBasketCapture,advanceBasketRecord,openBasketWatch} from './basket-watch.mjs';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55',base=Date.parse('2026-10-02T10:00:00.000Z'),stamp=ms=>new Date(base+ms).toISOString(),code={fixture:'frozen'};
const definitions=[{id:'a'.repeat(64),tournamentId:tid,legs:[{marketId:'1',exchangeId:'101',predicateHash:'1'.repeat(64)},{marketId:'2',exchangeId:'102',predicateHash:'2'.repeat(64)}]}];
function row(sequence,previous,event,ms){const body={sequence,previous,at:stamp(ms),event,state:{fixture:true}};return {...body,hash:digest(body)};}
function book(marketId,ms,seq=1){const id=marketId==='1'?'101':'102',bid=marketId==='1'?.19:.835;return {endpoint:'/markets/'+marketId+'/orderbook?tournamentId='+tid+'&depth=20',requestStartedAt:stamp(ms-100),receivedAt:stamp(ms),data:{contexts:[{type:'tournament',tournament:{id:tid},orderbook:{exchanges:[{exchangeId:id,option:'YES',asOf:{sequence:seq,at:stamp(ms-200)},bids:[{price:bid,quantity:1000}],asks:[{price:bid+.01,quantity:1000}]}]}}]}};}
function fixture(){const first=row(1,'',{type:'started',clockAt:stamp(0)},0),records=verifySource(JSON.stringify(first)+'\n'),{bank,state}=createBasketCapture(records,definitions,stamp(1000),code);return {first,bank,state};}
function cycle(sequence,previous,books,ms,extra={}){return row(sequence,previous,{type:'cycle',clockAt:stamp(ms),sourceSnapshots:books,supplementalDepth:[],accountExclusions:[],exclusionObservedAt:stamp(ms-5000),...extra},ms);}
test('receipt sorting merges core/supplemental books and respects future cohort cutoff',()=>{
 const {first,bank,state}=fixture(),r=cycle(2,first.hash,[book('2',8000)],10000,{supplementalDepth:[book('1',6000)]}),out=advanceBasketRecord(state,r,bank);
 assert.equal(out.observations.length,2);assert.equal(out.observations[1].status,'apparent_positive_basket');assert.equal(out.state.pairs[definitions[0].id].counts.receipts,2);assert.equal(out.observations[1].eligibleForExecution,false);
 const early=cycle(2,first.hash,[book('1',500),book('2',900)],2000,{exclusionObservedAt:stamp(100)}),e=advanceBasketRecord(state,early,bank);assert.equal(e.observations.length,0);
});
test('fresh account exclusions clear a cached leg before another leg can form a basket',()=>{
 const f=fixture(),r=cycle(2,f.first.hash,[book('1',6000),book('2',8000)],10000),s=advanceBasketRecord(f.state,r,f.bank).state;
 const next=cycle(3,r.hash,[book('2',14000,2)],16000,{accountExclusions:['101'],exclusionObservedAt:stamp(11000)}),out=advanceBasketRecord(s,next,f.bank);
 assert.equal(out.observations.length,1);assert.equal(out.observations[0].status,'waiting_for_pair');assert.equal(out.state.pairs[definitions[0].id].books['101'],undefined);
});
test('skips use their own receipt and malformed clocks invalidate after valid chronological books',()=>{
 const f=fixture(),r=cycle(2,f.first.hash,[{marketId:'1',skipped:'invalid',at:'bad'},book('2',8000)],10000,{supplementalDepth:[book('1',6000)]});
 const out=advanceBasketRecord(f.state,r,f.bank);assert.equal(out.state.pairs[definitions[0].id].books['101'],undefined);assert.equal(out.state.pairs[definitions[0].id].lastAt,stamp(10000));
 const barrier=cycle(2,f.first.hash,[book('2',8000),{marketId:'1',skipped:'invalid',at:'bad'},book('1',6000)],10000);
 const ordered=advanceBasketRecord(f.state,barrier,f.bank);assert.equal(ordered.observations.length,2);assert.equal(ordered.state.pairs[definitions[0].id].books['101'],undefined);
 const skipped=cycle(2,f.first.hash,[{marketId:'1',skipped:'http_503',at:stamp(5500)},book('1',6000),book('2',8000)],10000);
 assert.doesNotThrow(()=>advanceBasketRecord(f.state,skipped,f.bank));
});
test('outage and missing/rolled-back account context prevent stale cached pairing',()=>{
 const f=fixture(),r=cycle(2,f.first.hash,[book('1',6000),book('2',8000)],10000),s=advanceBasketRecord(f.state,r,f.bank).state;
 for(const event of [{type:'account_read_outage',clockAt:stamp(16000)},{type:'cycle',clockAt:stamp(16000),sourceSnapshots:[book('2',14000,2)]},{type:'cycle',clockAt:stamp(16000),sourceSnapshots:[book('2',14000,2)],accountExclusions:[],exclusionObservedAt:stamp(7000)}]){
  const out=advanceBasketRecord(s,row(3,r.hash,event,16000),f.bank);assert.deepEqual(out.state.pairs[definitions[0].id].books,{});assert.equal(out.observations.length,0);
 }
});
test('durable observation journal replays after restart without recounting; source/cutoff/code mutations fail',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'basket-watch-test-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const f=fixture(),source=path.join(dir,'journal.jsonl');fs.writeFileSync(source,JSON.stringify(f.first)+'\n');
 const w=openBasketWatch(dir,{initialize:true,cutoff:stamp(1000),definitions,codeIdentity:code});const r=cycle(2,f.first.hash,[book('1',6000),book('2',8000)],10000);fs.appendFileSync(source,JSON.stringify(r)+'\n');assert.equal(w.consume(),1);const before=w.status();
 const resumed=openBasketWatch(dir,{codeIdentity:code});assert.deepEqual(resumed.status(),before);assert.equal(resumed.consume(),0);assert.equal(resumed.status().journalSequence,1);
 assert.throws(()=>openBasketWatch(dir,{initialize:true,cutoff:stamp(1000),definitions,codeIdentity:code}),/replace/);assert.throws(()=>openBasketWatch(dir,{codeIdentity:{fixture:'changed'}}),/code changed/);
 fs.writeFileSync(source,JSON.stringify({...f.first,hash:'0'.repeat(64)})+'\n');assert.throws(()=>openBasketWatch(dir,{codeIdentity:code}),/integrity/);
});
