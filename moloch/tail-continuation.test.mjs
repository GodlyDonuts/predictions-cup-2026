import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {run,completedTail} from './run.mjs';import {payloadHash} from '../execution/ledger.mjs';
import {captureScopes,assertTailRecoveryScopes} from './live.mjs';
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
 append();await run(['--input',input,'--output',second,'--format','journal','--resume',first,'--follow','--seconds','0.25']);const next=completedTail(second);
 assert.equal(next.snapshot.state.cash,parent.snapshot.state.cash);assert.equal(next.snapshot.state.events,2);assert.equal(next.snapshot.sequence,1);assert.equal(next.snapshot.continuation.parentSnapshotHash,parent.snapshot.hash);assert.equal(next.snapshot.continuation.newPaperCapital,'0.000000');
 const snapshot=JSON.parse(fs.readFileSync(path.join(second,'state.json')));snapshot.state.cash='100001.000000';fs.writeFileSync(path.join(second,'state.json'),JSON.stringify(snapshot));assert.throws(()=>completedTail(second),/TERMINAL_PROOF/);
 fs.writeFileSync(input,fs.readFileSync(input,'utf8').replace('tick','gap '));assert.throws(()=>completedTail(first),/SOURCE_CHANGED/);
});
test('resume never accepts a running snapshot without a terminal report or a new policy',async t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-resume-deny-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));assert.throws(()=>completedTail(root));await assert.rejects(run(['--input','absent','--output',path.join(root,'out'),'--format','journal','--follow','--resume',root,'--policy','absent']),/RESUME_SCOPE/);
});
