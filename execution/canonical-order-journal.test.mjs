import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import test from 'node:test';import assert from 'node:assert/strict';
import {openCanonicalOrderJournal} from './canonical-order-journal.mjs';
const tid='00000000-0000-4000-8000-000000000001',at='2026-10-02T23:00:00.000Z';
const preparation=()=>({intentId:'paired-1-first',idempotencyKey:'immutable-key-1',binding:{ownerId:'Codex',marketId:'381',exchangeId:'1070',tournamentId:tid,side:'no',action:'buy',quantity:6,limit:'0.200000',createdAt:at,expirationDate:'2026-10-02T23:00:30.000Z'},wallet:{ownerId:'Codex',marketId:'381',exchangeId:'1070',tournamentId:tid,cash:'100.000000',realizedPnl:'0.000000',accountSignedQuantity:0,lots:[]}});
const evidence=(filled=2)=>({ack:{orderId:1001,exchangeId:'1070',side:'no',action:'buy',quantity:6,price:'0.200000',all:null,totalCost:(filled*.2).toFixed(6),quantityTraded:filled,remainingQuantity:6-filled,open:filled<6},rows:[{id:2001,orderId:1001,marketId:'381',exchangeId:'1070',tournamentId:tid,side:'no',quantity:-2,price:.8,filledAt:'2026-10-02T23:00:01.000Z'},...(filled>2?[{id:2002,orderId:1001,marketId:'381',exchangeId:'1070',tournamentId:tid,side:'no',quantity:-(filled-2),price:.8,filledAt:'2026-10-02T23:00:02.000Z'}]:[])],source:'realtime_account_batch',readAt:'2026-10-02T23:00:03.000Z'});
function fixture(t){const dir=fs.mkdtempSync(path.join(os.tmpdir(),'canonical-order-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));return {dir,file:path.join(dir,'canonical-order.jsonl'),open:o=>openCanonicalOrderJournal({dir,...o})};}
function start(f){const w=f.open();w.prepare(preparation());w.markDispatch();w.bind('1001');return w;}
test('restart retains original baseline and extends partial fills once',t=>{
 const f=fixture(t);let w=start(f);const p=w.observe(evidence());assert.equal(p.projection.wallet.cash,'99.600000');const prefix=fs.readFileSync(f.file);w.close();w=f.open();
 const full=w.observe(evidence(6));assert.equal(full.wallet.cash,'100.000000');assert.equal(full.projection.wallet.cash,'98.800000');assert.equal(full.projection.wallet.accountSignedQuantity,-6);assert.equal(full.phase,'terminal');
 assert(fs.readFileSync(f.file).subarray(0,prefix.length).equals(prefix));const seq=w.status().sequence;w.observe(evidence(6));assert.equal(w.status().sequence,seq);w.close();
 w=f.open({readOnly:true});assert.equal(w.status().state.projection.projectionHash,full.projection.projectionHash);assert.equal(w.status().executionAuthority,false);w.close();
});
test('preparation and uncertain order identity cannot be replaced',t=>{
 const f=fixture(t),w=f.open();w.prepare(preparation());const size=fs.statSync(f.file).size;w.prepare(preparation());assert.equal(fs.statSync(f.file).size,size);const changed=preparation();changed.idempotencyKey='replacement';assert.throws(()=>w.prepare(changed),/PREPARATION_CHANGED/);w.markDispatch();w.uncertain('ORDER_STATUS_UNKNOWN');w.close();const v=f.open();assert.equal(v.status().state.phase,'uncertain');assert.throws(()=>v.markDispatch(),/DISPATCH_PHASE/);const state=v.status().state;assert.throws(()=>v.retry({requestHash:state.requestHash,idempotencyKey:'replacement'}),/RETRY_CHANGED/);v.retry({requestHash:state.requestHash,idempotencyKey:state.idempotencyKey});assert.equal(v.status().state.attempts,2);v.bind('1001');assert.throws(()=>v.bind('1002'),/ORDER_ID_CHANGED/);v.close();
});
test('fill removal, changed ACK, changed fill and clock regressions leave journal intact',t=>{
 const f=fixture(t),w=start(f);w.observe(evidence());const before=fs.readFileSync(f.file);for(const mutate of [x=>x.rows=[],x=>x.rows[0].price=.805,x=>x.ack.price='0.195000',x=>x.readAt=at,x=>x.source='rest_order_fills',x=>x.ack.quantityTraded=1]){const x=evidence(6);mutate(x);assert.throws(()=>w.observe(x));assert(fs.readFileSync(f.file).equals(before));}w.close();
});
test('terminal projection cannot reopen or introduce another fill',t=>{
 const f=fixture(t),w=start(f);w.observe(evidence(6));assert.throws(()=>w.observe(evidence()),/REGRESSION|TERMINAL/);const x=evidence(6);x.rows.push({...x.rows[0],id:2003});assert.throws(()=>w.observe(x),/TERMINAL_CHANGED/);w.close();
});
test('STOP, read-only mode and duplicate writer block mutation, including no-op paths',t=>{
 const f=fixture(t);let enabled=true;const w=f.open({canCommit:()=>enabled});w.prepare(preparation());assert.throws(()=>f.open(),/EEXIST/);enabled=false;assert.throws(()=>w.prepare(preparation()),/STOP/);assert.throws(()=>w.markDispatch(),/STOP/);w.close();const ro=f.open({readOnly:true});assert.throws(()=>ro.prepare(preparation()),/UNWRITABLE/);ro.close();assert.throws(()=>ro.markDispatch(),/UNWRITABLE/);
});
test('same-size journal rewrite, truncation and partial tail fail without repair',t=>{
 const f=fixture(t),w=start(f),before=fs.readFileSync(f.file);const changed=Buffer.from(before);changed[0]=32;fs.writeFileSync(f.file,changed);assert.throws(()=>w.bind('1001'),/CHANGED/);w.close();assert.throws(()=>f.open(),/CHAIN|Unexpected|JSON/);fs.writeFileSync(f.file,Buffer.concat([before,Buffer.from('{')]));assert.throws(()=>f.open(),/PARTIAL/);assert.equal(fs.readFileSync(f.file).at(-1),123);
});
test('fsync failure fences live writer; restart replays complete appended event exactly once',t=>{
 const f=fixture(t),w=start(f),original=fs.fsyncSync;try{fs.fsyncSync=()=>{throw Error('injected durability failure');};assert.throws(()=>w.observe(evidence()),/durability/);}finally{fs.fsyncSync=original;}
 assert.throws(()=>w.observe(evidence()),/UNWRITABLE/);assert.equal(w.status().unconfirmedPersistence,true);w.close();const v=f.open();assert.equal(v.status().state.projection.wallet.cash,'99.600000');const seq=v.status().sequence;v.observe(evidence());assert.equal(v.status().sequence,seq);v.close();
});
test('unknown fields and unattributed holdings cannot enter restricted baseline or receipts',t=>{
 const f=fixture(t),w=f.open();for(const mutate of [x=>x.wallet.secret='do-not-persist',x=>x.wallet.accountSignedQuantity=3,x=>x.binding.orderId='1001',x=>x.binding.expirationDate='2026-10-03T23:00:00.000Z']){const x=preparation();mutate(x);assert.throws(()=>w.prepare(x));assert.equal(fs.statSync(f.file).size,0);}w.prepare(preparation());w.markDispatch();w.bind('1001');const e=evidence();e.ack.headers={};assert.throws(()=>w.observe(e),/SHAPE/);assert(!fs.readFileSync(f.file,'utf8').includes('do-not-persist'));w.close();
});
test('lock replaced by another writer is preserved rather than deleted',t=>{
 const f=fixture(t),w=f.open();fs.writeFileSync(path.join(f.dir,'canonical-order.lock'),'other owner');assert.throws(()=>w.prepare(preparation()),/LOCK_CHANGED/);assert.throws(()=>w.close(),/LOCK_CHANGED/);assert.equal(fs.readFileSync(path.join(f.dir,'canonical-order.lock'),'utf8'),'other owner');
});
