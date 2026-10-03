import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {digest} from '../live-paper/model.mjs';
import {atomicJson} from '../execution/ledger.mjs';
import {bulkDipWalletViews} from '../frontend/app/bulk-dip-view.mjs';

const root=process.cwd(), runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const json=p=>JSON.parse(fs.readFileSync(p)),sha=b=>createHash('sha256').update(b).digest('hex');
const need=(ok,code)=>{if(!ok)throw Error(code);};
const before=json('data/observer-retry-runtime-before-20261003.json'),pids=json('data/current-runtime-pids.json');
for(const key of ['collector','paper','dashboard','metrics'])process.kill(pids[key],0);
const preserved=before.cohorts.map(c=>{
 const dir=path.join(runtime,c.cohort),bankBytes=fs.readFileSync(path.join(dir,'bank.json'));
 const bank=JSON.parse(bankBytes),journal=fs.readFileSync(path.join(dir,'journal.jsonl'));
 need(sha(bankBytes)===c.bankHash,'BANK_CHANGED');
 need(journal.length>=c.journalBytes&&sha(journal.subarray(0,c.journalBytes))===c.journalHash,'JOURNAL_PREFIX_CHANGED');
 for(const [module,hash]of Object.entries(bank.codeIdentity)){
  const resolved=module.startsWith('./')||module.startsWith('../')?path.resolve(root,'live-paper',module):path.resolve(root,module);
  need(sha(fs.readFileSync(resolved))===hash,'FROZEN_CODE_CHANGED');
 }
 const retired=c.cohort.endsWith('bulk-dip-live-20261003-v1');
 let live=null;
 if(!retired){
  const stats=json(path.join(dir,'stats.json')),lock=json(path.join(dir,'worker.lock')),ageMs=Date.now()-Date.parse(stats.updatedAt);
  need(!stats.failed&&!stats.sourceClosed&&ageMs>=0&&ageMs<20000,'WORKER_NOT_FRESH');
  need(lock.pid===pids.paper&&stats.bankHash===digest(bank),'WORKER_IDENTITY_CHANGED');
  live={updatedAt:stats.updatedAt,ageMs,failed:stats.failed,sourceClosed:stats.sourceClosed,journalSequence:stats.journalSequence,
   sourceSequence:stats.sourceSequence??stats.bookSequence,priceSequence:stats.priceSequence??null};
 }
 return {cohort:c.cohort,bankHash:digest(bank),bankPreserved:true,originalJournalBytes:c.journalBytes,journalPrefixPreserved:true,frozenCodePreserved:true,live};
});
const activation=json(path.join(runtime,'competition-activation.json'));
need(activation.enabled===false,'CONTEST_ACTIVATION_CHANGED');
const collector=await(await fetch('http://127.0.0.1:8787/api/stats')).json(),budget=collector.runtime?.networkBudget;
need(collector.runtime?.pid===pids.collector&&collector.runtime.status==='running','COLLECTOR_UNAVAILABLE');
need(budget?.schemaVersion===3&&budget.maxReadsPerMinute===20&&budget.writesEnabled===false&&budget.executionOwner==='Daenerys','BUDGET_CHANGED');
const cohort='bulk-dip-live-20261003-v2',dir=path.join(runtime,'bulk-dip-cohorts',cohort),bank=json(path.join(dir,'bank.json'));
const manifest=json(path.join(runtime,'bulk-dip-active.json')),stats=json(path.join(dir,'stats.json'));
const views=bulkDipWalletViews(stats,manifest,bank,{workerAlive:true});
const lastBook=fs.readFileSync(path.join(runtime,'public-books.jsonl'),'utf8').trimEnd().split('\n').map(x=>JSON.parse(x)).reverse().find(x=>x.kind==='book').payload.book;
const instrument=bank.universe.exchanges.find(x=>x.exchangeId===lastBook.exchangeId&&x.marketId===lastBook.marketId);
need(instrument&&lastBook.instrument===`${instrument.exchangeId}:YES`,'EXCHANGE_INSTRUMENT_MISMATCH');
const companion=json(path.join(runtime,'executable-edge-cohorts/executable-edge-20261003-v2/companion-status.json'));
need(companion.mode==='bid_anchor_companion_running','COMPANION_UNAVAILABLE');
const result={at:new Date().toISOString(),pids,preserved,networkBudget:budget,competitionActivation:false,profitabilityEstablished:false,
 historicalAnchorFailure:before.historicalAnchorFailure,companion,
 dashboard:{productionBuild:'frontend-build-20261003-exchange',exchangeInstrumentVerified:true,publicExample:{marketId:instrument.marketId,exchangeId:instrument.exchangeId,instrument:lastBook.instrument},walletViews:views.length},
 bulk:{cohort,bankHash:stats.bankHash,updatedAt:stats.updatedAt,marketCount:stats.bulkReference.marketCount,candidates:stats.bulkReference.candidates.length,
 wallets:Object.fromEntries(Object.entries(stats.wallets).map(([latency,w])=>[latency,{fills:w.fills,pending:w.pending.length,positions:w.positions.length,realizedPnl:w.realizedPnl,fees:w.fees}]))}};
atomicJson(path.resolve('data/observer-retry-live-verification-20261003.json'),result);
console.log(JSON.stringify(result));
