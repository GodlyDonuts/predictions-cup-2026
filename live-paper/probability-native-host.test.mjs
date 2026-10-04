import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {readNativePublicationPrefix,nativeExecutable} from './rust-backend.mjs';
import {digest} from './model.mjs';
import {createBookPublication} from './book-publication.mjs';
import {openProbabilityReceipt as originalHost} from './probability-receipt-host.mjs';
import {openProbabilityReceipt as v2Host} from './probability-receipt-host-v2.mjs';
import {openProbabilityReceipt as nativeHost} from './probability-receipt-host-v3.mjs';
import {exportProbabilityOpening,probabilityContinuationBundle} from './probability-continuation.mjs';
import {exportNativeProbabilityOpening,nativeProbabilityContinuationBundle} from './probability-native-continuation.mjs';
import {archiveNativePublication} from '../data/prepare-native-publication-runtime-20261004.mjs';
import {atomicJson} from '../execution/ledger.mjs';
import {runProbabilityReceipt} from './probability-receipt-observe-v3.mjs';
import {projectProbabilityReadFocus,readProbabilityReadFocus} from './probability-read-focus.mjs';
import {validateUniverse} from './breadth.mjs';
import {probabilityWalletViews} from '../frontend/app/probability-view.mjs';
const base=Date.parse('2026-10-04T11:00:00Z'),at=n=>new Date(base+n).toISOString();
const universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url))),marketId='363',exchangeId='1052';
async function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-native-host-')),hosts=[];let clock=0,version=0;
 const writer=createBookPublication({file:path.join(dir,'public-books.jsonl'),universe,sessionId:'native-host-correctness',now:()=>base+clock});
 t.after(()=>{for(const w of hosts)w.close();writer.close();assert.equal(path.dirname(dir),os.tmpdir());assert.ok(path.basename(dir).startsWith('sig-native-host-'));fs.rmSync(dir,{recursive:true,force:true});});
 const sha=b=>createHash('sha256').update(b).digest('hex'),scope=path.join(dir,'correctness-scope.json'),journalFile=path.join(dir,'public-books.jsonl'),prefix=fs.readFileSync(journalFile);fs.writeFileSync(scope,JSON.stringify(universe));
 const native=await readNativePublicationPrefix({journal:journalFile,scope,endByte:prefix.length,expectedSha:sha(prefix),out:path.join(dir,'correctness-native-prefix.json')});assert.deepEqual(native.state,writer.status());
 const binding=archiveNativePublication(dir,{bindingEvidence:{binaryHash:sha(fs.readFileSync(nativeExecutable)),engineHash:native.engineHash,exactStateParity:true}}),research={schemaVersion:1,id:'correctness-only',marketId,exchangeId,side:'YES',predicateHash:'a'.repeat(64),recordedAt:at(-1000),sourceAt:at(-1000),expiresAt:at(20000),lower:'0.950000',upper:'0.990000',method:'subjective_range',settlementReviewed:true,flags:[],sources:[{url:'https://example.org/correctness-only',contentHash:'b'.repeat(64)}]};
 clock=1000;const old=originalHost({runtimeDir:dir,workspaceDir:dir,cohort:'correctness-original',initialize:true,bundle:{identities:{[marketId]:{exchangeId,predicateHash:research.predicateHash}},research:[research]},now:()=>base+clock});hosts.push(old);
 const f={dir,binding,old,set:n=>clock=n,now:()=>base+clock,
  account(n,orders=[]){atomicJson(path.join(dir,'official-account.json'),{positions:{at:at(n),rows:[{exchangeId,quantity:1,private:'PRIVATE'}]},orders:{at:at(n),rows:orders.map(exchangeId=>({exchangeId,quantity:1,open:true,private:'PRIVATE'}))},errors:{},raw:'PRIVATE'});},
  book(n,bid=.895,ask=.9){clock=n;assert.equal(writer.publishBook({marketId,response:{endpoint:`/markets/${marketId}/orderbook?tournamentId=${universe.tournamentId}&depth=20`,requestStartedAt:at(n-100),receivedAt:at(n-10),data:{contexts:[{type:'tournament',tournament:{id:universe.tournamentId},orderbook:{exchanges:[{exchangeId,option:'YES',asOf:{sequence:++version,at:at(n+100)},bids:[{price:bid,quantity:10000}],asks:[{price:ask,quantity:10000}]}]}}]}}}),true);},
  async opening(){f.v2.close();return exportNativeProbabilityOpening({runtimeDir:dir,cohort:f.v2.bank.name,nativeBackend:binding});},
  async native(opening,initialize=true){clock=Math.max(clock,14000);const w=await nativeHost({runtimeDir:dir,workspaceDir:dir,cohort:'correctness-native',initialize,bundle:nativeProbabilityContinuationBundle(opening,binding),now:()=>base+clock});hosts.push(w);return w;}
 };
 atomicJson(path.join(dir,'official-account.json'),{positions:{at:at(1000),rows:[]},orders:{at:at(1000),rows:[]},errors:{}});f.book(3000);old.consume();atomicJson(path.join(dir,'official-account.json'),{positions:{at:at(6000),rows:[]},orders:{at:at(6000),rows:[]},errors:{}});f.book(8000);old.consume();old.close();clock=10000;
 const opening=exportProbabilityOpening({runtimeDir:dir,cohort:old.bank.name});f.v2=v2Host({runtimeDir:dir,workspaceDir:dir,cohort:'correctness-v2',initialize:true,bundle:probabilityContinuationBundle(opening),now:f.now});hosts.push(f.v2);f.account(10000);f.book(12000);f.v2.consume();return f;
}
test('native host carries existing accounting, requires later-request exits and replays recorded account clocks on restart',async t=>{
 const f=await fixture(t),before=f.v2.status(),bank=fs.readFileSync(path.join(f.v2.dir,'bank.json')),journal=fs.readFileSync(path.join(f.v2.dir,'journal.jsonl')),opening=await f.opening(),w=await f.native(opening);
 for(const latency of [1000,3000]){const a=before.wallets[latency],b=w.status().wallets[latency];assert.equal(b.cash,a.cash);assert.equal(b.fills,a.fills);assert.equal(b.realizedPnl,a.realizedPnl);assert.deepEqual(b.recentFills,a.recentFills);assert.equal(b.positions[0].cost,a.positions[0].cost);assert.equal(b.liquidationPnl,null);}
 assert.equal(w.status().startup.nativePrefixRecords,opening.sourceSequence);assert.equal(w.status().startup.nativeEngineHash,f.binding.engineHash);
 f.account(14000);f.book(16000,.93,.935);w.consume();assert.equal(w.status().wallets[1000].fills,1);
 f.account(17000);f.book(20000,.93,.935);w.consume();const prior=w.status();assert.equal(prior.wallets[1000].cash,'100.020000');assert.equal(prior.wallets[1000].fees,'0.000000');assert.equal(prior.wallets[1000].closedTrades,1);assert.equal(prior.wallets[3000].closedTrades,1);
 w.close();f.account(20000,[exchangeId]);const resumed=await f.native(opening,false);assert.equal(resumed.status().stateHash,prior.stateHash);assert.equal(resumed.status().journalHash,prior.journalHash);assert.equal(resumed.consume(),0);assert.deepEqual(fs.readFileSync(path.join(f.v2.dir,'bank.json')),bank);assert.deepEqual(fs.readFileSync(path.join(f.v2.dir,'journal.jsonl')),journal);assert.equal(JSON.stringify(opening).includes('PRIVATE'),false);
});
test('native dependency identity and incomplete or altered prefixes fail before using a wallet',async t=>{
 const f=await fixture(t),opening=await f.opening(),binary=path.join(f.dir,'native-builds',f.binding.binaryHash,'baal-backend.exe'),bytes=fs.readFileSync(binary);fs.writeFileSync(binary,Buffer.concat([bytes,Buffer.from('changed')]));await assert.rejects(f.native(opening),/BINARY_CHANGED/);assert.equal(fs.existsSync(path.join(f.dir,'probability-receipt-cohorts/correctness-native/bank.json')),false);fs.writeFileSync(binary,bytes);
 const source=path.join(f.dir,'public-books.jsonl'),raw=fs.readFileSync(source),changed=Buffer.from(raw);changed[5]^=1;fs.writeFileSync(source,changed);await assert.rejects(f.native(opening),/HASH|JSON|RECORD|CHAIN/);fs.writeFileSync(source,raw);
 const w=await f.native(opening),before=w.status(),journal=fs.readFileSync(path.join(w.dir,'journal.jsonl'));fs.appendFileSync(source,'{"partial":');assert.equal(w.consume(),0,'incomplete tail waits rather than becoming a quote');fs.appendFileSync(source,'true}\n');assert.throws(()=>w.consume(),/PUBLIC_BOOK/);assert.equal(w.status().failed,true);assert.equal(w.status().stateHash,before.stateHash);assert.deepEqual(fs.readFileSync(path.join(w.dir,'journal.jsonl')),journal);fs.writeFileSync(source,raw);
});
test('export requires a stopped predecessor with a fully consumed and intact source frontier',async t=>{
 const f=await fixture(t);await assert.rejects(exportNativeProbabilityOpening({runtimeDir:f.dir,cohort:f.v2.bank.name,nativeBackend:f.binding}),/PREDECESSOR_RUNNING/);f.v2.close();f.book(13000);await assert.rejects(f.opening(),/UNCONSUMED_SOURCE/);
});
test('native observer publishes the carried wallets and held-risk focus without extra readers or contest authority',async t=>{
 const f=await fixture(t),opening=await f.opening(),w=await f.native(opening),before=w.status(),journal=fs.readFileSync(path.join(w.dir,'journal.jsonl'));w.close();
 await runProbabilityReceipt({runtimeDir:f.dir,workspaceDir:f.dir,cohort:w.bank.name,now:f.now,once:true});const stats=JSON.parse(fs.readFileSync(path.join(w.dir,'stats.json')));assert.equal(stats.stateHash,before.stateHash);assert.deepEqual(fs.readFileSync(path.join(w.dir,'journal.jsonl')),journal);assert.equal(stats.competitionOrders,0);assert.equal(stats.networkRequests,0);
 const resumed=await f.native(opening,false),lock=JSON.parse(fs.readFileSync(path.join(w.dir,'worker.lock'))),manifest={cohort:w.bank.name,bankHash:digest(w.bank)};atomicJson(path.join(w.dir,'read-focus.json'),projectProbabilityReadFocus(stats,lock,w.bank.name));
 const focus=readProbabilityReadFocus(f.dir,validateUniverse(universe,universe.tournamentId),f.now());assert.equal(focus.ownedRiskReads,true);assert.ok(focus.markets.some(x=>x.marketId===marketId&&x.phase==='held'));const views=probabilityWalletViews(stats,manifest,w.bank,{workerAlive:true,now:f.now()});assert.equal(views.length,2);assert.equal(views[0].probability.accountingContinuation.priorBankHash,opening.state.bankHash);assert.equal(views[0].probability.profitabilityEstablished,false);resumed.close();
});
