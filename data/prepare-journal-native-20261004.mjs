import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {Journal} from '../live-paper/transport-runtime.mjs';import {journalLines} from '../live-paper/journal-lines.mjs';
import {openNativeJournal,sourcePrefixHash} from '../live-paper/native-journal-restore.mjs';
import {collectorJournalCodeIdentity,collectorNativeJournalConfig} from '../live-paper/collector-journal-native.mjs';import {atomicJson,payloadHash} from '../execution/ledger.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(v,c)=>{if(!v)throw Error('JOURNAL_PREP_'+c);};
const check=()=>read(path.join(r,'COLLECTOR_STOP')).source==='collector-native-journal-deployment-20261004'&&!fs.existsSync(path.join(r,'..','bot.lock'))&&!fs.existsSync('STOP')&&!fs.existsSync(path.join(r,'STOP'));
need(check(),'STOP_OR_LEASE');need(read(path.join(r,'competition-activation.json')).enabled===false,'ACTIVATION');need(!fs.existsSync(path.join(r,'collector-native-journal.json')),'NO_OVERWRITE');
const budget=read(path.join(r,'coordinated-network-budget.json'));need(budget.schemaVersion===3&&budget.maxReadsPerMinute===20&&budget.writesEnabled===false&&budget.executionOwner==='Daenerys','BUDGET');
const file=path.join(r,'journal.jsonl'),sourceBytes=fs.statSync(file).size,jsStarted=performance.now(),legacy=new Journal(file),legacyMs=performance.now()-jsStarted;
console.log(JSON.stringify({phase:'legacy_restored',sourceBytes,records:legacy.sequence,legacyMs}));
const binding=read('data/journal-native-binding-20261004.json'),native=await openNativeJournal({runtimeDir:r,binding,file,receiptDir:path.join(r,'collector-native-journal-preparation-receipts'),canOperate:check});
need(native.nativeRestore.sourceBytes===sourceBytes&&legacy.sequence===native.sequence&&legacy.previous===native.previous&&payloadHash(legacy.last)===payloadHash(native.last),'FULL_SOURCE_PARITY');
const preservation={};for(const type of ['no-maker','baal-pair','probability-receipt']){
 const active=read(path.join(r,type+'-active.json')),dir=path.join(r,type+'-cohorts',active.cohort),journal=path.join(dir,'journal.jsonl'),endByte=fs.statSync(journal).size;
 need(sha(JSON.stringify(read(path.join(dir,'bank.json'))))===active.bankHash,'BANK');
 // Probability journals root their chain in the frozen bank hash; maker/pair
 // journals use the original Journal's empty genesis predecessor.
 let sequence=0,previous=type==='probability-receipt'?active.bankHash:'';
 for(const line of journalLines(journal,{endByte})){if(!line)continue;const row=JSON.parse(line),{hash,...body}=row;need(body.sequence===++sequence&&body.previous===previous&&sha(JSON.stringify(body))===hash,'WALLET_CHAIN_'+type);previous=hash;}
 preservation[type]={cohort:active.cohort,bankHash:active.bankHash,bankFileHash:sha(fs.readFileSync(path.join(dir,'bank.json'))),journalPrefixBytes:endByte,journalPrefixHash:await sourcePrefixHash(journal,endByte),journalSequence:sequence,journalHash:previous};
}
const codeIdentity=collectorJournalCodeIdentity(),sourceSnapshot=payloadHash(codeIdentity),snapshotRoot=path.join(r,'collector-journal-native-sources',sourceSnapshot);fs.mkdirSync(snapshotRoot,{recursive:true,mode:0o700});
for(const[name,hash]of Object.entries(codeIdentity)){const bytes=fs.readFileSync(name),target=path.join(snapshotRoot,name);need(sha(bytes)===hash,'SOURCE_CHANGED');fs.mkdirSync(path.dirname(target),{recursive:true,mode:0o700});if(fs.existsSync(target))need(sha(fs.readFileSync(target))===hash,'ARCHIVE_CHANGED');else fs.writeFileSync(target,bytes,{flag:'wx',mode:0o600});}
atomicJson(path.join(snapshotRoot,'manifest.json'),{schemaVersion:1,files:codeIdentity});
const config={schemaVersion:1,mode:'native_original_journal_collector_v1',nativeBackend:binding,codeIdentity,sourceSnapshot},receipt={at:new Date().toISOString(),source:{bytes:sourceBytes,records:legacy.sequence,sha256:native.nativeRestore.sourceHash,tailHash:legacy.previous,lastBodyHash:payloadHash(legacy.last)},legacyMs,native:native.nativeRestore,exactWholeBodyParity:true,configHash:payloadHash(config),config,preservation,publicProducerConfigHash:payloadHash(read(path.join(r,'collector-native-publication.json'))),pids:read('data/current-runtime-pids.json'),accountingReset:false,competitionOrders:0};
need(check(),'STOP_CHANGED');atomicJson(path.join(r,'collector-native-journal.json'),config);need(payloadHash(collectorNativeJournalConfig(r))===receipt.configHash,'CONFIG');atomicJson('data/journal-native-preservation-20261004.json',receipt);
console.log(JSON.stringify({at:receipt.at,source:receipt.source,legacyMs,nativeMs:receipt.native.totalMs,exactWholeBodyParity:true,accountingReset:false,competitionOrders:0}));
