import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {digest} from './model.mjs';
import {runSettlementValue} from './settlement-value-native.mjs';
import {SETTLEMENT_PAPER_POLICY,SETTLEMENT_PAPER_FILLS,createSettlementPaperState,advanceSettlementPaper,assertSettlementPaperState,settlementPaperReport} from './settlement-paper.mjs';

const start=Date.parse('2026-10-05T20:00:00.000Z');
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-funded-value-'));let serial=0;
 t.after(()=>{assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));fs.rmSync(dir,{recursive:true,force:true});});
 const research={schemaVersion:1,id:'governor-correctness-fixture',marketId:'195',exchangeId:'884',side:'YES',predicateHash:'a'.repeat(64),recordedAt:new Date(start-10000).toISOString(),sourceAt:new Date(start-10000).toISOString(),expiresAt:new Date(start+21600000).toISOString(),lower:'0.332804',upper:'0.673019',method:'subjective_range',settlementReviewed:true,flags:[],sources:[{url:'https://example.org/fixture',contentHash:'b'.repeat(64)}]};
 const bank={schemaVersion:1,mode:'funded_settlement_value_paper_v1',name:'test-funded-value',cutoff:new Date(start).toISOString(),universe:{exchanges:[{marketId:'195',exchangeId:'884',option:'YES'}]},identities:{195:{exchangeId:'884',predicateHash:research.predicateHash}},research:[research],riskGroups:{195:['national_election','texas_governor']},valuationDeadlineMs:start+30*86400000,policy:SETTLEMENT_PAPER_POLICY,fillPolicy:SETTLEMENT_PAPER_FILLS};
 const state=createSettlementPaperState(bank),evaluate=input=>runSettlementValue({input,dir:path.join(dir,String(++serial))}).result;
 const step=(ms,{bid='0.285000',ask='0.290000',depth=1000,sourceVersion='1:2026-10-05T20:00:00.000Z',account='fresh',requestAt=ms-100,kind='book',availableAt=ms,marketId='195'}={})=>{
  const row={kind,sequence:++serial,hash:digest({serial}),at:new Date(ms).toISOString(),payload:kind==='book'?{book:{marketId,exchangeId:'884',instrument:'884:YES',requestStartedAt:new Date(requestAt).toISOString(),receivedAt:new Date(ms).toISOString(),sourceAt:new Date(ms).toISOString(),sequence:'1',versionKey:sourceVersion,bids:[{price:bid,quantity:depth}],asks:[{price:ask,quantity:depth}]}}:kind==='invalidation'?{marketId,reason:'BOOK_READ_FAILED'}:{}};
  const eligible=account==='missing'?null:{schemaVersion:1,positionsAt:new Date(ms-500).toISOString(),ordersAt:new Date(account==='future'?ms:ms-400).toISOString(),excludedExchangeIds:account==='owned'?['884']:[]};
  advanceSettlementPaper(state,row,bank,new Date(availableAt).toISOString(),eligible,evaluate);return row;
 };
 return{bank,state,step};
}

