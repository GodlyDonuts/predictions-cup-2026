import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {journalLines} from '../live-paper/journal-lines.mjs';
import {frozenPaperModule} from '../live-paper/frozen-paper-runtime.mjs';import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';
import {nativePublicationState} from '../live-paper/native-publication.mjs';import {paperBookContinuationFrontier} from '../live-paper/paper-book-continuation-frontier.mjs';
import {validateCapacityReview} from '../live-paper/canonical-no-capacity-v2.mjs';
import {NO_MAKER_POLICY} from '../live-paper/no-maker-paper-v3.mjs';import * as makerPrior from '../live-paper/no-maker-host-v3.mjs';
import * as makerNext from '../live-paper/no-maker-host-v5.mjs';import * as pairPrior from '../live-paper/baal-pair-host-v2.mjs';
import * as pairNext from '../live-paper/baal-pair-host-v3.mjs';

const runtimeDir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex');
const need=(v,c)=>{if(!v)throw Error('NH_PAPER_DEPLOY_'+c);};
need(read(path.join(runtimeDir,'COLLECTOR_STOP')).source==='nh-paper-v5-deployment-20261004','MARKER');
need(!fs.existsSync(path.join(runtimeDir,'..','bot.lock'))&&!fs.existsSync('STOP')&&!fs.existsSync(path.join(runtimeDir,'STOP')),'STOP_OR_LEASE');
need(read(path.join(runtimeDir,'competition-activation.json')).enabled===false,'ACTIVATION');
const review=read('data/nh-conditional-refund-review-20261004.json');validateCapacityReview(review);
need(Date.parse(review.recordedAt)<=Date.now()&&Date.parse(review.expiresAt)>Date.now(),'REVIEW_EXPIRED');
function verifyJournal(file){let previous='',sequence=0,last;const prefix=createHash('sha256');let bytes=0;
 for(const line of journalLines(file)){const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===previous&&sha(JSON.stringify(body))===hash,'PRIOR_CHAIN');previous=hash;last=row;prefix.update(line+'\n');bytes+=Buffer.byteLength(line+'\n');}
 need(last,'EMPTY_PRIOR');return{last,sequence,hash:previous,bytes,fileHash:prefix.digest('hex')};
}
async function prior(type,expected,revision,mod,identity){
 const active=read(path.join(runtimeDir,type+'-active.json'));need(active.cohort===expected&&active.hostRevision===revision,'ACTIVE_'+type);
 const dir=path.join(runtimeDir,type+'-cohorts',active.cohort),bankBytes=fs.readFileSync(path.join(dir,'bank.json')),bank=JSON.parse(bankBytes),stats=read(path.join(dir,'stats.json'));
 const preserved=await frozenPaperModule({runtimeDir,bank,modulePath:'live-paper/'+type+'-host-v'+revision+'.mjs',fallback:mod});
 need(payloadHash(bank)===active.bankHash&&payloadHash(bank.codeIdentity)===payloadHash(preserved[identity]()),'FROZEN_PRIOR');
 const journal=verifyJournal(path.join(dir,'journal.jsonl'));need(!stats.failed&&stats.journalSequence===journal.sequence&&stats.journalHash===journal.hash,'PUBLISHED_FRONTIER');
 need(journal.last.event.kind==='boundary'&&journal.last.event.reason==='close','PRIOR_NOT_CLOSED');
 need(review.legs.length===bank.review.legs.length&&review.legs.every((l,i)=>['marketId','exchangeId','outcome'].every(k=>l[k]===bank.review.legs[i][k])),'SCOPE_CHANGED');
 const seed=journal.last.state.paper,seedBytes=Buffer.from(JSON.stringify(seed,null,2)+'\n');need(seed.bankHash===active.bankHash,'STATE_BANK');
 return{type,active,dir,bank,bankBytes,stats,journal,seed,seedBytes};
}
const maker=await prior('no-maker','no-maker-tape-20261004-v3',3,makerPrior,'noMakerCodeIdentityV3');
const pair=await prior('baal-pair','baal-no-pair-20261004-v2',2,pairPrior,'baalPairCodeIdentity');
const tape=verifyJournal(path.join(runtimeDir,'anonymous-trades.jsonl'));
need(maker.journal.last.state.tapeSequence===tape.sequence&&maker.journal.last.state.tapeHash===tape.hash,'UNCONSUMED_TAPE');
const probabilityActive=read(path.join(runtimeDir,'probability-receipt-active.json')),probabilityBank=read(path.join(runtimeDir,'probability-receipt-cohorts',probabilityActive.cohort,'bank.json'));
const publicFile=path.join(runtimeDir,'public-books.jsonl'),publicBytes=fs.readFileSync(publicFile),publicHash=sha(publicBytes);
const native=await nativePublicationState({runtimeDir,binding:probabilityBank.nativeBackend,universe:probabilityBank.universe,journal:publicFile,endByte:publicBytes.length,expectedSha:publicHash,receiptDir:path.join(runtimeDir,'nh-paper-deployment-native-receipts')});
need(native.state.closed,'SOURCE_NOT_CLOSED');
const frontier=paperBookContinuationFrontier({file:publicFile,endByte:publicBytes.length,priorState:pair.journal.last.state,legs:pair.bank.review.legs});
need(frontier.sealedSequence===native.state.sequence&&frontier.sealedHash===native.state.hash,'NATIVE_FRONTIER');
const cutoff=new Date().toISOString();need(Date.parse(cutoff)<Date.parse(review.expiresAt),'REVIEW_EXPIRED_AFTER_VALIDATION');
const continuation=(p,reason)=>({cohort:p.active.cohort,bankHash:p.active.bankHash,bankFileHash:sha(p.bankBytes),journalPrefixBytes:p.journal.bytes,journalPrefixHash:p.journal.fileHash,openingStateFileHash:sha(p.seedBytes),accountingCarried:true,reason});
const makerBank={...maker.bank,name:'no-maker-tape-20261004-v5',hostRevision:5,cutoff,policy:NO_MAKER_POLICY,codeIdentity:makerNext.noMakerCodeIdentityV5(),review,legs:review.legs,reviewExpiresAt:review.expiresAt,
 tapePrefixBytes:tape.bytes,tapePrefixHash:tape.fileHash,tapePrefixSequence:tape.sequence,tapePrefixTailHash:tape.hash,paperRevision:3,
 accountingContinuation:continuation(maker,'fixed_quote_checkpoint_and_exact_consumed_frontier'),executionAuthority:false};
