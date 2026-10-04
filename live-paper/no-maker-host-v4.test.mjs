import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';import {Journal} from './transport-runtime.mjs';
import {createBookPublication} from './book-publication-v2.mjs';import {CANONICAL_NO_POLICY} from './canonical-no-capacity.mjs';
import {NO_MAKER_POLICY} from './no-maker-paper-v2.mjs';import {openNoMakerHostV4,noMakerCodeIdentityV4} from './no-maker-host-v4.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),zero=Date.parse('2026-10-03T23:00:00Z'),at=n=>new Date(zero+n).toISOString();
function fixture(t){const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-maker-host-v2-'));t.after(()=>fs.rmSync(runtimeDir,{recursive:true,force:true}));
 const universe={tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',exchanges:[{marketId:'381',exchangeId:'1070',option:'YES'},{marketId:'382',exchangeId:'1071',option:'YES'}]};
 const review=JSON.parse(fs.readFileSync(new URL('../data/canonical-capacity-bank-20261003-v1.json',import.meta.url))).review;
 const bank={schemaVersion:1,hostRevision:4,mode:'no_maker_trade_tape_paper',name:'maker-v2-test-only',cutoff:at(0),policy:NO_MAKER_POLICY,tournamentId:universe.tournamentId,reviewExpiresAt:review.expiresAt,
  legs:review.legs,review,capacityPolicy:CANONICAL_NO_POLICY,codeIdentity:noMakerCodeIdentityV4(),tapePrefixBytes:0,tapePrefixHash:sha(''),tapePrefixSequence:0,tapePrefixTailHash:''};
 const dir=path.join(runtimeDir,'no-maker-cohorts',bank.name);fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'bank.json'),JSON.stringify(bank));
 fs.writeFileSync(path.join(runtimeDir,'anonymous-trades.jsonl'),'');fs.writeFileSync(path.join(runtimeDir,'no-maker-active.json'),JSON.stringify({cohort:bank.name,bankHash:payloadHash(bank),hostRevision:4}));
 let clock=zero;const open=()=>openNoMakerHostV4({runtimeDir,now:()=>clock}),w=open(),publisher=createBookPublication({file:path.join(runtimeDir,'books'),universe,sessionId:'test-source',now:()=>clock}),source=new Journal(path.join(runtimeDir,'anonymous-trades.jsonl'));
 t.after(()=>publisher.close('STOP'));const set=n=>clock=zero+n,account={schemaVersion:1,positionsAt:at(500),ordersAt:at(500),excludedExchangeIds:[]};
 const publish=(marketId,n,bid=.815,ask=.835,context=account)=>{set(n);const exchangeId=marketId==='381'?'1070':'1071';if(marketId==='382'&&bid===.815){bid=.215;ask=.225;}
  const response={endpoint:'/markets/'+marketId+'/orderbook?tournamentId='+universe.tournamentId+'&depth=20',requestStartedAt:at(n-100),receivedAt:at(n-10),data:{marketId,contexts:[{type:'tournament',tournament:{id:universe.tournamentId},orderbook:{exchanges:[{exchangeId,option:'YES',asOf:{sequence:n,at:at(n-20)},bids:[{price:bid,quantity:10000}],asks:[{price:ask,quantity:10000}]}]}}]}};
  assert.equal(publisher.publishBook({marketId,response}),true);w.onBook(publisher.status(),marketId,context);};
 return{runtimeDir,dir,w,publisher,source,set,publish,universe,open,bank};
}

