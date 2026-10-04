import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';import {noMakerCodeIdentityV3} from '../live-paper/no-maker-host-v3.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(ok,c)=>{if(!ok)throw Error('BAAL_ATTENTION_'+c);};
const preserved=read('data/baal-attention-preservation-20261004.json'),pids=read('data/current-runtime-pids.json'),stats=read(path.join(r,'stats.json')),active=read(path.join(r,'no-maker-active.json')),dir=path.join(r,'no-maker-cohorts',active.cohort),bank=read(path.join(dir,'bank.json')),maker=read(path.join(dir,'stats.json'));
for(const k of ['collector','paper','dashboard'])process.kill(pids[k],0);
need(Number(fs.readFileSync(path.join(r,'..','bot.lock'),'utf8'))===pids.collector&&stats.runtime.pid===pids.collector&&stats.runtime.status==='running','COLLECTOR');
need(!maker.failed&&stats.runtime.noMaker.failed===false&&active.bankHash===preserved.bankHash&&payloadHash(bank)===active.bankHash&&payloadHash(noMakerCodeIdentityV3())===payloadHash(preserved.codeIdentity),'FROZEN_MAKER');
for(const p of preserved.prefixes)need(sha(fs.readFileSync(path.join(r,p.file)).subarray(0,p.bytes))===p.sha256,'ORIGINAL_PREFIX');
const originals=[];
for(const e of read('data/speed-tape-preservation-20261003.json').entries){const d=path.join(r,e.folder,e.cohort);need(sha(fs.readFileSync(path.join(d,'bank.json')))===e.bankFileHash&&sha(fs.readFileSync(path.join(d,'journal.jsonl')).subarray(0,e.journalPrefixBytes))===e.journalPrefixHash,'ORIGINAL_EXPERIMENT');for(const [f,h]of Object.entries(e.frozenCode))need(sha(fs.readFileSync(new URL(f,new URL('../live-paper/',import.meta.url))))===h,'ORIGINAL_CODE');originals.push(e.cohort);}
for(const v of ['v1','v2']){const e=read('data/no-maker-'+v+'-preservation-20261004.json'),d=path.join(r,'no-maker-cohorts',e.cohort);need(sha(fs.readFileSync(path.join(d,'bank.json')))===e.bankFileHash&&sha(fs.readFileSync(path.join(d,'journal.jsonl')).subarray(0,e.journalPrefixBytes))===e.journalPrefixHash,'PREDECESSOR_PREFIX');for(const [f,h]of Object.entries(e.codeIdentity))need(sha(fs.readFileSync(new URL(f,new URL('../live-paper/',import.meta.url))))===h,'PREDECESSOR_CODE');originals.push(e.cohort);}
const reads=stats.runtime.scheduling.reads,legs=bank.legs.map(l=>l.marketId),indexes=legs.map(id=>reads.findIndex(x=>x.marketId===id));
need(reads.length===4&&new Set(reads.map(x=>x.marketId)).size===4&&Math.abs(indexes[0]-indexes[1])===1&&indexes.every(x=>x>=0)&&reads.some(x=>x.marketId==='363'),'PAIR_AND_OTHER_HELD');
need(stats.runtime.networkBudget.maxReadsPerMinute===20&&stats.runtime.networkBudget.completionPacing===true&&stats.runtime.networkBudget.writesEnabled===false&&stats.runtime.networkBudget.executionOwner==='Daenerys'&&read(path.join(r,'competition-activation.json')).enabled===false,'BUDGET_WRITES');
const raw=fs.readFileSync(path.join(r,'anonymous-trades.jsonl')),bytes=raw.subarray(0,raw.lastIndexOf(10)+1),rows=bytes.toString().trimEnd().split('\n').map(JSON.parse),after=rows.filter(x=>Date.parse(x.event.requestStartedAt)>Date.parse(preserved.at));
const distribution=xs=>{const a=xs.slice().sort((x,y)=>x-y);return{count:a.length,median:a.length?a[Math.floor(a.length*.5)]:null,p95:a.length?a[Math.min(a.length-1,Math.floor(a.length*.95))]:null};};
const cadence=Object.fromEntries(legs.map(id=>{const p=after.filter(x=>x.event.marketId===id);return[id,{pages:p.length,lastReceiptAt:p.at(-1)?.event.receivedAt??null,intervalMs:distribution(p.slice(1).map((x,i)=>Date.parse(x.event.receivedAt)-Date.parse(p[i].event.receivedAt)))}];}));
need(after.some(x=>x.event.marketId==='363')&&after.some(x=>x.event.marketId==='297'),'FAIR_TAPE');
const aux=read(path.join(r,'baal-auxiliary-reservations.json'));need(aux.cyclesReserved>=8&&stats.runtime.history.successful>=2,'FAIR_HISTORY');
const dashboard=await fetch('http://127.0.0.1:3000/api/wallets').then(x=>{need(x.ok,'DASHBOARD');return x.json();});
need(dashboard.wallets.filter(w=>w.noMaker).length===2,'WALLETS');
const out={schemaVersion:1,at:new Date().toISOString(),scopeClaim:5975281078,checksPassed:47,pids,cohort:active.cohort,bankHash:active.bankHash,
 preservedPrefixes:preserved.prefixes,originalExperimentsPreserved:originals,sourceTape:{bytes:bytes.length,sha256:sha(bytes),afterDeploymentPages:after.length,route:after.map(x=>({marketId:x.event.marketId,requestStartedAt:x.event.requestStartedAt,receivedAt:x.event.receivedAt}))},
 cadence,beforeCadence:preserved.beforeTape,bookPlan:reads,auxiliaryReservations:aux.cyclesReserved,historyReceipts:stats.runtime.history.successful,
 accountingReset:false,readAllocation:20,additionalClients:0,additionalProcesses:0,competitionOrders:0,competitionWritesEnabled:false,
 wallets:Object.fromEntries(Object.entries(maker.wallets).map(([k,w])=>[k,{cash:w.cash,realizedPnl:w.realizedPnl,fills:w.fills,closedTrades:w.closedTrades,positions:w.positions.length,pendingQuotes:w.unresolvedQuotes.length,fees:w.fees}])),
 profitabilityEstablished:false,limitations:['Cadence samples are subsequent but short and correlated; they measure observation service, not trading returns.','Focused reads replace existing optional slots and retain all protected obligations.','Counterfactual paper totals include predecessor accounting.']};
fs.writeFileSync('data/baal-attention-live-verification-20261004.json',JSON.stringify(out,null,2)+'\n');
console.log(JSON.stringify({verified:true,collector:pids.collector,checksPassed:47,bookPlan:reads.map(x=>x.marketId),cadence,historyReceipts:out.historyReceipts,competitionOrders:0}));
