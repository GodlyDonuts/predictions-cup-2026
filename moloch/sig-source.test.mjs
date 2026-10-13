import test from 'node:test';import assert from 'node:assert/strict';import {receiptEvents} from './sig-source.mjs';
const at='2026-10-13T00:00:01.000Z',receipt={source:'official_sig_api_v1',status:200,tournamentId:'cup',marketId:'11',exchangeId:'101',requestStartedAt:'2026-10-13T00:00:00.000Z',receivedAt:at};
test('book adapter selects explicit SIG context and preserves original receipt/engine version',()=>{
  const r={...receipt,kind:'book',endpoint:'/markets/11/orderbook?tournamentId=cup&depth=200',data:{contexts:[{type:'tournament',tournament:{id:'cup'},orderbook:{exchanges:[{exchangeId:'101',asOf:{sequence:99,at},bids:[{price:.45,quantity:100}],asks:[{price:.455,quantity:100}]}]}}]}};
  const [e]=receiptEvents(r);assert.equal(e.book.sequence,'99');assert.equal(e.book.bids[0].price,'0.450000');assert.equal(e.book.receivedAt,at);
  r.data.contexts[0].tournament.id='foreign';assert.throws(()=>receiptEvents(r),/EXPLICIT_BOOK_CONTEXT/);
});
test('tape prices stay YES normalized regardless of side; old execution is not changed to new receipt',()=>{
  const r={...receipt,kind:'trades',endpoint:'/exchanges/101/trades?tournamentId=cup',data:{marketId:'11',exchangeId:'101',data:[{id:'99',createdAt:'2026-10-12T23:00:00.000Z',price:.45,side:'NO',size:20}]}};
  const [e]=receiptEvents(r);assert.equal(e.yesPrice,'0.450000');assert.equal(e.executedAt,'2026-10-12T23:00:00.000Z');assert.equal(e.receivedAt,at);assert.equal(e.type,'trade');
  r.endpoint='/exchanges/101/trades?tournamentId=foreign';assert.throws(()=>receiptEvents(r),/TOURNAMENT/);
});
test('only fully delivered single-page original tape window produces coverage',()=>{
  const from='2026-10-13T00:00:00.000Z',to=at,r={...receipt,kind:'trades',endpoint:`/exchanges/101/trades?tournamentId=cup&from=${from}&to=${to}`,data:{marketId:'11',exchangeId:'101',from,to,data:[],pagination:{hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:100}}};
  assert.equal(receiptEvents(r)[0].type,'coverage');r.data.pagination.hasMore=true;assert.equal(receiptEvents(r).length,0);
  r.data.pagination.hasMore=false;r.endpoint+='&cursor=missing-prefix';assert.equal(receiptEvents(r).length,0);
});
test('truncated or changed served window cannot authenticate requested coverage',()=>{
  const from='2026-10-13T00:00:00.000Z',r={...receipt,kind:'trades',endpoint:`/exchanges/101/trades?tournamentId=cup&from=${from}&to=${at}`,data:{marketId:'11',exchangeId:'101',from:at,to:at,data:[],pagination:{hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:100}}};
  assert.throws(()=>receiptEvents(r),/COVERAGE_WINDOW/);
});
