import test from 'node:test';import assert from 'node:assert/strict';import {createMoloch,advanceMoloch} from './engine.mjs';
const at=n=>new Date(Date.UTC(2026,9,15)+n*1000).toISOString();
const book=(n,bid='0.450000',ask='0.460000')=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:'1',exchangeId:'2',sequence:String(n),sourceAt:at(n),requestStartedAt:at(n),receivedAt:at(n),bids:[{price:bid,quantity:1000}],asks:[{price:ask,quantity:1000}]}});
const trade=(n,yesPrice,id='t'+n,idNamespace='sig_engine_match')=>({type:'trade',receivedAt:at(n),executedAt:at(n),id,idNamespace,tournamentId:'cup',marketId:'1',exchangeId:'2',yesPrice,quantity:100});
test('flat quotes wait for two prior-observable current-price sides and cannot fill from the triggering print',()=>{
 const s=createMoloch({requireCurrentFlow:true});advanceMoloch(s,book(0));assert.equal(s.orders.length,0);
 advanceMoloch(s,trade(4,'0.450000'));assert.equal(s.orders.length,0);advanceMoloch(s,trade(5,'0.460000'));assert.equal(s.orders.length,2);assert.equal(s.fills.length,0);assert.ok(s.orders.every(q=>q.activationAt===Date.parse(at(8))));
});
test('old price regimes, reporting-only prints and pre-lead observations cannot open the gate',()=>{
 const s=createMoloch({requireCurrentFlow:true});advanceMoloch(s,book(0));advanceMoloch(s,trade(1,'0.450000'));advanceMoloch(s,trade(2,'0.460000'));assert.equal(s.orders.length,0);
 advanceMoloch(s,trade(4,'0.450000','report','sig_reporting_tape_only'));advanceMoloch(s,trade(5,'0.460000'));assert.equal(s.orders.length,0);advanceMoloch(s,book(6,'0.455000','0.465000'));advanceMoloch(s,trade(10,'0.455000'));assert.equal(s.orders.length,0);
});
test('gap and late-received books cannot manufacture observable two-sided flow',()=>{
 const s=createMoloch({requireCurrentFlow:true});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'0.450000'));advanceMoloch(s,{type:'gap',receivedAt:at(5),exchangeId:'2'});advanceMoloch(s,book(6));advanceMoloch(s,trade(10,'0.460000'));assert.equal(s.orders.length,0);
 const late=createMoloch({requireCurrentFlow:true});advanceMoloch(late,book(10));for(const [i,p]of [[0,'0.450000'],[1,'0.460000']])advanceMoloch(late,{...trade(11+i,p),executedAt:at(5+i)});assert.equal(late.orders.length,0);
});
test('entry gate never blocks covered exits or changes default policy history',()=>{
 const s=createMoloch({requireCurrentFlow:true,entriesEnabled:false,maximumInventoryAgeMs:1000});s.positions={'2':{YES:[{quantity:1,price:'0.450000'}],NO:[]}};s.cash='999.550000';s.fills=[{orderId:0,exchangeId:'2',side:'YES',action:'buy',quantity:1,price:'0.450000',fee:'0.000000',at:at(0)}];advanceMoloch(s,book(2));assert.equal(s.orders[0].action,'sell');assert.equal(s.orders[0].executionStyle,'inventoryAON');
 const legacy=createMoloch();advanceMoloch(legacy,book(0));assert.equal(Object.hasOwn(legacy,'entryFlow'),false);assert.throws(()=>createMoloch({requireCurrentFlow:'yes'}),/CURRENT_FLOW_POLICY/);
});
