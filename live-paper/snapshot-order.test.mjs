import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {digest} from './model.mjs';
import {orderedSnapshots,RECEIPT_SNAPSHOT_ORDER as RECEIPT,LEGACY_SNAPSHOT_ORDER as LEGACY} from './snapshot-order.mjs';
import {captureRows} from './ml-bank.mjs';
import {readTrainingSource} from './training-source.mjs';
import {replayJournal} from './replay.mjs';
import {advanceShadow as first} from './ml-shadow.mjs';
import {advanceShadow as second} from './ml-shadow-v2.mjs';
const c={...JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),marketIds:['377','381'],feeBps:0};
const epoch=Date.parse('2026-10-04T00:00:00Z'),at=n=>new Date(epoch+n*1000).toISOString();
function snapshot(marketId,n){return {endpoint:'/markets/'+marketId+'/orderbook?tournamentId='+c.tournamentId,
 requestStartedAt:at(n-.1),receivedAt:at(n),data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{
  exchangeId:marketId==='377'?'9':'10',option:'YES',asOf:{at:at(n),sequence:n},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]
 }]}}]}};}
const event=()=>({type:'cycle',clockAt:at(3),accountExclusions:[],sourceSnapshots:[snapshot('377',2)],supplementalDepth:[snapshot('381',1)]});
test('versioned ordering uses actual receipt clocks without changing captured arrays or old bank semantics',()=>{
 const e=event(),original=structuredClone(e);
 assert.deepEqual(orderedSnapshots(e,c).map(x=>x.receivedAt),[at(2),at(1)]);
 assert.deepEqual(orderedSnapshots(e,c,LEGACY).map(x=>x.receivedAt),[at(2),at(1)]);
 assert.deepEqual(orderedSnapshots(e,c,RECEIPT).map(x=>x.receivedAt),[at(1),at(2)]);
 assert.deepEqual(e,original);
 e.supplementalDepth[0].receivedAt=at(2);
 assert.deepEqual(orderedSnapshots(e,c,RECEIPT).map(x=>x.endpoint),[e.sourceSnapshots[0].endpoint,e.supplementalDepth[0].endpoint]);
 assert.throws(()=>orderedSnapshots(e,c,'unknown'),/Unknown snapshot order/);
 e.sourceSnapshots[0].receivedAt='bad';assert.throws(()=>orderedSnapshots(e,c,RECEIPT),/Invalid snapshot receipt/);
});
test('skips share a chronological receipt frontier, including event-clock fallback',()=>{
 const e=event();e.sourceSnapshots.push({marketId:'377',skipped:'outage',at:at(.5)});
 e.supplementalDepth.push({marketId:'381',skipped:'outage'});
 const sorted=orderedSnapshots(e,c,RECEIPT,at(4));
 assert.equal(sorted[0].at,at(.5));assert.equal(sorted.at(-1).marketId,'381');assert.equal(sorted.at(-1).receivedAt,undefined);
});
test('interleaved real-capture shape replays and extracts causal features identically from original bytes and verified streaming',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-order-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
 const body={sequence:1,previous:'',at:at(3),state:{},event:event()},raw=JSON.stringify({...body,hash:digest(body)})+'\n';
 const file=path.join(dir,'source.jsonl');fs.writeFileSync(file,raw);const source=readTrainingSource(file);
 assert.throws(()=>captureRows(source,c),/Observation time moved backward/);
 assert.throws(()=>replayJournal(source,c),/Observation time moved backward/);
 const options={snapshotOrderPolicy:RECEIPT},rows=captureRows(source,c,options);
 assert.deepEqual(rows,captureRows(raw,c,options));assert.deepEqual(rows.rows.map(x=>x.at),[at(1),at(2)]);
 assert.deepEqual(replayJournal(source,c,options),replayJournal(raw,c,options));
});
test('both wallet consumers follow new bank receipt policy, retain missing-field legacy and reject unknown policy before mutation',()=>{
 for(const advance of [first,second]){
  const common={config:c,trainingCaptureThrough:at(0),selected:[{id:'constant',horizon:60,weights:[.1],means:[0],scales:[1],rmse:.02}]};
  const records=[{sequence:1,hash:'fixture',at:at(3),event:event()}];
  const input=bank=>({bankHash:digest(bank),sourceSequence:0,sourceHash:null,wallets:{},histories:{},predictions:{}});
  assert.throws(()=>advance(input(common),records,common,{allowUnboundFixture:true}),/Observation time moved backward/);
  const bank={...common,snapshotOrderPolicy:RECEIPT},original=input(bank),state=advance(original,records,bank,{allowUnboundFixture:true});
  for(const wallet of Object.values(state.wallets))assert.equal(wallet.lastAt,at(2));
  assert.equal(state.histories['9:YES'][0].at,at(2));assert.equal(state.histories['10:YES'][0].at,at(1));
  assert.deepEqual(original,input(bank));assert.deepEqual(advance(state,records,bank,{allowUnboundFixture:true}),state);
  const unknown={...common,snapshotOrderPolicy:'unknown'},before=input(unknown);
  assert.throws(()=>advance(before,[],unknown,{allowUnboundFixture:true}),/Unknown snapshot order/);assert.deepEqual(before,input(unknown));
 }
});
