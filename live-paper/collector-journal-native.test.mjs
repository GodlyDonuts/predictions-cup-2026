import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {createHash} from 'node:crypto';
import {collectorNativeJournalConfig,collectorJournalCodeIdentity} from './collector-journal-native.mjs';
import {journalSourceFiles,journalEngineIdentity} from './native-journal-restore.mjs';import {payloadHash} from '../execution/ledger.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),sourceRoot=path.resolve('backend-rust/journal-restore'),available=fs.existsSync(path.join(sourceRoot,'target/release/baal-journal-restore.exe'));
test('explicit collector journal binding pins native engine, compiled sources, and JS append graph',{skip:!available},t=>{
 const r=fs.mkdtempSync(path.join(os.tmpdir(),'collector-journal-native-'));t.after(()=>{assert.equal(path.dirname(path.resolve(r)),path.resolve(os.tmpdir()));fs.rmSync(r,{recursive:true,force:true});});
 assert.equal(collectorNativeJournalConfig(r),null);
 const nativeBackend={schemaVersion:1,mode:'canonical_js_journal_restore_v1',binaryHash:sha(fs.readFileSync(path.join(sourceRoot,'target/release/baal-journal-restore.exe'))),runtimeLibraryHash:sha(fs.readFileSync(path.join(sourceRoot,'target/release/libunwind.dll'))),engineHash:journalEngineIdentity(sourceRoot)},codeIdentity=collectorJournalCodeIdentity();
 const root=path.join(r,'journal-native-builds',nativeBackend.binaryHash),sources=path.join(r,'journal-native-sources',nativeBackend.engineHash);fs.mkdirSync(root,{recursive:true});
 for(const f of ['baal-journal-restore.exe','libunwind.dll'])fs.copyFileSync(path.join(sourceRoot,'target/release',f),path.join(root,f));
 for(const f of journalSourceFiles){const target=path.join(sources,f);fs.mkdirSync(path.dirname(target),{recursive:true});fs.copyFileSync(path.join(sourceRoot,f),target);}
 const c={schemaVersion:1,mode:'native_original_journal_collector_v1',nativeBackend,codeIdentity,sourceSnapshot:payloadHash(codeIdentity)},file=path.join(r,'collector-native-journal.json');fs.writeFileSync(file,JSON.stringify(c));assert.deepEqual(collectorNativeJournalConfig(r),c);
 fs.writeFileSync(file,JSON.stringify({...c,codeIdentity:{...codeIdentity,'live-paper/transport-runtime.mjs':'0'.repeat(64)}}));assert.throws(()=>collectorNativeJournalConfig(r),/BINDING_CHANGED/);
 fs.writeFileSync(file,JSON.stringify(c));fs.appendFileSync(path.join(sources,'src/main.rs'),'\n// replacement');assert.throws(()=>collectorNativeJournalConfig(r),/SOURCE_CHANGED/);
});
