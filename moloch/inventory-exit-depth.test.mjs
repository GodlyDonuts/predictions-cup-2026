import test from 'node:test';import assert from 'node:assert/strict';import {createMoloch,advanceMoloch,assertMoloch} from './engine.mjs';
import {drainPolicyTransition} from './run.mjs';import {payloadHash} from '../execution/ledger.mjs';
const at=n=>new Date(Date.UTC(2026,9,15)+n*1000).toISOString();
function owned(extra={}){const s=createMoloch({initialCash:'10000.000000',maximumCommitted:'10000.000000',maximumMarketCost:'10000.000000',quantity:5000,entriesEnabled:false,maximumInventoryAgeMs:1000,inventoryAgeClock:'execution',...extra});s.positions={'2':{YES:[],NO:[{quantity:2500,price:'0.150000',executedAt:at(0)}]}};s.cash='9625.000000';s.fills=[{orderId:0,exchangeId:'2',side:'NO',action:'buy',quantity:2500,price:'0.150000',fee:'0.000000',at:at(0),executedAt:at(0)}];assertMoloch(s);return s;}
const book=(n,depth=2000)=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:'1',exchangeId:'2',sequence:String(n),requestStartedAt:at(n),receivedAt:at(n),sourceAt:at(n),bids:[{price:'0.875000',quantity:9000}],asks:[{price:'0.880000',quantity:depth}]}});
test('depth-sized covered exits release a large NO position across later independent books',()=>{
 const s=owned({inventoryExitDepthBps:5000});advanceMoloch(s,book(1));assert.equal(s.orders[0].quantity,1000);assert.equal(s.cash,'9625.000000');
 for(const n of [4,7,10])advanceMoloch(s,book(n));
 assert.deepEqual(s.fills.filter(f=>f.action==='sell').map(f=>f.quantity),[1000,1000,500]);assert.equal(s.positions['2'].NO.length,0);assert.equal(s.cash,'9925.000000');assert.equal(s.realized,'-75.000000');assertMoloch(s);
});
test('proposal depth is not a fill; later insufficient depth remains unfilled',()=>{
 const s=owned({inventoryExitDepthBps:5000});advanceMoloch(s,book(1));advanceMoloch(s,book(4,999));assert.equal(s.fills.length,1);assert.equal(s.positions['2'].NO[0].quantity,2500);assert.equal(s.cash,'9625.000000');
});
test('unset sizing preserves original all-or-none behavior and rejects invalid fractions',()=>{
 const s=owned();advanceMoloch(s,book(1));assert.equal(s.orders[0].quantity,2500);advanceMoloch(s,book(4));assert.equal(s.fills.length,1);
 for(const inventoryExitDepthBps of [0,-1,10001,.5,'5000'])assert.throws(()=>createMoloch({inventoryExitDepthBps}),/INVENTORY_EXIT_DEPTH/);
});
test('forward drain sizing changes only the declared exit policy and preserves all funded history',()=>{
 const s=owned(),clean=({policy,policyHash,...rest})=>rest,before=payloadHash(clean(s)),t=drainPolicyTransition(s,{inventoryExitDepthBps:5000});assert.equal(payloadHash(clean(s)),before);assert.equal(s.policy.entriesEnabled,false);assert.deepEqual(t.exitPolicy,{inventoryExitDepthBps:5000});assert.equal(s.policy.maximumInventoryAgeMs,1000);
 assert.throws(()=>drainPolicyTransition(s,{initialCash:'100000.000000'}),/DRAIN_EXIT_SCOPE/);assert.throws(()=>drainPolicyTransition(s,{inventoryExitDepthBps:5000,quantity:1}),/DRAIN_EXIT_SCOPE/);
});
