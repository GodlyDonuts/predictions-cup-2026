import test from 'node:test';import assert from 'node:assert/strict';
import {createMoloch} from './engine.mjs';import {flatEntryTransition} from './run.mjs';import {payloadHash} from '../execution/ledger.mjs';
test('flat-only exit policy transition preserves all finance, limits and source history',()=>{
 const s=createMoloch({entriesEnabled:false}),clean=({policy,policyHash,...f})=>f,before=payloadHash(clean(s));
 const exitPolicy={inventoryTakerAfterMs:0,inventoryMinimumGain:'0.005000',maximumInventoryAgeMs:1800000,inventoryExitMode:'fixedGain'};const t=flatEntryTransition(s,true,exitPolicy);
 assert.equal(payloadHash(clean(s)),before);assert.deepEqual(t.exitPolicy,exitPolicy);assert.equal(s.policy.maximumCommitted,'40.000000');assert.equal(s.policy.writesPerMinute,24);assert.equal(s.policy.inventoryMinimumGain,'0.005000');
});
test('flat entry improvement changes one quote tick without resetting finance or loosening limits',()=>{
 const s=createMoloch({entriesEnabled:false}),clean=({policy,policyHash,...f})=>f,before=payloadHash(clean(s));
 const quantity=s.policy.quantity,t=flatEntryTransition(s,true,null,{insideTicks:1});assert.equal(s.policy.insideTicks,1);assert.deepEqual(t.entryPolicy,{insideTicks:1});assert.equal(payloadHash(clean(s)),before);assert.equal(s.policy.quantity,quantity);assert.equal(s.policy.latencyMs,3000);assert.equal(s.policy.writesPerMinute,24);
 assert.throws(()=>flatEntryTransition(s,true,null,{insideTicks:2}),/ENTRY_POLICY_SCOPE/);assert.throws(()=>flatEntryTransition(s,true,null,{initialCash:'100000.000000'}),/ENTRY_POLICY_SCOPE/);
 s.orders.push({retired:false,action:'sell',exchangeId:'1',side:'YES',quantity:0,filled:0});assert.throws(()=>flatEntryTransition(s,true,null,{insideTicks:1}),/REENTRY_REQUIRES_FLAT/);
});
test('exit policy cannot change capital or alter a funded unresolved quote',()=>{
 const s=createMoloch();assert.throws(()=>flatEntryTransition(s,false,{initialCash:'100000.000000'}),/EXIT_POLICY_SCOPE/);assert.throws(()=>flatEntryTransition(s,false,{}),/EXIT_POLICY_SCOPE/);
 s.orders.push({retired:false,action:'sell',exchangeId:'1',side:'YES',quantity:0,filled:0});assert.throws(()=>flatEntryTransition(s,false,{inventoryMinimumGain:'0.005000'}),/REENTRY_REQUIRES_FLAT/);
});
