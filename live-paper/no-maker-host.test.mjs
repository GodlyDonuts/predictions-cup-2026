import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';import {Journal} from './transport-runtime.mjs';
import {createBookPublication} from './book-publication-v2.mjs';import {CANONICAL_NO_POLICY} from './canonical-no-capacity.mjs';
import {NO_MAKER_POLICY} from './no-maker-paper.mjs';import {openNoMakerHost,noMakerCodeIdentity} from './no-maker-host.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),zero=Date.parse('2026-10-03T23:00:00Z'),at=n=>new Date(zero+n).toISOString();
function fixture(t){const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-maker-host-'));t.after(()=>fs.rmSync(runtimeDir,{recursive:true,force:true}));
 const universe={tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',exchanges:[{marketId:'381',exchangeId:'1070',option:'YES'},{marketId:'382',exchangeId:'1071',option:'YES'}]};
 const review=JSON.parse(fs.readFileSync(new URL('../data/canonical-capacity-bank-20261003-v1.json',import.meta.url))).review;
 const bank={schemaVersion:1,mode:'no_maker_trade_tape_paper',name:'maker-test-only',cutoff:at(0),policy:NO_MAKER_POLICY,tournamentId:universe.tournamentId,reviewExpiresAt:review.expiresAt,
  legs:review.legs,review,capacityPolicy:CANONICAL_NO_POLICY,codeIdentity:noMakerCodeIdentity(),tapePrefixBytes:0,tapePrefixHash:sha(''),tapePrefixSequence:0,tapePrefixTailHash:''};
 const dir=path.join(runtimeDir,'no-maker-cohorts',bank.name);fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'bank.json'),JSON.stringify(bank));
 fs.writeFileSync(path.join(runtimeDir,'anonymous-trades.jsonl'),'');fs.writeFileSync(path.join(runtimeDir,'no-maker-active.json'),JSON.stringify({cohort:bank.name,bankHash:payloadHash(bank)}));
 let clock=zero;const w=openNoMakerHost({runtimeDir,now:()=>clock}),publisher=createBookPublication({file:path.join(runtimeDir,'books'),universe,sessionId:'test-source',now:()=>clock}),source=new Journal(path.join(runtimeDir,'anonymous-trades.jsonl'));
 t.after(()=>publisher.close('STOP'));
 const set=n=>clock=zero+n;
 const publish=(marketId,exchangeId,n,bid,ask)=>{set(n);const response={endpoint:'/markets/'+marketId+'/orderbook?tournamentId='+universe.tournamentId+'&depth=20',requestStartedAt:at(n-100),receivedAt:at(n-10),data:{marketId,contexts:[{type:'tournament',tournament:{id:universe.tournamentId},orderbook:{exchanges:[{exchangeId,option:'YES',asOf:{sequence:n,at:at(n-20)},bids:[{price:bid,quantity:10000}],asks:[{price:ask,quantity:10000}]}]}}]}};assert.equal(publisher.publishBook({marketId,response}),true);};
 return{runtimeDir,dir,w,publisher,source,set,publish,universe,open:()=>openNoMakerHost({runtimeDir,now:()=>clock})};
}
test('real publisher, durable tape callback, restart/suffix recovery and protected risk projection preserve original wallet',t=>{
 const f=fixture(t),account={schemaVersion:1,positionsAt:at(500),ordersAt:at(500),excludedExchangeIds:[]};
 f.publish('381','1070',1000,.815,.835);f.w.onBook(f.publisher.status(),'381',account);f.publish('382','1071',3000,.215,.225);f.w.onBook(f.publisher.status(),'382',account);
 assert.deepEqual(f.w.status().riskMarketIds,['382']);f.set(20000);
 const page={schemaVersion:1,tournamentId:f.universe.tournamentId,marketId:'382',exchangeId:'1071',from:at(0),to:at(19000),receivedAt:at(20000),requestStartedAt:at(19000),pagination:{limit:200,hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:20000},trades:[{key:'stable-trade',id:'9',createdAt:at(10000),yesPrice:'0.22999999999999998',size:1,executedSpace:'NO'}]};
 f.source.commit({fixtureOnly:true},page);f.w.onPage({...f.source.last,hash:f.source.previous});const before=f.w.status();assert.equal(before.wallets.at_quote.fills,1);assert.equal(before.wallets.at_quote.positions[0].cost,'0.775000');
 const restored=f.open();assert.deepEqual(restored.status(),before);f.set(25000);f.source.commit({fixtureOnly:true},{...page,receivedAt:at(25000)});
 const catchup=f.open();assert.equal(catchup.status().tapeSequence,2);assert.equal(catchup.status().wallets.at_quote.fills,1);assert.equal(catchup.status().wallets.at_quote.cash,before.wallets.at_quote.cash);
 fs.writeFileSync(path.join(f.dir,'journal.jsonl'),'');assert.throws(()=>f.open(),/PUBLISHED_JOURNAL_ROLLBACK/);
});
test('frozen-bank replacement cannot silently create a new experiment',t=>{
 const f=fixture(t),file=path.join(f.dir,'bank.json'),bank=JSON.parse(fs.readFileSync(file));bank.policy={...bank.policy,initialCash:'200.000000'};fs.writeFileSync(file,JSON.stringify(bank));assert.throws(()=>f.open(),/FROZEN_IDENTITY/);
});
