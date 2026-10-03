import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {digest} from './model.mjs';
import {validateUniverse} from './breadth.mjs';
import {createBookPublication} from './book-publication.mjs';
import {openBulkDip} from './bulk-dip-host-v2.mjs';
import {anchorFocusMarkets,projectBulkReadFocus,readBulkReadFocus} from './bulk-dip-read-focus.mjs';
import {atomicJson} from '../execution/ledger.mjs';
import {openExecutableEdge} from './executable-edge-watch.mjs';
import {openLiveBidAnchor} from './bid-anchor-live-host.mjs';
import {runObservedEdge} from './executable-edge-observe.mjs';

const universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url)));
const canonical=validateUniverse(universe,universe.tournamentId),spec=universe.exchanges.find(x=>x.marketId==='354');
const base=Date.parse('2026-10-03T00:00:00Z'),stamp=n=>new Date(base+n).toISOString();
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-bulk-v2-'));let time=0,version=0,sequence=0,previous='';
 const writer=createBookPublication({file:path.join(dir,'public-books.jsonl'),universe,sessionId:'review-v2',now:()=>base+time});
 t.after(()=>{writer.close();assert.equal(path.dirname(dir),os.tmpdir());assert.ok(path.basename(dir).startsWith('sig-bulk-v2-'));fs.rmSync(dir,{recursive:true,force:true});});
 return {dir,set:n=>time=n,now:()=>base+time,
  invalidation(n,id='354'){time=n;writer.invalidate({marketId:id,reason:'MANUAL_INVALIDATION'});},
  book(n,bid=.425,ask=.43,quantity=1000){time=n;assert.ok(writer.publishBook({marketId:spec.marketId,response:{endpoint:`/markets/${spec.marketId}/orderbook?tournamentId=${universe.tournamentId}&depth=20`,requestStartedAt:stamp(n-100),receivedAt:stamp(n-10),data:{contexts:[{type:'tournament',tournament:{id:universe.tournamentId},orderbook:{exchanges:[{exchangeId:spec.exchangeId,option:'YES',asOf:{sequence:++version,at:stamp(n-20)},bids:[{price:bid,quantity}],asks:[{price:ask,quantity}]}]}}]}}}));},
  price(n,bid=.45,ask=.455,start=n-100){time=n;const snapshot={tournamentId:universe.tournamentId,universeHash:canonical.hash,requestStartedAt:stamp(start),receivedAt:stamp(n-10),sourceTimestamp:null,depthAvailable:false,executable:false,quotes:[{exchangeId:spec.exchangeId,marketId:spec.marketId,option:'YES',latestPrice:bid,bestBid:bid,bestAsk:ask,spread:ask-bid,crossed:false}],missingIds:[]},body={sequence:++sequence,previous,at:stamp(n),event:{type:'market_price_observation',snapshot},state:null},hash=digest(body);fs.appendFileSync(path.join(dir,'market-prices.jsonl'),JSON.stringify({...body,hash})+'\n');previous=hash;}
 };
}
function seed(f){for(const n of [1000,61000,121000,181000,241000])f.price(n);f.book(245000,.45,.455);f.set(250000);}
function open(f,name='review-test'){return openBulkDip({runtimeDir:f.dir,cohort:name,initialize:true,now:f.now,workspaceDir:f.dir});}
test('market-specific invalidation before initialization excludes the previous reference',t=>{
 let w;t.after(()=>w?.close());const f=fixture(t);seed(f);f.invalidation(246000);f.set(250000);w=open(f);
 assert.equal(w.status().bulkReference.marketCount,0,'pre-invalidation prices cannot warm the new cohort');
 f.price(300000,.425,.43);f.book(300500);f.set(301000);w.consume();assert.equal(w.status().wallets[1000].pending.length,0);assert.equal(w.status().signals.length,0);
});
test('running host verifies unchanged-size price prefix before accepting an appended book',t=>{
 let w;t.after(()=>w?.close());const f=fixture(t);seed(f);w=open(f);
 const before=w.status(),journal=fs.readFileSync(path.join(w.dir,'journal.jsonl'));
 const file=path.join(f.dir,'market-prices.jsonl'),bytes=fs.readFileSync(file),position=bytes.indexOf(Buffer.from('0.45'));assert.ok(position>=0);bytes[position+3]=54;fs.writeFileSync(file,bytes);
 f.book(300500);f.set(301000);assert.throws(()=>w.consume(),/SOURCE_CHANGED/);
 assert.equal(w.status().stateHash,before.stateHash);assert.equal(w.status().journalHash,before.journalHash);assert.deepEqual(fs.readFileSync(path.join(w.dir,'journal.jsonl')),journal);
});
test('running host rejects unchanged-size original journal mutation before any source advancement',t=>{
 let w;t.after(()=>w?.close());const f=fixture(t);seed(f);w=open(f);const before=w.status(),file=path.join(w.dir,'journal.jsonl'),bytes=fs.readFileSync(file),pos=bytes.indexOf(Buffer.from('genesis'));assert.ok(pos>=0);bytes[pos]=120;fs.writeFileSync(file,bytes);
 f.price(300000,.425,.43);f.set(300050);assert.throws(()=>w.consume(),/JOURNAL_CHANGED/);assert.equal(w.status().stateHash,before.stateHash);assert.equal(w.status().journalHash,before.journalHash);
});
test('an in-flight bulk request cannot recreate reference across a market invalidation',t=>{
 let w;t.after(()=>w?.close());const f=fixture(t);seed(f);w=open(f);
 f.invalidation(260000);f.price(260100,.45,.455,259900);f.set(260200);w.consume();assert.equal(w.status().bulkReference.marketCount,0);
});
test('corrected cohort keeps causal reference, fees and exact original journal restart',t=>{
 let w;t.after(()=>w?.close());const f=fixture(t);seed(f);w=open(f);f.price(300000,.425,.43);f.set(300050);w.consume();assert.equal(w.status().bulkReference.candidates.length,1);
 f.book(300500);f.set(300600);w.consume();assert.equal(w.status().wallets[1000].pending.length,1);assert.equal(w.status().wallets[1000].fills,0);
 f.book(301800);f.set(301900);w.consume();assert.equal(w.status().wallets[1000].fills,1);assert.equal(w.status().wallets[3000].fills,0);assert.equal(w.status().wallets[1000].fees,'0.000000');
 const before=w.status(),bank=fs.readFileSync(path.join(w.dir,'bank.json')),journal=fs.readFileSync(path.join(w.dir,'journal.jsonl'));w.close();w=null;
 w=openBulkDip({runtimeDir:f.dir,cohort:'review-test',now:f.now,workspaceDir:f.dir});assert.equal(w.status().stateHash,before.stateHash);assert.equal(w.status().journalHash,before.journalHash);assert.deepEqual(fs.readFileSync(path.join(w.dir,'bank.json')),bank);assert.deepEqual(fs.readFileSync(path.join(w.dir,'journal.jsonl')),journal);
});
test('expired and re-prepared buys retain explicit scoped identity and held risk focus',t=>{
 let w;t.after(()=>w?.close());const f=fixture(t);seed(f);w=open(f);f.price(300000,.425,.43);f.book(300700);f.set(301000);w.consume();
 assert.equal(w.status().signals.length,1);f.book(441500,.425,.43,99);f.set(441600);w.consume();const report=w.status();
 assert.equal(report.signals.length,1);assert.equal(report.wallets[1000].pending[0].createdAt,stamp(441600));
 assert.equal(report.wallets[1000].pending[0].instrument,spec.exchangeId+':YES');assert.equal(report.wallets[1000].pending[0].marketId,spec.marketId);
 // A separate held instrument must remain focused while this buy is renewed.
 const held=universe.exchanges.find(x=>x.marketId!==spec.marketId);report.wallets[3000].positions.push({instrument:held.exchangeId+':YES',quantity:1});
 assert.deepEqual(anchorFocusMarkets(report,canonical),[{marketId:held.marketId,phase:'held'},{marketId:spec.marketId,phase:'entry_pending'}]);
 report.wallets[1000].pending[0].marketId=held.marketId;assert.throws(()=>anchorFocusMarkets(report,canonical),/INVALID_BULK_READ_FOCUS/);
});
test('corrected host runs in the existing three-journal observer with checkpoint-bound focus',async t=>{
 const f=fixture(t);seed(f);const options={runtimeDir:f.dir,workspaceDir:f.dir,now:f.now},edge=openExecutableEdge({...options,cohort:'edge-test',initialize:true}),anchor=openLiveBidAnchor({...options,cohort:'anchor-test',initialize:true}),bulk=open(f);
 const dirs=[edge.dir,anchor.dir,bulk.dir],banks=dirs.map(d=>fs.readFileSync(path.join(d,'bank.json'))),journals=dirs.map(d=>fs.readFileSync(path.join(d,'journal.jsonl')));edge.close();anchor.close();bulk.close();
 f.price(300000,.425,.43);f.book(300700);f.set(301000);await runObservedEdge({...options,cohort:'edge-test',bidAnchor:'anchor-test',bulkDip:'review-test',bulkHostVersion:2,once:true});
 for(let i=0;i<dirs.length;i++){assert.deepEqual(fs.readFileSync(path.join(dirs[i],'bank.json')),banks[i]);assert.deepEqual(fs.readFileSync(path.join(dirs[i],'journal.jsonl')).subarray(0,journals[i].length),journals[i]);assert.equal(fs.existsSync(path.join(dirs[i],'worker.lock')),false);}
 const report=JSON.parse(fs.readFileSync(path.join(bulk.dir,'stats.json')));assert.equal(report.hostVersion,2);assert.equal(report.wallets[1000].pending[0].instrument,spec.exchangeId+':YES');
 // One-shot mode releases its lock; the same verified report becomes readable
 // only under a current owner and matching original journal checkpoint.
 const lock={pid:process.pid,id:'test-reader-owner'};atomicJson(path.join(bulk.dir,'worker.lock'),lock);atomicJson(path.join(bulk.dir,'read-focus.json'),projectBulkReadFocus(report,lock,'edge-test'));
 assert.deepEqual(readBulkReadFocus(f.dir,canonical,f.now()).markets,[{marketId:spec.marketId,phase:'entry_pending'}]);fs.unlinkSync(path.join(bulk.dir,'worker.lock'));
});
