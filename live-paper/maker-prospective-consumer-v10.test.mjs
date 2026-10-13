import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {payloadHash,atomicJson} from '../execution/ledger.mjs';
import {MAKER_JOIN_CONTROL_SCHEDULER_V1} from '../paper/maker-join-control-v1.mjs';
import {updateMakerProspectiveV10} from './maker-prospective-consumer-v10.mjs';

// Characterizes V10's mixed-history control, not three new recovery samples.
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
  const tape=cursor(path.join(runtimeDir,'anonymous-trades.jsonl'));tape.hash='';
  const cohort='test-mixed-warmup',dir=path.join(runtimeDir,'maker-prospective-cohorts',cohort);
  fs.mkdirSync(dir,{recursive:true});
  const body={cohort,tournamentId:'cup',pushSourcePolicy:{requireContinuous:false},policy,codeIdentity:{},minimumSamples:12,minimumRecoverySamples:3,
    markoutReserve:{perShare:'0.000000',sourceHash:'a'.repeat(64),knownAt:at},
    bookAnchor:books,tapeAnchor:tape,publication:{scope:{exchanges:[{marketId:'11',exchangeId:'101'}]},books:{}},
    history:{'101':Array.from({length:inheritedSamples},(_,i)=>({at:clock-(inheritedSamples-i)*1000,mid:.4525}))}};
  const bank={...body,bankHash:payloadHash(body)};
  atomicJson(path.join(dir,'bank.json'),bank);
  atomicJson(path.join(runtimeDir,'maker-prospective-active-v10.json'),{cohort,bankHash:bank.bankHash});
  const book={marketId:'11',exchangeId:'101',requestStartedAt:new Date(clock-500).toISOString(),
    receivedAt:new Date(clock-100).toISOString(),sourceAt:at,sequence:'100',versionKey:'100:'+at,
    bids:[{price:'0.450000',quantity:1000}],asks:[{price:'0.455000',quantity:1000}],
    origin:'separate_realtime_full200_recovery',sourceBinding:{snapshotHash:'b'.repeat(64)}};
  const publication={at,tournamentId:'cup',executionAuthority:false,books:[book]};
  atomicJson(path.join(runtimeDir,'maker-ready-recoveries-v1.json'),{...publication,publicationHash:payloadHash(publication)});
  fs.writeFileSync(path.join(runtimeDir,'realtime-test.jsonl'),JSON.stringify({format:'sig-feed-capture',captureIndex:0,kind:'config',observedAt:at})+'\n');atomicJson(path.join(runtimeDir,'realtime-session.json'),{file:'realtime-test.jsonl',scope:{tournamentId:'cup'}});return{runtimeDir,dir,clock,policy,bank,book};
}

test('V10 can quote first new recovery using inherited history; identical recovery is counted once',t=>{
  const f=fixture(t,3);
  const first=updateMakerProspectiveV10({runtimeDir:f.runtimeDir,now:()=>f.clock});
  assert.equal(first.recoveryBooksConsumed,1);
  assert.equal(first.report.submittedQuotes,2);assert.equal(first.queueModels.observedQuotes,2);assert.equal(first.report.fills,0);
  const second=updateMakerProspectiveV10({runtimeDir:f.runtimeDir,now:()=>f.clock+1000});
  assert.equal(second.recoveryBooksConsumed,1);
  assert.equal(second.report.submittedQuotes,2);
  assert.equal(second.report.realized,'0.000000');
});

test('without inherited history the first recovery does not satisfy V10 warmup',t=>{
  const f=fixture(t,0);
  const report=updateMakerProspectiveV10({runtimeDir:f.runtimeDir,now:()=>f.clock});
  assert.equal(report.recoveryBooksConsumed,1);
  assert.equal(report.report.submittedQuotes,0);
});


