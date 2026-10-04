import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {nativeExecutable} from '../live-paper/rust-backend.mjs';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';

const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const read=file=>JSON.parse(fs.readFileSync(file));
const active=read(path.join(runtime,'native-quote-active.json'));
const originalDir=path.join(runtime,'research-cohorts',active.cohort);
const graph=[...fs.readFileSync('backend-rust/build.rs','utf8').matchAll(/^\s*"([^"]+)",$/gm)].map(x=>x[1]);
const digest=createHash('sha256'),sourceHashes={};
for(const file of graph){
 const bytes=fs.readFileSync(path.join('backend-rust',file)),name=Buffer.from(file),n=Buffer.alloc(8);
 n.writeBigUInt64LE(BigInt(name.length));digest.update(n).update(name);
 n.writeBigUInt64LE(BigInt(bytes.length));digest.update(n).update(bytes);
 sourceHashes['backend-rust/'+file]=sha(bytes);
}
const engineHash=digest.digest('hex'),binaryHash=sha(fs.readFileSync(nativeExecutable));
const cohort='baal-trend-mechanisms-20261004-v1',dir=path.join(runtime,'research-cohorts',cohort);
assert(!fs.existsSync(dir),'Refusing to replace a frozen mechanism protocol');fs.mkdirSync(dir);
const declaredAt=new Date().toISOString();
const protocol={schemaVersion:1,mode:'frozen_quote_mechanism_probe_v1',cohort,
 declaredAt,signalAfter:Date.parse(declaredAt),claimComment:5983314037,
 engineHash,binaryHash,runtimeLibraryHash:sha(fs.readFileSync(path.join(path.dirname(nativeExecutable),'libunwind.dll'))),
 sourceHashes,baseCohort:active.cohort,baseDeclarationHash:sha(fs.readFileSync(path.join(originalDir,'declaration.json'))),
 developmentInputHash:sha(fs.readFileSync(path.join(originalDir,'seed.sources.json'))),
 families:['trend_continuation','trend_reversal'],contractSides:['YES','NO'],
 lookbackMs:900000,minimumSpanMs:840000,minimumDistinctMinutes:10,
 minimumMidpointMove:'0.020000',minimumBidAndAskMove:'0.020000',maximumObservationGapMs:120000,
 horizonMs:[300000,1800000,7200000],scenarios:read(path.join(originalDir,'declaration.json')).scenarios,
 entry:'first_independent_request_after_latency_within_original_ask_limit_and120s',
 outcomes:'separate_strict_path_and_fixed_endpoint_views_no_intraperiod_stops_inferred',
 fees:0,networkRequests:0,competitionOrders:0,additionalPersistentProcesses:0,
 notAnActiveWalletOrExecutionBank:true,profitabilityEstablished:false,
 futureValidation:'Signals require both original request and publication strictly after this cutoff. Historical comparison is exploratory; prospective quote results still require independent executable-depth evidence.'};
const bytes=JSON.stringify(protocol,null,2)+'\n';
fs.writeFileSync(path.join(dir,'protocol.json'),bytes,{flag:'wx'});
fs.copyFileSync(path.join(originalDir,'declaration.json'),path.join(dir,'base-declaration.json'),fs.constants.COPYFILE_EXCL);
fs.copyFileSync(path.join(originalDir,'seed.sources.json'),path.join(dir,'development.sources.json'),fs.constants.COPYFILE_EXCL);
for(const [file,hash] of Object.entries(sourceHashes)){
 const target=path.join(runtime,'native-sources',engineHash,file.replace(/^backend-rust\//,''));
 fs.mkdirSync(path.dirname(target),{recursive:true});
 if(fs.existsSync(target))assert.equal(sha(fs.readFileSync(target)),hash);else fs.copyFileSync(file,target,fs.constants.COPYFILE_EXCL);
}
const binaryDir=path.join(runtime,'native-builds',binaryHash);fs.mkdirSync(binaryDir,{recursive:true});
for(const file of ['baal-backend.exe','libunwind.dll']){
 const source=path.join(path.dirname(nativeExecutable),file),target=path.join(binaryDir,file);
 if(fs.existsSync(target))assert.equal(sha(fs.readFileSync(target)),sha(fs.readFileSync(source)));else fs.copyFileSync(source,target,fs.constants.COPYFILE_EXCL);
}
fs.writeFileSync('data/native-mechanisms-protocol-20261004.json',bytes,{flag:'wx'});
console.log(JSON.stringify({cohort,declaredAt,signalAfter:protocol.signalAfter,engineHash,binaryHash,protocolHash:sha(bytes),cases:36,fees:0,newLiveWallets:0}));
