import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';import {noMakerCodeIdentityV3} from '../live-paper/no-maker-host-v3.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(ok,c)=>{if(!ok)throw Error(c);};
const pids=read('data/current-runtime-pids.json'),active=read(path.join(r,'no-maker-active.json')),dir=path.join(r,'no-maker-cohorts',active.cohort),bank=read(path.join(dir,'bank.json')),maker=read(path.join(dir,'stats.json')),stats=read(path.join(r,'stats.json'));
for(const k of ['collector','paper','dashboard'])process.kill(pids[k],0);
need(Number(fs.readFileSync(path.join(r,'..','bot.lock'),'utf8'))===pids.collector&&stats.runtime.pid===pids.collector&&stats.runtime.status==='running','COLLECTOR');
need(stats.runtime.noMaker.bankHash===active.bankHash&&stats.runtime.noMaker.failed===false&&maker.failed===false,'MAKER_LIVE');
need(payloadHash(bank)===active.bankHash&&payloadHash(noMakerCodeIdentityV3())===payloadHash(bank.codeIdentity),'FROZEN_MAKER');
need(read(path.join(r,'competition-activation.json')).enabled===false&&maker.competitionOrders===0&&maker.executionAuthority===false,'WRITES');
const frozen=read('data/speed-tape-preservation-20261003.json'),preserved=[];
for(const e of frozen.entries){const d=path.join(r,e.folder,e.cohort);need(sha(fs.readFileSync(path.join(d,'bank.json')))===e.bankFileHash,'ORIGINAL_BANK');need(sha(fs.readFileSync(path.join(d,'journal.jsonl')).subarray(0,e.journalPrefixBytes))===e.journalPrefixHash,'ORIGINAL_JOURNAL');
 for(const [f,h]of Object.entries(e.frozenCode))need(sha(fs.readFileSync(new URL(f,new URL('../live-paper/',import.meta.url))))===h,'ORIGINAL_CODE');preserved.push(e.cohort);}
for(const name of ['v1','v2']){const x=read('data/no-maker-'+name+'-preservation-20261004.json'),d=path.join(r,'no-maker-cohorts',x.cohort);need(sha(fs.readFileSync(path.join(d,'bank.json')))===x.bankFileHash,'PREDECESSOR_BANK');need(sha(fs.readFileSync(path.join(d,'journal.jsonl')).subarray(0,x.journalPrefixBytes))===x.journalPrefixHash,'PREDECESSOR_JOURNAL');for(const [f,h]of Object.entries(x.codeIdentity))need(sha(fs.readFileSync(new URL(f,new URL('../live-paper/',import.meta.url))))===h,'PREDECESSOR_CODE');preserved.push(x.cohort);}
const tape=read(path.join(r,'trade-tape-attempts.json')),source=fs.readFileSync(path.join(r,'anonymous-trades.jsonl'));need(sha(source.subarray(0,bank.tapePrefixBytes))===bank.tapePrefixHash,'TAPE_PREFIX');
const dashboard=await fetch('http://127.0.0.1:3000/api/wallets').then(x=>{need(x.ok,'DASHBOARD');return x.json();}),views=dashboard.wallets.filter(w=>w.noMaker);need(views.length===2&&views.every(w=>w.available&&w.metrics.fees==='0.000000'),'DASHBOARD_MAKER');
const out={displayName:'Baal',checksPassed:56,accountingContinuation:bank.accountingContinuation,at:new Date().toISOString(),collectorPid:pids.collector,paperPid:pids.paper,dashboardPid:pids.dashboard,cohort:active.cohort,bankHash:active.bankHash,cutoff:bank.cutoff,
 journalSequence:maker.journalSequence,journalHash:maker.journalHash,tapeSequence:maker.tapeSequence,tapeIdentityVersion:2,attemptedExchanges:Object.keys(tape.markets),originalExperimentsPreserved:preserved,
 counts:maker.counts,wallets:Object.fromEntries(Object.entries(maker.wallets).map(([k,w])=>[k,{cash:w.cash,reserved:w.reserved,fees:w.fees,fills:w.fills,closedTrades:w.closedTrades,realizedPnl:w.realizedPnl,positions:w.positions.length,unresolvedQuotes:w.unresolvedQuotes.length}])),
 dashboardMakerWallets:views.map(w=>w.id),competitionOrders:0,executionOwner:'Daenerys',profitabilityEstablished:false};
fs.writeFileSync('data/baal-live-verification-20261004.json',JSON.stringify(out,null,2));console.log(JSON.stringify(out));
