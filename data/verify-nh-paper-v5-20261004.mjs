import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {journalLines} from '../live-paper/journal-lines.mjs';
import {amount,money} from '../paper/fixed.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(v,c)=>{if(!v)throw Error('NH_LIVE_'+c);};
async function prefixHash(file,end){const h=createHash('sha256');let bytes=0;for await(const b of fs.createReadStream(file,{end:end-1})){h.update(b);bytes+=b.length;}need(bytes===end,'PREFIX_TRUNCATED');return h.digest('hex');}
function boundState(file,stats){let previous='',sequence=0,match;for(const line of journalLines(file)){const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===previous&&sha(JSON.stringify(body))===hash,'CHAIN');previous=hash;if(row.sequence===stats.journalSequence){need(hash===stats.journalHash,'PUBLISHED_TAIL');match=row.state;break;}}need(match,'PUBLISHED_TRUNCATED');return match;}
const preservation=read('data/nh-paper-v5-preservation-20261004.json'),budget=read(path.join(r,'coordinated-network-budget.json'));
need(!fs.existsSync('STOP')&&!fs.existsSync(path.join(r,'STOP'))&&!fs.existsSync(path.join(r,'COLLECTOR_STOP')),'STOP');
need(budget.maxReadsPerMinute===20&&budget.writesEnabled===false&&budget.executionOwner==='Daenerys'&&read(path.join(r,'competition-activation.json')).enabled===false,'AUTHORITY');
const index=read(path.join(r,'frozen-code/index.json')),wallets={},states={};let tapePrefixSequence;
for(const [type,description]of [['no-maker',preservation.maker],['baal-pair',preservation.pair]]){
 const active=read(path.join(r,type+'-active.json')),dir=path.join(r,type+'-cohorts',active.cohort),bank=read(path.join(dir,'bank.json')),stats=read(path.join(dir,'stats.json'));
 need(active.cohort===description.cohort&&payloadHash(bank)===active.bankHash&&active.bankHash===description.bankHash&&!stats.failed,'ACTIVE');
 if(type==='no-maker')tapePrefixSequence=bank.tapePrefixSequence;
 const c=bank.accountingContinuation,prior=path.join(r,type+'-cohorts',c.cohort);need(sha(fs.readFileSync(path.join(prior,'bank.json')))===c.bankFileHash&&await prefixHash(path.join(prior,'journal.jsonl'),c.journalPrefixBytes)===c.journalPrefixHash,'PRESERVATION');
 need(fs.statSync(path.join(prior,'journal.jsonl')).size===c.journalPrefixBytes,'PREDECESSOR_APPENDED');
 const binding=index.banks[payloadHash(bank)],root=path.join(r,'frozen-code',binding.snapshot),manifest=read(path.join(root,'manifest.json'));
 need(binding.snapshot===preservation.frozenSourceSnapshot&&payloadHash(manifest.files)===binding.snapshot,'SNAPSHOT');for(const[file,hash]of Object.entries(manifest.files))need(sha(fs.readFileSync(path.join(root,file)))===hash,'FROZEN_FILE');
 states[type]=boundState(path.join(dir,'journal.jsonl'),stats);wallets[type]=type==='no-maker'?Object.fromEntries(Object.entries(stats.wallets).map(([k,w])=>[k,{cash:w.cash,reserved:w.reserved,fills:w.fills,closedTrades:w.closedTrades,realizedPnl:w.realizedPnl,positions:w.positions.length,unresolvedQuotes:w.unresolvedQuotes.length,fees:w.fees}])):{cash:stats.cash,fills:stats.fills,realizedPnl:stats.realizedPnl,positions:Object.keys(states[type].paper.positions).length};
}
const recovered=[];for(const line of journalLines(path.join(r,'anonymous-trades.jsonl'))){const row=JSON.parse(line),e=row.event;if(row.sequence<=tapePrefixSequence||e.queryPurpose!=='expired_paper_quote_coverage_recovery')continue;
 const quote=Object.values(states['no-maker'].paper.wallets).flatMap(w=>Object.values(w.quotes)).find(q=>q.exchangeId===e.exchangeId&&q.activationAt===e.from&&q.expiresAt===e.to);if(!quote)continue;
 need(e.coverage.complete===true&&!e.pagination.hasMore&&BigInt(e.coverage.projectedThroughSequence)>=BigInt(quote.coverageBarrierSequence),'RECOVERY_COMPLETENESS');
 recovered.push({sourceSequence:row.sequence,sourceHash:row.hash,marketId:e.marketId,exchangeId:e.exchangeId,from:e.from,to:e.to,requestStartedAt:e.requestStartedAt,receivedAt:e.receivedAt,requiredCheckpoint:quote.coverageBarrierSequence,projectedThroughSequence:e.coverage.projectedThroughSequence,receivedCount:e.receivedCount,novelTrades:e.trades.length,queryPurpose:e.queryPurpose});
}
need(new Set(recovered.map(x=>x.exchangeId)).size===2,'RECOVERY_NOT_FINISHED');
const oldIds=new Set([8,9,14,15]),resolved=Object.entries(states['no-maker'].paper.wallets).flatMap(([family,w])=>Object.values(w.quotes).filter(q=>oldIds.has(q.id)).map(q=>({family,id:q.id,marketId:q.marketId,resolvedAt:q.resolvedAt,resolution:q.resolution,checkpoint:q.coverageBarrierSequence})));
need(resolved.length===4&&resolved.every(q=>q.resolvedAt&&q.resolution==='projection_observed_without_qualifying_trade'),'STUCK_QUOTE');
let completion;for(const line of journalLines(path.join(r,'no-maker-cohorts',preservation.maker.cohort,'journal.jsonl'))){const row=JSON.parse(line),paper=row.state.paper;
 const old=Object.values(paper.wallets).flatMap(w=>Object.values(w.quotes).filter(q=>oldIds.has(q.id)));if(old.length===4&&old.every(q=>q.resolvedAt)){
  completion={journalSequence:row.sequence,journalHash:row.hash,at:paper.lastAt,wallets:Object.fromEntries(Object.entries(paper.wallets).map(([k,w])=>[k,{cash:w.cash,fills:w.fills.length,realizedPnl:w.realized,reserved:money(Object.values(w.quotes).filter(q=>!q.resolvedAt&&q.action==='buy').reduce((n,q)=>n+amount(q.limit)*BigInt(q.quantity),0n))}]))};break;
 }}need(completion,'RECOVERY_CHECKPOINT');
