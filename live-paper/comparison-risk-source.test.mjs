import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {digest,normalizeBook,stepDip} from './model.mjs';
import {createEntryComparison,advanceEntryComparison} from './entry-comparison.mjs';
import {entryComparisonCodeIdentity} from './entry-comparison-watch.mjs';
import {createComparisonRiskSource} from './comparison-risk-source.mjs';
import {atomicJson} from '../execution/ledger.mjs';

const base=Date.parse('2026-10-02T10:00:00.000Z'),stamp=ms=>new Date(base+ms).toISOString();
const config=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),policy=JSON.parse(fs.readFileSync(new URL('./dip-policy.json',import.meta.url)));
const universe={tournamentId:config.tournamentId,exchanges:[['377','1066'],['354','1043'],['277','966'],['305','994']].map(([marketId,exchangeId])=>({marketId,exchangeId,option:'YES'}))};
function sourceRow(sequence,previous,ms){const body={sequence,previous,at:stamp(ms),event:{type:'diagnostic',clockAt:stamp(ms)},state:{fixture:true}};return {...body,hash:digest(body)};}
function fixture(t){
 const runtime=fs.mkdtempSync(path.join(os.tmpdir(),'sig-comparison-risk-')),cohort='fixture-panel',dir=path.join(runtime,'entry-comparisons',cohort);fs.mkdirSync(dir,{recursive:true});
 t.after(()=>{const target=path.resolve(runtime);assert.equal(path.dirname(target),path.resolve(os.tmpdir()));assert.ok(path.basename(target).startsWith('sig-comparison-risk-'));fs.rmSync(target,{recursive:true,force:true});});
 const first=sourceRow(1,'',0),prefixHash=digest({rawPrefix:JSON.stringify(first)+'\n'}),created=createEntryComparison({sourceRecords:1,sourceHash:prefixHash,cutoff:stamp(10000),config,universe,policy,codeIdentity:entryComparisonCodeIdentity()});
 assert.notEqual(prefixHash,first.hash,'bank sourceHash names the prefix, not the final individual row');
 const bank=created.bank,bankHash=digest(bank);let state=advanceEntryComparison(created.state,first,bank),now=10200,sequence=0,previous='';
 const manifest=path.join(runtime,'entry-comparison-active.json'),bankFile=path.join(dir,'bank.json'),journal=path.join(dir,'journal.jsonl'),stats=path.join(dir,'stats.json');
 fs.writeFileSync(manifest,JSON.stringify({schemaVersion:1,cohort,bankHash}));fs.writeFileSync(bankFile,JSON.stringify(bank));
 function publish(row,extra={}){atomicJson(stats,{cohort,bankHash,codeIdentity:bank.codeIdentity,status:'running',updatedAt:stamp(now),sourceSequence:row.state.sourceSequence,sourceHash:row.state.lastRecordHash,journalSequence:row.sequence,journalHash:row.hash,...extra});}
 function checkpoint(ms,write=true){const body={sequence:++sequence,previous,at:stamp(ms),event:{type:'source_advanced',sourceSequence:state.sourceSequence,sourceHash:state.lastRecordHash},state:structuredClone(state)},row={...body,hash:digest(body)};previous=row.hash;if(write){fs.appendFileSync(journal,JSON.stringify(row)+'\n');publish(row);}return row;}
 const firstCheckpoint=checkpoint(10100);
 function wallet(name,marketId,held){
  let w=structuredClone(created.state.strategies[name].wallet);const spec=universe.exchanges.find(x=>x.marketId===marketId);
  for(const ms of held?[12000,14000]:[12000]){const payload={contexts:[{type:'tournament',tournament:{id:config.tournamentId},orderbook:{exchanges:[{exchangeId:spec.exchangeId,option:'YES',asOf:{sequence:ms,at:stamp(ms-200)},bids:[{price:.565,quantity:10000}],asks:[{price:.57,quantity:10000}]}]}}]},b=normalizeBook(payload,marketId,config.tournamentId,stamp(ms),stamp(ms-100),20000,new Set(),'authoritative_receipt_v1');w=stepDip(w,b,bank.config,{reference:.6,allowEntry:true,minimumEdge:.02,takeProfit:'0.010000',horizonMs:3600000,maxMarketCost:'25.000000',executionPolicy:'responsive_v2'});}
  return w;
 }
 function append(mutator=()=>{},ms=16000,write=true){const r=sourceRow(state.sourceSequence+1,state.lastRecordHash,ms);state=advanceEntryComparison(state,r,bank);mutator(state);now=ms+200;return checkpoint(ms+100,write);}
 function risk(){return append(s=>{s.strategies.historical_dip.wallet=wallet('historical_dip','377',true);s.strategies.shock_rebound.wallet=wallet('shock_rebound','354',false);});}
 return {runtime,manifest,bankFile,journal,stats,bank,bankHash,firstCheckpoint,publish,append,risk,wallet,get now(){return now;},setNow(ms){now=ms;},reader(){return createComparisonRiskSource({runtimeDir:runtime,universe,now:()=>base+now});}};
}

