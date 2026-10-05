import test from 'node:test';import assert from 'node:assert/strict';
import {STABLE_MAKER_POLICY,createStableMakerState,advanceStableMaker,assertStableMaker,stableMakerReport} from './stable-maker-paper-v2.mjs';
const zero=Date.parse('2026-10-05T13:00:00Z'),at=n=>new Date(zero+n).toISOString();
const bank={schemaVersion:1,mode:'stable_two_side_trade_tape_paper_v2',name:'correctness-only',cutoff:at(0),entryExpiresAt:at(86400000),policy:STABLE_MAKER_POLICY,tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',legs:[{marketId:'381',exchangeId:'1070'}]};
const book=(n,changes={})=>({tournamentId:bank.tournamentId,marketId:'381',exchangeId:'1070',instrument:'1070:YES',requestStartedAt:at(n-100),receivedAt:at(n),sourceAt:at(n-50),sequence:String(n),versionKey:n+':'+at(n-50),publicationHash:'a'.repeat(64),publicationSequence:n,bids:[{price:'0.800000',quantity:1000}],asks:[{price:'0.825000',quantity:1000}],...changes});
const account=n=>({schemaVersion:1,positionsAt:at(n-200),ordersAt:at(n-200),excludedExchangeIds:[]});
const be=(n,changes={})=>({kind:'book',availableAt:at(n),book:book(n),accountEligibility:account(n),...changes});
const trade=(key,n,price)=>({key,id:key,createdAt:at(n),yesPrice:String(Number(price)),size:1,executedSpace:'YES'});
const pe=(n,trades,changes={})=>({kind:'trades',availableAt:at(n),page:{marketId:'381',exchangeId:'1070',tournamentId:bank.tournamentId,receivedAt:at(n),from:at(0),to:at(n-1),requestCursor:null,pagination:{hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:100000000},trades,...changes}});
const warmed=()=>[1000,61000,121000,181000,241000].reduce((s,n)=>advanceStableMaker(s,be(n),bank),createStableMakerState(bank));

test('a stable observed window enables inside quotes, while changed odds and source gaps prevent entry',()=>{
 let s=createStableMakerState(bank);for(const n of[1000,61000,121000,181000])s=advanceStableMaker(s,be(n),bank);assert.equal(Object.keys(s.wallets.YES.quotes).length,0);
 const ready=advanceStableMaker(s,be(241000),bank);assert.equal(ready.wallets.YES.quotes[1].limit,'0.800000');assert.equal(ready.wallets.NO.quotes[2].limit,'0.175000');
 const drift=advanceStableMaker(s,be(241000,{book:book(241000,{bids:[{price:'0.810000',quantity:1000}],asks:[{price:'0.835000',quantity:1000}]})}),bank);assert.equal(Object.keys(drift.wallets.YES.quotes).length,0);
 const gap=advanceStableMaker(s,be(400000),bank);assert.equal(Object.keys(gap.wallets.YES.quotes).length,0);assert.equal(gap.counts.history_gap,1);
});
test('both canonical sides require strict through prices; touches and pre-activation prints do not fill',()=>{
 const start=warmed();const touch=advanceStableMaker(start,pe(250000,[trade('early',242000,'0.800'),trade('yes-touch',246000,'0.800'),trade('no-touch',247000,'0.825')]),bank);
 assert.equal(touch.wallets.YES.fills.length,0);assert.equal(touch.wallets.NO.fills.length,0);
 const filled=advanceStableMaker(touch,pe(260000,[trade('yes-buy',251000,'0.795'),trade('no-buy',252000,'0.830')]),bank);
 assert.equal(filled.wallets.YES.cash,'99.200000');assert.equal(filled.wallets.NO.cash,'99.825000');assertStableMaker(filled,bank);
 const duplicate=advanceStableMaker(filled,pe(270000,[trade('yes-buy',251000,'0.795'),trade('no-buy',252000,'0.830')]),bank);assert.equal(duplicate.wallets.YES.fills.length,1);
});
test('covered passive round trips realize exact zero-fee FIFO only after an independently later print',()=>{
 let s=advanceStableMaker(warmed(),pe(260000,[trade('yes-buy',251000,'0.795'),trade('no-buy',252000,'0.830')]),bank);s=advanceStableMaker(s,be(270000),bank);
 assert.equal(Object.values(s.wallets.YES.quotes).at(-1).limit,'0.805000');assert.equal(Object.values(s.wallets.NO.quotes).at(-1).limit,'0.180000');
 s=advanceStableMaker(s,pe(290000,[trade('yes-sell',280000,'0.820'),trade('no-sell',281000,'0.805')]),bank);assert.equal(s.wallets.YES.realized,'0.005000');assert.equal(s.wallets.NO.realized,'0.005000');
 const r=stableMakerReport(s,bank,at(290000));assert.equal(r.wallets.YES.cash,'100.005000');assert.equal(r.wallets.YES.fees,'0.000000');assert.equal(r.profitabilityEstablished,false);
});
test('expiry alone cannot free a reservation; missing intervals and insufficient old checkpoints remain blocked',()=>{
 let s=warmed();s=advanceStableMaker(s,be(370000),bank);assert.equal(s.wallets.YES.quotes[1].coverageBarrierSequence,'370000');
 s=advanceStableMaker(s,pe(400000,[],{from:at(300000),to:at(399999)}),bank);assert.equal(stableMakerReport(s,bank,at(400000)).wallets.YES.reserved,'0.800000');
 s=advanceStableMaker(s,pe(410000,[],{from:at(244000),to:at(300000),coverage:{complete:true,projectedThroughSequence:1000}}),bank);assert.equal(stableMakerReport(s,bank,at(410000)).wallets.YES.reserved,'0.800000');
 s=advanceStableMaker(s,pe(420000,[],{from:at(244000),to:at(300000)}),bank);assert.equal(stableMakerReport(s,bank,at(420000)).wallets.YES.reserved,'0.000000');
});
test('pending resting exits prevent a duplicate risk sale, including late pre-expiry fills',()=>{
 let s=advanceStableMaker(warmed(),pe(260000,[trade('yes-buy',251000,'0.795')]),bank);s=advanceStableMaker(s,be(270000),bank);
 const bad=n=>be(n,{book:book(n,{bids:[{price:'0.765000',quantity:1000}],asks:[{price:'0.785000',quantity:1000}]})});
 s=advanceStableMaker(s,bad(280000),bank);s=advanceStableMaker(s,bad(290000),bank);assert.equal(s.wallets.YES.fills.length,1);assert.equal(s.wallets.YES.quotes[3].expiresAt,at(390000));
 s=advanceStableMaker(s,pe(410000,[trade('late-sell',300000,'0.820')]),bank);s=advanceStableMaker(s,bad(420000),bank);assert.equal(s.wallets.YES.fills.length,2);assert.equal(s.wallets.YES.realized,'0.005000');
});
test('a latched risk exit survives restart, expired entry window and price recovery after projection resolution',()=>{
 const expired={...bank,entryExpiresAt:at(300000)};let s=createStableMakerState(expired);for(const n of[1000,61000,121000,181000,241000])s=advanceStableMaker(s,be(n),expired);
 s=advanceStableMaker(s,pe(260000,[trade('yes-buy',251000,'0.795')]),expired);const bad=n=>be(n,{book:book(n,{bids:[{price:'0.765000',quantity:1000}],asks:[{price:'0.785000',quantity:1000}]})});
 s=advanceStableMaker(s,bad(270000),expired);s=JSON.parse(JSON.stringify(s));s=advanceStableMaker(s,be(280000),expired);
 assert.equal(s.wallets.YES.realized,'-0.005000');assert.equal(s.wallets.YES.positions['1070'],undefined);assert.equal(s.wallets.YES.risk['1070'],undefined);
});
test('account exclusion invalidates marks and prevents adopting account inventory or creating maker quotes',()=>{
 const s=warmed(),excluded=advanceStableMaker(s,be(250000,{accountEligibility:{...account(250000),excludedExchangeIds:['1070']}}),bank);assert.equal(excluded.books['1070'],undefined);assert.equal(excluded.wallets.YES.fills.length,0);
 let held=advanceStableMaker(s,pe(260000,[trade('yes-buy',251000,'0.795')]),bank);held=advanceStableMaker(held,be(270000,{accountEligibility:null}),bank);assert.equal(stableMakerReport(held,bank,at(270000)).wallets.YES.estimatedLiquidationPnl,null);
 assert.throws(()=>advanceStableMaker(held,be(250000),bank),/CLOCK/);
});
