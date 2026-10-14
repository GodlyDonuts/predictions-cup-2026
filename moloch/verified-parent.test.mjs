import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {createHash} from 'node:crypto';
import {completedTail,reuseCompletedTail} from './run.mjs';import {createMoloch,advanceMoloch,reportMoloch} from './engine.mjs';import {payloadHash} from '../execution/ledger.mjs';
const hash=b=>createHash('sha256').update(b).digest('hex');
function fixture(parentDirectory=null){
 const parent=parentDirectory?JSON.parse(fs.readFileSync(path.join(parentDirectory,'state.json'))):null;
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-proof-')),source=path.join(dir,'source.jsonl'),state=parent?structuredClone(parent.state):createMoloch(),event={type:'tick',receivedAt:parent?'2026-10-14T00:00:01.000Z':'2026-10-14T00:00:00.000Z'};
 const sourceBody={sequence:1,previous:'',event},sourceBytes=JSON.stringify({...sourceBody,hash:payloadHash(sourceBody)})+'\n';fs.writeFileSync(source,sourceBytes);advanceMoloch(state,event);
 const body={sequence:1,previous:'',event,stateHash:payloadHash(state)},row={...body,hash:payloadHash(body)};fs.writeFileSync(path.join(dir,'events.jsonl'),JSON.stringify(row)+'\n');
 const snapshot={state,source:{path:source,consumedBytes:Buffer.byteLength(sourceBytes),consumedSha256:hash(sourceBytes)},sequence:1,lastHash:row.hash,...(parent?{continuation:{parentDirectory,parentSnapshotHash:parent.hash,newPaperCapital:'0.000000'}}:{})};fs.writeFileSync(path.join(dir,'state.json'),JSON.stringify({...snapshot,hash:payloadHash(snapshot)}));
 fs.writeFileSync(path.join(dir,'report.json'),JSON.stringify({...reportMoloch(state),mode:'local_paper_tail',eventJournalHash:row.hash,source:{bytes:Buffer.byteLength(sourceBytes),sha256:hash(sourceBytes)}}));
 return{dir,source,cleanup:()=>{if(path.dirname(fs.realpathSync(dir))!==fs.realpathSync(os.tmpdir())||!path.basename(dir).startsWith('moloch-proof-'))throw Error('TEST_CLEANUP_PATH');fs.rmSync(dir,{recursive:true,force:true});}};
}
test('reuse requires a locally replayed proof and rechecks exact bytes without rejecting unrelated source suffixes',()=>{
 const f=fixture();try{const p=completedTail(f.dir);assert.equal(reuseCompletedTail(p),p);assert.throws(()=>reuseCompletedTail(structuredClone(p)),/UNVERIFIED_PARENT/);fs.appendFileSync(f.source,'later source suffix');assert.equal(reuseCompletedTail(p),p);p.snapshot.state.cash='999.000000';assert.throws(()=>reuseCompletedTail(p),/UNVERIFIED_PARENT/);}finally{f.cleanup();}
});
test('changed source prefix, snapshot, report or journal invalidates the prepared proof',()=>{
 for(const target of ['source.jsonl','state.json','report.json','events.jsonl']){const f=fixture();try{const p=completedTail(f.dir),file=path.join(f.dir,target),bytes=fs.readFileSync(file);bytes[0]=bytes[0]===123?91:123;fs.writeFileSync(file,bytes);assert.throws(()=>reuseCompletedTail(p),/VERIFIED_PARENT_CHANGED/);}finally{f.cleanup();}}
});
test('reuse checks ancestor evidence rather than trusting only the last bank',()=>{
 const parent=fixture(),child=fixture(parent.dir);try{const proof=completedTail(child.dir);assert.equal(reuseCompletedTail(proof),proof);fs.appendFileSync(path.join(parent.dir,'report.json'),' ');assert.throws(()=>reuseCompletedTail(proof),/VERIFIED_PARENT_CHANGED/);}finally{child.cleanup();parent.cleanup();}
});
