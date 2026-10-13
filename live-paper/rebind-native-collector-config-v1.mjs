import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {replaceFileSync} from '../execution/atomic-rename.mjs';
import {lock} from './transport-runtime.mjs';
import {collectorJournalCodeIdentity,collectorNativeJournalConfig} from './collector-journal-native.mjs';
import {journalEngineIdentity,nativeJournalBinding} from './native-journal-restore.mjs';

const workspace=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
export const PREDECESSOR_COMMIT='97e1426c7da60f153a1494cd38eb21c52eb60880';
export const CURRENT_COMMIT='6da6c194373621129c990797b321bb996d480b88';
export const LEGACY_SOURCE_SNAPSHOT='b9b4d01f237cc999397f0963db2b774a1c19e0c8cf924f4fb634ded235763f4e';
export const CURRENT_SOURCE_SNAPSHOT='459de45dd5d46df01b629a9241e2d513d16e034eeed5d746a3ee3a46dc3a8bae';
export const HISTORIC_READ_BODY_COMMIT_SHA256='c0d9a8a3815e3e10bbfde6ea5cd1740e54556aa106fa54fa66e2596c0fdf40c8';
export const LEGACY_CODE_IDENTITY=Object.freeze({
    "execution/atomic-rename.mjs": "afbe15eadb94f7e5df250d02c9e6cfdabbdc9b8e68c7d42325b91845f0fd0515",
    "execution/ledger.mjs": "dd57fe01ce6a4d7219228c01ebfc091ea006f78ba3c96644099abf5e5ff3c014",
    "feed/contract.mjs": "39b9b30a72093d99697bc03a3b20f88f1d835507219fc46f7a00bd28fe761465",
    "feed/runtime.mjs": "51327d008ad4c9c479b25c14186edeff02f7671bf5efcd367b4b42f06abc884d",
    "live-paper/collector-journal-native.mjs": "08cca3ad26a7fa5cc26de820b93d77894bc9c1ae6ff40852af80d01e77866d6e",
    "live-paper/frozen-paper-graph.mjs": "4d5e607cd11d0f5f60aadf9268e6e80fda757910da032b6eaeabea9b5b1f17bf",
    "live-paper/history.mjs": "d820b9d74752df3c2ecc28df3b1662bbc93080f4605db21a838d8957bf82c890",
    "live-paper/journal-lines.mjs": "f8adff2bddc36b4e3d305dec9395fae9f51dff957e35313994b6147502cc6823",
    "live-paper/latency-clock.mjs": "e0b79a03fbf1df2a3e82594be10e87917bd77f4fc436a0bb5133c9c1032e1984",
    "live-paper/model.mjs": "3cb8bbd7d31b478fe917270393954382373695149310243eec8b78440d207a2e",
    "live-paper/native-journal-restore.mjs": "2ed7ceb1c7f214026785fea382e62a5f81ae56bed552d2ba97bb3f283e0ebfe3",
    "live-paper/read-body-failure-v1.mjs": "688b94b45cbbe71b734d5b83f0bbfd6e8449a4550262331535bea145c6d86ab7",
    "live-paper/transport-network-budget.mjs": "7e85b338fca9e834d6fcac9e650a3eacf46601c8be04bfa1717de63df2bc5d30",
    "live-paper/transport-runtime.mjs": "dcd879eb768067d6f241bef34f9b96c96d5fa70055b1cbabb4a0f95ef9cda894",
    "paper/fixed.mjs": "a0947ef49854d4a3bf4bce88034aaef5d8aabbdc7e29dff3a7b9b4bc01b072f7"
  });
