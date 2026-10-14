import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {run,completedTail,entryPolicyTransition,drainPolicyTransition,flatEntryTransition} from './run.mjs';import {payloadHash} from '../execution/ledger.mjs';import {createMoloch,advanceMoloch} from './engine.mjs';
import {captureScopes,assertTailRecoveryScopes,recoveryTailDirectories} from './live.mjs';
test('multiple independent recovery tails are bounded, distinct and merged without financial adoption',()=>{
 assert.deepEqual(recoveryTailDirectories(null),[]);assert.deepEqual(recoveryTailDirectories('a'),['a']);assert.deepEqual(recoveryTailDirectories(['a','b']),['a','b']);for(const value of [[],['a','a'],['--policy'],Array.from({length:9},(_,i)=>String(i))])assert.throws(()=>recoveryTailDirectories(value),/RECOVERY_INPUT/);
 const scopes=[{marketId:'1',exchangeId:'2'}],main={orders:[]},a=[{exchangeId:'2',activationAt:500,expiresAt:2000,retired:false}],b=[{exchangeId:'2',activationAt:1000,expiresAt:4000,retired:false}];const [p]=captureScopes(scopes,main,5000,[...a,...b]);assert.equal(Date.parse(p.recoveryFrom),500);assert.equal(Date.parse(p.recoveryTo),4001);assert.deepEqual(main,{orders:[]});
});
test('feed recovery merges independent expired quote windows and refuses omitted liabilities',()=>{
 const scopes=[{marketId:'1',exchangeId:'2'}],orders=[{marketId:'1',exchangeId:'2',activationAt:1000,expiresAt:3000,retired:false}],additional=[{marketId:'1',exchangeId:'2',activationAt:500,expiresAt:4000,retired:false}];
 const result=captureScopes(scopes,{orders},5000,additional);assert.equal(Date.parse(result[0].recoveryFrom),500);assert.equal(Date.parse(result[0].recoveryTo),4001);assertTailRecoveryScopes(scopes,{orders:additional,positions:{}});
 assert.throws(()=>assertTailRecoveryScopes(scopes,{orders:[{exchangeId:'3',retired:false}],positions:{}}),/LIABILITY_SCOPE/);assert.throws(()=>assertTailRecoveryScopes(scopes,{orders:[],positions:{'3':{YES:[{quantity:1}],NO:[]}}}),/LIABILITY_SCOPE/);
});
test('completed tail continuation reproduces original bank and consumes an unseen pre-restart receipt',async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-resume-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));const input=path.join(root,'source.jsonl'),policy=path.join(root,'policy.json'),first=path.join(root,'first'),second=path.join(root,'second');fs.writeFileSync(input,'');fs.writeFileSync(policy,JSON.stringify({initialCash:'100000.000000'}));
 let sequence=0,previous='';const append=()=>{const b={sequence:++sequence,previous,event:{type:'tick',receivedAt:new Date().toISOString()}},row={...b,hash:payloadHash(b)};previous=row.hash;fs.appendFileSync(input,JSON.stringify(row)+'\n');};
 const timer=setTimeout(append,30);await run(['--input',input,'--output',first,'--format','journal','--policy',policy,'--follow','--seconds','0.5']);clearTimeout(timer);
 const parent=completedTail(first);assert.equal(parent.snapshot.state.cash,'100000.000000');assert.equal(parent.snapshot.sequence,1);
 append();await run(['--input',input,'--output',second,'--format','journal','--resume',first,'--entry-exchanges','2','--drain','--follow','--seconds','0.25']);const next=completedTail(second);
 assert.equal(next.snapshot.state.cash,parent.snapshot.state.cash);assert.equal(next.snapshot.state.events,2);assert.equal(next.snapshot.sequence,1);assert.equal(next.snapshot.continuation.parentSnapshotHash,parent.snapshot.hash);assert.equal(next.snapshot.continuation.newPaperCapital,'0.000000');
 assert.deepEqual(next.snapshot.state.policy.entryExchangeIds,['2']);assert.equal(next.snapshot.continuation.entryPolicyTransition.priorPolicyHash,parent.snapshot.state.policyHash);
 assert.equal(next.snapshot.state.policy.entriesEnabled,false);assert.equal(next.snapshot.continuation.drainPolicyTransition.entriesEnabled,false);
 const third=path.join(root,'third');append();await run(['--input',input,'--output',third,'--format','journal','--resume',second,'--resume-entries','--execution-age','--follow','--seconds','0.25']);const fresh=completedTail(third);assert.equal(fresh.snapshot.state.cash,parent.snapshot.state.cash);assert.equal(fresh.snapshot.state.events,3);assert.equal(fresh.snapshot.state.policy.entriesEnabled,true);assert.equal(fresh.snapshot.state.policy.inventoryAgeClock,'execution');assert.equal(fresh.snapshot.continuation.newPaperCapital,'0.000000');
 const snapshot=JSON.parse(fs.readFileSync(path.join(second,'state.json')));snapshot.state.cash='100001.000000';fs.writeFileSync(path.join(second,'state.json'),JSON.stringify(snapshot));assert.throws(()=>completedTail(second),/TERMINAL_PROOF/);
 fs.writeFileSync(input,fs.readFileSync(input,'utf8').replace('tick','gap '));assert.throws(()=>completedTail(first),/SOURCE_CHANGED/);
});
test('re-entry preserves a flat bank but refuses any unresolved funded quote',()=>{
 const s=createMoloch({initialCash:'100000.060000',entriesEnabled:false}),before=structuredClone(s),strip=x=>{const{policy,policyHash,...b}=x;return b;};const t=flatEntryTransition(s,true);assert.equal(payloadHash(strip(s)),payloadHash(strip(before)));assert.equal(s.policy.inventoryAgeClock,'execution');assert.equal(t.entriesEnabled,true);
 const active=createMoloch(),at='2026-10-14T00:00:00Z';advanceMoloch(active,{type:'book',receivedAt:at,book:{tournamentId:'cup',marketId:'1',exchangeId:'2',sequence:'1',requestStartedAt:at,receivedAt:at,sourceAt:at,bids:[{price:'0.450000',quantity:1000}],asks:[{price:'0.455000',quantity:1000}]}});assert.throws(()=>flatEntryTransition(active,true),/REQUIRES_FLAT/);
});
test('entry scope transition preserves funded quotes, limits and every financial field',()=>{
 const s=createMoloch({initialCash:'100000.000000'}),at='2026-10-14T00:00:00Z';advanceMoloch(s,{type:'book',receivedAt:at,book:{tournamentId:'cup',marketId:'1',exchangeId:'2',sequence:'1',requestStartedAt:at,receivedAt:at,sourceAt:at,bids:[{price:'0.450000',quantity:1000}],asks:[{price:'0.455000',quantity:1000}]}});
 const before=structuredClone(s);const transition=entryPolicyTransition(s,['3']);const strip=x=>{const {policy,policyHash,...rest}=x;return rest;};assert.equal(payloadHash(strip(s)),payloadHash(strip(before)));assert.equal(s.policy.maximumCommitted,before.policy.maximumCommitted);assert.equal(s.policy.quantity,before.policy.quantity);assert.equal(transition.priorPolicyHash,before.policyHash);assert.throws(()=>entryPolicyTransition(s,[]),/ENTRY_TRANSITION/);assert.throws(()=>entryPolicyTransition(s,['3','3']),/ENTRY_EXCHANGES/);
 const drained=drainPolicyTransition(s);assert.equal(s.policy.entriesEnabled,false);assert.equal(payloadHash(strip(s)),payloadHash(strip(before)));assert.equal(drained.entriesEnabled,false);
});
test('resume never accepts a running snapshot without a terminal report or a new policy',async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-resume-deny-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));assert.throws(()=>completedTail(root));await assert.rejects(run(['--input','absent','--output',path.join(root,'out'),'--format','journal','--follow','--resume',root,'--policy','absent']),/RESUME_SCOPE/);
});
