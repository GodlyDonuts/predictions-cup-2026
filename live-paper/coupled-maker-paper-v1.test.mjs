import test from 'node:test';import assert from 'node:assert/strict';
import {COUPLED_MAKER_POLICY,createCoupledMakerState,advanceCoupledMaker,assertCoupledMaker,coupledMakerReport} from './coupled-maker-paper-v1.mjs';
const zero=Date.parse('2026-10-05T15:00:00Z'),at=n=>new Date(zero+n).toISOString();
const bank={schemaVersion:1,mode:'coupled_two_side_trade_tape_paper_v1',name:'correctness-only',cutoff:at(0),entryExpiresAt:at(86400000),policy:COUPLED_MAKER_POLICY,tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',legs:[{marketId:'381',exchangeId:'1070'}]};
const book=(n,bid='0.800000',ask='0.825000')=>({tournamentId:bank.tournamentId,marketId:'381',exchangeId:'1070',instrument:'1070:YES',requestStartedAt:at(n-100),receivedAt:at(n),sourceAt:at(n-50),sequence:String(n),versionKey:n+':'+at(n-50),publicationHash:'a'.repeat(64),publicationSequence:n,bids:[{price:bid,quantity:1000}],asks:[{price:ask,quantity:1000}]});
const be=(n,bid,ask)=>({kind:'book',availableAt:at(n),book:book(n,bid,ask),accountEligibility:{schemaVersion:1,positionsAt:at(n-200),ordersAt:at(n-200),excludedExchangeIds:[]}});
const trade=(key,n,price,size=1)=>({key,id:key,createdAt:at(n),yesPrice:String(Number(price)),size,executedSpace:'YES'});
const pe=(n,trades,changes={})=>({kind:'trades',availableAt:at(n),page:{marketId:'381',exchangeId:'1070',tournamentId:bank.tournamentId,receivedAt:at(n),from:at(0),to:at(n-1),requestCursor:null,pagination:{hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:100000000},trades,...changes}});
const warmed=()=>[1000,61000,121000,181000,241000].reduce((s,n)=>advanceCoupledMaker(s,be(n),bank),createCoupledMakerState(bank));
test('one cash wallet reserves both legs and a touch is never a fill',()=>{
 const s=warmed(),r=coupledMakerReport(s,bank,at(241000));assert.deepEqual(Object.keys(s.wallets),['COUPLED']);assert.equal(r.wallets.COUPLED.reserved,'0.975000');assert.equal(r.wallets.COUPLED.cash,'100.000000');
 const t=advanceCoupledMaker(s,pe(260000,[trade('touch',251000,'0.800'),trade('touch-no',252000,'0.825')]),bank);assert.equal(t.wallets.COUPLED.fills.length,0);assert.equal(t.wallets.COUPLED.realized,'0.000000');
});
test('opposite strict-through fills redeem once with exact cash, FIFO and zero fees',()=>{
 let s=advanceCoupledMaker(warmed(),pe(260000,[trade('yes',251000,'0.795')]),bank);assert.equal(s.wallets.COUPLED.cash,'99.200000');assert.equal(s.wallets.COUPLED.positions['1070'].side,'YES');
 s=JSON.parse(JSON.stringify(s));s=advanceCoupledMaker(s,pe(270000,[trade('no',252000,'0.830')]),bank);const w=s.wallets.COUPLED;assert.equal(w.cash,'100.025000');assert.equal(w.realized,'0.025000');assert.equal(w.cumulativeBuys,'0.975000');assert.equal(w.redemptions.length,1);assert.equal(w.fees,'0.000000');assert.deepEqual(w.positions,{});assertCoupledMaker(s,bank);
 const d=advanceCoupledMaker(s,pe(280000,[trade('yes',251000,'0.795'),trade('no',252000,'0.830')]),bank);assert.equal(d.wallets.COUPLED.redemptions.length,1);assert.equal(d.wallets.COUPLED.fills.length,2);assert.equal(coupledMakerReport(d,bank,at(280000)).profitabilityEstablished,false);
});
test('learning NO first still closes the same pair; receipt order cannot mint extra capital',()=>{
 let s=advanceCoupledMaker(warmed(),pe(260000,[trade('no-newer',252000,'0.830')]),bank);assert.equal(s.wallets.COUPLED.positions['1070'].side,'NO');
 s=advanceCoupledMaker(s,pe(270000,[trade('yes-older',251000,'0.795')]),bank);assert.equal(s.wallets.COUPLED.realized,'0.025000');assert.equal(s.wallets.COUPLED.cash,'100.025000');assert.equal(s.wallets.COUPLED.redemptions[0].at,at(270000));
});
test('unmatched inventory is valued as a loss, and risk waits for the original quote projection',()=>{
 let s=advanceCoupledMaker(warmed(),pe(260000,[trade('yes',251000,'0.795')]),bank);s=advanceCoupledMaker(s,be(270000,'0.700000','0.725000'),bank);s=advanceCoupledMaker(s,be(280000,'0.700000','0.725000'),bank);
 assert.equal(s.wallets.COUPLED.fills.length,1);assert.equal(coupledMakerReport(s,bank,at(280000)).wallets.COUPLED.estimatedLiquidationPnl,'-0.105000');
 s=advanceCoupledMaker(s,be(370000,'0.700000','0.725000'),bank);assert.equal(s.wallets.COUPLED.quotes[2].coverageBarrierSequence,'370000');
 s=advanceCoupledMaker(s,pe(400000,[],{from:at(300000)}),bank);assert.equal(s.wallets.COUPLED.fills.length,1);
 s=advanceCoupledMaker(s,pe(410000,[],{from:at(244000)}),bank);s=JSON.parse(JSON.stringify(s));s=advanceCoupledMaker(s,be(420000,'0.700000','0.725000'),bank);
 assert.equal(s.wallets.COUPLED.fills.length,2);assert.equal(s.wallets.COUPLED.realized,'-0.105000');assert.equal(s.wallets.COUPLED.cash,'99.895000');assert.deepEqual(s.wallets.COUPLED.positions,{});
});
test('late learned purchase creates risk at its receipt, never a sale on an old book',()=>{
 let s=advanceCoupledMaker(warmed(),pe(8000000,[trade('late-yes',251000,'0.795')]),bank);s=advanceCoupledMaker(s,be(8000100),bank);assert.equal(s.wallets.COUPLED.fills.length,1);
 s=advanceCoupledMaker(s,pe(8000200,[],{from:at(244000),to:at(361000)}),bank);s=advanceCoupledMaker(s,be(8000300),bank);assert.equal(s.wallets.COUPLED.fills.length,1,'latency has not elapsed');
 s=advanceCoupledMaker(s,be(8005000),bank);assert.equal(s.wallets.COUPLED.fills.length,2);assert.equal(s.wallets.COUPLED.fills[1].at,at(8005000));assert.equal(s.wallets.COUPLED.realized,'-0.005000');
});
test('wrong account eligibility removes executable marks and prevents new orders',()=>{
 const e=be(250000);e.accountEligibility.excludedExchangeIds=['1070'];const s=advanceCoupledMaker(warmed(),e,bank);assert.equal(s.books['1070'],undefined);assert.equal(s.wallets.COUPLED.fills.length,0);
 assert.throws(()=>advanceCoupledMaker(s,be(240000),bank),/CLOCK/);
});
