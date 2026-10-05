import fs from 'node:fs';
import path from 'node:path';
import {performance} from 'node:perf_hooks';
import {createHash} from 'node:crypto';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {sourcePrefixHash,readNativeJournal,journalEngineIdentity} from '../live-paper/native-journal-restore.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex');
const runtimeDir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),active=JSON.parse(fs.readFileSync(path.join(runtimeDir,'coupled-bulk-active.json'))),file=path.join(runtimeDir,'coupled-bulk-cohorts',active.cohort,'journal.jsonl'),endByte=fs.statSync(file).size;
const sourceRoot=path.resolve('backend-rust/journal-restore'),build=path.join(sourceRoot,'target/release'),binding={schemaVersion:1,mode:'canonical_js_journal_restore_v1',binaryHash:sha(fs.readFileSync(path.join(build,'baal-journal-restore.exe'))),engineHash:journalEngineIdentity(sourceRoot),runtimeLibraryHash:sha(fs.readFileSync(path.join(build,'libunwind.dll')))};
const dir=path.join(runtimeDir,'journal-native-builds',binding.binaryHash);fs.mkdirSync(dir,{recursive:true,mode:0o700});for(const name of ['baal-journal-restore.exe','libunwind.dll']){const target=path.join(dir,name);if(!fs.existsSync(target))fs.copyFileSync(path.join(build,name),target,fs.constants.COPYFILE_EXCL);}
const expectedSha=await sourcePrefixHash(file,endByte),start=performance.now();let sequence=0,previous='',last=null;const prefix=createHash('sha256');
for(const line of journalLines(file,{endByte})){prefix.update(line+'\n');if(!line)continue;const row=JSON.parse(line),{hash,...body}=row;if(body.sequence!==sequence+1||body.previous!==previous||sha(JSON.stringify(body))!==hash)throw Error('LEGACY_CHAIN');sequence++;previous=hash;last=body;}
const jsMs=performance.now()-start;if(prefix.digest('hex')!==expectedSha)throw Error('LEGACY_PREFIX');
const nativeStart=performance.now(),native=await readNativeJournal({runtimeDir,binding,file,endByte,expectedSha,receiptDir:path.join(runtimeDir,'rust-coupled-benchmark-20261005')});const rustMs=performance.now()-nativeStart;
if(native.records!==sequence||native.tailHash!==previous||payloadHash(native.lastBody)!==payloadHash(last)||await sourcePrefixHash(file,endByte)!==expectedSha)throw Error('PARITY');
const evidence={schemaVersion:1,observedAt:new Date().toISOString(),mode:'actual_original_coupled_journal_same_work_v1',cohort:active.cohort,bankHash:active.bankHash,sourceBytes:endByte,sourceHash:expectedSha,records:sequence,tailHash:previous,lastBodyHash:payloadHash(last),jsMs,rustMs,rustInternalVerifyMs:native.verifyMs,speedup:jsMs/rustMs,binding,sourceParity:true,walletParity:true,readOnly:true,networkRequests:0,competitionOrders:0,profitabilityEstablished:false};fs.writeFileSync('data/coupled-rust-benchmark-20261005.json',JSON.stringify(evidence,null,2)+'\n');console.log(JSON.stringify(evidence));
