import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {digest} from './model.mjs';
import {createBookPublication} from './book-publication.mjs';
import {openProbabilityReceipt as originalHost} from './probability-receipt-host.mjs';
import {openProbabilityReceipt as continuedHost} from './probability-receipt-host-v2.mjs';
import {exportProbabilityOpening,probabilityContinuationBundle} from './probability-continuation.mjs';
import {probabilityAccountEligibility,probabilityAccountGate} from './probability-receipt-v2.mjs';
import {prioritizeAnchorReads} from './bid-anchor-read-focus.mjs';
import {atomicJson} from '../execution/ledger.mjs';
import {projectProbabilityReadFocus,readProbabilityReadFocus} from './probability-read-focus.mjs';
import {validateUniverse} from './breadth.mjs';
import {probabilityWalletViews} from '../frontend/app/probability-view.mjs';
import {runProbabilityReceipt} from './probability-receipt-observe-v2.mjs';

const base=Date.parse('2026-10-04T10:00:00Z'),at=n=>new Date(base+n).toISOString();
const universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url))),marketId='363',exchangeId='1052';
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-owned-risk-'));let clock=0,version=0;const hosts=[];
 const writer=createBookPublication({file:path.join(dir,'public-books.jsonl'),universe,sessionId:'owned-risk-correctness',now:()=>base+clock});
 const research={schemaVersion:1,id:'correctness-only',marketId,exchangeId,side:'YES',predicateHash:'a'.repeat(64),recordedAt:at(-1000),sourceAt:at(-1000),expiresAt:at(20000),lower:'0.950000',upper:'0.990000',method:'subjective_range',settlementReviewed:true,flags:[],sources:[{url:'https://example.org/correctness-only',contentHash:'b'.repeat(64)}]};
 const bundle={identities:{[marketId]:{exchangeId,predicateHash:research.predicateHash}},research:[research]};clock=1000;
 const old=originalHost({runtimeDir:dir,workspaceDir:dir,cohort:'correctness-old',initialize:true,bundle,now:()=>base+clock});hosts.push(old);
 const f={dir,old,set:n=>clock=n,now:()=>base+clock,
  account(n,{positions=[],orders=[],error=false}={}){atomicJson(path.join(dir,'official-account.json'),{positions:{at:at(n),rows:positions.map(exchangeId=>({exchangeId,quantity:1,cost:'PRIVATE'}))},orders:{at:at(n),rows:orders.map(exchangeId=>({exchangeId,quantity:1,open:true,private:'PRIVATE'}))},errors:error?{orders:'failed'}:{},raw:'PRIVATE'});},
  book(n,bid=.895,ask=.9){clock=n;assert.equal(writer.publishBook({marketId,response:{endpoint:`/markets/${marketId}/orderbook?tournamentId=${universe.tournamentId}&depth=20`,requestStartedAt:at(n-100),receivedAt:at(n-10),data:{contexts:[{type:'tournament',tournament:{id:universe.tournamentId},orderbook:{exchanges:[{exchangeId,option:'YES',asOf:{sequence:++version,at:at(n+100)},bids:[{price:bid,quantity:10000}],asks:[{price:ask,quantity:10000}]}]}}]}}}),true);},
  seed(){f.account(1000);f.book(3000);old.consume();f.account(6000);f.book(8000);old.consume();assert.equal(old.status().wallets[1000].fills,1);assert.equal(old.status().wallets[3000].fills,1);},
  opening(){old.close();return exportProbabilityOpening({runtimeDir:dir,cohort:'correctness-old'});},
  continue(opening,initialize=true){clock=Math.max(clock,10000);const w=continuedHost({runtimeDir:dir,workspaceDir:dir,cohort:'correctness-next',initialize,bundle:probabilityContinuationBundle(opening),now:()=>base+clock});hosts.push(w);return w;}
 };
 t.after(()=>{for(const w of hosts)w.close();writer.close();assert.equal(path.dirname(dir),os.tmpdir());assert.ok(path.basename(dir).startsWith('sig-owned-risk-'));fs.rmSync(dir,{recursive:true,force:true});});return f;
}

