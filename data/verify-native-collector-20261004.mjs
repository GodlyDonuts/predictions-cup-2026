import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';import {collectorNativePublicationConfig} from '../live-paper/collector-publication-native.mjs';
const r=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f)),sha=b=>createHash('sha256').update(b).digest('hex'),need=(v,c)=>{if(!v)throw Error('NATIVE_COLLECTOR_LIVE_'+c);};
async function prefixHash(file,end){const h=createHash('sha256');let bytes=0;for await(const b of fs.createReadStream(file,{end:end-1})){h.update(b);bytes+=b.length;}need(bytes===end,'PREFIX_TRUNCATED');return h.digest('hex');}
const preserved=read('data/native-collector-preservation-20261004.json'),pids=read('data/current-runtime-pids.json'),stats=read(path.join(r,'stats.json')),runtime=stats.runtime;
need(runtime.pid===pids.collector&&runtime.status==='running'&&runtime.cycles>0&&runtime.bookPublisher?.revision===3&&!runtime.noMaker?.failed&&!runtime.baalPair?.failed,'NOT_RUNNING');
need(!fs.existsSync('STOP')&&!fs.existsSync(path.join(r,'STOP'))&&!fs.existsSync(path.join(r,'COLLECTOR_STOP')),'STOP');
const probability=read(path.join(r,'probability-receipt-active.json')),bank=read(path.join(r,'probability-receipt-cohorts',probability.cohort,'bank.json')),c=collectorNativePublicationConfig(r,bank.universe);
need(payloadHash(c)===preserved.configHash&&probability.cohort===preserved.paperWorker.cohort&&probability.bankHash===preserved.paperWorker.bankHash&&pids.paper===preserved.paperWorker.pid,'CONFIG_OR_WORKER');
const startup=runtime.bookPublisher.native;
need(startup.sourceBytes===preserved.source.bytes&&startup.sourceHash===preserved.source.hash&&startup.sourceRecords===preserved.source.sequence&&startup.sourceTailHash===preserved.source.tailHash&&startup.engineHash===c.nativeBackend.engineHash&&startup.binaryHash===c.nativeBackend.binaryHash,'STARTUP_BINDING');
need(await prefixHash(path.join(r,'public-books.jsonl'),preserved.source.bytes)===preserved.source.hash,'SOURCE_CHANGED');
const manifest=read(path.join(r,'collector-native-sources',c.sourceSnapshot,'manifest.json'));need(payloadHash(manifest.files)===c.sourceSnapshot&&payloadHash(manifest.files)===payloadHash(c.codeIdentity),'MANIFEST');
for(const[file,hash]of Object.entries(manifest.files))need(sha(fs.readFileSync(path.join(r,'collector-native-sources',c.sourceSnapshot,file)))===hash,'ARCHIVED_SOURCE');
for(const [type,old]of Object.entries(preserved.preservation)){
 const active=read(path.join(r,type+'-active.json')),dir=path.join(r,type+'-cohorts',active.cohort);need(active.cohort===old.cohort&&active.bankHash===old.bankHash&&sha(fs.readFileSync(path.join(dir,'bank.json')))===old.bankFileHash&&await prefixHash(path.join(dir,'journal.jsonl'),old.journalPrefixBytes)===old.journalPrefixHash,'WALLET_PRESERVATION');
}
const budget=read(path.join(r,'coordinated-network-budget.json'));need(budget.maxReadsPerMinute===20&&!budget.writesEnabled&&budget.executionOwner==='Daenerys'&&read(path.join(r,'competition-activation.json')).enabled===false,'AUTHORITY');
const report={at:new Date().toISOString(),status:'running_native_collector_publication',pids,publisher:runtime.bookPublisher,source:preserved.source,sourceFiles:preserved.sourceFiles,cycles:runtime.cycles,lastCycleAt:runtime.lastCycleAt,
 previousJavaScriptPublisherMs:preserved.previousPublisher.startupReplayMs,nativePublisherMs:runtime.bookPublisher.startupReplayMs,observedPublicationStageSpeedup:preserved.previousPublisher.startupReplayMs/runtime.bookPublisher.startupReplayMs,
 measurementLimit:'Separate live restarts with slightly different source sizes; publication-stage timing only, not total collector startup or trading latency. Original collector journal restoration remains JavaScript.',
 accountingReset:false,walletPrefixPreservationVerified:true,tests:11,extraPersistentProcesses:0,extraReadSlots:0,competitionOrders:0,profitabilityEstablished:false};
atomicJson('data/native-collector-live-20261004.json',report);console.log(JSON.stringify(report));