test('real native decisions fund only one share after a later request and latency; independent wallets conserve cash',t=>{
 const {bank,state,step}=fixture(t);step(start+1000);assert.equal(state.wallets[1000].fills.length,0);assert.equal(state.wallets[1000].pending[195].quantity,1);
 assert.equal(state.wallets[1000].pending[195].limit,'0.295000');
 step(start+1900);assert.equal(state.wallets[1000].fills.length,0);
 step(start+2300);assert.equal(state.wallets[1000].fills.length,1);assert.equal(state.wallets[3000].fills.length,0);
 step(start+4300);assert.equal(state.wallets[3000].fills.length,1);
 for(const w of Object.values(state.wallets)){assert.equal(w.cash,'99.705000');assert.equal(w.positions[195].cost,'0.295000');assert.equal(w.realizedPnl,'0.000000');}
 const report=settlementPaperReport(state,bank,new Date(start+4300).toISOString());assert.equal(report.wallets[1000].liquidationPnl,'-0.015000');assert.equal(report.competitionOrders,0);assert.equal(report.fees,0);
});
test('a price drawdown alone retains value exposure; forecast expiry and the deadline never manufacture a payout',t=>{
 const {bank,state,step}=fixture(t);step(start+1000);step(start+5000);step(start+10000,{bid:'0.200000',ask:'0.210000',sourceVersion:'2:2026-10-05T20:00:09.000Z'});
 assert.equal(state.wallets[1000].pending[195],undefined);assert.equal(state.decisions.at(-1).action,'hold');
 step(start+3*86400000,{sourceVersion:'3:2026-10-08T20:00:00.000Z'});assert.equal(state.decisions.at(-1).action,'review_required');assert.equal(state.wallets[1000].positions[195].quantity,1);
 step(bank.valuationDeadlineMs+1000,{sourceVersion:'4:2026-11-04T20:00:00.000Z'});assert.equal(state.wallets[1000].realizedPnl,'0.000000');assert.equal(state.wallets[1000].fills.length,1);
});
test('value exit uses a later executable bid and exact original basis, then blocks immediate re-entry',t=>{
 const {bank,state,step}=fixture(t);step(start+1000);step(start+5000);
 step(start+10000,{bid:'0.680000',ask:'0.685000',sourceVersion:'2:2026-10-05T20:00:09.000Z'});assert.equal(state.wallets[1000].fills.length,1);assert.equal(state.wallets[1000].pending[195].action,'sell');
 step(start+14000,{bid:'0.680000',ask:'0.685000',sourceVersion:'3:2026-10-05T20:00:13.000Z'});assert.equal(state.wallets[1000].positions[195],undefined);assert.equal(state.wallets[1000].realizedPnl,'0.380000');
 step(start+15000,{sourceVersion:'4:2026-10-05T20:00:14.000Z'});assert.equal(state.wallets[1000].pending[195],undefined);assertSettlementPaperState(state,bank);
});
test('source boundary cancels a reservation without erasing held cost; missing, future or own account liquidity cannot fill',async t=>{
 for(const kind of ['missing','future','owned','boundary','stale'])await t.test(kind,t=>{
  const {state,step}=fixture(t);step(start+1000);
  if(kind==='boundary')step(start+5000,{kind:'invalidation'});else step(start+5000,{account:kind==='stale'?'fresh':kind,availableAt:kind==='stale'?start+26000:start+5000});
  assert.equal(state.wallets[1000].fills.length,0);assert.equal(state.wallets[1000].pending[195],undefined);assert.equal(state.wallets[1000].cash,'100.000000');
 });
 const {state,step}=fixture(t);step(start+1000);step(start+5000);step(start+10000,{kind:'invalidation'});assert.equal(state.wallets[1000].positions[195].cost,'0.295000');
});
test('insufficient later depth and unfavorable price never consume the reservation; native input is causal',t=>{
 const {state,step}=fixture(t);step(start+1000);step(start+5000,{bid:'0.290000',ask:'0.300000'});assert.equal(state.wallets[1000].fills.length,0);assert.equal(state.wallets[1000].pending[195].limit,'0.295000');
 step(start+10000,{depth:99});assert.equal(state.wallets[1000].fills.length,0);assert.equal(state.wallets[1000].pending[195],undefined);
 step(start+15000);assert.equal(state.wallets[1000].fills.length,0);step(start+19000);assert.equal(state.wallets[1000].fills.length,1);
});
test('tampered cash or attributed inventory fails conservation; changed forecast binds a distinct bank',t=>{
 const {bank,state,step}=fixture(t);step(start+1000);step(start+5000);state.wallets[1000].cash='100.000000';assert.throws(()=>assertSettlementPaperState(state,bank),/CONSERVATION/);
 state.wallets[1000].cash='99.705000';bank.research[0].lower='0.333000';assert.throws(()=>assertSettlementPaperState(state,bank),/BANK_CHANGED/);
});
