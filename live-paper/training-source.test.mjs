import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {digest} from './model.mjs';
import {verifySource} from './ml-shadow-v2.mjs';
import {readTrainingSource,isVerifiedTrainingSource,trainingSourcePrefix} from './training-source.mjs';
import {replayJournal} from './replay.mjs';
import {captureRows,trainBank} from './ml-bank.mjs';
import {trainPortfolioBank} from './portfolio-bank.mjs';
const c={...JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),marketIds:['377'],feeBps:0};
const epoch=Date.parse('2026-10-01T16:00:00Z'),at=n=>new Date(epoch+n*30000).toISOString();
const sha=x=>createHash('sha256').update(x).digest('hex');
function capture(n=300){let previous='';return Array.from({length:n},(_,i)=>{
 const t=at(i),bid=i%12<3?.35:.6;
 const snap={endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,requestStartedAt:t,receivedAt:t,
  data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',asOf:{at:t,sequence:i},bids:[{price:bid,quantity:10000}],asks:[{price:bid+.01,quantity:10000}]}]}}]}};
 const event=i===17?{type:'cycle',clockAt:t,accountExclusions:[],sourceSnapshots:[{marketId:'377',skipped:'test_missing_clock'}]}:{type:'cycle',clockAt:t,accountExclusions:[],sourceSnapshots:[snap]};
 const body={sequence:i+1,previous,at:t,event,state:{skips:{'377':{at:t,reason:'test_missing_clock'}},payload:'élève 🚀'.repeat(50)}};
 const hash=digest(body);previous=hash;return '  '+JSON.stringify({...body,hash})+'  ';
}).join('\n')+'\n';}
function fixture(t,raw=capture()){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-training-source-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
 const file=path.join(dir,'capture.jsonl');fs.writeFileSync(file,raw);return {file,raw};
}
const stable=bank=>{const {createdAt,...rest}=bank;return rest;};

test('streamed original bytes retain exact prefixes and skip-clock fallback with immutable sanitized state',t=>{
 const f=fixture(t,capture(20)),s=readTrainingSource(f.file,{chunkBytes:7,maxRecordBytes:4096}),original=verifySource(f.raw);
 assert.ok(isVerifiedTrainingSource(s));assert.equal(s.sourceHash,sha(f.raw));assert.equal(s.sourceBytes,Buffer.byteLength(f.raw));
 assert.deepEqual(s.records.prefixHashes,original.prefixHashes);assert.equal(s.records[0].state.payload,undefined);
 assert.equal(s.records[17].state.skips['377'].at,at(17));
 assert.throws(()=>s.records[0].event.accountExclusions.push('9'),TypeError);
 assert.throws(()=>s.fileIdentity.ino=0,TypeError);
 const p=trainingSourcePrefix(s,9),rawPrefix=f.raw.split('\n').slice(0,9).join('\n')+'\n';
 assert.equal(p.sourceHash,sha(rawPrefix));assert.equal(p.sourceBytes,Buffer.byteLength(rawPrefix));
 assert.equal(p.finalEventClock,at(8));assert.equal(p.sourceRecords,9);
 assert.throws(()=>trainingSourcePrefix({...s},9),/Invalid verified/);
 assert.throws(()=>trainingSourcePrefix(s,0),/Invalid verified/);
});

test('corrupt unused state, partial tails, oversize records and validly rehashed clock rollback are rejected',t=>{
 const f=fixture(t,capture(20));fs.writeFileSync(f.file,f.raw.replace('élève','wrong'));assert.throws(()=>readTrainingSource(f.file),/integrity/);
 fs.writeFileSync(f.file,f.raw.slice(0,-1));assert.throws(()=>readTrainingSource(f.file),/Incomplete/);
 fs.writeFileSync(f.file,f.raw);assert.throws(()=>readTrainingSource(f.file,{chunkBytes:1,maxRecordBytes:20}),/TOO_LARGE/);
 const body={sequence:21,previous:JSON.parse(f.raw.trimEnd().split('\n').at(-1)).hash,at:at(0),state:{},event:{type:'test',clockAt:at(0)}};
 fs.appendFileSync(f.file,JSON.stringify({...body,hash:digest(body)})+'\n');assert.throws(()=>readTrainingSource(f.file),/clock rollback/);
});

test('snapshot excludes appended future rows, and unverified lookalikes cannot enter replay or prefixes',t=>{
 const f=fixture(t,capture(20));const end=Buffer.byteLength(f.raw),tail=JSON.parse(f.raw.trimEnd().split('\n').at(-1));
 const body={sequence:21,previous:tail.hash,at:at(21),state:{},event:{type:'test',clockAt:at(21)}};
 fs.appendFileSync(f.file,JSON.stringify({...body,hash:digest(body)})+'\n');const s=readTrainingSource(f.file,{endByte:end});
 assert.equal(s.sourceRecords,20);assert.equal(s.sourceHash,sha(f.raw));assert.throws(()=>replayJournal({...s},c),/Incomplete/);
});

test('whole replay, causal features and fitted model bank are exactly equal on raw and streamed inputs',t=>{
 const f=fixture(t),s=readTrainingSource(f.file);
 assert.deepEqual(replayJournal(s,c),replayJournal(f.raw,c));
 assert.deepEqual(captureRows(s,c),captureRows(f.raw,c));
 assert.deepEqual(captureRows(s,c),captureRows(s,c,{observationValidation:'legacy_portfolio_validation_v1'}));
 assert.throws(()=>captureRows(s,c,{observationValidation:'unknown'}),/Unknown feature/);
 assert.deepEqual(stable(trainBank(s,c)),stable(trainBank(f.raw,c)));
});

test('portfolio windows, purged weights, selected margin, scorecards and untouched holdout match original-byte training',t=>{
 const f=fixture(t),s=readTrainingSource(f.file);
 const a=trainPortfolioBank(f.raw,c),b=trainPortfolioBank(s,c);
 assert.deepEqual(stable(a),stable(b));assert.equal(b.sourceHash,sha(f.raw));
 assert.equal(b.portfolioSelection.fitSourceHash,s.records.prefixHashes[b.portfolioSelection.windows.fitRecords-1]);
 assert.equal(b.portfolioSelection.developmentSourceHash,s.records.prefixHashes[b.portfolioSelection.windows.selectionRecords-1]);
});
