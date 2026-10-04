import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {payloadHash,atomicJson} from '../execution/ledger.mjs';
import {NO_MAKER_POLICY} from '../live-paper/no-maker-paper.mjs';import {noMakerCodeIdentity} from '../live-paper/no-maker-host.mjs';import {noMakerCodeIdentityV2} from '../live-paper/no-maker-host-v2.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),sha=b=>createHash('sha256').update(b).digest('hex');
const marker=JSON.parse(fs.readFileSync(path.join(r,'COLLECTOR_STOP')));if(marker.source!=='no-maker-v2-deployment-20261004')throw Error('Deployment STOP provenance');
if(fs.existsSync(path.join(r,'..','bot.lock'))||fs.existsSync('STOP')||fs.existsSync(path.join(r,'STOP')))throw Error('Lease or human STOP');
if(JSON.parse(fs.readFileSync(path.join(r,'competition-activation.json'))).enabled!==false)throw Error('Contest activation');
const activeFile=path.join(r,'no-maker-active.json'),oldActive=JSON.parse(fs.readFileSync(activeFile));if(oldActive.cohort!=='no-maker-tape-20261004-v1')throw Error('Unexpected active cohort');
const oldDir=path.join(r,'no-maker-cohorts',oldActive.cohort),oldBytes=fs.readFileSync(path.join(oldDir,'bank.json')),old=JSON.parse(oldBytes),oldStats=JSON.parse(fs.readFileSync(path.join(oldDir,'stats.json'))),oldJournal=fs.readFileSync(path.join(oldDir,'journal.jsonl'));
if(payloadHash(old)!==oldActive.bankHash||payloadHash(old.codeIdentity)!==payloadHash(noMakerCodeIdentity()))throw Error('Original frozen identity');
if(oldStats.failed||Object.values(oldStats.wallets).some(w=>w.positions.length||w.unresolvedQuotes.length))throw Error('Original inventory or unresolved quote requires continued original operation');
const source=fs.readFileSync(path.join(r,'anonymous-trades.jsonl'));if(source.length&&source.at(-1)!==10)throw Error('Partial tape source');
let sequence=0,previous='';for(const line of source.toString().trimEnd().split('\n').filter(Boolean)){const row=JSON.parse(line),{hash,...body}=row;if(row.sequence!==++sequence||row.previous!==previous||sha(JSON.stringify(body))!==hash)throw Error('Tape integrity');previous=hash;}
const name='no-maker-tape-20261004-v2',dir=path.join(r,'no-maker-cohorts',name),cutoff=new Date().toISOString();if(fs.existsSync(dir))throw Error('Frozen successor exists; no overwrite');
const bank={schemaVersion:1,hostRevision:2,mode:old.mode,name,cutoff,policy:NO_MAKER_POLICY,tournamentId:old.tournamentId,reviewExpiresAt:old.reviewExpiresAt,
 legs:old.legs,review:old.review,capacityPolicy:old.capacityPolicy,baseCapacityBankHash:old.baseCapacityBankHash,codeIdentity:noMakerCodeIdentityV2(),
 tapePrefixBytes:source.length,tapePrefixHash:sha(source),tapePrefixSequence:sequence,tapePrefixTailHash:previous,
 predecessor:{cohort:old.name,bankHash:oldActive.bankHash,reason:'source_order_admission_correction',accountingCarried:false},
 representation:old.representation,nativeNoObserved:false,executionAuthority:false};
fs.mkdirSync(dir,{recursive:true,mode:0o700});atomicJson(path.join(dir,'bank.json'),bank);
const preserved={at:cutoff,cohort:old.name,bankHash:oldActive.bankHash,bankFileHash:sha(oldBytes),codeIdentity:old.codeIdentity,journalPrefixBytes:oldJournal.length,journalPrefixHash:sha(oldJournal),journalSequence:oldStats.journalSequence,journalHash:oldStats.journalHash,
 reason:'Retired flat after confirmed source-order admission defect; original zero-fill accounting preserved',wallets:Object.fromEntries(Object.entries(oldStats.wallets).map(([k,w])=>[k,{cash:w.cash,reserved:w.reserved,fills:w.fills,realizedPnl:w.realizedPnl,positions:w.positions.length,unresolvedQuotes:w.unresolvedQuotes.length}]))};
atomicJson(path.join(oldDir,'STOP'),{source:'Codex successor deployment',at:cutoff,reason:preserved.reason,successor:name});
atomicJson(activeFile,{cohort:name,bankHash:payloadHash(bank),hostRevision:2});
fs.writeFileSync('data/no-maker-v1-preservation-20261004.json',JSON.stringify(preserved,null,2));fs.writeFileSync('data/no-maker-bank-20261004-v2.json',JSON.stringify(bank,null,2));
console.log(JSON.stringify({cohort:name,cutoff,bankHash:payloadHash(bank),hostRevision:2,priorTapePagesSkipped:sequence,originalWalletsRetained:true,competitionOrders:0}));
