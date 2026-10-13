import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';
import {collectorNativeJournalConfig} from './collector-journal-native.mjs';
import {journalSourceFiles,journalEngineIdentity} from './native-journal-restore.mjs';
import {LEGACY_NATIVE_BACKEND} from './rebind-native-collector-config-v1.mjs';
import {
  CURRENT_CODE_IDENTITY,CURRENT_SOURCE_SNAPSHOT,NATIVE_RESTORE_CHANGE_COMMIT,NATIVE_RESTORE_SHA256,
  PREDECESSOR_CODE_IDENTITY,PREDECESSOR_SOURCE_SNAPSHOT,rebindNativeCollectorJournalV3
} from './rebind-native-collector-config-v3.mjs';

const sha=b=>createHash('sha256').update(b).digest('hex');
const workspace=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const hostRuntime=process.env.LOCALAPPDATA?path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'):null;
const hostBinary=hostRuntime&&path.join(hostRuntime,'journal-native-builds',LEGACY_NATIVE_BACKEND.binaryHash);
const hostSource=hostRuntime&&path.join(hostRuntime,'journal-native-sources',LEGACY_NATIVE_BACKEND.engineHash);
const artifactsAvailable=!!hostRuntime&&['baal-journal-restore.exe','libunwind.dll'].every(f=>fs.existsSync(path.join(hostBinary,f)))&&journalSourceFiles.every(f=>fs.existsSync(path.join(hostSource,f)));
function fixture(t){
 const temp=fs.mkdtempSync(path.join(os.tmpdir(),'native-collector-rebind-v3-'));
 const priorLocalAppData=process.env.LOCALAPPDATA,priorPaperDir=process.env.SIG_PAPER_DIR;
 process.env.LOCALAPPDATA=temp;delete process.env.SIG_PAPER_DIR;
 t.after(()=>{if(priorLocalAppData===undefined)delete process.env.LOCALAPPDATA;else process.env.LOCALAPPDATA=priorLocalAppData;if(priorPaperDir===undefined)delete process.env.SIG_PAPER_DIR;else process.env.SIG_PAPER_DIR=priorPaperDir;fs.rmSync(temp,{recursive:true,force:true});});
 const control=path.join(temp,'SigTradingCup'),runtimeDir=path.join(control,'paper-runtime');fs.mkdirSync(runtimeDir,{recursive:true});
 const stop=path.join(runtimeDir,'EXECUTION_STOP');fs.writeFileSync(stop,JSON.stringify({kind:'test_execution_stop'}));
 const capture={schemaVersion:2,enabled:true,mode:'market_capture_only',channelMode:'tournament'};
 const captureFile=path.join(runtimeDir,'realtime-capture.json');fs.writeFileSync(captureFile,JSON.stringify(capture));
 fs.writeFileSync(path.join(runtimeDir,'pid.txt'),'2147483647\n');
 if(artifactsAvailable){
  const binaryDir=path.join(runtimeDir,'journal-native-builds',LEGACY_NATIVE_BACKEND.binaryHash);fs.mkdirSync(binaryDir,{recursive:true});
  fs.copyFileSync(path.join(hostBinary,'baal-journal-restore.exe'),path.join(binaryDir,'baal-journal-restore.exe'));
  fs.copyFileSync(path.join(hostBinary,'libunwind.dll'),path.join(binaryDir,'libunwind.dll'));
  const archived=path.join(runtimeDir,'journal-native-sources',LEGACY_NATIVE_BACKEND.engineHash);
  for(const name of journalSourceFiles){const target=path.join(archived,name);fs.mkdirSync(path.dirname(target),{recursive:true});fs.copyFileSync(path.join(hostSource,name),target);}
 }
 const config={schemaVersion:1,mode:'native_original_journal_collector_v1',nativeBackend:LEGACY_NATIVE_BACKEND,codeIdentity:PREDECESSOR_CODE_IDENTITY,sourceSnapshot:PREDECESSOR_SOURCE_SNAPSHOT,restorePublicTape:true};
 const configFile=path.join(runtimeDir,'collector-native-journal.json');fs.writeFileSync(configFile,JSON.stringify(config)+'\n');
 const journal=path.join(runtimeDir,'original-journal.jsonl'),receipt=path.join(runtimeDir,'receipts','receipt.json');
 fs.mkdirSync(path.dirname(receipt),{recursive:true});fs.writeFileSync(journal,'original journal bytes\n');fs.writeFileSync(receipt,'original receipt bytes\n');
 const preserved={
  executionStop:stop,capture:captureFile,journal,receipt,
  rate:path.join(runtimeDir,'rate.json'),failure:path.join(runtimeDir,'read-timing-failure.json'),stats:path.join(runtimeDir,'realtime-stats.json')
 };
 fs.writeFileSync(preserved.rate,'{"next":null,"manualReview":true}\n');
 fs.writeFileSync(preserved.failure,'{"reason":"prior_local_failure"}\n');
 fs.writeFileSync(preserved.stats,'{"status":"stopped"}\n');
 return {temp,control,runtimeDir,configFile,config,stop,capture,captureFile,journal,receipt,preserved};
}
function rebind(f,apply=false,extra={}){return rebindNativeCollectorJournalV3({runtimeDir:f.runtimeDir,apply,...extra});}
function expectUnchangedFailure(f,reason){const before=fs.readFileSync(f.configFile);assert.throws(()=>rebind(f,true),e=>e.code===reason);assert.deepEqual(fs.readFileSync(f.configFile),before);}
function snapshot(files){return new Map(Object.entries(files).map(([name,file])=>[name,fs.readFileSync(file)]));}
function assertSnapshot(files,before){for(const [name,bytes]of before)assert.deepEqual(fs.readFileSync(files[name]),bytes,name);}