import {Journal} from './transport-runtime.mjs';
test('integrated improvement quote records exact original depth while retaining strict financial matcher',t=>{const f=fixture(t,3),book={...f.book,bids:[{price:'0.450000',quantity:900}],asks:[{price:'0.460000',quantity:100}]},at=new Date(f.clock).toISOString(),publication={at,tournamentId:'cup',executionAuthority:false,books:[book]};atomicJson(path.join(f.runtimeDir,'maker-ready-recoveries-v1.json'),{...publication,publicationHash:payloadHash(publication)});const result=updateMakerProspectiveV10({runtimeDir:f.runtimeDir,now:()=>f.clock}),state=new Journal(path.join(f.dir,'journal.jsonl')).last.state;assert.equal(result.report.fills,0);const yes=state.wallet.wallet.quotes.find(q=>q.side==='YES');assert.equal(yes.limit,'0.455000');assert.equal(yes.queueDecisionBook.bids[0].quantity,900);assert.equal(yes.queueDecisionBook.originalSourceBookHash,payloadHash(book));assert.equal(yes.decisionBookHash,payloadHash(yes.queueDecisionBook));assert.equal(result.queueModels.cashAffected,false);assert.equal(result.report.cash,'100.000000');});

test('live observed trade affects an already activated quote and later REST copy cannot duplicate the fill',t=>{const f=fixture(t,3);updateMakerProspectiveV10({runtimeDir:f.runtimeDir,now:()=>f.clock});const stamp=n=>new Date(f.clock+n).toISOString(),r={format:'sig-feed-capture',captureIndex:1,kind:'trade_observed',continuousAtReceipt:false,observedAt:stamp(5000),trade:{tournamentId:'cup',marketId:'11',exchangeId:'101',id:'105',sequence:105,price:1-.56,quantity:1000,executedAt:stamp(4000)}};fs.appendFileSync(path.join(f.runtimeDir,'realtime-test.jsonl'),JSON.stringify(r)+'\n');const first=updateMakerProspectiveV10({runtimeDir:f.runtimeDir,now:()=>f.clock+5000});assert.equal(first.pushObservationRows,1);assert.equal(first.nonContinuousPushRows,1);assert.equal(first.report.fills,1);assert.equal(first.report.cash,'99.550000');const page={type:'anonymous_trade_page',tournamentId:'cup',marketId:'11',exchangeId:'101',from:stamp(3000),to:stamp(6000),requestStartedAt:stamp(7000),receivedAt:stamp(7000),requestCursor:null,pagination:{hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:110},trades:[{key:'historical-105',id:'105',createdAt:stamp(4000),yesPrice:String(1-.56),size:1000,executedSpace:'YES'}]};new Journal(path.join(f.runtimeDir,'anonymous-trades.jsonl')).commit({},page);const second=updateMakerProspectiveV10({runtimeDir:f.runtimeDir,now:()=>f.clock+7000});assert.equal(second.report.fills,1);assert.equal(second.report.cash,'99.550000');assert.equal(second.report.realized,'0.000000');});

import {createMakerSelectedWalletV4,advanceMakerSelectedWalletV4} from '../paper/maker-selected-wallet-v4.mjs';
test('an optional queue provenance error is reported without preventing financial journal progress',t=>{const f=fixture(t,3),stamp=n=>new Date(f.clock+n).toISOString(),wallet=createMakerSelectedWalletV4(f.policy);advanceMakerSelectedWalletV4(wallet,{kind:'quotes',at:stamp(0),sourceHash:'a'.repeat(64),entriesEnabled:true,proposals:[{marketId:'11',exchangeId:'101',side:'yes',action:'buy',quantity:1,price:'0.450000',netEdgePerShare:'0.010000',sourceAt:stamp(0),validUntil:stamp(10000),orderExpiresAt:stamp(120000),reduceOnly:false,sourceBinding:{bookVersion:'100:'+stamp(0)}}]},f.policy);const q=wallet.wallet.quotes[0];q.queueDecisionBook=f.book;q.decisionBookHash='a'.repeat(64);q.queueStudy={pages:[],models:[],sourceHashes:[],final:false};atomicJson(path.join(f.dir,'initial-state.json'),{bankHash:f.bank.bankHash,books:f.bank.bookAnchor,tape:f.bank.tapeAnchor,publication:f.bank.publication,history:f.bank.history,wallet,targets:{},updates:0,staleBooks:0});const r=updateMakerProspectiveV10({runtimeDir:f.runtimeDir,now:()=>f.clock+4000});assert.equal(r.queueModels.status,'observer_failed');assert.equal(r.report.cash,'100.000000');assert.equal(r.report.reservedCash,'0.450000');assert.equal(new Journal(path.join(f.dir,'journal.jsonl')).sequence,1);});
