import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {collectorPublicationCodeIdentity} from '../live-paper/collector-publication-native.mjs';
import {nativePublicationState} from '../live-paper/native-publication.mjs';import {journalLines} from '../live-paper/journal-lines.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(v,c)=>{if(!v)throw Error('NATIVE_COLLECTOR_PREP_'+c);};
need(read(path.join(r,'COLLECTOR_STOP')).source==='collector-native-v3-deployment-20261004'&&!fs.existsSync(path.join(r,'..','bot.lock'))&&!fs.existsSync('STOP')&&!fs.existsSync(path.join(r,'STOP')),'STOP_OR_LEASE');
need(read(path.join(r,'competition-activation.json')).enabled===false,'ACTIVATION');
need(!fs.existsSync(path.join(r,'collector-native-publication.json')),'NO_OVERWRITE');
const probability=read(path.join(r,'probability-receipt-active.json')),bank=read(path.join(r,'probability-receipt-cohorts',probability.cohort,'bank.json')),file=path.join(r,'public-books.jsonl'),bytes=fs.readFileSync(file),hash=sha(bytes);
const native=await nativePublicationState({runtimeDir:r,binding:bank.nativeBackend,universe:bank.universe,journal:file,endByte:bytes.length,expectedSha:hash,receiptDir:path.join(r,'collector-native-deployment-receipts')});need(native.state.closed,'SOURCE_NOT_CLOSED');
const identities=collectorPublicationCodeIdentity(),snapshot=payloadHash(identities),root=path.join(r,'collector-native-sources',snapshot);fs.mkdirSync(root,{recursive:true,mode:0o700});
for(const[name,hash]of Object.entries(identities)){const source=fs.readFileSync(name),target=path.join(root,name);need(sha(source)===hash,'SOURCE_CHANGED');fs.mkdirSync(path.dirname(target),{recursive:true,mode:0o700});if(fs.existsSync(target))need(sha(fs.readFileSync(target))===hash,'SNAPSHOT_CHANGED');else fs.writeFileSync(target,source,{flag:'wx',mode:0o600});}
atomicJson(path.join(root,'manifest.json'),{schemaVersion:1,files:identities});
const preservation={};for(const type of ['no-maker','baal-pair']){
 const active=read(path.join(r,type+'-active.json')),dir=path.join(r,type+'-cohorts',active.cohort),priorBank=fs.readFileSync(path.join(dir,'bank.json')),prefix=createHash('sha256');let sequence=0,previous='',last,byteCount=0;
 for(const line of journalLines(path.join(dir,'journal.jsonl'))){const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===previous&&sha(JSON.stringify(body))===hash,'WALLET_CHAIN');prefix.update(line+'\n');byteCount+=Buffer.byteLength(line+'\n');previous=hash;last=row;}
 const stats=read(path.join(dir,'stats.json'));need(last.event.kind==='boundary'&&last.event.reason==='close'&&!stats.failed&&stats.journalSequence===sequence&&stats.journalHash===previous,'WALLET_NOT_CLOSED');
 preservation[type]={cohort:active.cohort,bankHash:active.bankHash,bankFileHash:sha(priorBank),journalPrefixBytes:byteCount,journalPrefixHash:prefix.digest('hex'),journalSequence:sequence,journalHash:previous,
  wallets:type==='no-maker'?Object.fromEntries(Object.entries(stats.wallets).map(([k,w])=>[k,{cash:w.cash,realizedPnl:w.realizedPnl,fills:w.fills,positions:w.positions.length}])):{cash:stats.cash,realizedPnl:stats.realizedPnl,fills:stats.fills,positions:Object.keys(last.state.paper.positions).length}};
}
const config={schemaVersion:1,mode:'native_publication_collector_v1',scopeHash:native.state.scopeHash,nativeBackend:bank.nativeBackend,codeIdentity:identities,sourceSnapshot:snapshot};
const receipt={at:new Date().toISOString(),source:{bytes:bytes.length,hash,sequence:native.state.sequence,tailHash:native.state.hash},previousPublisher:read(path.join(r,'stats.json')).runtime.bookPublisher,
 configHash:payloadHash(config),config,sourceFiles:Object.keys(identities).length,preservation,paperWorker:{cohort:probability.cohort,bankHash:probability.bankHash,pid:read('data/current-runtime-pids.json').paper,stopped:false},accountingReset:false,additionalPersistentProcesses:0,additionalReadSlots:0,competitionOrders:0};
atomicJson(path.join(r,'collector-native-publication.json'),config);atomicJson('data/native-collector-preservation-20261004.json',receipt);
console.log(JSON.stringify({at:receipt.at,source:receipt.source,previousPublisher:receipt.previousPublisher,sourceFiles:receipt.sourceFiles,accountingReset:false,competitionOrders:0}));
