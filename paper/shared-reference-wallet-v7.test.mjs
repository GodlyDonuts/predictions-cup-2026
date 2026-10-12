import test from 'node:test';
import assert from 'node:assert/strict';
import {payloadHash} from '../execution/core.mjs';
import {amount,money,SCALE} from './fixed.mjs';
import {createSharedReferenceWalletV7 as create,advanceSharedReferenceWalletV7 as advance,assertSharedReferenceWalletV7 as check} from './shared-reference-wallet-v7.mjs';
const cutoff='2026-10-12T08:00:00.000Z', root={id:'fixture'},rootHash=payloadHash(root);
function bank(risk={maxHoldingMs:30000,maximumAdverseTicks:4}) {
 const b={exitMode:'net_harvest',cutoff,initialCash:'1000.000000',capitalLimit:'50.000000',fees:'0.000000',executionAuthority:false,latencyMs:3000,orderLifetimeMs:120000,maxBookAgeMs:20000,harvestMinimum:'0.005000',reentryGap:'0.005000',cooldownMs:60000,kellyFractionBps:1000,eventFractionBps:1000,inventoryRisk:risk,instruments:[{marketId:'1',exchangeId:'1',eventId:'1',side:'YES',rootHash,sourceAt:cutoff,entryExpiresAt:'2026-10-12T09:00:00.000Z',lowerValue:'0.700000',minimumGap:'0.005000',maximumPrincipal:'50.000000'}]};
 return {...b,bankHash:payloadHash(b)};
}
function sample(seconds,bid='0.545000',ask='0.550000',depth=10000) {
 const at=new Date(Date.parse(cutoff)+seconds*1000).toISOString();
 return {book:{marketId:'1',exchangeId:'1',versionKey:'v'+seconds,requestStartedAt:at,receivedAt:at,bids:[{price:bid,quantity:depth}],asks:[{price:ask,quantity:depth}]},rules:{marketId:'1',root,rootHash,requestStartedAt:at,receivedAt:at}};
}
function filled(b=bank()) {const s=create(b);advance(s,b,sample(1,'0.555000','0.560000'));advance(s,b,sample(8));assert.equal(s.positions['1'].quantity,58);return {s,b};}
test('a holding deadline proposes a losing exit, but no cash or custody changes until a later receipt',()=>{
 const {s,b}=filled();const cash=s.cash;advance(s,b,sample(39));
 assert.equal(s.marketRiskStop['1'].reason,'HOLDING_DEADLINE');assert.equal(s.cash,cash);assert.equal(s.fills.length,1);
 assert.equal(Object.values(s.pending)[0].action,'sell');advance(s,b,sample(40));assert.equal(s.fills.length,1);
 advance(s,b,sample(42));assert.equal(s.fills.length,2);assert.equal(s.realized,'-0.870000');assert.equal(s.positions['1'],undefined);check(s,b);
 advance(s,b,sample(110));assert.equal(Object.keys(s.pending).length,0);
});
test('adverse move stops adding inventory and realizes a supported loss without pretending profit',()=>{
 const {s,b}=filled();advance(s,b,sample(12,'0.500000','0.505000'));assert.equal(s.marketRiskStop['1'].reason,'ADVERSE_PRICE_MOVE');
 advance(s,b,sample(15,'0.500000','0.505000'));assert.equal(s.realized,'-3.480000');assert.equal(s.cash,'996.520000');assert.equal(s.positions['1'],undefined);check(s,b);
});
test('partial buy fills are applied before remaining buy quotes are retired on risk',()=>{
 const b=bank({maxHoldingMs:30000,maximumAdverseTicks:1}),s=create(b);advance(s,b,sample(1,'0.555000','0.560000'));
 advance(s,b,sample(8,'0.545000','0.550000',1000));assert.equal(s.fills[0].quantity,10);assert.equal(s.positions['1'].quantity,10);
 assert.ok(s.events.some(x=>x.type==='paper_buy_quote_retired_after_fills'&&x.remainingQuantity===48));assert.equal(Object.keys(s.pending).length,0);check(s,b);
});
test('risk exit honors scarce later depth, leaving unfilled inventory visible',()=>{
 const {s,b}=filled();advance(s,b,sample(12,'0.500000','0.505000',10000));advance(s,b,sample(15,'0.500000','0.505000',1000));
 assert.equal(s.fills[1].quantity,10);assert.equal(s.positions['1'].quantity,48);assert.equal(Object.values(s.pending)[0].quantity,48);check(s,b);
});
test('unavailable exit liquidity never creates an imaginary deadline fill',()=>{
 const {s,b}=filled();advance(s,b,sample(39));advance(s,b,sample(42,'0.400000','0.405000'));
 assert.equal(s.fills.length,1);assert.equal(s.positions['1'].quantity,58);assert.equal(s.realized,'0.000000');check(s,b);
});
test('profitable harvesting is retained when no declared inventory risk is triggered',()=>{
 const {s,b}=filled();advance(s,b,sample(12,'0.580000','0.585000'));assert.equal(s.marketRiskStop['1'],undefined);
 advance(s,b,sample(15,'0.580000','0.585000'));assert.equal(s.realized,'1.160000');check(s,b);
});
test('risk parameters are declared, bank-hashed and required',()=>{
 assert.throws(()=>create(bank({maxHoldingMs:2000,maximumAdverseTicks:4})),/INVENTORY_RISK_POLICY/);
 assert.throws(()=>create(bank({maxHoldingMs:30000,maximumAdverseTicks:0})),/INVENTORY_RISK_POLICY/);
 const b=bank(),s=create(b);b.inventoryRisk.maxHoldingMs=60000;assert.throws(()=>check(s,b),/STATE_BANK/);
});
test('NO-side inventory uses the complementary executable book and preserves loss accounting',()=>{
 const original=bank(),{bankHash,...body}=original;body.instruments[0].side='NO';const b={...body,bankHash:payloadHash(body)},s=create(b);
 const noSample=(seconds,bid,ask)=>{const r=sample(seconds,bid,ask);r.book.bids[0].price=money(SCALE-amount(ask));r.book.asks[0].price=money(SCALE-amount(bid));return r;};
 advance(s,b,noSample(1,'0.555000','0.560000'));advance(s,b,noSample(8,'0.545000','0.550000'));
 advance(s,b,noSample(12,'0.500000','0.505000'));advance(s,b,noSample(15,'0.500000','0.505000'));
 assert.equal(s.realized,'-3.480000');assert.equal(s.positions['1'],undefined);assert.ok(s.fills.every(f=>f.side==='NO'));check(s,b);
});
test('repeated receipts cannot fill twice or release inventory twice after risk liquidation',()=>{
 const {s,b}=filled();const observation=sample(12,'0.500000','0.505000');advance(s,b,observation);
 assert.equal(advance(s,b,observation).reason,'DUPLICATE');const exit=sample(15,'0.500000','0.505000');advance(s,b,exit);
 assert.equal(advance(s,b,exit).reason,'DUPLICATE');assert.equal(s.fills.length,2);assert.equal(s.realized,'-3.480000');check(s,b);
});