export const CURRENT_CODE_IDENTITY=Object.freeze({
    "execution/atomic-rename.mjs": "afbe15eadb94f7e5df250d02c9e6cfdabbdc9b8e68c7d42325b91845f0fd0515",
    "execution/ledger.mjs": "dd57fe01ce6a4d7219228c01ebfc091ea006f78ba3c96644099abf5e5ff3c014",
    "feed/contract.mjs": "39b9b30a72093d99697bc03a3b20f88f1d835507219fc46f7a00bd28fe761465",
    "feed/runtime.mjs": "51327d008ad4c9c479b25c14186edeff02f7671bf5efcd367b4b42f06abc884d",
    "live-paper/collector-journal-native.mjs": "08cca3ad26a7fa5cc26de820b93d77894bc9c1ae6ff40852af80d01e77866d6e",
    "live-paper/frozen-paper-graph.mjs": "4d5e607cd11d0f5f60aadf9268e6e80fda757910da032b6eaeabea9b5b1f17bf",
    "live-paper/history.mjs": "d820b9d74752df3c2ecc28df3b1662bbc93080f4605db21a838d8957bf82c890",
    "live-paper/journal-lines.mjs": "f8adff2bddc36b4e3d305dec9395fae9f51dff957e35313994b6147502cc6823",
    "live-paper/latency-clock.mjs": "e0b79a03fbf1df2a3e82594be10e87917bd77f4fc436a0bb5133c9c1032e1984",
    "live-paper/model.mjs": "3cb8bbd7d31b478fe917270393954382373695149310243eec8b78440d207a2e",
    "live-paper/native-journal-restore.mjs": "eeafbb63a9be725bd96c794f2d93af7169e662e2762bf22160bbb2b3929a545e",
    "live-paper/read-body-failure-v1.mjs": "688b94b45cbbe71b734d5b83f0bbfd6e8449a4550262331535bea145c6d86ab7",
    "live-paper/transport-network-budget.mjs": "7e85b338fca9e834d6fcac9e650a3eacf46601c8be04bfa1717de63df2bc5d30",
    "live-paper/transport-runtime.mjs": "dcd879eb768067d6f241bef34f9b96c96d5fa70055b1cbabb4a0f95ef9cda894",
    "paper/fixed.mjs": "a0947ef49854d4a3bf4bce88034aaef5d8aabbdc7e29dff3a7b9b4bc01b072f7"
  });
export const LEGACY_NATIVE_BACKEND=Object.freeze({
    "binaryHash": "a571587f0f3cfa075401fa0200d4a81e7d89fe61e75131e8a4d47cc03b5d4879",
    "engineHash": "a59a58c8959e88653b692ecff39277466411aadf293058b262b8c7bf2e3a4d41",
    "mode": "canonical_js_journal_restore_v1",
    "runtimeLibraryHash": "13bf4e99b0193634ebdeb0bbdeff9753b39f1d04799183f664111014aabf4c2c",
    "schemaVersion": 1
  });

const changedPaths=Object.keys(LEGACY_CODE_IDENTITY).filter(k=>LEGACY_CODE_IDENTITY[k]!==CURRENT_CODE_IDENTITY[k]);
const reject=code=>{const e=Error(code);e.code=code;throw e;};
const sha=b=>createHash('sha256').update(b).digest('hex');
function exactObject(value,keys){return value&&typeof value==='object'&&!Array.isArray(value)&&Object.keys(value).sort().join('|')===[...keys].sort().join('|');}
function assertStaticManifest(){
 if(payloadHash(LEGACY_CODE_IDENTITY)!==LEGACY_SOURCE_SNAPSHOT||payloadHash(CURRENT_CODE_IDENTITY)!==CURRENT_SOURCE_SNAPSHOT)reject('REBIND_MANIFEST_INVALID');
 if(changedPaths.length!==1||changedPaths[0]!=='live-paper/native-journal-restore.mjs')reject('REBIND_MANIFEST_INVALID');
 if(LEGACY_CODE_IDENTITY['live-paper/read-body-failure-v1.mjs']!=='688b94b45cbbe71b734d5b83f0bbfd6e8449a4550262331535bea145c6d86ab7')reject('REBIND_MANIFEST_INVALID');
 if(HISTORIC_READ_BODY_COMMIT_SHA256!=='c0d9a8a3815e3e10bbfde6ea5cd1740e54556aa106fa54fa66e2596c0fdf40c8')reject('REBIND_MANIFEST_INVALID');
}
assertStaticManifest();

