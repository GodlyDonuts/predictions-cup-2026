import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {digest} from './model.mjs';import {atomicJson} from '../execution/ledger.mjs';import {validateUniverse} from './breadth.mjs';
import {projectAnchorReadFocus,readAnchorReadFocus,prioritizeAnchorReads,anchorFocusMarkets} from './bid-anchor-read-focus.mjs';
const universe=validateUniverse({tournamentId:'fixture',exchanges:['1','2','3','4','5','6'].map(marketId=>({marketId,exchangeId:String(100+Number(marketId)),option:'YES'}))},'fixture');
const wallets=()=>({'1000':{positions:[{instrument:'105:YES',quantity:1}],pending:[{action:'sell'}]},'3000':{positions:[],pending:[{action:'buy',createdAt:'2026-10-03T07:00:00Z'}]}});
const signals=[{marketId:'6',actualAvailableAt:'2026-10-03T07:00:00Z'}];
test('risk focus deduplicates held wallets and maps pending buys from actual causal receipts',()=>{
 assert.deepEqual(anchorFocusMarkets({wallets:wallets(),signals},universe),[{marketId:'5',phase:'held'},{marketId:'6',phase:'entry_pending'}]);
 const s={wallets:wallets(),signals:[]};assert.throws(()=>anchorFocusMarkets(s,universe));s.wallets['1000'].positions[0].instrument='unknown';assert.throws(()=>anchorFocusMarkets(s,universe));
});
test('paper risk replaces optional slots only without expanding reads or admitting orders',()=>{
 const plan={reads:[{marketId:'1',reason:'held'},{marketId:'2',reason:'edge_trial_deadline'},{marketId:'3',reason:'edge_observation'},{marketId:'4',reason:'discovery'}],entryMarketIds:['1','2','3','4']},focus={markets:[{marketId:'5',phase:'held'},{marketId:'6',phase:'entry_pending'}]};
 const p=prioritizeAnchorReads(plan,focus);assert.deepEqual(p.reads.map(x=>x.marketId),['1','2','5','6']);assert.equal(p.reads.length,4);assert.equal(p.bidAnchorReadFocus.requestsAdded,0);assert.equal(p.bidAnchorReadFocus.executionAuthority,false);assert.deepEqual(p.entryMarketIds,['1','2']);assert.equal(plan.reads[2].marketId,'3');
 for(const overlay of [{comparisonSampling:{}},{basketMeasurement:{marketIds:['3']}},{paperPackageFocus:{marketIds:['3']}}]){const reserved={...plan,...overlay};assert.equal(prioritizeAnchorReads(reserved,focus),reserved);}
 const excluded=prioritizeAnchorReads(plan,focus,{excludedMarketIds:['5']});assert.deepEqual(excluded.bidAnchorReadFocus.marketIds,['6']);
});
test('fresh active lock, original checkpoint and complete stats binding are required',t=>{
 const rt=fs.mkdtempSync(path.join(os.tmpdir(),'sig-anchor-focus-'));t.after(()=>{assert.equal(path.dirname(rt),os.tmpdir());assert.ok(path.basename(rt).startsWith('sig-anchor-focus-'));fs.rmSync(rt,{recursive:true,force:true});});const cohort='anchor-test',dir=path.join(rt,'bid-anchor-live-cohorts',cohort);fs.mkdirSync(dir,{recursive:true});
 const rawUniverse=structuredClone(universe);delete rawUniverse.hash;const bank={universe:rawUniverse},bankHash=digest(bank),now=Date.now(),lock={pid:process.pid,id:'live-owner'},body={sequence:2,previous:'a'.repeat(64),bankHash,kind:'batch',availableAt:new Date(now).toISOString(),stateHash:'b'.repeat(64)},journalHash=digest(body);
 const stats={bankHash,cohort,mode:'bid_anchor_actual_receipt_paper',updatedAt:new Date(now).toISOString(),failed:false,sourceClosed:false,networkRequests:0,competitionOrders:0,stateHash:body.stateHash,journalSequence:2,journalHash,wallets:wallets(),signals};
 atomicJson(path.join(rt,'bid-anchor-live-active.json'),{cohort,bankHash});fs.writeFileSync(path.join(dir,'bank.json'),JSON.stringify(bank));atomicJson(path.join(dir,'worker.lock'),lock);fs.writeFileSync(path.join(dir,'journal.jsonl'),JSON.stringify({...body,hash:journalHash})+'\n');
 const publish=()=>{atomicJson(path.join(dir,'stats.json'),stats);atomicJson(path.join(dir,'read-focus.json'),projectAnchorReadFocus(stats,lock,'edge-test'));};publish();assert.equal(readAnchorReadFocus(rt,universe,now).markets.length,2);assert.equal(readAnchorReadFocus(rt,universe,now+20001),null);
 stats.wallets['1000'].positions[0].quantity=2;atomicJson(path.join(dir,'stats.json'),stats);assert.equal(readAnchorReadFocus(rt,universe,now),null);publish();assert.ok(readAnchorReadFocus(rt,universe,now));
 fs.writeFileSync(path.join(dir,'journal.jsonl'),JSON.stringify({...body,stateHash:'c'.repeat(64),hash:journalHash})+'\n');assert.equal(readAnchorReadFocus(rt,universe,now),null);fs.writeFileSync(path.join(dir,'journal.jsonl'),JSON.stringify({...body,hash:journalHash})+'\n');
 atomicJson(path.join(dir,'worker.lock'),{...lock,id:'other-owner'});assert.equal(readAnchorReadFocus(rt,universe,now),null);atomicJson(path.join(dir,'worker.lock'),lock);
 fs.writeFileSync(path.join(rt,'BID_ANCHOR_LIVE_STOP'),'human');assert.equal(readAnchorReadFocus(rt,universe,now),null);
});
