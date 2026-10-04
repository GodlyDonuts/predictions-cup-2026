import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex');
const need=(v,c)=>{if(!v)throw Error('BAAL_RECOVERY_'+c);};
const pids=read('data/current-runtime-pids.json');
for(const k of ['collector','paper','dashboard'])process.kill(pids[k],0);
const stats=read(path.join(r,'stats.json'));
need(stats.runtime.pid===pids.collector&&stats.runtime.status==='running','COLLECTOR');
need(Number(fs.readFileSync(path.join(r,'..','bot.lock')))==pids.collector,'LEASE');
need(!stats.runtime.noMaker.failed&&!stats.runtime.baalPair.failed,'PAPER_HOSTS');
const b=stats.runtime.networkBudget;
need(b.maxReadsPerMinute===20&&b.completionPacing&&b.writesEnabled===false&&b.executionOwner==='Daenerys','BUDGET');
need(read(path.join(r,'competition-activation.json')).enabled===false,'ACTIVATION');
const preservation=read('data/baal-frozen-code-preservation-20261004.json');
const snapshot=path.join(r,'frozen-code',preservation.snapshot);
for(const [file,hash]of Object.entries(preservation.files))need(sha(fs.readFileSync(path.join(snapshot,file)))===hash,'FROZEN_SOURCE');
for(const p of read('data/baal-attention-preservation-20261004.json').prefixes)need(sha(fs.readFileSync(path.join(r,p.file)).subarray(0,p.bytes))===p.sha256,'SOURCE_PREFIX');
const active=read(path.join(r,'no-maker-active.json')),bank=read(path.join(r,'no-maker-cohorts',active.cohort,'bank.json'));
const prior=read('data/no-maker-v2-preservation-20261004.json');
const rows=fs.readFileSync(path.join(r,'no-maker-cohorts',prior.cohort,'journal.jsonl')).subarray(0,prior.journalPrefixBytes).toString().trimEnd().split('\n').map(JSON.parse);
const tail=rows.at(-1).state;
need(tail.tapeSequence===bank.tapePrefixSequence&&tail.tapeHash===bank.tapePrefixTailHash,'ACTUAL_MIGRATION_FRONTIER');
const maker=read(path.join(r,'no-maker-cohorts',active.cohort,'stats.json'));
const pairActive=read(path.join(r,'baal-pair-active.json')),pairBank=read(path.join(r,'baal-pair-cohorts',pairActive.cohort,'bank.json')),pair=read(path.join(r,'baal-pair-cohorts',pairActive.cohort,'stats.json'));
need(payloadHash(bank)===active.bankHash&&payloadHash(pairBank)===pairActive.bankHash,'BANKS');
need(!pair.failed&&pair.fees==='0.000000'&&!pair.profitabilityEstablished,'PAIR');
const response=await fetch('http://127.0.0.1:3000/api/wallets');need(response.ok,'DASHBOARD');
const api=await response.json(),view=api.wallets.find(w=>w.baalPair),makerViews=api.wallets.filter(w=>w.noMaker);
need(view?.available&&view.status==='running'&&makerViews.length===2,'DASHBOARD_PAPER');
need(view.metrics.realizedPnl===pair.realizedPnl&&view.metrics.fills===pair.fills,'DASHBOARD_ACCOUNTING');
const result={at:new Date().toISOString(),pids,sourceSnapshot:preservation.snapshot,frozenBanksVerified:preservation.banks.length,
 migration:{predecessor:prior.cohort,successor:active.cohort,consumedSequence:tail.tapeSequence,sealedSequence:bank.tapePrefixSequence,hash:tail.tapeHash,matched:true},
 wallets:Object.fromEntries(Object.entries(maker.wallets).map(([k,w])=>[k,{cash:w.cash,realizedPnl:w.realizedPnl,fees:w.fees,fills:w.fills,closedTrades:w.closedTrades,positions:w.positions.length}])),
 paired:{cohort:pairActive.cohort,bankHash:pairActive.bankHash,cash:pair.cash,realizedPnl:pair.realizedPnl,fills:pair.fills,positions:pair.positions.length,reviewCurrent:pair.reviewCurrent,reviewExpiresAt:pair.reviewExpiresAt},
 timingClocks:stats.runtime.readTiming.clocks,dashboardVerified:true,additionalCollectors:0,readAdmissionsPerMinute:20,competitionOrders:0,accountingReset:false,profitabilityEstablished:false};
fs.writeFileSync('data/baal-recovery-verification-20261004.json',JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result));