const pairBank={...pair.bank,name:'baal-no-pair-20261004-v3',hostRevision:3,cutoff,review,codeIdentity:pairNext.baalPairCodeIdentity(),
 sourcePrefixBytes:publicBytes.length,sourcePrefixHash:publicHash,sourcePrefixSequence:native.state.sequence,sourcePrefixTailHash:native.state.hash,
 accountingContinuation:{...continuation(pair,'fresh_source_refund_scenarios_unchanged_pair_economics'),sourceFrontier:frontier},executionAuthority:false};
const plans=[[maker,makerBank],[pair,pairBank]];
for(const [p,b] of plans)need(!fs.existsSync(path.join(runtimeDir,p.type+'-cohorts',b.name)),'NO_OVERWRITE');
const files=collectFrozenPaperFiles(process.cwd(),['live-paper/no-maker-host-v5.mjs','live-paper/baal-pair-host-v3.mjs']),snapshot=payloadHash(files),root=path.join(runtimeDir,'frozen-code',snapshot);
fs.mkdirSync(root,{recursive:true,mode:0o700});for(const [file,hash]of Object.entries(files)){const target=path.join(root,file),bytes=fs.readFileSync(file);need(sha(bytes)===hash,'SOURCE_CHANGED');fs.mkdirSync(path.dirname(target),{recursive:true,mode:0o700});if(fs.existsSync(target))need(sha(fs.readFileSync(target))===hash,'SNAPSHOT_CHANGED');else fs.writeFileSync(target,bytes,{flag:'wx',mode:0o600});}
atomicJson(path.join(root,'manifest.json'),{schemaVersion:1,files});
const indexFile=path.join(runtimeDir,'frozen-code/index.json'),index=read(indexFile);
for(const [p,b]of plans){const dir=path.join(runtimeDir,p.type+'-cohorts',b.name);fs.mkdirSync(dir,{recursive:true,mode:0o700});fs.writeFileSync(path.join(dir,'opening-state.json'),p.seedBytes,{flag:'wx',mode:0o600});atomicJson(path.join(dir,'bank.json'),b);index.banks[payloadHash(b)]={snapshot};}
atomicJson(indexFile,index);
for(const [p,b]of plans)atomicJson(path.join(runtimeDir,p.type+'-active.json'),{schemaVersion:1,cohort:b.name,bankHash:payloadHash(b),hostRevision:b.hostRevision,executionAuthority:false});
const makerFrozen=await frozenPaperModule({runtimeDir,bank:makerBank,modulePath:'live-paper/no-maker-host-v5.mjs',fallback:makerNext}),pairFrozen=await frozenPaperModule({runtimeDir,bank:pairBank,modulePath:'live-paper/baal-pair-host-v3.mjs',fallback:pairNext});
const makerHost=makerFrozen.openNoMakerHostV5({runtimeDir}),pairHost=pairFrozen.openBaalPairHost({runtimeDir});need(!makerHost.status().failed&&!pairHost.status().failed,'BOOT_PROBE');
for(const [p,b]of plans){
 need(sha(fs.readFileSync(path.join(p.dir,'bank.json')))===sha(p.bankBytes)&&verifyJournal(path.join(p.dir,'journal.jsonl')).fileHash===p.journal.fileHash,'PREDECESSOR_CHANGED');
 atomicJson(path.join(p.dir,'STOP'),{source:'Codex successor deployment',at:cutoff,successor:b.name,reason:b.accountingContinuation.reason});
 atomicJson('data/'+p.type+'-bank-20261004-v'+b.hostRevision+'.json',b);
}
const makerWallets=Object.fromEntries(Object.entries(makerHost.status().wallets).map(([k,w])=>[k,{cash:w.cash,reserved:w.reserved,fills:w.fills,realizedPnl:w.realizedPnl,positions:w.positions.length,unresolvedQuotes:w.unresolvedQuotes.length}]));
const report={at:cutoff,reviewHash:payloadHash(review),reviewExpiresAt:review.expiresAt,frozenSourceSnapshot:snapshot,frozenFiles:Object.keys(files).length,
 maker:{cohort:makerBank.name,bankHash:payloadHash(makerBank),accountingContinuation:makerBank.accountingContinuation,openingWallets:makerWallets},
 pair:{cohort:pairBank.name,bankHash:payloadHash(pairBank),accountingContinuation:pairBank.accountingContinuation,openingWallet:{cash:pairHost.status().cash,fills:pairHost.status().fills,realizedPnl:pairHost.status().realizedPnl}},
 nativeSealedSource:{bytes:publicBytes.length,hash:publicHash,sequence:native.state.sequence,engineHash:native.engineHash},accountingReset:false,additionalReadSlots:0,additionalPersistentProcesses:0,competitionOrders:0,profitabilityEstablished:false};
atomicJson('data/nh-paper-v5-preservation-20261004.json',report);console.log(JSON.stringify(report));
