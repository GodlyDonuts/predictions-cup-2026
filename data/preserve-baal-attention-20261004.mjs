import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {payloadHash} from '../execution/ledger.mjs';
import {noMakerCodeIdentityV3} from '../live-paper/no-maker-host-v3.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),sha=x=>createHash('sha256').update(x).digest('hex'),read=f=>JSON.parse(fs.readFileSync(path.join(r,f)));
if(read('COLLECTOR_STOP').source!=='baal-attention-deployment-20261004'||fs.existsSync(path.join(r,'..','bot.lock')))throw Error('STOP or lease provenance');
const active=read('no-maker-active.json'),m=read('no-maker-cohorts/'+active.cohort+'/stats.json'),bank=read('no-maker-cohorts/'+active.cohort+'/bank.json');
if(active.hostRevision!==3||m.failed||payloadHash(bank)!==active.bankHash||payloadHash(bank.codeIdentity)!==payloadHash(noMakerCodeIdentityV3()))throw Error('Frozen maker identity');
const files=['anonymous-trades.jsonl','public-books.jsonl','no-maker-cohorts/'+active.cohort+'/journal.jsonl'];
const prefixes=files.map(file=>{const bytes=fs.readFileSync(path.join(r,file));if(!bytes.length||bytes.at(-1)!==10)throw Error('Partial source prefix');return{file,bytes:bytes.length,sha256:sha(bytes)};});
const tape=fs.readFileSync(path.join(r,'anonymous-trades.jsonl'),'utf8').trimEnd().split('\n').map(JSON.parse),gaps={};
for(const id of ['986','1052','1070','1071']){const rows=tape.filter(x=>x.event.exchangeId===id);const values=rows.slice(1).map((x,i)=>Date.parse(x.event.receivedAt)-Date.parse(rows[i].event.receivedAt)).sort((a,b)=>a-b);gaps[id]={pages:rows.length,lastReceiptAt:rows.at(-1).event.receivedAt,medianIntervalMs:values[Math.floor(values.length*.5)],p95IntervalMs:values[Math.floor(values.length*.95)]};}
const out={at:new Date().toISOString(),cohort:active.cohort,bankHash:active.bankHash,codeIdentity:bank.codeIdentity,prefixes,beforeTape:gaps,wallets:Object.fromEntries(Object.entries(m.wallets).map(([k,w])=>[k,{cash:w.cash,fills:w.fills,closedTrades:w.closedTrades,realizedPnl:w.realizedPnl,positions:w.positions,unresolvedQuotes:w.unresolvedQuotes.length}])),competitionOrders:0};
fs.writeFileSync('data/baal-attention-preservation-20261004.json',JSON.stringify(out,null,2));console.log(JSON.stringify({cohort:out.cohort,prefixes:prefixes.length,bankUnchanged:true,modelAccountingPreserved:true}));
