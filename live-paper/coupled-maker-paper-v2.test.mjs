import test from 'node:test';import assert from 'node:assert/strict';
import {validateUniverse} from './breadth.mjs';import {payloadHash,canonicalJson} from '../execution/ledger.mjs';
import {COUPLED_BULK_POLICY,createCoupledBulkState,advanceCoupledBulk,coupledBulkCandidates,coupledBulkReport,assertCoupledBulk} from './coupled-maker-paper-v2.mjs';
const zero=Date.parse('2026-10-05T16:00:00Z'),at=n=>new Date(zero+n).toISOString();
const universe=validateUniverse({tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',exchanges:[{marketId:'381',exchangeId:'1070',option:'YES'},{marketId:'382',exchangeId:'1071',option:'YES'}]},'bda92870-621e-47b0-bc3c-3602c5c26f55');
const bank={schemaVersion:1,mode:'coupled_bulk_history_trade_tape_paper_v2',name:'correctness-only',cutoff:at(0),entryExpiresAt:at(86400000),policy:COUPLED_BULK_POLICY,tournamentId:universe.tournamentId,universe,legs:universe.exchanges.map(({marketId,exchangeId})=>({marketId,exchangeId}))};
const snap=(n,bid=.8,ask=.825)=>({tournamentId:bank.tournamentId,universeHash:universe.hash,requestStartedAt:at(n-100),receivedAt:at(n),sourceTimestamp:null,depthAvailable:false,executable:false,quotes:universe.exchanges.map(x=>({...x,latestPrice:.8,bestBid:bid,bestAsk:ask,spread:ask-bid,crossed:bid>=ask})),missingIds:[]});
const prices=(n,bid,ask)=>({kind:'prices',availableAt:at(n),snapshot:snap(n,bid,ask)});
const book=(n,bid='0.800000',ask='0.825000',quantity=1000)=>({kind:'book',availableAt:at(n),book:{tournamentId:bank.tournamentId,marketId:'381',exchangeId:'1070',instrument:'1070:YES',requestStartedAt:at(n-100),receivedAt:at(n),sourceAt:at(n-50),sequence:String(n),versionKey:n+':'+at(n-50),publicationHash:'a'.repeat(64),publicationSequence:n,bids:[{price:bid,quantity}],asks:[{price:ask,quantity}]},accountEligibility:{schemaVersion:1,positionsAt:at(n-200),ordersAt:at(n-200),excludedExchangeIds:[]}});
const warmed=(bid=.8,ask=.825)=>[1000,61000,121000,181000,241000].reduce((s,n)=>advanceCoupledBulk(s,prices(n,bid,ask),bank),createCoupledBulkState(bank));
const trade=(key,n,price)=>({key,id:key,createdAt:at(n),yesPrice:String(Number(price)),size:1,executedSpace:'YES'});
const page=(n,trades,changes={})=>({kind:'trades',availableAt:at(n),page:{marketId:'381',exchangeId:'1070',tournamentId:bank.tournamentId,receivedAt:at(n),from:at(0),to:at(n-1),requestCursor:null,pagination:{hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:100000000},trades,...changes}});
test('bulk history warms both markets without depth or fills; one fresh book enables only its own quotes',()=>{
 const s=warmed();assert.equal(coupledBulkCandidates(s,bank,at(241000)).length,2);assert.equal(s.wallets.COUPLED.fills.length,0);assert.deepEqual(s.wallets.COUPLED.quotes,{});assert.deepEqual(s.books,{});
 const before=payloadHash(s),next=advanceCoupledBulk(s,book(242000),bank);assert.equal(payloadHash(s),before);assert.equal(Object.keys(next.wallets.COUPLED.quotes).length,2);assert.equal(next.wallets.COUPLED.quotes[1].limit,'0.805000');assert.equal(next.wallets.COUPLED.quotes[2].limit,'0.180000');assert.equal(next.wallets.COUPLED.cash,'100.000000');
});
test('inside quote touches do not fill; original bid/ask prints are strict-through and net the paired costs once',()=>{
 let s=advanceCoupledBulk(warmed(),book(242000),bank);s=advanceCoupledBulk(s,page(260000,[trade('touch-yes',250000,.805),trade('touch-no',251000,.82)]),bank);assert.equal(s.wallets.COUPLED.fills.length,0);
 s=advanceCoupledBulk(s,page(270000,[trade('bid-through',252000,.8),trade('ask-through',253000,.825)]),bank);assert.equal(s.wallets.COUPLED.realized,'0.015000');assert.equal(s.wallets.COUPLED.cash,'100.015000');assert.equal(s.wallets.COUPLED.redemptions.length,1);assertCoupledBulk(s,bank);assert.equal(coupledBulkReport(s,bank,at(270000)).profitabilityEstablished,false);
});
test('two-tick spreads cannot fund inside improvement; changed book and insufficient depth reject entry',()=>{
 for(const ask of[.805,.81]){const s=warmed(.8,ask);assert.equal(coupledBulkCandidates(s,bank,at(241000)).length,0);const next=advanceCoupledBulk(s,book(242000,'0.800000',ask.toFixed(6)),bank);assert.deepEqual(next.wallets.COUPLED.quotes,{});}
 assert.deepEqual(advanceCoupledBulk(warmed(),book(242000,'0.820000','0.845000'),bank).wallets.COUPLED.quotes,{});
 assert.deepEqual(advanceCoupledBulk(warmed(),book(242000,'0.800000','0.825000',99),bank).wallets.COUPLED.quotes,{});
});
test('receipt history learned after a book request, stale data, missing identities and source gaps cannot warm entry',()=>{
 const earlier=[1000,61000,121000,181000].reduce((s,n)=>advanceCoupledBulk(s,prices(n),bank),createCoupledBulkState(bank));
 const late=advanceCoupledBulk(earlier,{...prices(241000),availableAt:at(241500)},bank);assert.deepEqual(advanceCoupledBulk(late,book(241550),bank).wallets.COUPLED.quotes,{},'the required fifth feature was learned after this request began');
 assert.equal(Object.keys(advanceCoupledBulk(late,book(242000),bank).wallets.COUPLED.quotes).length,2,'a subsequent request can use the already learned feature');
 assert.deepEqual(advanceCoupledBulk(warmed(),book(400000),bank).wallets.COUPLED.quotes,{});
 const missing=prices(250000);missing.snapshot.quotes=[];missing.snapshot.missingIds=['1070','1071'];const cleared=advanceCoupledBulk(warmed(),missing,bank);assert.equal(coupledBulkCandidates(cleared,bank,at(250000)).length,0);
 const gap=advanceCoupledBulk(warmed(),prices(400000),bank);assert.equal(coupledBulkCandidates(gap,bank,at(400000)).length,0);
});
test('cached repeated requests and malformed prices never provide independent feature evidence',()=>{
 let s=createCoupledBulkState(bank);for(const n of[1000,2000,3000,4000,5000])s=advanceCoupledBulk(s,prices(n),bank);assert(s.history['1070'].length<=2);assert.equal(coupledBulkCandidates(s,bank,at(5000)).length,0);
 const invalid=prices(250000,.803,.825),next=advanceCoupledBulk(warmed(),invalid,bank);assert.equal(next.history['1070'],undefined);assert.equal(next.wallets.COUPLED.fills.length,0);
 const foreign=prices(250000);foreign.snapshot.universeHash='f'.repeat(64);assert.throws(()=>advanceCoupledBulk(warmed(),foreign,bank),/NOT_NORMALIZED/);
});
test('bulk activity cannot release expired reservations or sell an unmatched leg',()=>{
 let s=advanceCoupledBulk(warmed(),book(242000),bank);s=advanceCoupledBulk(s,page(260000,[trade('yes',250000,.8)]),bank);s=advanceCoupledBulk(s,prices(400000,.7,.725),bank);assert.equal(s.wallets.COUPLED.fills.length,1);assert.equal(s.wallets.COUPLED.realized,'0.000000');assert.equal(s.wallets.COUPLED.quotes[2].resolvedAt,null);assert.equal(s.wallets.COUPLED.positions['1070'].side,'YES');
 const e=book(410000,'0.700000','0.725000');e.accountEligibility.excludedExchangeIds=['1070'];s=advanceCoupledBulk(s,e,bank);assert.equal(coupledBulkReport(s,bank,at(410000)).wallets.COUPLED.estimatedLiquidationPnl,null);
});
test('catalogue byte identity survives canonical bank persistence while changed members remain rejected',()=>{
 const raw={tournamentId:bank.tournamentId,catalogueObservedAt:at(-1000),exchanges:universe.exchanges},sourceBank={...bank,universe:validateUniverse(raw,bank.tournamentId),universeSourceJson:JSON.stringify(raw)};
 const persisted=JSON.parse(canonicalJson(sourceBank));assert.equal(createCoupledBulkState(persisted).bankHash,payloadHash(sourceBank));
 persisted.universe.exchanges[0].marketId='999';assert.throws(()=>createCoupledBulkState(persisted),/UNIVERSE_HASH/);
});
