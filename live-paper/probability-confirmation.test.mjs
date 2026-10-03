import test from 'node:test';import assert from 'node:assert/strict';
import {fixtureBank,fixtureBook} from './probability-receipt.test.mjs';
import {createProbabilityReceiptState,advanceReceiptProbability,receiptProbabilityReport} from './probability-receipt.mjs';
import {digest} from './model.mjs';
const base=Date.parse('2026-10-03T21:00:00Z'),at=n=>new Date(base+n).toISOString();
test('independent later request can confirm unchanged engine depth; requesting before latency cannot',()=>{
 const bank=fixtureBank();
 for(const started of [3900,7900]){let s=createProbabilityReceiptState(bank);for(const [n,start,available]of [[2990,2900,3100],[started+90,started,started+200]]){const book=fixtureBook(n,{start,source:3100,sequence:1});advanceReceiptProbability(s,{kind:'book',at:book.receivedAt,sequence:n,hash:digest(book),payload:{book}},bank,at(available),{schemaVersion:1,positionsAt:at(start-100),ordersAt:at(start-100),excludedExchangeIds:[]});}
  for(const [latency,w]of Object.entries(s.wallets))assert.equal(w.state.strategies.mean_reversion.fills.length,started===7900?1:0,'latency '+latency);
 }
});
test('raw legacy allowance remains a labeled diagnostic; ages expose a mark that strict UI must reject',()=>{
 const bank=fixtureBank(),s=createProbabilityReceiptState(bank);for(const n of [1000,5000]){const b=fixtureBook(n);advanceReceiptProbability(s,{kind:'book',at:b.receivedAt,sequence:n,hash:digest(b),payload:{book:b}},bank,at(n),{schemaVersion:1,positionsAt:at(n-1000),ordersAt:at(n-1000),excludedExchangeIds:[]});}
 const r=receiptProbabilityReport(s,bank,at(137010));assert.equal(r.wallets['1000'].positions[0].quoteAgeMs,132010);assert.equal(r.wallets['1000'].liquidationPnl,'-0.015000');assert.equal(r.profitabilityEstablished,false);
});
