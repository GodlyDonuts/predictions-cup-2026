import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {payloadHash,atomicJson} from '../execution/ledger.mjs';
import {MAKER_JOIN_CONTROL_SCHEDULER_V1} from '../paper/maker-join-control-v1.mjs';
import {updateMakerProspectiveV7} from './maker-prospective-consumer-v7.mjs';

// Characterizes V7's mixed-history control, not three new recovery samples.
// Synthetic quotes below are not market observations or profit evidence.
function fixture(t, inheritedSamples) {
  const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'maker-v5-warmup-'));
  t.after(()=>fs.rmSync(runtimeDir,{recursive:true,force:true}));
  const clock=Date.parse('2026-10-13T02:00:00.000Z'), at=new Date(clock).toISOString();
  const policy={tournamentId:'cup',scheduler:MAKER_JOIN_CONTROL_SCHEDULER_V1,participationBps:100,
    wallet:{cutoff:at,initialCash:'100.000000',maximumCommitted:'5.000000',maximumMarkets:3,
      maximumOrderCost:'1.000000',quantity:1,latencyMs:3000,lifetimeMs:120000,
      maximumGapMs:120000,minimumSamples:1,minimumHistoryMs:0,depthFractionBps:100,fillModel:'strict_tape_through'}};
  const cursor=file=>{fs.writeFileSync(file,'');const s=fs.statSync(file);return{dev:s.dev,ino:s.ino,bytes:0,sequence:0,hash:'a'.repeat(64)};};
  const books=cursor(path.join(runtimeDir,'public-books.jsonl'));
  const tape=cursor(path.join(runtimeDir,'anonymous-trades.jsonl'));
  const cohort='test-mixed-warmup',dir=path.join(runtimeDir,'maker-prospective-cohorts',cohort);
  fs.mkdirSync(dir,{recursive:true});
  const body={cohort,tournamentId:'cup',policy,codeIdentity:{},minimumSamples:12,minimumRecoverySamples:3,
    markoutReserve:{perShare:'0.000000',sourceHash:'a'.repeat(64),knownAt:at},
    bookAnchor:books,tapeAnchor:tape,publication:{scope:{exchanges:[{marketId:'11',exchangeId:'101'}]},books:{}},
    history:{'101':Array.from({length:inheritedSamples},(_,i)=>({at:clock-(inheritedSamples-i)*1000,mid:.4525}))}};
  const bank={...body,bankHash:payloadHash(body)};
  atomicJson(path.join(dir,'bank.json'),bank);
  atomicJson(path.join(runtimeDir,'maker-prospective-active-v7.json'),{cohort,bankHash:bank.bankHash});
  const book={marketId:'11',exchangeId:'101',requestStartedAt:new Date(clock-500).toISOString(),
    receivedAt:new Date(clock-100).toISOString(),sourceAt:at,sequence:'100',versionKey:'100:'+at,
    bids:[{price:'0.450000',quantity:1000}],asks:[{price:'0.455000',quantity:1000}],
    origin:'separate_realtime_full200_recovery',sourceBinding:{snapshotHash:'b'.repeat(64)}};
  const publication={at,tournamentId:'cup',executionAuthority:false,books:[book]};
  atomicJson(path.join(runtimeDir,'maker-ready-recoveries-v1.json'),{...publication,publicationHash:payloadHash(publication)});
  return{runtimeDir,dir,clock,policy,bank,book};
}

test('V7 can quote first new recovery using inherited history; identical recovery is counted once',t=>{
  const f=fixture(t,3);
  const first=updateMakerProspectiveV7({runtimeDir:f.runtimeDir,now:()=>f.clock});
  assert.equal(first.recoveryBooksConsumed,1);
  assert.equal(first.report.submittedQuotes,2);assert.equal(first.queueModels.observedQuotes,2);assert.equal(first.report.fills,0);
  const second=updateMakerProspectiveV7({runtimeDir:f.runtimeDir,now:()=>f.clock+1000});
  assert.equal(second.recoveryBooksConsumed,1);
  assert.equal(second.report.submittedQuotes,2);
  assert.equal(second.report.realized,'0.000000');
});

test('without inherited history the first recovery does not satisfy V7 warmup',t=>{
  const f=fixture(t,0);
  const report=updateMakerProspectiveV7({runtimeDir:f.runtimeDir,now:()=>f.clock});
  assert.equal(report.recoveryBooksConsumed,1);
  assert.equal(report.report.submittedQuotes,0);
});


import {Journal} from './transport-runtime.mjs';
test('integrated improvement quote records exact original depth while retaining strict financial matcher',t=>{const f=fixture(t,3),book={...f.book,bids:[{price:'0.450000',quantity:900}],asks:[{price:'0.460000',quantity:100}]},at=new Date(f.clock).toISOString(),publication={at,tournamentId:'cup',executionAuthority:false,books:[book]};atomicJson(path.join(f.runtimeDir,'maker-ready-recoveries-v1.json'),{...publication,publicationHash:payloadHash(publication)});const result=updateMakerProspectiveV7({runtimeDir:f.runtimeDir,now:()=>f.clock}),state=new Journal(path.join(f.dir,'journal.jsonl')).last.state;assert.equal(result.report.fills,0);const yes=state.wallet.wallet.quotes.find(q=>q.side==='YES');assert.equal(yes.limit,'0.455000');assert.equal(yes.queueDecisionBook.bids[0].quantity,900);assert.equal(yes.queueDecisionBook.originalSourceBookHash,payloadHash(book));assert.equal(yes.decisionBookHash,payloadHash(yes.queueDecisionBook));assert.equal(result.queueModels.cashAffected,false);assert.equal(result.report.cash,'100.000000');});
