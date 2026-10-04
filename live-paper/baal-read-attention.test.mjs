import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import path from 'node:path';import os from 'node:os';
import {createBaalReadAttention} from './baal-read-attention.mjs';
test('three tape cycles plus one history cycle persist across every restart inside the original budget',t=>{
 const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-baal-aux-'));t.after(()=>fs.rmSync(runtimeDir,{recursive:true,force:true}));
 let tapes=0,histories=0;for(let n=0;n<12;n++){
  const a=createBaalReadAttention({runtimeDir,tournamentId:'test'}).reserve(1,{makerActive:true});assert.equal(a.reservedOrdinal,n);assert.equal(a.tapeReads,n%4===3?0:1);assert.equal(a.totalReads,1);assert.equal(a.historyReads+a.tapeReads,1);tapes+=a.tapeReads;histories+=a.historyReads;
 }assert.equal(tapes,9);assert.equal(histories,3);
 assert.throws(()=>createBaalReadAttention({runtimeDir,tournamentId:'other'}),/STATE_INVALID/);
});
test('STOP and zero budget reserve nothing; inactive maker returns to alternating allocation, retaining history headroom',t=>{
 const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-baal-stop-'));t.after(()=>fs.rmSync(runtimeDir,{recursive:true,force:true}));let allowed=false;
 const a=createBaalReadAttention({runtimeDir,tournamentId:'test',canOperate:()=>allowed});assert.equal(a.reserve(1,{makerActive:true}).totalReads,0);assert.equal(fs.existsSync(path.join(runtimeDir,'baal-auxiliary-reservations.json')),false);
 allowed=true;assert.equal(a.reserve(0).totalReads,0);assert.equal(a.reserve(2).tapeReads,1);assert.equal(a.reserve(2).tapeReads,0);assert.equal(a.reserve(2,{makerActive:true}).historyReads,1);assert.equal(a.reserve(2,{makerActive:true}).historyReads,2);
});
