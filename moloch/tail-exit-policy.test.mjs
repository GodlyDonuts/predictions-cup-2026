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
test('fill-assumption experiment requires a flat bank and preserves participation and cash',()=>{
 const s=createMoloch({entriesEnabled:false}),cash=s.cash,participation=s.policy.participationBps;flatEntryTransition(s,true,null,{fillModel:'conditionalFifo',touchAttributionBps:5000});assert.equal(s.cash,cash);assert.equal(s.policy.participationBps,participation);assert.equal(s.policy.fillModel,'conditionalFifo');assert.equal(s.policy.touchAttributionBps,5000);
 assert.throws(()=>flatEntryTransition(s,true,null,{fillModel:'invented'}));assert.throws(()=>flatEntryTransition(s,true,null,{participationBps:10000}),/ENTRY_POLICY_SCOPE/);
});
test('flat price-backoff experiment preserves wealth and cannot combine with inside quoting',()=>{
 const s=createMoloch({entriesEnabled:false}),clean=({policy,policyHash,...f})=>f,before=payloadHash(clean(s)),limits=[s.policy.quantity,s.policy.maximumCommitted,s.policy.participationBps];
 flatEntryTransition(s,true,{maximumInventoryAgeMs:60000},{insideTicks:0,entryBackoffTicks:4});
 assert.equal(payloadHash(clean(s)),before);assert.deepEqual([s.policy.quantity,s.policy.maximumCommitted,s.policy.participationBps],limits);assert.equal(s.policy.entryBackoffTicks,4);
 assert.throws(()=>flatEntryTransition(s,true,null,{insideTicks:1,entryBackoffTicks:4}),/ENTRY_BACKOFF/);
});
