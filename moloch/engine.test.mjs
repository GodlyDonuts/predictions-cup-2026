import test from 'node:test';import assert from 'node:assert/strict';
import {createMoloch,advanceMoloch,reportMoloch,assertMoloch} from './engine.mjs';
const base=Date.parse('2026-10-13T00:00:00Z'),at=n=>new Date(base+n*1000).toISOString();
const book=(n,ex='101',seq=1)=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:ex,exchangeId:ex,sequence:String(seq),requestStartedAt:at(n),receivedAt:at(n),sourceAt:at(n),bids:[{price:'0.450000',quantity:1000}],asks:[{price:'0.455000',quantity:1000}]}});
const trade=(n,id,p='0.440000',quantity=100,executed=n)=>({type:'trade',receivedAt:at(n),executedAt:at(executed),id,tournamentId:'cup',marketId:'101',exchangeId:'101',yesPrice:p,quantity});
const tick=n=>({type:'tick',receivedAt:at(n)});

test('paired strict-through fills reconcile exact cash, fees and conditional redemption',()=>{
  const s=createMoloch({quantity:1,feePerShare:'0.000100'});advanceMoloch(s,book(0));
  advanceMoloch(s,trade(4,'a'));advanceMoloch(s,trade(5,'b','0.465000'));
  const r=reportMoloch(s);assert.equal(r.fills,2);assert.equal(r.cash,'1000.004800');assert.equal(r.realized,'0.004800');assert.equal(r.fees,'0.000200');assert.equal(r.ownedCost,'0.000000');assert.equal(r.profitabilityEstablished,false);assertMoloch(s);
});
test('pre-activation and at-touch prints never fabricate queue fills; duplicate IDs cannot fill twice',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,trade(1,'early'));advanceMoloch(s,trade(4,'touch','0.450000'));assert.equal(s.fills.length,0);
  advanceMoloch(s,trade(5,'through'));advanceMoloch(s,trade(6,'through','0.440000',100,5));assert.equal(s.fills.length,1);assert.equal(s.counts.duplicateTrades,1);
  assert.throws(()=>advanceMoloch(s,trade(7,'through','0.435000',100,5)),/TRADE_CONFLICT/);
});
test('cancel latency stops active quotes without freeing unresolved funding or globally blocking another market',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,{type:'gap',exchangeId:'101',receivedAt:at(4)});
  advanceMoloch(s,tick(7));advanceMoloch(s,book(10,'102'));
  assert.ok(s.orders.some(q=>q.exchangeId==='102'));assert.equal(s.orders[0].retired,false);assert.equal(s.orders[0].cancelEffectiveAt,base+7000);assert.ok(Number(reportMoloch(s).reservedCash)>0);
});
test('delayed pre-cancel match uses reserved capital; only scoped full coverage and post-end checkpoint retire it',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,{type:'gap',exchangeId:'101',receivedAt:at(4)});advanceMoloch(s,tick(7));
  advanceMoloch(s,trade(8,'delayed','0.440000',100,6));assert.equal(s.fills.length,1);
  advanceMoloch(s,book(12,'101',20));
  const coverage={type:'coverage',receivedAt:at(13),tournamentId:'cup',marketId:'101',exchangeId:'101',from:at(3),to:at(12),complete:true,projectedThroughSequence:'19'};
  const pending=s.orders.find(q=>q.side==='NO');
  advanceMoloch(s,coverage);assert.equal(pending.retired,false);
  advanceMoloch(s,{...coverage,receivedAt:at(14),projectedThroughSequence:'20'});assert.equal(pending.retired,true);assertMoloch(s);
});
test('funding caps, stale liquidation, receipt causality and immutable policy are enforced',()=>{
  const s=createMoloch({maximumCommitted:'0.500000',maximumMarketCost:'0.500000',quantity:5});advanceMoloch(s,book(0));assert.ok(Number(reportMoloch(s).reservedCash)<=.5);
  advanceMoloch(s,trade(4,'a'));advanceMoloch(s,tick(20));assert.equal(reportMoloch(s).netLiquidationPnl,null);
  assert.throws(()=>advanceMoloch(s,book(2)),/RECEIPT_ORDER/);s.policy.quantity=99;assert.throws(()=>advanceMoloch(s,tick(21)),/POLICY_CHANGED/);
});
test('fractional participation accumulates observed volume without enlarging a single print',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'a','0.440000',50));assert.equal(s.fills.length,0);advanceMoloch(s,trade(5,'b','0.440000',50));assert.equal(s.fills.length,1);assert.equal(s.fills[0].quantity,1);
});
test('opposite acquisition liability cannot pledge the same inventory to an exit',()=>{
  const s=createMoloch({quantity:5});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'a','0.440000',100));advanceMoloch(s,book(8,'101',2));
  assert.equal(s.orders.filter(q=>q.action==='sell').length,0);assertMoloch(s);
});
test('actual processing clock controls activation; delayed historical prints cannot fill a newly processed quote',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,{...book(0),processedAt:at(5)});
  assert.equal(s.orders[0].activationAt,base+8000);
  advanceMoloch(s,{...trade(6,'historical','0.440000',100,4),processedAt:at(7)});assert.equal(s.fills.length,0);
  advanceMoloch(s,{...trade(9,'future'),processedAt:at(10)});assert.equal(s.fills.length,1);
});
test('duplicate delivery still advances processing chronology and budget accounting',()=>{
  const s=createMoloch();advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'a'));advanceMoloch(s,trade(20,'a','0.440000',100,4));
  assert.equal(s.lastAt,at(20));assert.throws(()=>advanceMoloch(s,tick(19)),/RECEIPT_ORDER/);
});
test('funding reserves per-share fee rounding for partial fills',()=>{
  const s=createMoloch({initialCash:'1.635164',maximumCommitted:'1.635164',maximumMarketCost:'1.635164',quantity:3,feeBps:1});
  advanceMoloch(s,book(0));const no=s.orders.find(q=>q.side==='NO');assert.equal(no.quantity,2);
  for(let n=4;n<=5;n++)advanceMoloch(s,trade(n,'partial'+n,'0.465000',100));assertMoloch(s);
  assert.ok(Number(s.cash)>=Number(reportMoloch(s).reservedCash));
});
