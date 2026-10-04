import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {sourcePrefixHash,journalEngineIdentity} from '../live-paper/native-journal-restore.mjs';import {collectorNativeJournalConfig} from '../live-paper/collector-journal-native.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(v,c)=>{if(!v)throw Error('JOURNAL_LIVE_'+c);};
const old=read('data/journal-native-preservation-20261004.json'),pids=read('data/current-runtime-pids.json'),runtime=read(path.join(r,'stats.json')).runtime,config=collectorNativeJournalConfig(r);
need(runtime.pid===pids.collector&&runtime.status==='running'&&runtime.cycles>0&&runtime.originalJournal?.revision===2&&runtime.bookPublisher?.revision===3,'RUNNING');
need(pids.paper===old.pids.paper&&pids.dashboard===old.pids.dashboard&&payloadHash(config)===old.configHash&&payloadHash(read(path.join(r,'collector-native-publication.json')))===old.publicProducerConfigHash,'PRODUCER_OR_WORKER_CHANGED');
const startup=runtime.originalJournal.native;
need(startup.sourceBytes===old.source.bytes&&startup.sourceHash===old.source.sha256&&startup.sequence===old.source.records&&startup.tailHash===old.source.tailHash&&startup.lastBodyHash===old.source.lastBodyHash&&startup.engineHash===config.nativeBackend.engineHash&&startup.binaryHash===config.nativeBackend.binaryHash,'STARTUP_PARITY');
need(await sourcePrefixHash(path.join(r,'journal.jsonl'),old.source.bytes)===old.source.sha256,'ORIGINAL_CHANGED');
const manifest=read(path.join(r,'collector-journal-native-sources',config.sourceSnapshot,'manifest.json'));need(payloadHash(manifest.files)===config.sourceSnapshot,'ARCHIVE');
for(const[name,hash]of Object.entries(manifest.files))need(sha(fs.readFileSync(path.join(r,'collector-journal-native-sources',config.sourceSnapshot,name)))===hash,'ARCHIVED_SOURCE');
need(journalEngineIdentity(path.join(r,'journal-native-sources',config.nativeBackend.engineHash))===config.nativeBackend.engineHash,'COMPILED_SOURCE');
for(const[type,prior]of Object.entries(old.preservation)){
 const active=read(path.join(r,type+'-active.json')),dir=path.join(r,type+'-cohorts',active.cohort);need(active.cohort===prior.cohort&&active.bankHash===prior.bankHash&&sha(fs.readFileSync(path.join(dir,'bank.json')))===prior.bankFileHash&&await sourcePrefixHash(path.join(dir,'journal.jsonl'),prior.journalPrefixBytes)===prior.journalPrefixHash,'WALLET_RESET');
}
const budget=read(path.join(r,'coordinated-network-budget.json'));need(budget.schemaVersion===3&&budget.maxReadsPerMinute===20&&budget.writesEnabled===false&&budget.executionOwner==='Daenerys'&&read(path.join(r,'competition-activation.json')).enabled===false,'AUTHORITY');
need(!fs.existsSync('STOP')&&!fs.existsSync(path.join(r,'STOP'))&&!fs.existsSync(path.join(r,'COLLECTOR_STOP')),'STOP');
const report={at:new Date().toISOString(),status:'running_native_original_journal',pids,source:old.source,startup:runtime.originalJournal,publicationStartupMs:runtime.bookPublisher.startupReplayMs,sourceFiles:Object.keys(manifest.files).length,cycles:runtime.cycles,lastCycleAt:runtime.lastCycleAt,originalPrefixPreserved:true,walletsAndBanksPreserved:true,accountingReset:false,extraPersistentProcesses:0,extraReadSlots:0,competitionOrders:0,profitabilityEstablished:false};
atomicJson('data/journal-native-live-20261004.json',report);console.log(JSON.stringify(report));
