import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import path from 'node:path';import os from 'node:os';
import {payloadHash} from '../execution/ledger.mjs';
import {digest} from './model.mjs';
import {readFundamentalEntryDrain,fundamentalDrainAdmission,wrapFundamentalEntryDrain} from './fundamental-entry-drain.mjs';
const bank={name:'fixture-fundamental',research:[{marketId:'195'}]},held=()=>({failed:false,sourceBytes:0,sourceSequence:0,sourceHash:'a'.repeat(64),wallets:{1000:{positions:[{marketId:'195',quantity:1,cost:'0.295000'}],pending:[]},3000:{positions:[{marketId:'195',quantity:1,cost:'0.295000'}],pending:[]}}}),row={kind:'book',payload:{book:{marketId:'195'}}};
test('all latency wallets must own their sole share; pending buy, divergence or multi-book batch cannot consume',()=>{
 assert.equal(fundamentalDrainAdmission(held(),bank,[row]).allowed,true);assert.equal(fundamentalDrainAdmission(held(),bank,[row,row]).allowed,false);
 for(const change of [s=>s.wallets[1000].positions=[],s=>s.wallets[1000].pending=[{marketId:'195',action:'buy'}],s=>s.failed=true,s=>s.wallets[3000].positions[0].quantity=2]){const s=held();change(s);assert.equal(fundamentalDrainAdmission(s,bank,[row]).allowed,false);}
 const sell=held();sell.wallets[1000].pending=[{marketId:'195',action:'sell'}];assert.equal(fundamentalDrainAdmission(sell,bank,[row]).allowed,true);
});
test('operator marker is exact bank/cohort/source bound and grants no trading authority',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-fundamental-drain-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const file=path.join(dir,'FUNDAMENTAL_ENTRY_DRAIN');assert.equal(readFundamentalEntryDrain(dir,bank),null);
 const m={schemaVersion:1,owner:'Codex',cohort:bank.name,bankHash:digest(bank),at:'2026-10-05T21:00:00Z',reasonCode:'CONTRARY_PRIMARY_RESEARCH',sourceEvidenceHashes:['b'.repeat(64)]};fs.writeFileSync(file,JSON.stringify(m));assert.equal(readFundamentalEntryDrain(dir,bank).markerHash,payloadHash(m));
 for(const change of [{bankHash:'c'.repeat(64)},{cohort:'other'},{owner:'agent'},{sourceEvidenceHashes:[]},{ordersEnabled:true}]){fs.writeFileSync(file,JSON.stringify({...m,...change}));assert.throws(()=>readFundamentalEntryDrain(dir,bank),/INVALID/);}
});
test('wrapper preserves every wallet and host identity, continues held exit receipts then freezes flat/divergence and persists separate evidence',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-drain-host-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const original=held();let calls=0,published=0;fs.writeFileSync(path.join(dir,'public-books.jsonl'),JSON.stringify(row)+'\n');
 const host={dir,bank,status:()=>structuredClone(original),consume(){calls++;original.sourceBytes=fs.statSync(path.join(dir,'public-books.jsonl')).size;original.sourceSequence++;return 1;},publish(){published++;},close(){}};
 const m={schemaVersion:1,owner:'Codex',cohort:bank.name,bankHash:digest(bank),at:'2026-10-05T21:00:00Z',reasonCode:'CONTRARY_PRIMARY_RESEARCH',sourceEvidenceHashes:['b'.repeat(64)]};fs.writeFileSync(path.join(dir,'FUNDAMENTAL_ENTRY_DRAIN'),JSON.stringify(m));
 const wrapped=wrapFundamentalEntryDrain(host,{runtimeDir:dir,now:()=>Date.parse(m.at)});assert.equal(wrapped.bank,bank);assert.equal(wrapped.consume(),1);assert.equal(calls,1);assert.equal(wrapped.status().operatorEntryDrain.mode,'held_exit_only');
 original.wallets[1000].positions=[];original.wallets[1000].pending=[{action:'buy',marketId:'195',limit:'0.295000'}];const before=JSON.stringify(original);fs.appendFileSync(path.join(dir,'public-books.jsonl'),JSON.stringify(row)+'\n');assert.equal(wrapped.consume(),0);assert.equal(calls,1);assert.equal(JSON.stringify(original),before);assert.equal(wrapped.status().operatorEntryDrain.continuationRequired,true);wrapped.publish();assert.equal(published,1);
 assert(fs.existsSync(path.join(dir,'operator-entry-drain-status.json')));const records=fs.readFileSync(path.join(dir,'operator-entry-drain.jsonl'),'utf8').trim().split('\n').map(JSON.parse);assert.equal(records.length,2);assert.equal(records[1].previous,records[0].hash);
 fs.unlinkSync(path.join(dir,'FUNDAMENTAL_ENTRY_DRAIN'));assert.equal(wrapped.entryDraining(),true);const resumed=wrapFundamentalEntryDrain(host,{runtimeDir:dir,now:()=>Date.parse(m.at)});assert.equal(resumed.consume(),0);assert.equal(resumed.status().operatorEntryDrain.continuationRequired,true);
});
