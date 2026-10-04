import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {payloadHash,atomicJson} from '../execution/ledger.mjs';
import {NO_MAKER_POLICY} from '../live-paper/no-maker-paper-v2.mjs';import * as previousHost from '../live-paper/no-maker-host-v3.mjs';import {frozenPaperModule} from '../live-paper/frozen-paper-runtime.mjs';import {noMakerCodeIdentityV4} from '../live-paper/no-maker-host-v4.mjs';import {validateCapacityReview} from '../live-paper/canonical-no-capacity.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),sha=b=>createHash('sha256').update(b).digest('hex');
const marker=JSON.parse(fs.readFileSync(path.join(r,'COLLECTOR_STOP')));if(marker.source!=='no-maker-v4-deployment-20261004')throw Error('Deployment STOP provenance');
if(fs.existsSync(path.join(r,'..','bot.lock'))||fs.existsSync('STOP')||fs.existsSync(path.join(r,'STOP')))throw Error('Lease or human STOP');
if(JSON.parse(fs.readFileSync(path.join(r,'competition-activation.json'))).enabled!==false)throw Error('Contest activation');
const activeFile=path.join(r,'no-maker-active.json'),oldActive=JSON.parse(fs.readFileSync(activeFile));if(oldActive.cohort!=='no-maker-tape-20261004-v3')throw Error('Unexpected active cohort');
const oldDir=path.join(r,'no-maker-cohorts',oldActive.cohort),oldBytes=fs.readFileSync(path.join(oldDir,'bank.json')),old=JSON.parse(oldBytes),oldStats=JSON.parse(fs.readFileSync(path.join(oldDir,'stats.json'))),oldJournal=fs.readFileSync(path.join(oldDir,'journal.jsonl'));
const preservedHost=await frozenPaperModule({runtimeDir:r,bank:old,modulePath:'live-paper/no-maker-host-v3.mjs',fallback:previousHost});
if(payloadHash(old)!==oldActive.bankHash||payloadHash(old.codeIdentity)!==payloadHash(preservedHost.noMakerCodeIdentityV3()))throw Error('Original frozen identity');
if(oldStats.failed)throw Error('Predecessor failed; accounting must be investigated');
const oldRows=oldJournal.toString().trimEnd().split('\n').map(JSON.parse);let oldPrevious='';for(const [i,row]of oldRows.entries()){const {hash,...body}=row;if(row.sequence!==i+1||row.previous!==oldPrevious||sha(JSON.stringify(body))!==hash)throw Error('Predecessor journal integrity');oldPrevious=hash;}
const seed=oldRows.at(-1).state.paper;if(seed.bankHash!==oldActive.bankHash||oldStats.journalSequence!==oldRows.length||oldStats.journalHash!==oldPrevious)throw Error('Predecessor publication frontier');
const seedBytes=Buffer.from(JSON.stringify(seed,null,2));
const source=fs.readFileSync(path.join(r,'anonymous-trades.jsonl'));if(source.length&&source.at(-1)!==10)throw Error('Partial tape source');
let sequence=0,previous='';for(const line of source.toString().trimEnd().split('\n').filter(Boolean)){const row=JSON.parse(line),{hash,...body}=row;if(row.sequence!==++sequence||row.previous!==previous||sha(JSON.stringify(body))!==hash)throw Error('Tape integrity');previous=hash;}
if(oldRows.at(-1).state.tapeSequence!==sequence||oldRows.at(-1).state.tapeHash!==previous)throw Error('Unconsumed source suffix; resume predecessor before migration');
const args=process.argv.slice(2),reviewArg=args.indexOf('--review');const review=reviewArg>=0?JSON.parse(fs.readFileSync(args[reviewArg+1])):old.review;validateCapacityReview(review);if(review.legs.length!==old.legs.length||review.legs.some((leg,i)=>['marketId','exchangeId','outcome'].some(k=>leg[k]!==old.legs[i][k])))throw Error('Carried accounting scope changed');if(Date.parse(review.expiresAt)<=Date.now()||Date.parse(review.recordedAt)>Date.now())throw Error('Fresh source-backed review required');
const name='no-maker-tape-20261004-v4',dir=path.join(r,'no-maker-cohorts',name),cutoff=new Date().toISOString();if(fs.existsSync(dir))throw Error('Frozen successor exists; no overwrite');
const bank={schemaVersion:1,hostRevision:4,mode:old.mode,name,cutoff,policy:NO_MAKER_POLICY,tournamentId:old.tournamentId,reviewExpiresAt:review.expiresAt,
 legs:review.legs,review,capacityPolicy:old.capacityPolicy,baseCapacityBankHash:old.baseCapacityBankHash,codeIdentity:noMakerCodeIdentityV4(),
 tapePrefixBytes:source.length,tapePrefixHash:sha(source),tapePrefixSequence:sequence,tapePrefixTailHash:previous,
 paperRevision:2,accountingContinuation:{cohort:old.name,bankHash:oldActive.bankHash,bankFileHash:sha(oldBytes),journalPrefixBytes:oldJournal.length,journalPrefixHash:sha(oldJournal),openingStateFileHash:sha(seedBytes),accountingCarried:true,reason:'consumed_frontier_checked_continuation',openingWallets:Object.fromEntries(Object.entries(oldStats.wallets).map(([k,w])=>[k,{cash:w.cash,realizedPnl:w.realizedPnl,fills:w.fills,reserved:w.reserved,positions:w.positions.length,unresolvedQuotes:w.unresolvedQuotes.length}]))},
 representation:old.representation,nativeNoObserved:false,executionAuthority:false};
fs.mkdirSync(dir,{recursive:true,mode:0o700});atomicJson(path.join(dir,'bank.json'),bank);fs.writeFileSync(path.join(dir,'opening-state.json'),seedBytes,{mode:0o600});
const preserved={at:cutoff,cohort:old.name,bankHash:oldActive.bankHash,bankFileHash:sha(oldBytes),codeIdentity:old.codeIdentity,journalPrefixBytes:oldJournal.length,journalPrefixHash:sha(oldJournal),journalSequence:oldStats.journalSequence,journalHash:oldStats.journalHash,
 reason:'Retired after consumed-frontier transition correction; exact cash, inventory, fills and unresolved quotes carried to V4 with source lineage',wallets:Object.fromEntries(Object.entries(oldStats.wallets).map(([k,w])=>[k,{cash:w.cash,reserved:w.reserved,fills:w.fills,realizedPnl:w.realizedPnl,positions:w.positions.length,unresolvedQuotes:w.unresolvedQuotes.length}]))};
atomicJson(path.join(oldDir,'STOP'),{source:'Codex successor deployment',at:cutoff,reason:preserved.reason,successor:name});
atomicJson(activeFile,{cohort:name,bankHash:payloadHash(bank),hostRevision:4});
fs.writeFileSync('data/no-maker-v3-preservation-20261004.json',JSON.stringify(preserved,null,2));fs.writeFileSync('data/no-maker-bank-20261004-v4.json',JSON.stringify(bank,null,2));
console.log(JSON.stringify({cohort:name,cutoff,bankHash:payloadHash(bank),hostRevision:4,sealedConsumedTapePages:sequence,originalWalletsRetained:true,competitionOrders:0}));