test('continuation carries exact cash, cost basis and fills, then permits only paper risk reduction on official holdings',t=>{
 const f=fixture(t);f.seed();const before=f.old.status(),bankBytes=fs.readFileSync(path.join(f.old.dir,'bank.json')),journalBytes=fs.readFileSync(path.join(f.old.dir,'journal.jsonl')),opening=f.opening(),w=f.continue(opening);
 for(const latency of [1000,3000]){const old=before.wallets[latency],next=w.status().wallets[latency];assert.equal(next.cash,old.cash);assert.equal(next.realizedPnl,old.realizedPnl);assert.equal(next.fills,old.fills);assert.equal(next.positions[0].cost,old.positions[0].cost);assert.deepEqual(next.recentFills,old.recentFills);assert.equal(next.liquidationPnl,null,'old prices cannot become new valid marks');}
 f.account(10000,{positions:[exchangeId]});f.book(12000,.93,.935);w.consume();assert.equal(w.status().wallets[1000].fills,1,'exit requires a later independent request');
 f.account(13000,{positions:[exchangeId]});f.book(16000,.93,.935);w.consume();
 for(const latency of [1000,3000]){const s=w.status().wallets[latency];assert.equal(s.fills,2);assert.equal(s.closedTrades,1);assert.equal(s.realizedPnl,'0.020000');assert.equal(s.cash,'100.020000');assert.equal(s.fees,'0.000000');assert.equal(s.positions.length,0);}
 f.account(16000,{positions:[exchangeId]});f.book(18000);w.consume();assert.equal(w.status().wallets[1000].fills,2,'no new buys on official position');
 assert.deepEqual(fs.readFileSync(path.join(f.old.dir,'bank.json')),bankBytes);assert.deepEqual(fs.readFileSync(path.join(f.old.dir,'journal.jsonl')),journalBytes);assert.equal(w.status().profitabilityEstablished,false);
});
test('minimal account projection separates exposure from resting liquidity and rejects unavailable, stale or future state',()=>{
 const cache={positions:{at:at(1000),rows:[{exchangeId,quantity:1,cost:'PRIVATE'}]},orders:{at:at(1000),rows:[]},errors:{},raw:'PRIVATE'},a=probabilityAccountEligibility(cache),b={exchangeId,requestStartedAt:at(2000)};
 assert.equal(JSON.stringify(a).includes('PRIVATE'),false);assert.equal(probabilityAccountGate(a,b,at(3000)), 'own_account_exposure');assert.equal(probabilityAccountGate(a,b,at(3000),undefined,'reduce_only'),null);
 for(const patch of [{openOrderExchangeIds:[exchangeId]},{ordersAt:at(2500)},{positionsAt:at(-18001)},{raw:'PRIVATE'},{openOrderExchangeIds:[exchangeId,exchangeId]}])assert.ok(probabilityAccountGate({...a,...patch},b,at(3000),undefined,'reduce_only'));
 assert.equal(probabilityAccountEligibility({...cache,errors:{orders:'failed'}}),null);assert.equal(probabilityAccountEligibility({...cache,orders:{at:at(1000),rows:[{exchangeId,quantity:1,open:false}]}}),null);
});
test('our resting order blocks paper mark/fill, then a fresh independent quote after removal resumes the existing risk exit',t=>{
 const f=fixture(t);f.seed();const w=f.continue(f.opening());f.account(10000,{positions:[exchangeId]});f.book(12000,.93,.935);w.consume();
 f.account(13000,{positions:[exchangeId],orders:[exchangeId]});f.book(16000,.93,.935);w.consume();assert.equal(w.status().wallets[1000].fills,1);assert.equal(w.status().wallets[1000].liquidationPnl,null);assert.ok(w.status().counts.own_account_open_order);
 f.account(16000,{positions:[exchangeId]});f.book(18000,.93,.935);w.consume();assert.equal(w.status().wallets[1000].fills,2);assert.equal(w.status().wallets[3000].fills,2);
});
test('expired forecast permits reducing carried inventory but never authorizes fresh entries',t=>{
 const f=fixture(t);f.seed();const w=f.continue(f.opening());f.account(20000);f.book(22000,.93,.935);w.consume();f.account(23000);f.book(26000,.93,.935);w.consume();
 assert.equal(w.status().wallets[1000].realizedPnl,'0.020000');assert.ok(w.status().counts.expired_research);f.account(26000);f.book(28000);w.consume();assert.equal(w.status().wallets[1000].pending.length,0);assert.equal(w.status().wallets[1000].fills,2);
});
test('successor restart replays recorded projection instead of today\'s changed account, and binds opening bytes',t=>{
 const f=fixture(t);f.seed();const opening=f.opening(),w=f.continue(opening);f.account(10000,{positions:[exchangeId]});f.book(12000,.93,.935);w.consume();const before=w.status();w.close();f.account(12000,{positions:[exchangeId],orders:[exchangeId]});
 const resumed=f.continue(opening,false);assert.equal(resumed.status().stateHash,before.stateHash);assert.equal(resumed.status().journalHash,before.journalHash);assert.equal(resumed.consume(),0);const file=path.join(resumed.dir,'opening-state.json'),raw=fs.readFileSync(file);raw[raw.indexOf('correctness-old')]=88;fs.writeFileSync(file,raw);assert.throws(()=>resumed.consume(),/IDENTITY_CHANGED/);assert.equal(resumed.status().stateHash,before.stateHash);
});
test('source frontier and original owner lock prevent skipping unconsumed data or taking a live predecessor',t=>{
 const f=fixture(t);f.seed();assert.throws(()=>exportProbabilityOpening({runtimeDir:f.dir,cohort:'correctness-old'}),/PREDECESSOR_RUNNING/);f.old.close();f.book(9000);assert.throws(()=>f.opening(),/UNCONSUMED_SOURCE/);
});
test('bound live continuation focus can read held official exposure without extra slots or entry authorization',t=>{
 const f=fixture(t);f.seed();const w=f.continue(f.opening()),stats=w.status(),lock=JSON.parse(fs.readFileSync(path.join(w.dir,'worker.lock')));atomicJson(path.join(f.dir,'probability-receipt-active.json'),{cohort:w.bank.name,bankHash:stats.bankHash});atomicJson(path.join(w.dir,'stats.json'),stats);atomicJson(path.join(w.dir,'read-focus.json'),projectProbabilityReadFocus(stats,lock,w.bank.name));
 const focus=readProbabilityReadFocus(f.dir,validateUniverse(universe,universe.tournamentId),f.now());assert.equal(focus.ownedRiskReads,true);assert.ok(focus.markets.some(x=>x.marketId===marketId&&x.phase==='held'));
 const plan={reads:[{marketId:'277',reason:'held'},{marketId:'305',reason:'discovery'},{marketId:'354',reason:'discovery'}],entryMarketIds:['305','354']};assert.equal(prioritizeAnchorReads(plan,focus,{excludedMarketIds:[marketId]}),plan);
 const p=prioritizeAnchorReads(plan,focus,{excludedMarketIds:[marketId],ownedRiskMarketIds:[marketId]});assert.equal(p.reads.length,plan.reads.length);assert.equal(p.reads[0].marketId,'277');assert.equal(p.bidAnchorReadFocus.requestsAdded,0);assert.equal(p.bidAnchorReadFocus.executionAuthority,false);assert.ok(!p.entryMarketIds.includes(marketId));
 const onlyBuy={markets:[{marketId,phase:'entry_pending'}]};assert.equal(prioritizeAnchorReads(plan,onlyBuy,{excludedMarketIds:[marketId],ownedRiskMarketIds:[marketId]}),plan);
 const view=probabilityWalletViews(stats,{cohort:w.bank.name,bankHash:digest(w.bank)},w.bank,{workerAlive:true,now:f.now()});assert.equal(view.length,2);assert.equal(view[0].probability.accountingContinuation.accountingCarried,true);assert.equal(view[0].probability.profitabilityEstablished,false);
});
test('lean continuation observer publishes the existing accounting without opening another strategy or changing the journal',async t=>{
 const f=fixture(t);f.seed();const opening=f.opening(),w=f.continue(opening),bank=fs.readFileSync(path.join(w.dir,'bank.json')),journal=fs.readFileSync(path.join(w.dir,'journal.jsonl'));w.close();
 await runProbabilityReceipt({runtimeDir:f.dir,workspaceDir:f.dir,cohort:w.bank.name,now:f.now,once:true});
 assert.deepEqual(fs.readFileSync(path.join(w.dir,'bank.json')),bank);assert.deepEqual(fs.readFileSync(path.join(w.dir,'journal.jsonl')),journal);assert.equal(fs.existsSync(path.join(f.dir,'executable-edge-cohorts')),false);assert.equal(fs.existsSync(path.join(w.dir,'worker.lock')),false);const stats=JSON.parse(fs.readFileSync(path.join(w.dir,'stats.json')));assert.equal(stats.failed,false);assert.equal(stats.competitionOrders,0);assert.equal(stats.networkRequests,0);
});