test('versioned accounting continuation binds original inventory/journal and consumes a late covered exit once',t=>{
 const f=fixture(t);f.publish('381',1000);f.publish('382',3000);f.set(10000);
 const page={schemaVersion:1,tournamentId:f.universe.tournamentId,marketId:'381',exchangeId:'1070',from:at(0),to:at(9999),receivedAt:at(10000),requestCursor:null,pagination:{hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:10000},trades:[{key:'buy',id:'1',createdAt:at(7000),yesPrice:'0.84',size:1,executedSpace:'YES'}]};
 f.source.commit({fixtureOnly:true},page);f.w.onPage({...f.source.last,hash:f.source.previous});f.publish('381',11000,.815,.825);
 const bytes=fs.readFileSync(path.join(f.dir,'journal.jsonl')),rows=bytes.toString().trimEnd().split('\n').map(JSON.parse),seed=rows.at(-1).state.paper,seedBytes=Buffer.from(JSON.stringify(seed));
 const originalBank=fs.readFileSync(path.join(f.dir,'bank.json')),tape=fs.readFileSync(path.join(f.runtimeDir,'anonymous-trades.jsonl'));
 const nextBank={...f.bank,name:'maker-continuation-test',cutoff:at(18000),tapePrefixBytes:tape.length,tapePrefixHash:sha(tape),tapePrefixSequence:f.source.sequence,tapePrefixTailHash:f.source.previous,
  accountingContinuation:{cohort:f.bank.name,bankHash:payloadHash(f.bank),bankFileHash:sha(originalBank),journalPrefixBytes:bytes.length,journalPrefixHash:sha(bytes),openingStateFileHash:sha(seedBytes),accountingCarried:true}};
 const dir=path.join(f.runtimeDir,'no-maker-cohorts',nextBank.name);fs.mkdirSync(dir);fs.writeFileSync(path.join(dir,'bank.json'),JSON.stringify(nextBank));fs.writeFileSync(path.join(dir,'opening-state.json'),seedBytes);
 fs.writeFileSync(path.join(f.runtimeDir,'no-maker-active.json'),JSON.stringify({cohort:nextBank.name,bankHash:payloadHash(nextBank),hostRevision:4}));f.set(18000);
 fs.writeFileSync(path.join(dir,'opening-state.json'),JSON.stringify({...seed,nextId:999}));assert.throws(()=>f.open(),/CONTINUATION_SEED/);fs.writeFileSync(path.join(dir,'opening-state.json'),seedBytes);
 const extra={...page,to:at(17000),receivedAt:at(17500),trades:[]};f.source.commit({fixtureOnly:true},extra);const more=fs.readFileSync(path.join(f.runtimeDir,'anonymous-trades.jsonl'));const badBank={...nextBank,tapePrefixBytes:more.length,tapePrefixHash:sha(more),tapePrefixSequence:f.source.sequence,tapePrefixTailHash:f.source.previous};fs.writeFileSync(path.join(dir,'bank.json'),JSON.stringify(badBank));fs.writeFileSync(path.join(f.runtimeDir,'no-maker-active.json'),JSON.stringify({cohort:badBank.name,bankHash:payloadHash(badBank),hostRevision:4}));assert.throws(()=>f.open(),/CONTINUATION_FRONTIER/,'a committed but unconsumed page cannot be silently sealed');fs.writeFileSync(path.join(f.runtimeDir,'anonymous-trades.jsonl'),tape);f.source.sequence--;f.source.previous=nextBank.tapePrefixTailHash;fs.writeFileSync(path.join(dir,'bank.json'),JSON.stringify(nextBank));fs.writeFileSync(path.join(f.runtimeDir,'no-maker-active.json'),JSON.stringify({cohort:nextBank.name,bankHash:payloadHash(nextBank),hostRevision:4}));
 const h=f.open(),r=h.status();assert.equal(r.wallets.at_quote.cash,'99.835000');assert.equal(r.wallets.at_quote.fills,1);assert.equal(r.wallets.at_quote.positions[0].enteredAt,at(10000));assert.equal(r.wallets.at_quote.estimatedLiquidationPnl,null);
 assert.ok(r.wallets.at_quote.unresolvedQuotes.every(q=>Date.parse(q.expiresAt)<=Date.parse(nextBank.cutoff)));
 f.set(20000);f.source.commit({fixtureOnly:true},{...page,to:at(19999),receivedAt:at(20000),trades:[{key:'late-exit',id:'2',createdAt:at(16000),yesPrice:'0.81',size:1,executedSpace:'NO'}]});
 h.onPage({...f.source.last,hash:f.source.previous});assert.equal(h.status().wallets.at_quote.fills,2);assert.equal(h.status().wallets.at_quote.cash,'100.020000');assert.deepEqual(f.open().status(),h.status());
 assert.deepEqual(fs.readFileSync(path.join(f.dir,'journal.jsonl')),bytes,'original experiment remains byte-for-byte intact');
});
test('both receipt orders reconsider both legs only after the coherent pair is available',t=>{
 for(const order of [['381','382'],['382','381']]){const f=fixture(t);f.publish(order[0],1000);assert.deepEqual(f.w.status().riskMarketIds,[]);
  f.publish(order[1],3000);const r=f.w.status();assert.deepEqual(r.riskMarketIds,['381','382']);assert.equal(r.lastAdmission.entryAllowed,true);
  for(const w of Object.values(r.wallets)){assert.equal(w.unresolvedQuotes.length,2);assert.ok(w.unresolvedQuotes.every(q=>q.createdAt===at(3000)&&q.activationAt===at(6000)),'no backdated quote after first receipt');}
  assert.equal(r.wallets.at_quote.reserved,'0.940000');assert.equal(r.wallets.improve_one_tick.reserved,'0.945000');
 }
});
test('one-tick second-leg spread cannot suppress an eligible first-leg quote; no duplicate reservations',t=>{
 const f=fixture(t);f.publish('381',1000);f.publish('382',3000,.215,.220);const r=f.w.status();assert.deepEqual(r.riskMarketIds,['381']);
 for(const w of Object.values(r.wallets))assert.equal(w.unresolvedQuotes.length,1);f.set(4000);f.w.onBook(f.publisher.status(),'382',{schemaVersion:1,positionsAt:at(500),ordersAt:at(500),excludedExchangeIds:[]});
 assert.equal(f.w.status().wallets.at_quote.unresolvedQuotes.length,1);assert.equal(f.w.status().wallets.at_quote.reserved,r.wallets.at_quote.reserved);
});
test('stale pair and account exposure still prohibit both entry legs',t=>{
 const f=fixture(t);f.publish('381',1000);f.publish('382',12000);assert.equal(f.w.status().lastAdmission.reason,'whole_window_skew');assert.deepEqual(f.w.status().riskMarketIds,[]);
 const g=fixture(t);g.publish('381',1000);g.publish('382',3000,.215,.225,{schemaVersion:1,positionsAt:at(500),ordersAt:at(500),excludedExchangeIds:['1070']});assert.deepEqual(g.w.status().riskMarketIds,[]);assert.equal(g.w.status().lastAdmission.reason,'own_account_exposure');
});
test('first-leg early prints do not fill paired-clock quotes; exact restart/suffix recovery retains first-leg inventory',t=>{
 const f=fixture(t);f.publish('381',1000);f.publish('382',3000);f.set(10000);
 const page={schemaVersion:1,tournamentId:f.universe.tournamentId,marketId:'381',exchangeId:'1070',from:at(0),to:at(9999),receivedAt:at(10000),requestStartedAt:at(9999),pagination:{limit:200,hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:10000},trades:[{key:'early',id:'1',createdAt:at(4000),yesPrice:'0.845',size:1,executedSpace:'YES'}]};
 f.source.commit({fixtureOnly:true},page);f.w.onPage({...f.source.last,hash:f.source.previous});assert.equal(f.w.status().wallets.at_quote.fills,0);assert.equal(f.w.status().lastAdmission.entryAllowed,true,'trade pages retain the latest admission diagnostics');
 f.set(11000);f.source.commit({fixtureOnly:true},{...page,to:at(10999),receivedAt:at(11000),trades:[{key:'later',id:'2',createdAt:at(7000),yesPrice:'0.84',size:1,executedSpace:'NO'}]});
 const restored=f.open(),r=restored.status();assert.equal(r.wallets.at_quote.fills,1);assert.equal(r.wallets.at_quote.positions[0].cost,'0.165000');assert.equal(r.wallets.at_quote.positions[0].enteredAt,at(11000));assert.equal(r.wallets.at_quote.realizedPnl,'0.000000');assert.deepEqual(f.open().status(),r);
 fs.writeFileSync(path.join(f.dir,'journal.jsonl'),'');assert.throws(()=>f.open(),/PUBLISHED_JOURNAL_ROLLBACK/);
});