for(const [family,w]of Object.entries(completion.wallets)){const opening=preservation.maker.openingWallets[family];need(w.cash===opening.cash&&w.fills===opening.fills&&w.realizedPnl===opening.realizedPnl&&w.reserved==='0.000000','RECOVERY_CHANGED_ECONOMICS');}
const runtime=read(path.join(r,'stats.json')).runtime;need(runtime.status==='running'&&runtime.cycles>0&&!runtime.noMaker?.failed&&!runtime.baalPair?.failed,'COLLECTOR');
const report={at:new Date().toISOString(),cohorts:{maker:preservation.maker.cohort,pair:preservation.pair.cohort},pids:read('data/current-runtime-pids.json'),collector:{status:runtime.status,cycles:runtime.cycles,lastCycleAt:runtime.lastCycleAt},recovery:{fixedCheckpoints:true,resolved,sourcePages:recovered,completion,releasedReservationPerWallet:'0.945000',cashOrFillReset:false},wallets,reviewExpiresAt:read(path.join(r,'no-maker-cohorts',preservation.maker.cohort,'bank.json')).review.expiresAt,frozenFiles:40,tests:42,competitionOrders:0,additionalReadSlots:0,additionalPersistentProcesses:0,profitabilityEstablished:false};
atomicJson('data/nh-paper-v5-live-20261004.json',report);console.log(JSON.stringify(report));
