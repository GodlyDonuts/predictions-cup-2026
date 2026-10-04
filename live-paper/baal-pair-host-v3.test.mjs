import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';import {createBookPublication} from './book-publication-v2.mjs';
import {openBaalPairHost,baalPairCodeIdentity} from './baal-pair-host-v3.mjs';import {BAAL_PAIR_POLICY} from './baal-pair-paper-v2.mjs';
import {CANONICAL_NO_POLICY} from './canonical-no-capacity.mjs';
const base=Date.parse('2026-10-04T02:00:00.000Z'),at=n=>new Date(base+n).toISOString(),sha=x=>createHash('sha256').update(x).digest('hex');
function fixture(t){
 const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-pair-host-'));t.after(()=>fs.rmSync(runtimeDir,{recursive:true,force:true}));
 const original=JSON.parse(fs.readFileSync(new URL('../data/canonical-capacity-bank-20261003-v1.json',import.meta.url))),universe={tournamentId:original.tournamentId,exchanges:original.review.legs.map(l=>({marketId:l.marketId,exchangeId:l.exchangeId,option:'YES'}))};
 let clock=base;const sourceFile=path.join(runtimeDir,'public-books.jsonl'),publisher=createBookPublication({file:sourceFile,universe,sessionId:'pair-test-source',now:()=>clock});
 t.after(()=>publisher.close('STOP'));const source=fs.readFileSync(sourceFile),prefix=publisher.status();
 original.review.refundPolicy={schemaVersion:1,representation:'held_purchase_principal_not_face_value',feeRefundAssumed:false,sourceHash:'a'.repeat(64)}; original.review.refundScenarios=[{id:'both_refunded',outcomes:['refund','refund']},{id:'first_refunded_second_loses',outcomes:['refund',0]},{id:'second_refunded_first_loses',outcomes:[0,'refund']}];
 const bank={schemaVersion:1,mode:'baal_sequential_no_pair_paper',name:'baal-test-pair',cutoff:at(0),policy:BAAL_PAIR_POLICY,
  review:{...original.review,recordedAt:at(-1000),expiresAt:at(3600000)},capacityPolicy:CANONICAL_NO_POLICY,codeIdentity:baalPairCodeIdentity(),
  sourcePrefixBytes:source.length,sourcePrefixHash:sha(source),sourcePrefixSequence:prefix.sequence,sourcePrefixTailHash:prefix.hash};
 const dir=path.join(runtimeDir,'baal-pair-cohorts',bank.name);fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'bank.json'),JSON.stringify(bank));
 fs.writeFileSync(path.join(runtimeDir,'baal-pair-active.json'),JSON.stringify({cohort:bank.name,bankHash:payloadHash(bank),hostRevision:3}));
 const open=()=>openBaalPairHost({runtimeDir,now:()=>clock});let host=open();
 const publish=(id,n,accountN=500)=>{clock=base+n;const marketId=id==='1070'?'381':'382',d=id==='1070';
  const response={endpoint:'/markets/'+marketId+'/orderbook?tournamentId='+universe.tournamentId+'&depth=20',requestStartedAt:at(n-100),receivedAt:at(n-10),data:{marketId,
   contexts:[{type:'tournament',tournament:{id:universe.tournamentId},orderbook:{exchanges:[{exchangeId:id,option:'YES',asOf:{sequence:n,at:at(n-20)},
    bids:[{price:d?.81:.22,quantity:1000}],asks:[{price:d?.82:.23,quantity:1000}]}]}}]}};
  assert.equal(publisher.publishBook({marketId,response}),true);host.onBook(publisher.status(),marketId,{schemaVersion:1,positionsAt:at(accountN),ordersAt:at(accountN),excludedExchangeIds:[]});};
 return{dir,runtimeDir,publish,open,publisher,get host(){return host;},restart(n){clock=base+n;host=open();return host;}};
}
test('verified publisher drives causal sequential fills and restart preserves the unmatched obligation',t=>{
 const f=fixture(t);f.publish('1070',1000);f.publish('1071',3000);assert.equal(f.host.status().fills,0);
 f.publish('1070',11000);f.publish('1071',14000);assert.equal(f.host.status().pending.stage,'entry_first');
 f.publish('1070',20000,18000);assert.equal(f.host.status().fills,1);assert.equal(f.host.status().pending.stage,'entry_second');
 const cash=f.host.status().cash;f.restart(21000);assert.equal(f.host.status().cash,cash);assert.equal(f.host.status().fills,1);
 assert.equal(f.host.status().package.stage,'orphan');assert.equal(f.host.status().pending,null);assert.equal(f.host.status().estimatedLiquidationPnl,null);
 const source=f.publisher.status();assert.throws(()=>f.host.onBook(source,'381',{schemaVersion:1,positionsAt:at(18000),ordersAt:at(18000),excludedExchangeIds:[]}),/SOURCE_FRONTIER/);
});
test('published journal rollback and bank/code changes prevent a reset or alternate strategy on restart',t=>{
 const f=fixture(t);f.publish('1070',1000);f.publish('1071',3000);
 const journal=path.join(f.dir,'journal.jsonl'),saved=fs.readFileSync(journal);fs.writeFileSync(journal,'');assert.throws(()=>f.open(),/PUBLISHED_ROLLBACK/);fs.writeFileSync(journal,saved);
 const bankFile=path.join(f.dir,'bank.json'),b=JSON.parse(fs.readFileSync(bankFile));b.policy.quantity=2;fs.writeFileSync(bankFile,JSON.stringify(b));assert.throws(()=>f.open(),/FROZEN_IDENTITY/);
});
