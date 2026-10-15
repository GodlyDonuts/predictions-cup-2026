import test from 'node:test';import assert from 'node:assert/strict';
import {createMoloch,advanceMoloch,reportMoloch,assertMoloch} from './engine.mjs';
const base=Date.parse('2026-10-14T00:00:00Z'),at=n=>new Date(base+n*1000).toISOString();
const book=(n,seq,ask='0.455000')=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:'1',exchangeId:'2',sequence:String(seq),requestStartedAt:at(n),sourceAt:at(n),receivedAt:at(n),bids:[{price:'0.450000',quantity:1000}],asks:[{price:ask,quantity:1000}]}});
const trade=(n,id,yesPrice,executed=n)=>({type:'trade',marketId:'1',exchangeId:'2',tournamentId:'cup',receivedAt:at(n),executedAt:at(executed),id,yesPrice,quantity:100});
const policy={quantity:1,retainProfitableHedge:true,repriceProfitableHedge:true,inventoryTakerAfterMs:600000,maximumInventoryAgeMs:1800000};
function start(){const s=createMoloch(policy);advanceMoloch(s,book(0,1));advanceMoloch(s,trade(4,'entry','0.440000'));advanceMoloch(s,book(6,2,'0.465000'));return s;}
const coverage=(n,to)=>({type:'coverage',receivedAt:at(n),marketId:'1',exchangeId:'2',tournamentId:'cup',from:at(3),to:at(to),complete:true,projectedThroughSequence:'3'});
test('fresh hedge waits for old quote reconciliation and nets only owned shares',()=>{
 const s=start();assert.equal(s.orders.find(q=>q.side==='NO').cancelEffectiveAt,base+9000);advanceMoloch(s,book(10,3,'0.465000'));assert.equal(s.orders.length,2);
 advanceMoloch(s,coverage(11,10));const hedge=s.orders.at(-1);assert.equal(hedge.side,'NO');assert.equal(hedge.quantity,1);assert.equal(hedge.price,'0.535000');assert.equal(s.orders.filter(q=>q.action==='sell').length,0);
 advanceMoloch(s,trade(15,'hedge','0.470000'));assert.equal(reportMoloch(s).ownedCost,'0.000000');assert.equal(s.realized,'0.015000');assertMoloch(s);
});
test('late old hedge fill during cancellation cannot cause a replacement oversized hedge',()=>{
 const s=start();advanceMoloch(s,trade(10,'late-old','0.465000',8));assert.equal(s.realized,'0.005000');assert.equal(reportMoloch(s).ownedCost,'0.000000');assert.equal(s.fills.length,2);assertMoloch(s);
});
test('maximum age takes precedence over a currently profitable resting hedge',()=>{
 const s=createMoloch({...policy,maximumInventoryAgeMs:0});advanceMoloch(s,book(0,1));advanceMoloch(s,trade(4,'entry','0.440000'));advanceMoloch(s,book(10,3));advanceMoloch(s,coverage(11,10));
 const q=s.orders.at(-1);assert.equal(q.action,'sell');assert.equal(q.executionStyle,'inventoryAON');assert.equal(q.inventoryExitReason,'maximumAge');assertMoloch(s);
 assert.throws(()=>createMoloch({repriceProfitableHedge:true}),/REPRICE_HEDGE/);
});
