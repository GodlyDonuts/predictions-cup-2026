import test from 'node:test';import assert from 'node:assert/strict';
import {createMoloch,advanceMoloch} from './engine.mjs';
const at=n=>new Date(Date.parse('2026-10-14T00:00:00Z')+n*1000).toISOString();
const book=(n,bid,ask)=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:'101',exchangeId:'101',sequence:String(n),requestStartedAt:at(n),receivedAt:at(n),sourceAt:at(n),bids:[{price:bid,quantity:100}],asks:[{price:ask,quantity:100}]}});
function owned(policy={}){const s=createMoloch({quantity:1,inventoryTakerAfterMs:0,inventoryMinimumGain:'0.005000',...policy});s.positions={'101':{YES:[{quantity:1,price:'0.450000'}],NO:[]}};s.cash='999.550000';s.fills=[{orderId:0,exchangeId:'101',side:'YES',action:'buy',price:'0.450000',quantity:1,fee:'0.000000',at:at(0)}];return s;}
test('minimum net gain avoids break-even taker exit and admits one-tick opportunity',()=>{
 const a=owned();advanceMoloch(a,book(1,'0.450000','0.455000'));assert.equal(a.orders.some(q=>q.executionStyle==='inventoryAON'),false);
 const b=owned();advanceMoloch(b,book(1,'0.455000','0.460000'));assert.equal(b.orders[0].executionStyle,'inventoryAON');assert.equal(b.orders[0].price,'0.455000');
});
test('fees reduce qualification but maximum age retains a forced losing exit',()=>{
 const a=owned({feePerShare:'0.003000'});advanceMoloch(a,book(1,'0.455000','0.460000'));assert.equal(a.orders.some(q=>q.executionStyle==='inventoryAON'),false);
 const b=owned({maximumInventoryAgeMs:60000});advanceMoloch(b,book(61,'0.440000','0.445000'));assert.equal(b.orders[0].inventoryExitReason,'maximumAge');
 assert.throws(()=>createMoloch({inventoryMinimumGain:'-0.005000'}),/INVENTORY_MINIMUM_GAIN/);
});
