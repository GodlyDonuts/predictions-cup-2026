import test from 'node:test';import assert from 'node:assert/strict';
import {NO_MAKER_POLICY,createNoMakerState,advanceNoMaker,assertNoMaker,noMakerReport} from './no-maker-paper-v2.mjs';
import {studyCapacityPaths,tapeTick,CAPACITY_PATH_POLICY} from './canonical-entry-exit-study.mjs';
const zero=Date.parse('2026-10-03T23:00:00Z'),at=n=>new Date(zero+n).toISOString();
const bank={mode:'no_maker_trade_tape_paper',name:'correctness-only',cutoff:at(0),policy:NO_MAKER_POLICY,tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',reviewExpiresAt:at(3600000),legs:[{marketId:'381',exchangeId:'1070'},{marketId:'382',exchangeId:'1071'}]};
const book=(n,changes={})=>({tournamentId:bank.tournamentId,marketId:'381',exchangeId:'1070',instrument:'1070:YES',requestStartedAt:at(n-100),receivedAt:at(n),publishedAt:at(n),sourceAt:at(n-50),sequence:String(n),versionKey:n+':'+at(n-50),publicationHash:'a'.repeat(64),publicationSequence:n,bids:[{price:'0.800000',quantity:1000}],asks:[{price:'0.835000',quantity:1000}],...changes});
const account=n=>({schemaVersion:1,positionsAt:at(n-200),ordersAt:at(n-200),excludedExchangeIds:[]});
const bEvent=(n,changes={})=>({kind:'book',availableAt:at(n),book:book(n),accountEligibility:account(n),entryAllowed:true,...changes});
const trade=(key,n,yesPrice,side='YES')=>({key,id:key,createdAt:at(n),yesPrice,size:1,executedSpace:side});
const pEvent=(n,trades,changes={})=>({kind:'trades',availableAt:at(n),page:{marketId:'381',exchangeId:'1070',tournamentId:bank.tournamentId,receivedAt:at(n),from:at(0),to:at(n-1),pagination:{hasMore:false},coverage:{complete:true,projectedThroughSequence:1000000},trades,...changes}});

test('later completed windows cannot release a missed quote lifetime or upgrade its earlier checkpoint',()=>{
 let s=advanceNoMaker(createNoMakerState(bank),bEvent(1000),bank);s=advanceNoMaker(s,bEvent(122100),bank);
 s=advanceNoMaker(s,pEvent(200000,[],{coverage:{complete:false,projectedThroughSequence:1000000}}),bank);
 s=advanceNoMaker(s,pEvent(300001,[],{from:at(140000),to:at(300000)}),bank);
 assert.equal(noMakerReport(s,bank,at(300001)).wallets.at_quote.reserved,'0.165000');
 s=advanceNoMaker(s,pEvent(310001,[],{from:at(0),to:at(140000),coverage:{complete:true,projectedThroughSequence:1000}}),bank);
 assert.equal(noMakerReport(s,bank,at(310001)).wallets.at_quote.reserved,'0.165000','old coverage cannot borrow a newer checkpoint');
 s=advanceNoMaker(s,pEvent(320001,[],{from:at(0),to:at(140000)}),bank);
 assert.equal(noMakerReport(s,bank,at(320001)).wallets.at_quote.reserved,'0.000000');
});
test('contiguous coverage needs every interval and every pagination page at a sufficient projection checkpoint',()=>{
 let s=advanceNoMaker(createNoMakerState(bank),bEvent(1000),bank);s=advanceNoMaker(s,bEvent(122100),bank);
 s=advanceNoMaker(s,pEvent(130000,[],{from:at(4000),to:at(60000)}),bank);
 s=advanceNoMaker(s,pEvent(140000,[],{from:at(61000),to:at(122000)}),bank);
 assert.equal(noMakerReport(s,bank,at(140000)).wallets.at_quote.reserved,'0.165000','one-second uncovered gap remains');
 s=advanceNoMaker(s,pEvent(150000,[],{from:at(60000),to:at(61000),requestCursor:null,pagination:{hasMore:true,nextCursor:'A'}}),bank);
 s=advanceNoMaker(s,pEvent(160000,[],{from:at(60000),to:at(61000),requestCursor:'wrong'}),bank);
 assert.equal(noMakerReport(s,bank,at(160000)).wallets.at_quote.reserved,'0.165000','orphan final page is not a complete window');
 s=advanceNoMaker(s,pEvent(170000,[],{from:at(60000),to:at(61000),requestCursor:null,pagination:{hasMore:true,nextCursor:'B'},coverage:{complete:false,projectedThroughSequence:1000000}}),bank);
 s=advanceNoMaker(s,pEvent(180000,[],{from:at(60000),to:at(61000),requestCursor:'B'}),bank);
 assert.equal(noMakerReport(s,bank,at(180000)).wallets.at_quote.reserved,'0.165000');
 s=advanceNoMaker(s,pEvent(190000,[],{from:at(60000),to:at(61000),requestCursor:null,pagination:{hasMore:true,nextCursor:'C'}}),bank);
 s=advanceNoMaker(s,pEvent(200000,[],{from:at(60000),to:at(61000),requestCursor:'C'}),bank);
 assert.equal(noMakerReport(s,bank,at(200000)).wallets.at_quote.reserved,'0.000000');
});
test('rejected account and stale books invalidate liquidation marks while preserving owned cash and FIFO',()=>{
 const base=advanceNoMaker(advanceNoMaker(createNoMakerState(bank),bEvent(1000),bank),pEvent(10000,[trade('entry',9000,'0.84')]),bank);
 for(const e of [bEvent(20000,{book:book(20000,{asks:[{price:'0.700000',quantity:1000}]}),accountEligibility:{...account(20000),excludedExchangeIds:['1070']}}),bEvent(20000,{accountEligibility:null}),bEvent(20000,{book:book(20000,{requestStartedAt:at(-2000)})})]){
  const s=advanceNoMaker(base,e,bank),r=noMakerReport(s,bank,at(20000)).wallets.at_quote;
  assert.equal(r.estimatedLiquidationPnl,null);assert.equal(r.positions[0].estimatedLiquidationProceeds,null);assert.equal(r.cash,'99.835000');assert.equal(r.realizedPnl,'0.000000');assert.equal(r.fills,1);
  const recovered=advanceNoMaker(s,bEvent(21000),bank);assert.notEqual(noMakerReport(recovered,bank,at(21000)).wallets.at_quote.estimatedLiquidationPnl,null);
 }
});
test('covered resting exit keeps risk taker blocked through coverage gaps and consumes a late pre-cancel print once',()=>{
 let s=advanceNoMaker(advanceNoMaker(createNoMakerState(bank),bEvent(1000),bank),pEvent(10000,[trade('entry',9000,'0.84')]),bank);
 s=advanceNoMaker(s,bEvent(20000,{book:book(20000,{asks:[{price:'0.815000',quantity:1000}]})}),bank);
 const bad=n=>bEvent(n,{book:book(n,{bids:[{price:'0.870000',quantity:1000}],asks:[{price:'0.880000',quantity:1000}]})});
 s=advanceNoMaker(s,bad(30000),bank);s=advanceNoMaker(s,bad(35000),bank);
 s=advanceNoMaker(s,pEvent(40000,[],{from:at(31000),to:at(39999)}),bank);s=advanceNoMaker(s,bad(45000),bank);
 assert.equal(s.wallets.at_quote.fills.length,1,'risk sell cannot race unresolved resting exit');
 const late=pEvent(50000,[trade('late-exit',28000,'0.795')],{from:at(20000),to:at(39999)});s=advanceNoMaker(s,late,bank);
 s=advanceNoMaker(s,{...late,availableAt:at(51000)},bank);s=advanceNoMaker(s,bad(55000),bank);
 assert.equal(s.wallets.at_quote.fills.length,2);assert.equal(s.wallets.at_quote.realized,'0.035000');assertNoMaker(s,bank);
});
test('strict-through maker buys and covered exits conserve cash/FIFO; ordinary quote touch cannot fill',()=>{
 let s=createNoMakerState(bank);s=advanceNoMaker(s,bEvent(1000),bank);s=advanceNoMaker(s,pEvent(6000,[trade('touch',5000,'0.835')]),bank);
 assert.equal(s.wallets.at_quote.fills.length,0);assert.equal(s.wallets.improve_one_tick.fills.length,1,'one tick improvement has a distinct, better price');
 s=advanceNoMaker(s,pEvent(10000,[trade('buy',9000,'0.84','NO')]),bank);assert.equal(s.wallets.at_quote.positions['1070'].cost,'0.165000');assert.equal(s.wallets.improve_one_tick.positions['1070'].cost,'0.170000');
 s=advanceNoMaker(s,bEvent(20000,{book:book(20000,{asks:[{price:'0.815000',quantity:1000}]})}),bank);
 s=advanceNoMaker(s,pEvent(30000,[trade('exit',28000,'0.795')]),bank);
 assert.equal(s.wallets.at_quote.cash,'100.035000');assert.equal(s.wallets.at_quote.realized,'0.035000');assert.equal(s.wallets.improve_one_tick.cash,'100.025000');assertNoMaker(s,bank);
 assert.equal(noMakerReport(s,bank,at(30000)).profitabilityEstablished,false,'synthetic accounting never establishes real returns');
});
test('pre-activation prints, shrinking depth and repeated prints do not manufacture inventory',()=>{
 let s=advanceNoMaker(createNoMakerState(bank),bEvent(1000),bank);s=advanceNoMaker(s,pEvent(6000,[trade('early',2000,'0.85')]),bank);
 s=advanceNoMaker(s,bEvent(7000,{book:book(7000,{asks:[{price:'0.835000',quantity:100}]})}),bank);assert.equal(s.wallets.at_quote.fills.length,0);
 const e=pEvent(10000,[trade('once',9000,'0.84')]);s=advanceNoMaker(s,e,bank);s=advanceNoMaker(s,{...e,availableAt:at(11000)},bank);assert.equal(s.wallets.at_quote.fills.length,1);
});
test('late reporting keeps reservations until a post-expiry engine checkpoint; reported prints resolve before non-fill',()=>{
 let s=advanceNoMaker(createNoMakerState(bank),bEvent(1000),bank);s=advanceNoMaker(s,bEvent(122100),bank);
 let e=pEvent(130000,[],{coverage:{complete:true,projectedThroughSequence:1000}});s=advanceNoMaker(s,e,bank);assert.equal(noMakerReport(s,bank,at(130000)).wallets.at_quote.reserved,'0.165000');
 e=pEvent(140000,[trade('late',5000,'0.84')]);s=advanceNoMaker(s,e,bank);assert.equal(s.wallets.at_quote.fills.length,1);assert.equal(s.wallets.at_quote.positions['1070'].enteredAt,at(140000),'receipt time governs actual model availability, not backdated source time');
});
test('incomplete pagination cannot release capital; a confirmed no-qualifying-print window can',()=>{
 let s=advanceNoMaker(createNoMakerState(bank),bEvent(1000),bank);s=advanceNoMaker(s,bEvent(122100),bank);s=advanceNoMaker(s,pEvent(130000,[],{pagination:{hasMore:true}}),bank);
 assert.equal(noMakerReport(s,bank,at(130000)).wallets.at_quote.reserved,'0.165000');s=advanceNoMaker(s,pEvent(140000,[]),bank);assert.equal(noMakerReport(s,bank,at(140000)).wallets.at_quote.reserved,'0.000000');
});
test('loss exits wait for independent request and retain exact zero-fee accounting',()=>{
 let s=advanceNoMaker(createNoMakerState(bank),bEvent(1000),bank);s=advanceNoMaker(s,pEvent(10000,[trade('entry',9000,'0.84')]),bank);
 const bad=n=>bEvent(n,{book:book(n,{bids:[{price:'0.870000',quantity:1000}],asks:[{price:'0.880000',quantity:1000}]})});s=advanceNoMaker(s,bad(20000),bank);assert.equal(s.wallets.at_quote.fills.length,1);
 s=advanceNoMaker(s,bad(22000),bank);assert.equal(s.wallets.at_quote.fills.length,1);s=advanceNoMaker(s,bad(25000),bank);assert.equal(s.wallets.at_quote.fills.length,2);assert.equal(s.wallets.at_quote.realized,'-0.050000');assert.equal(s.wallets.at_quote.fees,'0.000000');
});
test('exclusions, stale receipts and expired review block entries; fresh marks expire strictly after20s',()=>{
 for(const e of [bEvent(1000,{entryAllowed:false}),bEvent(1000,{accountEligibility:{...account(1000),excludedExchangeIds:['1070']}}),bEvent(1000,{availableAt:at(22000)})])assert.equal(advanceNoMaker(createNoMakerState(bank),e,bank).wallets.at_quote.fills.length,0);
 let s=advanceNoMaker(createNoMakerState(bank),bEvent(1000),bank);s=advanceNoMaker(s,pEvent(10000,[trade('entry',9000,'0.84')]),bank);assert.equal(noMakerReport(s,bank,at(21001)).wallets.at_quote.estimatedLiquidationPnl,null);
 assert.equal(advanceNoMaker(createNoMakerState({...bank,reviewExpiresAt:at(500)}),bEvent(1000),{...bank,reviewExpiresAt:at(500)}).counts.quote_buy,undefined);
});
test('only known binary complement encodings normalize; arbitrary off-tick and legacy nulls remain unusable',()=>{
 assert.equal(tapeTick({yesPrice:'0.22499999999999998'}).price,'0.225000');assert.equal(tapeTick({yesPrice:'0.21999999999999997'}).price,'0.220000');assert.equal(tapeTick({yesPrice:'0.224999'}),null);assert.equal(tapeTick({yesPrice:null}),null);
});
test('hindsight exit and strict-through diagnostic remain conditional and expose a single-leg-only outcome',()=>{
 const first={at:at(1000),books:[book(1000),{...book(1000),marketId:'382',exchangeId:'1071',instrument:'1071:YES'}],sourceSequence:1000,sourceHash:'b'.repeat(64),versions:[book(1000).versionKey,book(1000).versionKey],accountEligibility:account(1000),result:{positiveConditionalCapacity:true,matchedQuantity:'1',acquisitionCost:'0.410000',conditionalOrdinaryMinimumProfit:'0.590000',stressedMinimumProfit:'-0.410000'}};
 const second={...first,at:at(150000),sourceSequence:150000,books:first.books.map(b=>({...book(150000),marketId:b.marketId,exchangeId:b.exchangeId,instrument:b.instrument})),versions:[book(150000).versionKey,book(150000).versionKey],accountEligibility:account(150000),result:{...first.result,positiveConditionalCapacity:false}};
 const t={...trade('oneleg',5000,'0.84'),tournamentId:bank.tournamentId,marketId:'381',exchangeId:'1070',firstAvailableAt:at(150000)};
 const r=studyCapacityPaths({frames:[first,second],trades:[t]});assert.equal(r.anchorsWithAnyPositiveLaterDepthMark,0);assert.equal(r.makerOneLegThrough,1);assert.equal(r.makerBothLegsThrough,0);assert.equal(r.simulatedFills,null);assert.equal(r.realizedPnl,null);
 assert.throws(()=>studyCapacityPaths({frames:[first],trades:[],policy:{...CAPACITY_PATH_POLICY,feePerShare:'0.001000'}}),/POLICY/);
});