test('manifest pins one exact predecessor-to-target source graph transition',()=>{
 assert.equal(payloadHash(PREDECESSOR_CODE_IDENTITY),PREDECESSOR_SOURCE_SNAPSHOT);
 assert.equal(payloadHash(CURRENT_CODE_IDENTITY),CURRENT_SOURCE_SNAPSHOT);
 assert.equal(PREDECESSOR_SOURCE_SNAPSHOT,'bb67d7024b6f6bf5c3e3a68247872a0555da625de1f539c20ef4921f255d0d79');
 assert.equal(CURRENT_SOURCE_SNAPSHOT,'a4d96bf3f421be19e45fcc55b99126039936d7e62f0eda789c2fd38a13ba1f06');
 assert.equal(NATIVE_RESTORE_CHANGE_COMMIT,'a5737e0c08af66e65a9c3a6ddd7e7c17b3daeb4c');
 assert.equal(sha(fs.readFileSync(path.join(workspace,'live-paper','native-journal-restore.mjs'))),NATIVE_RESTORE_SHA256);
 assert.equal(NATIVE_RESTORE_SHA256,'752602728ad00f3c5a7a295dd1f7cedf27d937d15d2a5e716ffde8128371a9f2');
 assert.deepEqual(Object.keys(PREDECESSOR_CODE_IDENTITY).filter(k=>PREDECESSOR_CODE_IDENTITY[k]!==CURRENT_CODE_IDENTITY[k]),['live-paper/native-journal-restore.mjs']);
});

test('apply changes only the two pinned identity fields and preserves stop, capture, journals, receipts, rate gate and reports',{skip:!artifactsAvailable},t=>{
 const f=fixture(t),before=snapshot(f.preserved),priorConfig=fs.readFileSync(f.configFile);
 const result=rebind(f,true);
 assert.equal(result.status,'rebound');assert.equal(result.executionStopPreserved,true);assert.equal(result.networkRequests,0);assert.equal(result.competitionOrders,0);
 const updated=JSON.parse(fs.readFileSync(f.configFile,'utf8'));
 assert.deepEqual(updated,{...f.config,codeIdentity:CURRENT_CODE_IDENTITY,sourceSnapshot:CURRENT_SOURCE_SNAPSHOT});
 assert.deepEqual(Object.keys(f.config).sort(),Object.keys(updated).sort());
 assert.deepEqual(collectorNativeJournalConfig(f.runtimeDir),updated);
 assert.notDeepEqual(fs.readFileSync(f.configFile),priorConfig);assertSnapshot(f.preserved,before);
 assert.equal(fs.existsSync(path.join(f.control,'bot.lock')),false);
});

test('dry run validates exact native artifacts and does not mutate any runtime file',{skip:!artifactsAvailable},t=>{
 const f=fixture(t),beforeConfig=fs.readFileSync(f.configFile),before=snapshot(f.preserved);
 const result=rebind(f);
 assert.equal(result.status,'ready');assert.equal(result.applied,false);assert.equal(result.networkRequests,0);assert.equal(result.competitionOrders,0);
 assert.deepEqual(fs.readFileSync(f.configFile),beforeConfig);assertSnapshot(f.preserved,before);
 assert.equal(fs.existsSync(path.join(f.control,'bot.lock')),false);
});

test('wrong or self-hashed predecessor identity is rejected without mutation',t=>{
 const f=fixture(t),bad={...f.config,codeIdentity:{...f.config.codeIdentity,'live-paper/transport-runtime.mjs':'0'.repeat(64)}};
 bad.sourceSnapshot=payloadHash(bad.codeIdentity);fs.writeFileSync(f.configFile,JSON.stringify(bad));expectUnchangedFailure(f,'RUNTIME_PREDECESSOR_IDENTITY_MISMATCH');
});

