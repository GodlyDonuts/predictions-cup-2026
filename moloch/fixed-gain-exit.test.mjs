import test from 'node:test';import assert from 'node:assert/strict';
import {createMoloch,advanceMoloch,reportMoloch} from './engine.mjs';
import {amount,money,fee} from '../paper/fixed.mjs';
const at=n=>new Date(Date.parse('2026-10-14T00:00:00Z')+n*1000).toISOString();
const book=(n,bid='0.450000',ask='0.465000')=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:'101',exchangeId:'101',sequence:String(n+1),requestStartedAt:at(n),receivedAt:at(n),sourceAt:at(n),bids:[{price:bid,quantity:100}],asks:[{price:ask,quantity:100}]}});
function owned(overrides={},basis='0.450000',side='YES'){const s=createMoloch({entriesEnabled:false,quantity:1,inventoryExitMode:'fixedGain',inventoryMinimumGain:'0.005000',maximumInventoryAgeMs:1800000,...overrides}),f=fee(amount(basis),1,s.policy);s.cash=money(amount(s.cash)-amount(basis)-f);s.realized=money(-f);s.fees=money(f);s.positions={'101':{YES:[],NO:[]}};s.positions['101'][side]=[{quantity:1,price:basis}];s.fills=[{orderId:0,exchangeId:'101',side,action:'buy',quantity:1,price:basis,fee:money(f),at:at(0)}];return s;}
test('fixed profit offer captures eligible opposing flow rather than following the latest ask',()=>{
 const a=owned(),b=owned({inventoryExitMode:'join'});for(const s of [a,b])advanceMoloch(s,book(0));assert.equal(a.orders[0].price,'0.455000');assert.equal(b.orders[0].price,'0.465000');
 const event={type:'trade',receivedAt:at(4),executedAt:at(4),tournamentId:'cup',marketId:'101',exchangeId:'101',id:'flow',yesPrice:'0.460000',quantity:100};advanceMoloch(a,event);advanceMoloch(b,event);assert.equal(reportMoloch(a).realized,'0.005000');assert.equal(reportMoloch(b).realized,'0.000000');
});
test('fixed offers retain queue price when the market ask changes and include both fees',()=>{
 const s=owned({feePerShare:'0.002000'});advanceMoloch(s,book(0));assert.equal(s.orders[0].price,'0.460000');advanceMoloch(s,book(4,'0.450000','0.470000'));assert.equal(s.orders.length,1);assert.equal(s.orders[0].cancelEffectiveAt,undefined);
 const no=owned({},'0.550000','NO');advanceMoloch(no,book(0));assert.equal(no.orders[0].price,'0.555000');
});
test('unreachable profit targets never cross the price ceiling; age stop still permits loss',()=>{
 const s=owned({},'0.995000');advanceMoloch(s,book(0,'0.990000','0.995000'));assert.equal(s.orders.length,0);
 const aged=owned();advanceMoloch(aged,book(1801,'0.440000','0.445000'));assert.equal(aged.orders[0].executionStyle,'inventoryAON');assert.equal(aged.orders[0].inventoryExitReason,'maximumAge');
 assert.throws(()=>createMoloch({inventoryExitMode:'fixedGain'}),/INVENTORY_EXIT_MODE/);
});
