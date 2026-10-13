import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {replaceFileSync} from '../execution/atomic-rename.mjs';
import {lock} from './transport-runtime.mjs';
import {collectorJournalCodeIdentity,collectorNativeJournalConfig} from './collector-journal-native.mjs';
import {journalEngineIdentity,nativeJournalBinding} from './native-journal-restore.mjs';
import {
  CURRENT_CODE_IDENTITY as PREDECESSOR_CODE_IDENTITY,
  CURRENT_SOURCE_SNAPSHOT as V1_TARGET_SOURCE_SNAPSHOT,
  CURRENT_COMMIT as PREDECESSOR_SOURCE_COMMIT,
  LEGACY_NATIVE_BACKEND as NATIVE_BACKEND
} from './rebind-native-collector-config-v1.mjs';
export {PREDECESSOR_CODE_IDENTITY};

const workspace=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
export const PREDECESSOR_SOURCE_SNAPSHOT='459de45dd5d46df01b629a9241e2d513d16e034eeed5d746a3ee3a46dc3a8bae';
export const NATIVE_RESTORE_CHANGE_COMMIT='ba63c238c33c94c00b9ab3d4665a44dec235ad02';
export const CURRENT_SOURCE_SNAPSHOT='bb67d7024b6f6bf5c3e3a68247872a0555da625de1f539c20ef4921f255d0d79';
export const NATIVE_RESTORE_SHA256='15d10bfc8c19fdfbbb88f751bded3dc2a5c83178117c83e8cb8c8e108138668c';
export const CURRENT_CODE_IDENTITY=Object.freeze({
  ...PREDECESSOR_CODE_IDENTITY,
  'live-paper/native-journal-restore.mjs':NATIVE_RESTORE_SHA256
});

const changedPaths=Object.keys(PREDECESSOR_CODE_IDENTITY).filter(k=>PREDECESSOR_CODE_IDENTITY[k]!==CURRENT_CODE_IDENTITY[k]);
const reject=code=>{const e=Error(code);e.code=code;throw e;};
const sha=b=>createHash('sha256').update(b).digest('hex');
function exactObject(value,keys){return value&&typeof value==='object'&&!Array.isArray(value)&&Object.keys(value).sort().join('|')===[...keys].sort().join('|');}
function assertStaticManifest(){
 if(V1_TARGET_SOURCE_SNAPSHOT!==PREDECESSOR_SOURCE_SNAPSHOT||payloadHash(PREDECESSOR_CODE_IDENTITY)!==PREDECESSOR_SOURCE_SNAPSHOT)reject('REBIND_MANIFEST_INVALID');
 if(payloadHash(CURRENT_CODE_IDENTITY)!==CURRENT_SOURCE_SNAPSHOT||changedPaths.length!==1||changedPaths[0]!=='live-paper/native-journal-restore.mjs')reject('REBIND_MANIFEST_INVALID');
 if(PREDECESSOR_SOURCE_COMMIT!=='6da6c194373621129c990797b321bb996d480b88'||NATIVE_RESTORE_CHANGE_COMMIT!=='ba63c238c33c94c00b9ab3d4665a44dec235ad02')reject('REBIND_MANIFEST_INVALID');
 if(PREDECESSOR_CODE_IDENTITY['live-paper/native-journal-restore.mjs']!=='eeafbb63a9be725bd96c794f2d93af7169e662e2762bf22160bbb2b3929a545e'||NATIVE_RESTORE_SHA256!=='15d10bfc8c19fdfbbb88f751bded3dc2a5c83178117c83e8cb8c8e108138668c')reject('REBIND_MANIFEST_INVALID');
}
assertStaticManifest();

