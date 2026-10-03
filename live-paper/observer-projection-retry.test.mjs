import test from 'node:test';import assert from 'node:assert/strict';
import {writeObservedProjections,observedPaperErrorCode} from './executable-edge-observe.mjs';
test('partial cache publication retries the same derived snapshots after bounded file-lock waits',async()=>{
 const writes=[],waits=[],stats={journalHash:'a'},focus={statsHash:'b'};let failures=0;
 const result=await writeObservedProjections([['stats',stats],['focus',focus]],{write:(file,value)=>{writes.push([file,value]);if(file==='focus'&&failures++<2)throw Object.assign(Error('locked'),{code:'EPERM'});},delay:async ms=>waits.push(ms)});
 assert.equal(result.attempts,3);assert.deepEqual(waits,[25,50]);assert.equal(writes.length,6);
 assert.ok(writes.filter(([file])=>file==='stats').every(([,value])=>value===stats));
 assert.ok(writes.filter(([file])=>file==='focus').every(([,value])=>value===focus));
});
test('persistent cache failures are bounded and integrity failures are never retried',async()=>{
 let attempts=0;const waits=[];
 await assert.rejects(writeObservedProjections([['stats',{}]],{write:()=>{attempts++;throw Object.assign(Error('busy'),{code:'EBUSY'});},delay:async ms=>waits.push(ms)}),/busy/);
 assert.equal(attempts,4);assert.deepEqual(waits,[25,50,100]);attempts=0;
 await assert.rejects(writeObservedProjections([['stats',{}]],{write:()=>{attempts++;throw Error('ANCHOR_HOST_JOURNAL_CHANGED');},delay:async()=>assert.fail('Integrity failure retried')}),/JOURNAL_CHANGED/);
 assert.equal(attempts,1);
});
test('failure reports preserve actionable enums without copying paths, credentials or raw exception text',()=>{
 assert.equal(observedPaperErrorCode(Error('ANCHOR_HOST_SOURCE_CHANGED')),'ANCHOR_HOST_SOURCE_CHANGED');
 assert.equal(observedPaperErrorCode(Object.assign(Error('private filesystem path'),{code:'EACCES'})),'EACCES');
 assert.equal(observedPaperErrorCode(Error('Bearer private-token ace_private')),'PAPER_WORKER_ERROR');
 assert.equal(observedPaperErrorCode({message:'ANCHOR_HOST_bad secret',code:'not an enum'}),'PAPER_WORKER_ERROR');
});