test('absent manifest is an unconfigured baseline and poll never writes or calls fetch',t=>{
 const f=fixture(t);fs.unlinkSync(f.manifest);let calls=0;t.mock.method(globalThis,'fetch',async()=>{calls++;throw Error('NETWORK_FORBIDDEN');});
 const before=fs.readdirSync(f.runtime),r=f.reader(),s=r.poll();assert.equal(s.available,true);assert.equal(s.configured,false);assert.deepEqual(s.heldMarketIds,[]);assert.deepEqual(s.pendingMarketIds,[]);assert.equal(s.reason,'NOT_CONFIGURED');assert.equal(calls,0);assert.deepEqual(fs.readdirSync(f.runtime),before);
 r.close();assert.equal(r.poll().available,false);assert.equal(r.poll().reason,'CLOSED');
});

test('full initial chain derives both arms risk and unchanged polls only retain curated identities',t=>{
 const f=fixture(t);f.risk();const r=f.reader(),s=r.poll();assert.equal(s.available,true,s.reason);assert.deepEqual(s.heldMarketIds,['377']);assert.deepEqual(s.pendingMarketIds,['354']);assert.equal(s.journalSequence,2);assert.equal(s.sourceSequence,2);assert.equal(s.bankHash,f.bankHash);assert.equal(s.eligibleForExecution,false);
 assert.equal(s.journalBytesRead,fs.statSync(f.journal).size);const again=r.poll();assert.equal(again.journalBytesRead,s.journalBytesRead);assert.equal(again.available,true);assert.equal('state' in s,false);assert.equal('wallet' in s,false);
 s.heldMarketIds.push('999');s.codeIdentity.fake='bad';assert.deepEqual(r.poll().heldMarketIds,['377']);assert.equal(r.poll().codeIdentity.fake,undefined);
});

test('stopped, stale and temporarily missing heartbeats never hide held or pending risk',t=>{
 const f=fixture(t),last=f.risk(),r=f.reader();assert.equal(r.poll().available,true);
 for(const status of ['stopped','failed','awaiting_capture']){f.publish(last,{status});const s=r.poll();assert.equal(s.available,false);assert.equal(s.reason,'COMPARISON_WORKER_NOT_RUNNING');assert.deepEqual(s.heldMarketIds,['377']);assert.deepEqual(s.pendingMarketIds,['354']);}
 f.setNow(f.now+60001);f.publish(last,{updatedAt:stamp(16000)});assert.equal(r.poll().reason,'COMPARISON_WORKER_STALE');fs.unlinkSync(f.stats);assert.equal(r.poll().reason,'COMPARISON_WORKER_UNAVAILABLE');
 f.publish(last,{updatedAt:stamp(f.now+1)});assert.equal(r.poll().reason,'COMPARISON_WORKER_STALE','future heartbeat must not enable sampling');
 f.publish(last);assert.equal(r.poll().available,true,'worker health recovers without restarting the evidence reader');
});

test('a fresh heartbeat may reference a retained prior checkpoint while the newest journal supplies risk',t=>{
 const f=fixture(t),r=f.reader();assert.equal(r.poll().available,true);const old=fs.readFileSync(f.stats);f.risk();fs.writeFileSync(f.stats,old);
 const s=r.poll();assert.equal(s.available,true,s.reason);assert.deepEqual(s.heldMarketIds,['377']);assert.deepEqual(s.pendingMarketIds,['354']);assert.equal(s.journalSequence,2);
 for(let i=0;i<18;i++)f.append(()=>{},17000+i*1000);fs.writeFileSync(f.stats,old);const lag=r.poll();assert.equal(lag.available,false);assert.equal(lag.reason,'COMPARISON_WORKER_CHECKPOINT_UNVERIFIED');assert.equal(lag.retainedCheckpoints,16);assert.deepEqual(lag.heldMarketIds,['377']);
});

test('split UTF-8 tail disables optional sampling until complete without losing the last verified risk',t=>{
 const f=fixture(t);f.risk();const r=f.reader();assert.equal(r.poll().available,true);
 const next=f.append(s=>{s.note='pending \u03b1 proof';s.strategies.shock_rebound.wallet=f.wallet('shock_rebound','305',false);},18000,false),bytes=Buffer.from(JSON.stringify(next)+'\n'),split=bytes.indexOf(Buffer.from('\u03b1'))+1;
 fs.appendFileSync(f.journal,bytes.subarray(0,split));const partial=r.poll();assert.equal(partial.available,false);assert.equal(partial.reason,'COMPARISON_INCOMPLETE_TAIL');assert.deepEqual(partial.pendingMarketIds,['354']);assert.ok(partial.incompleteTailBytes>0);
 fs.appendFileSync(f.journal,bytes.subarray(split));f.publish(next);const complete=r.poll();assert.equal(complete.available,true,complete.reason);assert.deepEqual(complete.heldMarketIds,['377']);assert.deepEqual(complete.pendingMarketIds,['305']);assert.equal(complete.incompleteTailBytes,0);assert.equal(r.poll().journalSequence,3);
});

