import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import test from 'node:test';import assert from 'node:assert/strict';
import {createBookPublication} from './book-publication.mjs';import {openLiveBidAnchor} from './bid-anchor-live-host.mjs';
const universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url))),base=Date.parse('2026-10-03T00:00:00Z'),at=n=>new Date(base+n).toISOString();
test('actual receipts fence delayed fills, stale backlog and restart replay under exclusive ownership',t=>{
 const rt=fs.mkdtempSync(path.join(os.tmpdir(),'sig-anchor-host-'));let writer,w,n=0,version=1;
 t.after(()=>{w?.close();writer?.close();assert.equal(path.dirname(path.resolve(rt)),path.resolve(os.tmpdir()));assert.ok(path.basename(rt).startsWith('sig-anchor-host-'));fs.rmSync(rt,{recursive:true,force:true});});
 writer=createBookPublication({file:path.join(rt,'public-books.jsonl'),universe,sessionId:'actual-test',now:()=>base+n});
 const publish=(time,bid=.45,ask=.455)=>{n=time;const spec=universe.exchanges.find(x=>x.marketId==='354');assert.equal(writer.publishBook({marketId:'354',response:{endpoint:`/markets/354/orderbook?tournamentId=${universe.tournamentId}&depth=20`,requestStartedAt:at(n-100),receivedAt:at(n-10),data:{contexts:[{type:'tournament',tournament:{id:universe.tournamentId},orderbook:{exchanges:[{exchangeId:spec.exchangeId,option:'YES',asOf:{sequence:version++,at:at(n-20)},bids:[{price:bid,quantity:1000}],asks:[{price:ask,quantity:1000}]}]}}]}}}),true);};
 for(const time of [1000,61000,121000,181000,241000])publish(time);n=250000;w=openLiveBidAnchor({runtimeDir:rt,cohort:'actual-test',initialize:true,now:()=>base+n,workspaceDir:rt});const dir=w.dir,bank=fs.readFileSync(path.join(dir,'bank.json'));
 assert.throws(()=>openLiveBidAnchor({runtimeDir:rt,cohort:'actual-test',now:()=>base+n,workspaceDir:rt}),/EEXIST/);
 publish(300000,.425,.43);n=301500;w.consume();const prepared=w.status();assert.equal(prepared.wallets[1000].pending[0].createdAt,at(301500));assert.equal(prepared.wallets[1000].fills,0);assert.equal(prepared.signals[0].actualAvailableAt,at(301500));
 publish(302400,.425,.43);n=302500;w.consume();assert.equal(w.status().wallets[1000].fills,0,'request before actual decision plus latency cannot fill');
 publish(302700,.425,.43);n=302800;w.consume();assert.equal(w.status().wallets[1000].fills,1);assert.equal(w.status().wallets[3000].fills,0);
 publish(305000,.425,.43);n=330001;w.consume();assert.equal(w.status().actualConsumer.counts.stale_consumer_book,1);assert.equal(w.status().wallets[3000].pending.length,0);assert.equal(w.status().wallets[1000].positions.length,1);assert.equal(w.status().wallets[1000].liquidationPnl,null);
 publish(330101,.425,.43);n=330200;w.consume();assert.equal(w.status().actualConsumer.counts.request_before_consumer_barrier,1);
 publish(331000,.425,.43);n=331100;w.consume();const before=w.status(),journal=fs.readFileSync(path.join(dir,'journal.jsonl'));w.close();w=null;
 w=openLiveBidAnchor({runtimeDir:rt,cohort:'actual-test',now:()=>base+n,workspaceDir:rt});const after=w.status();assert.equal(after.stateHash,before.stateHash);assert.equal(after.journalHash,before.journalHash);assert.deepEqual(fs.readFileSync(path.join(dir,'bank.json')),bank);assert.deepEqual(fs.readFileSync(path.join(dir,'journal.jsonl')),journal);
 fs.writeFileSync(path.join(rt,'BID_ANCHOR_LIVE_STOP'),'operator stop');assert.throws(()=>w.consume(),/OWNERSHIP/);w.close();w=null;assert.throws(()=>openLiveBidAnchor({runtimeDir:rt,cohort:'actual-test',now:()=>base+n,workspaceDir:rt}),/STOPPED/);
});
