import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {payloadHash} from '../execution/ledger.mjs';
import {collectorNativeJournalConfig} from './collector-journal-native.mjs';
import {
  CURRENT_CODE_IDENTITY,CURRENT_SOURCE_SNAPSHOT,HISTORIC_READ_BODY_COMMIT_SHA256,
  LEGACY_CODE_IDENTITY,LEGACY_NATIVE_BACKEND,LEGACY_SOURCE_SNAPSHOT,
  PREDECESSOR_COMMIT,rebindNativeCollectorJournalV1
} from './rebind-native-collector-config-v1.mjs';
import {journalSourceFiles,journalEngineIdentity} from './native-journal-restore.mjs';

const sha=b=>createHash('sha256').update(b).digest('hex');
const workspace=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const hostRuntime=process.env.LOCALAPPDATA?path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'):null;
const hostBinary=hostRuntime&&path.join(hostRuntime,'journal-native-builds',LEGACY_NATIVE_BACKEND.binaryHash);
const hostSource=hostRuntime&&path.join(hostRuntime,'journal-native-sources',LEGACY_NATIVE_BACKEND.engineHash);
const available=!!hostRuntime&&['baal-journal-restore.exe','libunwind.dll'].every(f=>fs.existsSync(path.join(hostBinary,f)))&&journalSourceFiles.every(f=>fs.existsSync(path.join(hostSource,f)));
function fixture(t){
 const temp=fs.mkdtempSync(path.join(os.tmpdir(),'native-collector-rebind-'));
 const priorLocalAppData=process.env.LOCALAPPDATA,priorPaperDir=process.env.SIG_PAPER_DIR;
 process.env.LOCALAPPDATA=temp;delete process.env.SIG_PAPER_DIR;
 t.after(()=>{if(priorLocalAppData===undefined)delete process.env.LOCALAPPDATA;else process.env.LOCALAPPDATA=priorLocalAppData;if(priorPaperDir===undefined)delete process.env.SIG_PAPER_DIR;else process.env.SIG_PAPER_DIR=priorPaperDir;fs.rmSync(temp,{recursive:true,force:true});});
 const control=path.join(temp,'SigTradingCup'),runtimeDir=path.join(control,'paper-runtime');fs.mkdirSync(runtimeDir,{recursive:true});
 const stop=path.join(runtimeDir,'EXECUTION_STOP');fs.writeFileSync(stop,JSON.stringify({kind:'test_execution_stop'}));
 const capture={schemaVersion:2,enabled:true,mode:'market_capture_only',channelMode:'tournament'};
 fs.writeFileSync(path.join(runtimeDir,'realtime-capture.json'),JSON.stringify(capture));
 if(available){
  const binaryDir=path.join(runtimeDir,'journal-native-builds',LEGACY_NATIVE_BACKEND.binaryHash);fs.mkdirSync(binaryDir,{recursive:true});
  fs.copyFileSync(path.join(hostBinary,'baal-journal-restore.exe'),path.join(binaryDir,'baal-journal-restore.exe'));
  fs.copyFileSync(path.join(hostBinary,'libunwind.dll'),path.join(binaryDir,'libunwind.dll'));
  const archivedSource=path.join(runtimeDir,'journal-native-sources',LEGACY_NATIVE_BACKEND.engineHash);
  for(const name of journalSourceFiles){const target=path.join(archivedSource,name);fs.mkdirSync(path.dirname(target),{recursive:true});fs.copyFileSync(path.join(hostSource,name),target);}
 }
 const config={schemaVersion:1,mode:'native_original_journal_collector_v1',nativeBackend:LEGACY_NATIVE_BACKEND,codeIdentity:LEGACY_CODE_IDENTITY,sourceSnapshot:LEGACY_SOURCE_SNAPSHOT,restorePublicTape:true};
 const configFile=path.join(runtimeDir,'collector-native-journal.json');fs.writeFileSync(configFile,JSON.stringify(config)+'\n');
 const journal=path.join(runtimeDir,'original-journal.jsonl'),receipt=path.join(runtimeDir,'receipts','receipt.json');
 fs.mkdirSync(path.dirname(receipt),{recursive:true});fs.writeFileSync(journal,'original journal bytes\n');fs.writeFileSync(receipt,'original receipt bytes\n');
 return {temp,control,runtimeDir,configFile,config,stop,capture,journal,receipt};
}
function rebind(f,apply=false,extra={}){return rebindNativeCollectorJournalV1({runtimeDir:f.runtimeDir,apply,...extra});}
function expectUnchangedFailure(f,reason){
 const before=fs.readFileSync(f.configFile);
 assert.throws(()=>rebind(f,true),e=>e.code===reason);
 assert.deepEqual(fs.readFileSync(f.configFile),before);
}