function readConfig(file){
 let value;try{value=JSON.parse(fs.readFileSync(file,'utf8'));}catch{reject('RUNTIME_CONFIG_INVALID');}
 if(!exactObject(value,['schemaVersion','mode','nativeBackend','codeIdentity','sourceSnapshot','restorePublicTape'])||
    value.schemaVersion!==1||value.mode!=='native_original_journal_collector_v1'||value.restorePublicTape!==true||
    payloadHash(value.codeIdentity)!==LEGACY_SOURCE_SNAPSHOT||value.sourceSnapshot!==LEGACY_SOURCE_SNAPSHOT||
    payloadHash(value.nativeBackend)!==payloadHash(LEGACY_NATIVE_BACKEND))reject('LEGACY_RUNTIME_IDENTITY_MISMATCH');
 return value;
}
function readControls(runtimeDir,workspaceRoot){
 const executionStop=path.join(runtimeDir,'EXECUTION_STOP');
 if(!fs.existsSync(executionStop))reject('EXECUTION_STOP_REQUIRED');
 for(const file of [path.join(workspaceRoot,'STOP'),path.join(runtimeDir,'STOP'),path.join(runtimeDir,'COLLECTOR_STOP'),path.join(runtimeDir,'REALTIME_STOP')])if(fs.existsSync(file))reject('STOP_MARKER_PRESENT');
 const captureFile=path.join(runtimeDir,'realtime-capture.json');
 let capture;try{capture=JSON.parse(fs.readFileSync(captureFile,'utf8'));}catch{reject('CAPTURE_CONFIG_INVALID');}
 if(capture.schemaVersion!==2||capture.enabled!==true||capture.mode!=='market_capture_only'||capture.channelMode!=='tournament')reject('CAPTURE_SCOPE_CHANGED');
 return {executionStop:fs.readFileSync(executionStop),capture:fs.readFileSync(captureFile)};
}
function assertControlsUnchanged(runtimeDir,workspaceRoot,before){
 const after=readControls(runtimeDir,workspaceRoot);
 if(!after.executionStop.equals(before.executionStop)||!after.capture.equals(before.capture))reject('CONTROL_STATE_CHANGED');
}
function assertNoLiveRunner(runtimeDir){
 const pidFile=path.join(runtimeDir,'pid.txt');
 if(!fs.existsSync(pidFile))return;
 const text=fs.readFileSync(pidFile,'utf8').trim();
 if(!/^[1-9][0-9]*$/.test(text)||!Number.isSafeInteger(Number(text)))reject('RUNNER_PID_INVALID');
 try{process.kill(Number(text),0);reject('RUNNER_PROCESS_ACTIVE');}
 catch(error){if(error?.code==='RUNNER_PROCESS_ACTIVE')throw error;if(error?.code==='ESRCH')return;reject('RUNNER_PROCESS_UNVERIFIED');}
}
function assertReviewedGraph(){
 const current=collectorJournalCodeIdentity();
 if(payloadHash(current)!==CURRENT_SOURCE_SNAPSHOT||payloadHash(current)!==payloadHash(CURRENT_CODE_IDENTITY))reject('CURRENT_SOURCE_GRAPH_CHANGED');
}
function assertNativeBinding(runtimeDir,backend){
 try{
  nativeJournalBinding(runtimeDir,backend);
  const sourceRoot=path.join(runtimeDir,'journal-native-sources',backend.engineHash);
  if(journalEngineIdentity(sourceRoot)!==backend.engineHash)reject('NATIVE_SOURCE_CHANGED');
 }catch(error){if(error?.code)throw error;reject('NATIVE_BINDING_INVALID');}
}
function restoreExactBytes(file,bytes){
 const temporary=file+'.'+process.pid+'.'+Date.now()+'.rollback.tmp';let fd;
 try{fd=fs.openSync(temporary,'wx',0o600);fs.writeFileSync(fd,bytes);fs.fsyncSync(fd);fs.closeSync(fd);fd=undefined;replaceFileSync(temporary,file);}
 catch(error){if(fd!==undefined)fs.closeSync(fd);try{fs.unlinkSync(temporary);}catch(cleanup){if(cleanup?.code!=='ENOENT')error.cleanupFailed=true;}throw error;}
}