test('requires execution stop and rejects competing runtime stops without changing config',async t=>{
 for(const [name,mutate,reason] of [
  ['missing execution stop',f=>fs.unlinkSync(f.stop),'EXECUTION_STOP_REQUIRED'],
  ['workspace stop',f=>{const marker=path.join(workspace,'STOP'),existed=fs.existsSync(marker),before=existed?fs.readFileSync(marker):null;if(!existed)fs.writeFileSync(marker,'temporary test stop');try{expectUnchangedFailure(f,'STOP_MARKER_PRESENT');}finally{if(!existed)fs.unlinkSync(marker);else assert.deepEqual(fs.readFileSync(marker),before);}},'STOP_MARKER_PRESENT'],
  ['runtime stop',f=>fs.writeFileSync(path.join(f.runtimeDir,'STOP'),'test'),'STOP_MARKER_PRESENT'],
  ['collector stop',f=>fs.writeFileSync(path.join(f.runtimeDir,'COLLECTOR_STOP'),'test'),'STOP_MARKER_PRESENT'],
  ['realtime stop',f=>fs.writeFileSync(path.join(f.runtimeDir,'REALTIME_STOP'),'test'),'STOP_MARKER_PRESENT']
 ])await t.test(name,t=>{const f=fixture(t);if(name==='workspace stop')mutate(f);else{mutate(f);expectUnchangedFailure(f,reason);}});
});

test('capture scope, configured path, apply flag and environment override are strict',t=>{
 const f=fixture(t);fs.writeFileSync(f.captureFile,JSON.stringify({...f.capture,mode:'live'}));expectUnchangedFailure(f,'CAPTURE_SCOPE_CHANGED');
 fs.writeFileSync(f.captureFile,JSON.stringify(f.capture));
 assert.throws(()=>rebind(f,false,{runtimeDir:path.join(f.temp,'other','paper-runtime')}),e=>e.code==='RUNTIME_PATH_INVALID');
 assert.throws(()=>rebind(f,'false'),e=>e.code==='APPLY_FLAG_INVALID');
 process.env.SIG_PAPER_DIR=f.runtimeDir;assert.throws(()=>rebind(f),e=>e.code==='RUNTIME_OVERRIDE_UNSUPPORTED');delete process.env.SIG_PAPER_DIR;
});

test('missing, malformed, active and unverified runner identities fail closed',t=>{
 const f=fixture(t),pid=path.join(f.runtimeDir,'pid.txt');
 fs.unlinkSync(pid);expectUnchangedFailure(f,'RUNNER_PID_MISSING');
 fs.writeFileSync(pid,'not-a-pid');expectUnchangedFailure(f,'RUNNER_PID_INVALID');
 fs.writeFileSync(pid,String(process.pid));expectUnchangedFailure(f,'RUNNER_PROCESS_ACTIVE');
 fs.writeFileSync(pid,'2147483647');
});

test('shared lock prevents rebind and lock-acquisition race rechecks stop state',t=>{
 const f=fixture(t),lockFile=path.join(f.control,'bot.lock');fs.writeFileSync(lockFile,'other');expectUnchangedFailure(f,'COLLECTOR_LOCKED');fs.unlinkSync(lockFile);
 const before=fs.readFileSync(f.configFile),stopFile=path.join(f.runtimeDir,'REALTIME_STOP'),open=fs.openSync;
 t.mock.method(fs,'openSync',function(file,...args){const fd=open.call(fs,file,...args);if(path.resolve(String(file))===path.resolve(lockFile))fs.writeFileSync(stopFile,'raced');return fd;});
 assert.throws(()=>rebind(f,true),e=>e.code==='STOP_MARKER_PRESENT');assert.deepEqual(fs.readFileSync(f.configFile),before);assert.equal(fs.existsSync(lockFile),false);
});

test('native binary, library and source archive must match existing pinned backend',{skip:!artifactsAvailable},async t=>{
 await t.test('binary',t=>{const f=fixture(t),file=path.join(f.runtimeDir,'journal-native-builds',LEGACY_NATIVE_BACKEND.binaryHash,'baal-journal-restore.exe');fs.writeFileSync(file,'tampered');expectUnchangedFailure(f,'NATIVE_BINDING_INVALID');});
 await t.test('runtime library',t=>{const f=fixture(t),file=path.join(f.runtimeDir,'journal-native-builds',LEGACY_NATIVE_BACKEND.binaryHash,'libunwind.dll');fs.writeFileSync(file,'tampered');expectUnchangedFailure(f,'NATIVE_BINDING_INVALID');});
 await t.test('archived source',t=>{const f=fixture(t),file=path.join(f.runtimeDir,'journal-native-sources',LEGACY_NATIVE_BACKEND.engineHash,journalSourceFiles.at(-1));fs.writeFileSync(file,'tampered');expectUnchangedFailure(f,'NATIVE_SOURCE_CHANGED');});
});

test('post-write guard restores exact config bytes after a concurrent control change',{skip:!artifactsAvailable},t=>{
 const f=fixture(t),before=fs.readFileSync(f.configFile),stopFile=path.join(f.runtimeDir,'REALTIME_STOP'),originalRename=fs.renameSync;let injected=false;
 t.mock.method(fs,'renameSync',function(source,target,...args){const result=originalRename.call(fs,source,target,...args);if(!injected&&path.resolve(String(target))===path.resolve(f.configFile)){injected=true;fs.writeFileSync(stopFile,'post-write race');}return result;});
 assert.throws(()=>rebind(f,true),e=>e.code==='STOP_MARKER_PRESENT');assert.equal(injected,true);assert.deepEqual(fs.readFileSync(f.configFile),before);assert.equal(fs.existsSync(path.join(f.control,'bot.lock')),false);
});