test('pinned source maps bind the historical commit evidence and exact current graph',()=>{
 assert.equal(payloadHash(LEGACY_CODE_IDENTITY),LEGACY_SOURCE_SNAPSHOT);
 assert.equal(payloadHash(CURRENT_CODE_IDENTITY),CURRENT_SOURCE_SNAPSHOT);
 assert.equal(PREDECESSOR_COMMIT,'97e1426c7da60f153a1494cd38eb21c52eb60880');
 const changed=Object.keys(LEGACY_CODE_IDENTITY).filter(k=>LEGACY_CODE_IDENTITY[k]!==CURRENT_CODE_IDENTITY[k]);
 assert.deepEqual(changed,['live-paper/native-journal-restore.mjs']);
 const historicalBody=fs.readFileSync(path.join(workspace,'live-paper','read-body-failure-v1.mjs'));
 assert.equal(sha(historicalBody),LEGACY_CODE_IDENTITY['live-paper/read-body-failure-v1.mjs']);
 assert.equal(sha(Buffer.concat([historicalBody,Buffer.from('\r\n')])),HISTORIC_READ_BODY_COMMIT_SHA256);
});

test('rebind changes only identity fields under execution stop and preserves journal/receipt bytes',{skip:!available},t=>{
 const f=fixture(t),journalBefore=fs.readFileSync(f.journal),receiptBefore=fs.readFileSync(f.receipt),stopBefore=fs.readFileSync(f.stop),captureBefore=fs.readFileSync(path.join(f.runtimeDir,'realtime-capture.json'));
 const result=rebind(f,true);
 assert.equal(result.status,'rebound');assert.equal(result.executionStopPreserved,true);assert.equal(result.networkRequests,0);assert.equal(result.competitionOrders,0);
 const updated=JSON.parse(fs.readFileSync(f.configFile,'utf8'));
 assert.deepEqual(updated,{...f.config,codeIdentity:CURRENT_CODE_IDENTITY,sourceSnapshot:CURRENT_SOURCE_SNAPSHOT});
 assert.deepEqual(fs.readFileSync(f.journal),journalBefore);assert.deepEqual(fs.readFileSync(f.receipt),receiptBefore);assert.deepEqual(fs.readFileSync(f.stop),stopBefore);assert.deepEqual(fs.readFileSync(path.join(f.runtimeDir,'realtime-capture.json')),captureBefore);
 assert.deepEqual(collectorNativeJournalConfig(f.runtimeDir),updated);
 assert.equal(fs.existsSync(path.join(f.control,'bot.lock')),false);
});

test('dry run validates under the shared lock without changing runtime config',{skip:!available},t=>{
 const f=fixture(t),before=fs.readFileSync(f.configFile);
 const result=rebind(f);
 assert.equal(result.status,'ready');assert.equal(result.applied,false);assert.deepEqual(fs.readFileSync(f.configFile),before);assert.equal(fs.existsSync(path.join(f.control,'bot.lock')),false);
});

test('production entry ignores a caller-supplied workspace STOP path',t=>{
 const f=fixture(t);
 fs.writeFileSync(path.join(f.temp,'caller-workspace-STOP'),'stop');
 if(available){assert.equal(rebind(f,false,{workspaceRoot:f.temp}).status,'ready');}
 else assert.throws(()=>rebind(f,false,{workspaceRoot:f.temp}),e=>e.code==='NATIVE_BINDING_INVALID');
});

test('launcher runtime overrides and truthy non-boolean apply flags fail closed',t=>{
 const f=fixture(t),override=f.runtimeDir;
 process.env.SIG_PAPER_DIR=override;
 assert.throws(()=>rebind(f),e=>e.code==='RUNTIME_OVERRIDE_UNSUPPORTED');
 delete process.env.SIG_PAPER_DIR;
 const before=fs.readFileSync(f.configFile);
 assert.throws(()=>rebind(f,'false'),e=>e.code==='APPLY_FLAG_INVALID');
 assert.deepEqual(fs.readFileSync(f.configFile),before);
});

test('rejects a self-hashed but unrecognized historical identity without mutation',t=>{
 const f=fixture(t),bad={...f.config,codeIdentity:{...f.config.codeIdentity,'live-paper/transport-runtime.mjs':'0'.repeat(64)}};
 bad.sourceSnapshot=payloadHash(bad.codeIdentity);fs.writeFileSync(f.configFile,JSON.stringify(bad));
 expectUnchangedFailure(f,'LEGACY_RUNTIME_IDENTITY_MISMATCH');
});