function rebindCore({runtimeDir,apply=false}={}){
 if(typeof runtimeDir!=='string'||!path.isAbsolute(runtimeDir))reject('RUNTIME_PATH_INVALID');
 runtimeDir=path.resolve(runtimeDir);
 if(path.basename(runtimeDir)!=='paper-runtime'||path.basename(path.dirname(runtimeDir))!=='SigTradingCup')reject('RUNTIME_PATH_INVALID');
 const lockFile=path.join(path.dirname(runtimeDir),'bot.lock'),configFile=path.join(runtimeDir,'collector-native-journal.json');
 const controls=readControls(runtimeDir,workspace);assertNoLiveRunner(runtimeDir);assertReviewedGraph();
 if(fs.existsSync(lockFile))reject('COLLECTOR_LOCKED');
 const beforeBytes=fs.readFileSync(configFile);const beforeHash=sha(beforeBytes);
 let release;try{release=lock(lockFile);}catch(error){if(error?.code==='EEXIST')reject('COLLECTOR_LOCKED');throw error;}
 try{
  assertControlsUnchanged(runtimeDir,workspace,controls);assertNoLiveRunner(runtimeDir);assertReviewedGraph();
  const lockedBytes=fs.readFileSync(configFile);if(sha(lockedBytes)!==beforeHash)reject('RUNTIME_CONFIG_CHANGED');
  const lockedConfig=readConfig(configFile);assertNativeBinding(runtimeDir,lockedConfig.nativeBackend);
  if(sha(fs.readFileSync(configFile))!==beforeHash)reject('RUNTIME_CONFIG_CHANGED');
  const next={...lockedConfig,codeIdentity:{...CURRENT_CODE_IDENTITY},sourceSnapshot:CURRENT_SOURCE_SNAPSHOT};
  if(!apply)return {status:'ready',applied:false,sourceSnapshot:CURRENT_SOURCE_SNAPSHOT,executionStopPreserved:true,networkRequests:0,competitionOrders:0};
  if(sha(fs.readFileSync(configFile))!==beforeHash)reject('RUNTIME_CONFIG_CHANGED');
  atomicJson(configFile,next);
  try{
   assertControlsUnchanged(runtimeDir,workspace,controls);
   const verified=collectorNativeJournalConfig(runtimeDir);
   if(payloadHash(verified)!==payloadHash(next))reject('POST_REBIND_VALIDATION_FAILED');
  }catch(error){
   try{restoreExactBytes(configFile,beforeBytes);}catch{reject('REBIND_ROLLBACK_FAILED');}
   throw error;
  }
  return {status:'rebound',applied:true,predecessorCommit:PREDECESSOR_COMMIT,currentCommit:CURRENT_COMMIT,sourceSnapshot:CURRENT_SOURCE_SNAPSHOT,executionStopPreserved:true,networkRequests:0,competitionOrders:0};
 }finally{release();}
}

function configuredRuntimeDir(){
 const localAppData=process.env.LOCALAPPDATA;
 if(typeof localAppData!=='string'||!path.isAbsolute(localAppData))reject('RUNTIME_PATH_INVALID');
 if(process.env.SIG_PAPER_DIR)reject('RUNTIME_OVERRIDE_UNSUPPORTED');
 return path.resolve(localAppData,'SigTradingCup','paper-runtime');
}
function samePath(a,b){return path.resolve(a).toLowerCase()===path.resolve(b).toLowerCase();}

// The collector launcher allows explicit runtime overrides, but this one-shot migration does not.
export function rebindNativeCollectorJournalV1({runtimeDir,apply=false}={}){
 if(typeof apply!=='boolean')reject('APPLY_FLAG_INVALID');
 const expectedRuntime=configuredRuntimeDir();
 if(typeof runtimeDir!=='string'||!path.isAbsolute(runtimeDir)||!samePath(runtimeDir,expectedRuntime))reject('RUNTIME_PATH_INVALID');
 return rebindCore({runtimeDir:expectedRuntime,apply});
}

if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
 const action=process.argv[2]||'--check';
 if(!['--check','--apply'].includes(action)){process.stderr.write('Use --check or --apply\n');process.exitCode=2;}
 else{
  const runtimeDir=configuredRuntimeDir();
  try{process.stdout.write(JSON.stringify(rebindNativeCollectorJournalV1({runtimeDir,apply:action==='--apply'}))+'\n');}
  catch(error){process.stdout.write(JSON.stringify({status:'blocked',reason:error?.code||'REBIND_FAILED'})+'\n');process.exitCode=1;}
 }
}