test('bank or manifest identity changes latch failure and preserve existing risk even after restoration',async t=>{
 for(const mode of ['bank','manifest','missing_manifest'])await t.test(mode,t=>{
  const f=fixture(t);f.risk();const r=f.reader();assert.equal(r.poll().available,true);const target=mode==='bank'?f.bankFile:f.manifest,original=fs.readFileSync(target);
  if(mode==='missing_manifest')fs.unlinkSync(target);else{const value=JSON.parse(original);if(mode==='bank')value.cutoff=stamp(9000);else value.cohort='another-panel';fs.writeFileSync(target,JSON.stringify(value));}
  const bad=r.poll();assert.equal(bad.available,false);assert.deepEqual(bad.heldMarketIds,['377']);assert.deepEqual(bad.pendingMarketIds,['354']);fs.writeFileSync(target,original);assert.equal(r.poll().available,false,'identity restoration requires a new verified reader');assert.equal(f.reader().poll().available,true);
 });
});

test('foreign instruments, noninteger quantities, broken chains and future journal clocks cannot replace risk',async t=>{
 for(const mode of ['instrument','quantity','chain','future'])await t.test(mode,t=>{
  const f=fixture(t);f.risk();const r=f.reader();assert.equal(r.poll().available,true);
  const next=f.append(s=>{if(mode==='instrument')s.strategies.shock_rebound.wallet.books['1043:YES'].exchangeId='9999';if(mode==='quantity')s.strategies.shock_rebound.wallet.strategies.mean_reversion.pending['1043:YES'].quantity=1.5;},18000,false);
  if(mode==='future'){next.at=stamp(90000);const {hash,...body}=next;next.hash=digest(body);}if(mode==='chain')next.previous='0'.repeat(64);
  fs.appendFileSync(f.journal,JSON.stringify(next)+'\n');const bad=r.poll();assert.equal(bad.available,false);assert.deepEqual(bad.heldMarketIds,['377']);assert.deepEqual(bad.pendingMarketIds,['354']);assert.equal(bad.journalSequence,2);assert.equal(r.poll().available,false);
 });
});

test('truncated or replaced journals are never accepted as a new empty wallet',async t=>{
 for(const mode of ['truncate','replace','mutate'])await t.test(mode,t=>{
  const f=fixture(t);f.risk();const r=f.reader();assert.equal(r.poll().available,true);
  if(mode==='truncate')fs.truncateSync(f.journal,10);else if(mode==='replace'){fs.renameSync(f.journal,f.journal+'.old');fs.copyFileSync(f.journal+'.old',f.journal);}else{const bytes=fs.readFileSync(f.journal);bytes[25]^=1;fs.writeFileSync(f.journal,bytes);}
  const bad=r.poll();assert.equal(bad.available,false);assert.deepEqual(bad.heldMarketIds,['377']);assert.deepEqual(bad.pendingMarketIds,['354']);assert.match(bad.reason,/JOURNAL_(TRUNCATED|REPLACED|CHANGED)/);
 });
});

test('strict manifest names, code identity and trusted universe mapping reject foreign proposals',async t=>{
 for(const mode of ['reserved','extra','hash','code','scope'])await t.test(mode,t=>{
  const f=fixture(t),manifest=JSON.parse(fs.readFileSync(f.manifest));
  if(mode==='reserved')manifest.cohort='com9';if(mode==='extra')manifest.marketIds=['999'];if(mode==='hash')manifest.bankHash='bad';
  if(['code','scope'].includes(mode)){const bank=JSON.parse(fs.readFileSync(f.bankFile));if(mode==='code')bank.codeIdentity['live-paper/model.mjs']='0'.repeat(64);else bank.universe.exchanges[0].exchangeId='9999';fs.writeFileSync(f.bankFile,JSON.stringify(bank));manifest.bankHash=digest(bank);}
  fs.writeFileSync(f.manifest,JSON.stringify(manifest));const s=f.reader().poll();assert.equal(s.available,false);assert.deepEqual(s.heldMarketIds,[]);assert.deepEqual(s.pendingMarketIds,[]);
 });
});

test('an oversized incomplete record cannot grow the stream buffer without bound or erase risk',t=>{
 const f=fixture(t);f.risk();const r=f.reader();assert.equal(r.poll().available,true);
 fs.appendFileSync(f.journal,Buffer.alloc(16*1024*1024,32));const bad=r.poll();assert.equal(bad.available,false);assert.equal(bad.reason,'INVALID_COMPARISON_RECORD_SIZE');assert.deepEqual(bad.heldMarketIds,['377']);assert.deepEqual(bad.pendingMarketIds,['354']);assert.equal(r.poll().available,false);
});
