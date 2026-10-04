import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {verifyNativeDipResult,micros,sha} from './native-dip-proof-v1.mjs';
import {nativeDipHostIdentity,openNativeDipHost,sealedNativePrefix,hashNativePrefix,runNativeDipChild} from './native-dip-host-v1.mjs';
import {createSchedulerState,planBookReads} from './book-scheduler.mjs';
const iso=n=>new Date(n).toISOString(),start=100000,through=200000,clone=structuredClone;
function fixture(){
 const declaration={schemaVersion:1,mode:'causal_dip_subsequent_v2',cohort:'test-native',declaredAt:iso(start),config:{initialCash:'100.000000',fillClockPolicy:'independent_request_latency_v2',feeBps:0,maxDataAgeMs:20000,marketIds:['277']},strategies:[{id:'dip',parameters:{lookback:12}}],scenarios:[{id:'base',latencyMs:3000}]};
 const binding={schemaVersion:1,cohort:declaration.cohort,declarationHash:sha(JSON.stringify(declaration)),binaryHash:'a'.repeat(64),engineHash:'b'.repeat(64),runtimeLibraryHash:'c'.repeat(64)};
 const f={intentId:1,instrument:'966:YES',action:'buy',quantity:2,price:'0.875000',fee:'0.000000',at:110000,sourceClock:{policy:declaration.config.fillClockPolicy,decisionAt:106000,minimumLatencyMs:3000,requestStartedAt:109500,receivedAt:110000,decisionSequence:'9007199254740991',decisionSourceNs:'106000000001',fillSequence:'9007199254740991',fillSourceNs:'110000000001'},economics:{reason:'ENTRY',decisionAt:106000}};
 const card={strategy:'dip',parameters:{lookback:12},scenario:declaration.scenarios[0],exitPolicy:'snapshot_limit_v1',point:{at:iso(through),fillClockPolicy:declaration.config.fillClockPolicy,fillExecutions:1,openShares:2,completedTrades:0,cash:'98.250000',realizedPnl:'0.000000',turnover:'1.750000',fees:'0.000000',pendingIntents:0,liquidationPnl:null,valuation:'cash_only_inventory_zero'},economics:{netPnl:'0.000000',matchedShares:0,fees:'0.000000'},fills:[f]};
 const result={declarationFileHash:binding.declarationHash,capture:{engine_hash:binding.engineHash,source_hash:sha('one\n'),source_bytes:4,source_records:1,through},evaluation:{mode:declaration.mode,cohort:declaration.cohort,evaluationStart:declaration.declaredAt,evaluationThrough:iso(through),networkRequests:0,competitionOrders:0,profitabilityEstablished:false,subsequentRecords:1,subsequentCycles:1,candidates:2,scorecards:[card,{...clone(card),exitPolicy:'profit_floor_stop_buffer_v1'}]}};
 return{result,declaration,binding,universe:{exchanges:[{exchangeId:'966',marketId:'277',option:'YES'}]}};
}
function changed(fn){const f=fixture();fn(f);return f;}
test('risk deduplicates copied wallets and preserves exact cash/FIFO',()=>{const p=verifyNativeDipResult(fixture());assert.deepEqual(p.heldMarketIds,['277']);assert.equal(p.uniqueFillEvents,1);assert.equal(p.groups[0].wallets,2);assert.equal(p.executionAuthority,false);assert.equal(micros('999999999999.123456'),999999999999123456n);});
test('partial then complete sales reconcile and release the market',()=>{
 const f=fixture();for(const card of f.result.evaluation.scorecards){for(let i=0;i<2;i++){const sell=clone(card.fills[0]);sell.intentId=i+2;sell.action='sell';sell.quantity=1;sell.at=120000+i*10000;sell.price='0.885000';sell.sourceClock.decisionAt=sell.at-4000;sell.sourceClock.requestStartedAt=sell.at-500;sell.sourceClock.receivedAt=sell.at;sell.sourceClock.fillSourceNs=String(BigInt(sell.at)*1000000n);sell.economics={reason:'EXIT',decisionAt:sell.at-4000};card.fills.push(sell);}Object.assign(card.point,{fillExecutions:3,openShares:0,completedTrades:1,cash:'100.020000',realizedPnl:'0.020000',turnover:'3.520000'});Object.assign(card.economics,{netPnl:'0.020000',matchedShares:2});}
 const p=verifyNativeDipResult(f);assert.deepEqual(p.heldMarketIds,[]);assert.equal(p.groups[0].realizedPnl,'0.020000');
});
for(const [name,fn,code] of [
 ['pre-declaration fill',f=>f.result.evaluation.scorecards[0].fills[0].at=start,'FILL'],
 ['request began before latency',f=>f.result.evaluation.scorecards[0].fills[0].sourceClock.requestStartedAt=108999,'CLOCK'],
 ['numeric nanoseconds',f=>f.result.evaluation.scorecards[0].fills[0].sourceClock.fillSourceNs=110000000001,'CLOCK_PRECISION'],
 ['same decision version',f=>{const q=f.result.evaluation.scorecards[0].fills[0].sourceClock;q.fillSourceNs=q.decisionSourceNs;},'CLOCK_VERSION'],
 ['foreign exchange',f=>f.result.evaluation.scorecards[0].fills[0].instrument='967:YES','FILL'],
 ['nonzero fee',f=>f.result.evaluation.scorecards[0].fills[0].fee='0.000001','FILL'],
 ['oversell',f=>f.result.evaluation.scorecards[0].fills[0].action='sell','OVERSELL'],
 ['wrong cash',f=>f.result.evaluation.scorecards[0].point.cash='98.250001','RECONCILIATION'],
 ['missing inventory',f=>f.result.evaluation.scorecards[0].point.openShares=0,'RECONCILIATION'],
 ['wrong model parameters',f=>f.result.evaluation.scorecards[0].parameters.lookback=13,'CARD_IDENTITY'],
 ['duplicate card',f=>f.result.evaluation.scorecards[1]=clone(f.result.evaluation.scorecards[0]),'CARD_IDENTITY'],
 ['wrong engine',f=>f.result.capture.engine_hash='d'.repeat(64),'CAPTURE']
])test('rejects '+name,()=>assert.throws(()=>verifyNativeDipResult(changed(fn)),new RegExp('NATIVE_DIP_'+code)));
test('later replay cannot alter old fills or rewind source',()=>{const f=fixture(),prior=clone(f.result);f.result.evaluation.scorecards[0].fills[0].intentId=3;assert.throws(()=>verifyNativeDipResult({...f,previous:prior}),/FILL_HISTORY_CHANGED/);f.result=clone(prior);f.result.capture.source_bytes=3;assert.throws(()=>verifyNativeDipResult({...f,previous:prior}),/SOURCE_REGRESSION/);});
function temp(t){const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-native-risk-'));t.after(()=>{assert(path.resolve(dir).startsWith(path.resolve(os.tmpdir())+path.sep));fs.rmSync(dir,{recursive:true,force:true});});return dir;}
test('seals complete lines and verifies both prior and current original prefixes',async t=>{const dir=temp(t),file=path.join(dir,'journal');fs.writeFileSync(file,'one\ntwo\npartial');assert.equal(sealedNativePrefix(file).bytes,8);assert.deepEqual(await hashNativePrefix(file,8,4),{hash:sha('one\ntwo\n'),priorHash:sha('one\n')});const c=new AbortController();c.abort();await assert.rejects(hashNativePrefix(file,8,4,c.signal),/STOPPED/);});
test('aborts a native child without leaking its error output',async()=>{const control=new AbortController(),p=runNativeDipChild(process.execPath,['-e','setTimeout(()=>process.exit(0),20000)'],control.signal);setTimeout(()=>control.abort(),50);await assert.rejects(p,/NATIVE_DIP_STOPPED/);});
test('verified held risk reserves a slot without expanding book budget',()=>{const p=verifyNativeDipResult(fixture()),scope={coreMarketIds:['280'],universe:{tournamentId:'test',exchanges:[{exchangeId:'966',marketId:'277',option:'YES'},{exchangeId:'1070',marketId:'381',option:'YES'},{exchangeId:'1071',marketId:'382',option:'YES'},{exchangeId:'970',marketId:'280',option:'YES'}]}};const plan=planBookReads(createSchedulerState(scope),scope,{heldMarketIds:[...p.heldMarketIds,'381','382']});assert.equal(plan.reads.find(r=>r.marketId==='277').reason,'held_or_pending');assert.equal(plan.reads.length,4);});
test('host retains verified inventory on child failure, then refuses work after close',async t=>{
 const dir=temp(t),f=fixture(),cohort=path.join(dir,'research-cohorts',f.binding.cohort);fs.mkdirSync(cohort,{recursive:true});
 const binary=fs.readFileSync(process.execPath);f.binding.binaryHash=sha(binary);f.binding.runtimeLibraryHash=sha('library');f.binding.sourceHashes={'backend-rust/test.txt':sha('source')};
 const build=path.join(dir,'native-builds',f.binding.binaryHash),source=path.join(dir,'native-sources',f.binding.engineHash);fs.mkdirSync(build,{recursive:true});fs.mkdirSync(source,{recursive:true});fs.writeFileSync(path.join(build,'baal-backend.exe'),binary);fs.writeFileSync(path.join(build,'libunwind.dll'),'library');fs.writeFileSync(path.join(source,'test.txt'),'source');
 const output='12345678-1234-1234-1234-123456789abc.json';fs.writeFileSync(path.join(cohort,'declaration.json'),JSON.stringify(f.declaration));fs.writeFileSync(path.join(cohort,'native-clock-binding.json'),JSON.stringify(f.binding));fs.writeFileSync(path.join(cohort,output),JSON.stringify(f.result));const outputHash=sha(JSON.stringify(f.result));
 fs.writeFileSync(path.join(cohort,output+'.receipt.json'),JSON.stringify({binaryHash:f.binding.binaryHash,engineHash:f.binding.engineHash,declarationFileHash:f.binding.declarationHash,outputHash,sourceHash:f.result.capture.source_hash,sourceBytes:4,competitionOrders:0}));
 fs.writeFileSync(path.join(dir,'native-dip-active.json'),JSON.stringify({schemaVersion:1,mode:'frozen_native_dip_paper_host_v1',cohort:f.binding.cohort,bindingHash:sha(JSON.stringify(f.binding)),intervalMs:60000,workers:1,hostSourceHashes:nativeDipHostIdentity(),seed:{output,outputHash}}));fs.writeFileSync(path.join(dir,'journal.jsonl'),'one\ntwo\n');
 const host=openNativeDipHost({runtimeDir:dir,universe:f.universe});const p=host.kick();assert.equal(host.kick(),p);await p;assert.equal(host.status().status,'degraded');assert.deepEqual(host.status().heldMarketIds,['277']);assert.equal(host.status().lastError,'NATIVE_DIP_CHILD_FAILED');await host.close();assert.equal(host.kick(),null);
});
