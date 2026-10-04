import test from 'node:test';
import assert from 'node:assert/strict';
import {digest} from './model.mjs';
import {settlementInputFromProbability} from './settlement-value-native.mjs';

const at='2026-10-04T20:00:00.000Z';
function fixture(){
 const r={id:'r1',marketId:'1',exchangeId:'101',side:'YES',predicateHash:'a'.repeat(64),recordedAt:'2026-10-03T21:00:00Z',sourceAt:'2026-10-03T20:00:00Z',expiresAt:'2026-10-04T03:00:00Z',method:'subjective_range',lower:'0.940000',upper:'0.990000',settlementReviewed:true,flags:[],sources:[{url:'https://example.org/research',contentHash:'b'.repeat(64)}]};
 const bank={config:{initialCash:'100.000000'},research:[r],researchEvidence:[{researchId:r.id,binding:{event:'2026 Example Senate General Election'}}]};
 const stats={bankHash:digest(bank),sourceSequence:5,sourceHash:'c'.repeat(64),wallets:{3000:{cash:'99.145000',positions:[{instrument:'101:YES',quantity:1,cost:'0.855000',enteredAt:'2026-10-03T21:01:00Z'}],pending:[],fills:1,recentFills:[{action:'buy',price:'0.855000',quantity:1}]}}};
 const publication={sequence:5,hash:'c'.repeat(64),books:{'101:YES':{requestStartedAt:'2026-10-04T19:59:59.000Z',receivedAt:'2026-10-04T19:59:59.500Z',availableAt:'2026-10-04T19:59:59.900Z',sequence:'123',bids:[{price:'0.845000',quantity:1000}],asks:[{price:'0.850000',quantity:1000}],publicationHash:'d'.repeat(64)}}};
 return{bank,stats,publication,account:null,asOfMs:Date.parse(at)};
}
test('adapter retains original research expiry and full paper cost without inventing account eligibility',()=>{
 const f=fixture(),before=JSON.stringify(f),input=settlementInputFromProbability(f);
 assert.equal(JSON.stringify(f),before);assert.equal(input.candidates[0].research.expiresAtMs,Date.parse(f.bank.research[0].expiresAt));assert.equal(input.wallet.cumulativeBuys,'0.855000');assert.equal(input.wallet.positions[0].cost,'0.855000');assert.equal(input.candidates[0].account,null);assert.equal(input.candidates[0].book.sourceHash,'d'.repeat(64));
});
test('adapter refuses incomplete fill history, pending intents, and unbound inherited positions',()=>{
 for(const change of [f=>f.stats.wallets['3000'].fills=2,f=>f.stats.wallets['3000'].pending=[{}],f=>f.stats.wallets['3000'].positions[0].instrument='999:YES']){
  const f=fixture();change(f);assert.throws(()=>settlementInputFromProbability(f),/COMPLETE_WALLET|POSITION_UNBOUND/);
 }
});
test('adapter binds the paper report to the verified public prefix and immutable bank',()=>{
 for(const change of [f=>f.stats.bankHash='e'.repeat(64),f=>f.publication.sequence++,f=>f.publication.hash='e'.repeat(64)]){
  const f=fixture();change(f);assert.throws(()=>settlementInputFromProbability(f),/FRONTIER/);
 }
});
test('only causal exclusion booleans and clocks enter the decision, never raw account values',()=>{
 const f=fixture();f.account={positionsAt:'2026-10-04T19:59:57Z',ordersAt:'2026-10-04T19:59:58Z',positionExchangeIds:['101'],openOrderExchangeIds:[],private:'PRIVATE_SENTINEL'};
 const input=settlementInputFromProbability(f);assert.equal(input.candidates[0].account.positionExcluded,true);assert.equal(input.candidates[0].account.openOrderExcluded,false);assert(!JSON.stringify(input).includes('PRIVATE_SENTINEL'));
});
