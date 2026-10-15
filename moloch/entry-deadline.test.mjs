import test from 'node:test';import assert from 'node:assert/strict';import {createMoloch,advanceMoloch,reportMoloch} from './engine.mjs';import {flatEntryTransition} from './run.mjs';import {payloadHash} from '../execution/ledger.mjs';
const at=n=>new Date(Date.parse('2026-10-15T00:00:00Z')+n*1000).toISOString();
const book=n=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:'101',exchangeId:'101',sequence:String(n+1),requestStartedAt:at(n),receivedAt:at(n),sourceAt:at(n),bids:[{price:'0.450000',quantity:100}],asks:[{price:'0.465000',quantity:100}]}});
test('deadline cancels funded entry quotes with normal latency before the feed stops',()=>{
 const s=createMoloch({quantity:1,entryStopAt:at(10)});advanceMoloch(s,book(0));advanceMoloch(s,{type:'tick',receivedAt:at(10)});advanceMoloch(s,{type:'tick',receivedAt:at(13)});
 assert.equal(s.orders.length,2);assert.ok(s.orders.every(q=>q.cancelEffectiveAt!==undefined));assert.equal(s.orders[0].cancelEffectiveAt,Date.parse(at(13)));assert.ok(reportMoloch(s).reservedCash!=='0.000000');
 advanceMoloch(s,{type:'trade',receivedAt:at(18),executedAt:at(17),tournamentId:'cup',marketId:'101',exchangeId:'101',id:'after-cancel',yesPrice:'0.440000',quantity:100});assert.equal(s.fills.length,0);
 advanceMoloch(s,book(20));assert.equal(s.orders.length,2);
});
test('deadline stops entries while retaining covered inventory exit behavior',()=>{
 const s=createMoloch({entriesEnabled:false,entryStopAt:at(1),quantity:1});s.cash='999.550000';s.positions={'101':{YES:[{quantity:1,price:'0.450000'}],NO:[]}};s.fills=[{orderId:0,exchangeId:'101',side:'YES',action:'buy',quantity:1,price:'0.450000',fee:'0.000000',at:at(0)}];s.policy.entriesEnabled=true;s.policyHash=payloadHash(s.policy);
 advanceMoloch(s,book(4));assert.equal(s.orders.length,1);assert.equal(s.orders[0].action,'sell');
});
test('flat deadline transitions preserve financial history and reject invalid clocks or financial overrides',()=>{
 const s=createMoloch({entriesEnabled:false}),clean=({policy,policyHash,...body})=>body,before=payloadHash(clean(s));flatEntryTransition(s,true,null,{entryStopAt:at(10)});assert.equal(payloadHash(clean(s)),before);
 assert.throws(()=>createMoloch({entryStopAt:'invalid'}));assert.throws(()=>flatEntryTransition(s,true,null,{entryStopAt:at(10),initialCash:'100000.000000'}),/ENTRY_POLICY_SCOPE/);
});
