import test from 'node:test';import assert from 'node:assert/strict';
import {createMoloch,advanceMoloch,assertMoloch} from './engine.mjs';
const at=n=>new Date(Date.parse('2026-10-14T00:00:00Z')+n*1000).toISOString();
const book=(n,bid='0.450000',ask='0.455000')=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:'101',exchangeId:'101',sequence:String(n+1),requestStartedAt:at(n),receivedAt:at(n),sourceAt:at(n),bids:[{price:bid,quantity:100}],asks:[{price:ask,quantity:100}]}});
const trade=(n,id,p)=>({type:'trade',receivedAt:at(n),executedAt:at(n),id,tournamentId:'cup',marketId:'101',exchangeId:'101',yesPrice:p,quantity:100});
test('entry backoff lowers both funded purchase limits and rejects original-touch fills',()=>{
 const s=createMoloch({entryBackoffTicks:2,quantity:1});advanceMoloch(s,book(0));
 assert.equal(s.orders.find(q=>q.side==='YES').price,'0.440000');assert.equal(s.orders.find(q=>q.side==='NO').price,'0.535000');
 advanceMoloch(s,trade(4,'original','0.450000'));assert.equal(s.fills.length,0);
 advanceMoloch(s,trade(5,'through','0.435000'));assert.equal(s.fills.length,1);assert.equal(s.fills[0].price,'0.440000');assertMoloch(s);
});
test('backoff does not produce invalid boundary prices or combine with inside improvement',()=>{
 const s=createMoloch({entryBackoffTicks:2});advanceMoloch(s,book(0,'0.005000','0.010000'));assert.equal(s.orders.length,0);
 assert.throws(()=>createMoloch({entryBackoffTicks:1,insideTicks:1}),/ENTRY_BACKOFF/);
 assert.throws(()=>createMoloch({entryBackoffTicks:11}),/ENTRY_BACKOFF/);
});
test('zero backoff preserves the exact baseline financial state',()=>{
 const a=createMoloch(),b=createMoloch({entryBackoffTicks:0});for(const e of [book(0),trade(4,'fill','0.440000'),book(8)]){advanceMoloch(a,e);advanceMoloch(b,e);}
 const clean=({policy,policyHash,...state})=>state;assert.deepEqual(clean(a),clean(b));
});
