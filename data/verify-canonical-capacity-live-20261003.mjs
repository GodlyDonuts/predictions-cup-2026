import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {digest} from '../live-paper/model.mjs';import {canonicalJson,atomicJson} from '../execution/ledger.mjs';
import {createBookPublicationState,applyBookPublication} from '../live-paper/book-publication.mjs';
import {evaluateCanonicalNoCapacity} from '../live-paper/canonical-no-capacity.mjs';import {capacityCodeIdentity} from '../live-paper/canonical-capacity-monitor.mjs';
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=p=>JSON.parse(fs.readFileSync(p)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(v,c)=>{if(!v)throw Error(c);};
const active=read(path.join(rt,'canonical-capacity-active.json')),dir=path.join(rt,'canonical-capacity-cohorts',active.cohort),bank=read(path.join(dir,'bank.json')),stats=read(path.join(dir,'stats.json')),universe=read('live-paper/price-universe.json'),pids=read('data/current-runtime-pids.json');
need(digest(bank)===active.bankHash&&digest(bank.codeIdentity)===digest(capacityCodeIdentity()),'BANK_IDENTITY');
const source=fs.readFileSync(path.join(rt,'public-books.jsonl')),complete=source.subarray(0,source.lastIndexOf(10)+1),records=complete.toString().split('\n').slice(0,-1).map(JSON.parse),raw=fs.readFileSync(path.join(dir,'journal.jsonl'));need(!raw.length||raw.at(-1)===10,'PARTIAL_JOURNAL');
const journal=raw.toString().split('\n').filter(Boolean).map(JSON.parse).filter(r=>r.sequence<=stats.journalSequence),prefix=complete.subarray(0,bank.sourcePrefixBytes);need(sha(prefix)===bank.sourcePrefixHash,'SOURCE_PREFIX');
let publication=createBookPublicationState(universe),index=0,sequence=0,previous=digest(bank);const results=[];
const instruments=new Set(bank.review.legs.map(l=>l.exchangeId+':YES'));
for(const row of journal){const{hash,...body}=row;need(body.sequence===++sequence&&body.previous===previous&&body.bankHash===active.bankHash&&digest(body)===hash,'CAPACITY_JOURNAL_CHAIN');
 while(publication.sequence<body.pair.sourceSequence){need(records[index],'SOURCE_TRUNCATED');publication=applyBookPublication(publication,records[index++]);
  // Validate every original row/version/boundary; retain only the two required
  // depth arrays to avoid copying hundreds of unrelated ladders per replay row.
  publication={...publication,books:Object.fromEntries(Object.entries(publication.books).filter(([id])=>instruments.has(id)))};
 }
 need(publication.hash===body.pair.sourceHash,'ORIGINAL_SOURCE_IDENTITY');const books=bank.review.legs.map(l=>publication.books[l.exchangeId+':YES']);
 need(books.every((b,i)=>b.versionKey===body.pair.versions[i]&&b.publicationSequence>bank.sourcePrefixSequence&&Date.parse(b.requestStartedAt)>=Date.parse(bank.cutoff)),'PROSPECTIVE_PAIR');
 const replay=evaluateCanonicalNoCapacity({review:bank.review,books,account:body.accountEligibility,now:body.at,policy:bank.policy});need(canonicalJson(replay)===canonicalJson(body.result),'EXACT_CAPACITY_REPLAY');
 results.push({at:body.at,reason:replay.reason,matchedQuantity:replay.matchedQuantity,acquisitionCost:replay.acquisitionCost,ordinaryProfit:replay.conditionalOrdinaryMinimumProfit,exceptionProfit:replay.stressedMinimumProfit,
  requestWindowMs:Math.max(...books.map(b=>Date.parse(b.receivedAt)))-Math.min(...books.map(b=>Date.parse(b.requestStartedAt))),versions:body.pair.versions});previous=hash;
}
need(sequence>0&&sequence===stats.journalSequence&&previous===stats.journalHash,'LIVE_CHECKPOINT');
const saved=read('data/canonical-capacity-probability-preservation-20261003.json'),pd=path.join(rt,'probability-receipt-cohorts/source-probability-20261003-v1'),pj=fs.readFileSync(path.join(pd,'journal.jsonl'));
need(sha(fs.readFileSync(path.join(pd,'bank.json')))===saved.bankSha256&&sha(pj.subarray(0,saved.journalBytes))===saved.journalSha256,'PROBABILITY_ACCOUNTING_CHANGED');
for(const k of ['collector','paper','dashboard','metrics'])process.kill(pids[k],0);
const runtime=(await fetch('http://127.0.0.1:8787/api/stats').then(r=>r.json())).runtime;
need(runtime.pid===pids.collector&&runtime.status==='running'&&runtime.networkBudget.maxReadsPerMinute===20&&runtime.networkBudget.writesEnabled===false&&runtime.networkBudget.executionOwner==='Daenerys','COLLECTOR_OR_AUTHORITY');
need(read(path.join(rt,'competition-activation.json')).enabled===false,'CONTEST_ACTIVATION');
const result={at:new Date().toISOString(),cohort:active.cohort,bankHash:active.bankHash,cutoff:bank.cutoff,codeCommit:'0f205146801a150fcf9dbabc1caa4b02c946e4c2',sourceSequence:publication.sequence,journalSequence:sequence,journalHash:previous,
 exactOriginalSourceAndAccountReplay:true,probabilityBankAndOriginalJournalPrefixPreserved:true,observations:results,latest:stats.last,pids,networkBudget:runtime.networkBudget,
 fourBookSlots:runtime.collectionPlan.fullBookBudget,extraBackgroundProcesses:0,newApiClients:0,competitionOrders:0,profitabilityEstablished:false};atomicJson('data/canonical-capacity-live-verification-20261003.json',result);
console.log(JSON.stringify({at:result.at,journalSequence:sequence,exactReplay:true,probabilityAccountingPreserved:true,observations:results,extraBackgroundProcesses:0,competitionOrders:0,profitabilityEstablished:false}));
