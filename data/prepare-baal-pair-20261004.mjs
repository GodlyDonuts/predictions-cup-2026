import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {payloadHash,atomicJson} from '../execution/ledger.mjs';import {BAAL_PAIR_POLICY} from '../live-paper/baal-pair-paper.mjs';
import {baalPairCodeIdentity} from '../live-paper/baal-pair-host.mjs';import {noMakerCodeIdentityV3} from '../live-paper/no-maker-host-v3.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(v,c)=>{if(!v)throw Error(c);};
need(read(path.join(r,'COLLECTOR_STOP')).source==='baal-pair-deployment-20261004'&&!fs.existsSync(path.join(r,'..','bot.lock')),'STOP_OR_LEASE');
const name='baal-no-pair-20261004-v1',dir=path.join(r,'baal-pair-cohorts',name);need(!fs.existsSync(dir)&&!fs.existsSync(path.join(r,'baal-pair-active.json')),'NEW_COHORT_ONLY');
const active=read(path.join(r,'no-maker-active.json')),makerDir=path.join(r,'no-maker-cohorts',active.cohort),makerBank=read(path.join(makerDir,'bank.json')),maker=read(path.join(makerDir,'stats.json'));
need(payloadHash(makerBank)===active.bankHash&&!maker.failed&&payloadHash(makerBank.codeIdentity)===payloadHash(noMakerCodeIdentityV3()),'FROZEN_CONTROL');
const files=['public-books.jsonl','anonymous-trades.jsonl','no-maker-cohorts/'+active.cohort+'/journal.jsonl'];
const prefixes=files.map(file=>{const bytes=fs.readFileSync(path.join(r,file));need(bytes.length&&bytes.at(-1)===10,'SOURCE_COMPLETE');return{file,bytes:bytes.length,sha256:sha(bytes)};});
const source=fs.readFileSync(path.join(r,'public-books.jsonl')),tail=JSON.parse(source.subarray(source.lastIndexOf(10,source.length-2)+1,source.length-1));
const cutoff=new Date().toISOString();need(Date.parse(cutoff)<Date.parse(makerBank.review.expiresAt),'CURRENT_REVIEW');
const tournament=read('tournaments-midterm-elections.json');
const bank={schemaVersion:1,name,mode:'baal_sequential_no_pair_paper',tournamentId:makerBank.tournamentId,cutoff,policy:BAAL_PAIR_POLICY,
 review:makerBank.review,capacityPolicy:makerBank.capacityPolicy,codeIdentity:baalPairCodeIdentity(),
 sourcePrefixBytes:source.length,sourcePrefixHash:sha(source),sourcePrefixSequence:tail.sequence,sourcePrefixTailHash:tail.hash,
 tournamentContext:{id:tournament.id,startDate:tournament.startDate,endDate:tournament.endDate,status:tournament.status,source:'historical_local_official_v1_capture',freshnessVerified:false},
 economics:'Fixed price-capped sequential NO pair; ordinary surplus is conditional, never booked as cash or P&L. Failed hedge/second sale creates an actual modeled orphan loss.',
 executionAuthority:false,competitionOrders:0};
fs.mkdirSync(dir,{recursive:true,mode:0o700});atomicJson(path.join(dir,'bank.json'),bank);atomicJson(path.join(r,'baal-pair-active.json'),{schemaVersion:1,cohort:name,bankHash:payloadHash(bank),executionAuthority:false});
fs.writeFileSync('data/baal-pair-bank-20261004-v1.json',JSON.stringify(bank,null,2)+'\n');
const preservation={at:cutoff,cohort:active.cohort,bankHash:active.bankHash,prefixes,originalMakerWallets:Object.fromEntries(Object.entries(maker.wallets).map(([k,w])=>[k,{cash:w.cash,realizedPnl:w.realizedPnl,fills:w.fills,closedTrades:w.closedTrades,positions:w.positions.length,pendingQuotes:w.unresolvedQuotes.length}])),newCohort:name,newBankHash:payloadHash(bank),independentNewWallet:true,originalAccountingReset:false,competitionOrders:0};
fs.writeFileSync('data/baal-pair-preservation-20261004.json',JSON.stringify(preservation,null,2)+'\n');
console.log(JSON.stringify({cohort:name,cutoff,bankHash:payloadHash(bank),originalAccountingReset:false,preservedPrefixes:prefixes.length,competitionOrders:0}));