function readConfig(file){
 let value;try{value=JSON.parse(fs.readFileSync(file,'utf8'));}catch{reject('RUNTIME_CONFIG_INVALID');}
 if(!exactObject(value,['schemaVersion','mode','nativeBackend','codeIdentity','sourceSnapshot','restorePublicTape'])||
    value.schemaVersion!==1||value.mode!=='native_original_journal_collector_v1'||value.restorePublicTape!==true||
    payloadHash(value.codeIdentity)!==PREDECESSOR_SOURCE_SNAPSHOT||value.sourceSnapshot!==PREDECESSOR_SOURCE_SNAPSHOT||
    payloadHash(value.nativeBackend)!==payloadHash(NATIVE_BACKEND))reject('RUNTIME_PREDECESSOR_IDENTITY_MISMATCH');
 return value;
}
function readControls(runtimeDir){
 const executionStop=path.join(runtimeDir,'EXECUTION_STOP');
 if(!fs.existsSync(executionStop))reject('EXECUTION_STOP_REQUIRED');
 for(const file of [path.join(workspace,'STOP'),path.join(runtimeDir,'STOP'),path.join(runtimeDir,'COLLECTOR_STOP'),path.join(runtimeDir,'REALTIME_STOP')])if(fs.existsSync(file))reject('STOP_MARKER_PRESENT');
 const captureFile=path.join(runtimeDir,'realtime-capture.json');
 let capture;try{capture=JSON.parse(fs.readFileSync(captureFile,'utf8'));}catch{reject('CAPTURE_CONFIG_INVALID');}
 if(capture.schemaVersion!==2||capture.enabled!==true||capture.mode!=='market_capture_only'||capture.channelMode!=='tournament')reject('CAPTURE_SCOPE_CHANGED');
 return {executionStop:fs.readFileSync(executionStop),capture:fs.readFileSync(captureFile)};
}
function assertControlsUnchanged(runtimeDir,before){
 const after=readControls(runtimeDir);
 if(!after.executionStop.equals(before.executionStop)||!after.capture.equals(before.capture))reject('CONTROL_STATE_CHANGED');
}
function assertNoLiveRunner(runtimeDir){
 const pidFile=path.join(runtimeDir,'pid.txt');if(!fs.existsSync(pidFile))reject('RUNNER_PID_MISSING');
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
 const controls=readControls(runtimeDir);assertNoLiveRunner(runtimeDir);assertReviewedGraph();
 if(fs.existsSync(lockFile))reject('COLLECTOR_LOCKED');
 const beforeBytes=fs.readFileSync(configFile),beforeHash=sha(beforeBytes);
 let release;try{release=lock(lockFile);}catch(error){if(error?.code==='EEXIST')reject('COLLECTOR_LOCKED');throw error;}
 try{
  assertControlsUnchanged(runtimeDir,controls);assertNoLiveRunner(runtimeDir);assertReviewedGraph();
  const lockedBytes=fs.readFileSync(configFile);if(sha(lockedBytes)!==beforeHash)reject('RUNTIME_CONFIG_CHANGED');
  const lockedConfig=readConfig(configFile);assertNativeBinding(runtimeDir,lockedConfig.nativeBackend);
  if(sha(fs.readFileSync(configFile))!==beforeHash)reject('RUNTIME_CONFIG_CHANGED');
  const next={...lockedConfig,codeIdentity:{...CURRENT_CODE_IDENTITY},sourceSnapshot:CURRENT_SOURCE_SNAPSHOT};
  if(!apply)return {status:'ready',applied:false,predecessorSnapshot:PREDECESSOR_SOURCE_SNAPSHOT,sourceSnapshot:CURRENT_SOURCE_SNAPSHOT,executionStopPreserved:true,networkRequests:0,competitionOrders:0};
  if(sha(fs.readFileSync(configFile))!==beforeHash)reject('RUNTIME_CONFIG_CHANGED');
  atomicJson(configFile,next);
  try{
   assertControlsUnchanged(runtimeDir,controls);
   const verified=collectorNativeJournalConfig(runtimeDir);
   if(payloadHash(verified)!==payloadHash(next))reject('POST_REBIND_VALIDATION_FAILED');
  }catch(error){
   try{restoreExactBytes(configFile,beforeBytes);}catch{reject('REBIND_ROLLBACK_FAILED');}
   throw error;
  }
  return {status:'rebound',applied:true,predecessorCommit:PREDECESSOR_SOURCE_COMMIT,nativeRestoreChangeCommit:NATIVE_RESTORE_CHANGE_COMMIT,predecessorSnapshot:PREDECESSOR_SOURCE_SNAPSHOT,sourceSnapshot:CURRENT_SOURCE_SNAPSHOT,executionStopPreserved:true,networkRequests:0,competitionOrders:0};
 }finally{release();}
}
function configuredRuntimeDir(){
 const localAppData=process.env.LOCALAPPDATA;
 if(typeof localAppData!=='string'||!path.isAbsolute(localAppData))reject('RUNTIME_PATH_INVALID');
 if(process.env.SIG_PAPER_DIR)reject('RUNTIME_OVERRIDE_UNSUPPORTED');
 return path.resolve(localAppData,'SigTradingCup','paper-runtime');
}
function samePath(a,b){return path.resolve(a).toLowerCase()===path.resolve(b).toLowerCase();}

// The one-shot migration is pinned to the configured runtime and cannot target a caller-selected tree.
export function rebindNativeCollectorJournalV2({runtimeDir,apply=false}={}){
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
  try{process.stdout.write(JSON.stringify(rebindNativeCollectorJournalV2({runtimeDir,apply:action==='--apply'}))+'\n');}
  catch(error){process.stdout.write(JSON.stringify({status:'blocked',reason:error?.code||'REBIND_FAILED'})+'\n');process.exitCode=1;}
 }
}