test('requires execution stop and refuses every competing stop marker',async t=>{
 const cases=[
  ['missing execution stop',f=>fs.unlinkSync(f.stop),'EXECUTION_STOP_REQUIRED'],
  ['runtime stop',f=>fs.writeFileSync(path.join(f.runtimeDir,'STOP'),'stop'),'STOP_MARKER_PRESENT'],
  ['collector stop',f=>fs.writeFileSync(path.join(f.runtimeDir,'COLLECTOR_STOP'),'stop'),'STOP_MARKER_PRESENT'],
  ['realtime stop',f=>fs.writeFileSync(path.join(f.runtimeDir,'REALTIME_STOP'),'stop'),'STOP_MARKER_PRESENT']
 ];
 for(const [name,change,reason] of cases)await t.test(name,t=>{
  const f=fixture(t);
  change(f);expectUnchangedFailure(f,reason);
 });
});

test('existing shared lock and live runner pid block the rebind',t=>{
 const f=fixture(t),lockFile=path.join(f.control,'bot.lock');fs.writeFileSync(lockFile,'other');
 expectUnchangedFailure(f,'COLLECTOR_LOCKED');fs.unlinkSync(lockFile);
 fs.writeFileSync(path.join(f.runtimeDir,'pid.txt'),String(process.pid));
 expectUnchangedFailure(f,'RUNNER_PROCESS_ACTIVE');
});

test('a realtime stop created after preflight blocks the rebind under the shared lock',t=>{
 const f=fixture(t),lockFile=path.join(f.control,'bot.lock'),stopFile=path.join(f.runtimeDir,'REALTIME_STOP'),before=fs.readFileSync(f.configFile),open=fs.openSync;
 t.mock.method(fs,'openSync',function(file,...args){const fd=open.call(fs,file,...args);if(path.resolve(String(file))===path.resolve(lockFile))fs.writeFileSync(stopFile,'stop');return fd;});
 assert.throws(()=>rebind(f,true),e=>e.code==='STOP_MARKER_PRESENT');
 assert.deepEqual(fs.readFileSync(f.configFile),before);
 assert.equal(fs.existsSync(lockFile),false);
});

test('post-write validation failure restores the exact original config bytes',{skip:!available},t=>{
 const f=fixture(t),before=fs.readFileSync(f.configFile),stopFile=path.join(f.runtimeDir,'REALTIME_STOP'),originalRename=fs.renameSync;let injected=false;
 t.mock.method(fs,'renameSync',function(source,target,...args){const result=originalRename.call(fs,source,target,...args);if(!injected&&path.resolve(String(target))===path.resolve(f.configFile)){injected=true;fs.writeFileSync(stopFile,'stop');}return result;});
 assert.throws(()=>rebind(f,true),e=>e.code==='STOP_MARKER_PRESENT');
 assert.equal(injected,true);assert.deepEqual(fs.readFileSync(f.configFile),before);assert.equal(fs.existsSync(path.join(f.control,'bot.lock')),false);
});

test('requires exact capture-only scope and verified native artifacts',t=>{
 const f=fixture(t),captureFile=path.join(f.runtimeDir,'realtime-capture.json');
 fs.writeFileSync(captureFile,JSON.stringify({...f.capture,mode:'live'}));expectUnchangedFailure(f,'CAPTURE_SCOPE_CHANGED');
 fs.writeFileSync(captureFile,JSON.stringify(f.capture));
 if(!available){t.skip('Pinned local native artifacts unavailable; portable capture guard was exercised above');return;}
 const binary=path.join(f.runtimeDir,'journal-native-builds',LEGACY_NATIVE_BACKEND.binaryHash,'baal-journal-restore.exe');
 fs.writeFileSync(binary,Buffer.from('tampered'));expectUnchangedFailure(f,'NATIVE_BINDING_INVALID');
});

test('rejects tampered runtime library and archived Rust source',{skip:!available},async t=>{
 await t.test('runtime library',t=>{const f=fixture(t),file=path.join(f.runtimeDir,'journal-native-builds',LEGACY_NATIVE_BACKEND.binaryHash,'libunwind.dll');fs.writeFileSync(file,Buffer.from('tampered dll'));expectUnchangedFailure(f,'NATIVE_BINDING_INVALID');});
 await t.test('source archive',t=>{const f=fixture(t),file=path.join(f.runtimeDir,'journal-native-sources',LEGACY_NATIVE_BACKEND.engineHash,'src','canonical.rs');fs.writeFileSync(file,'tampered source');expectUnchangedFailure(f,'NATIVE_SOURCE_CHANGED');});
});

