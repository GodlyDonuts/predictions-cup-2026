import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {Journal} from './transport-runtime.mjs';import {openNativeJournal,readNativeJournal,journalEngineIdentity,journalSourceFiles,nativeJournalHashMode,nativeJournalRuntimeHashMode} from './native-journal-restore.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),sourceRoot=path.resolve('backend-rust/journal-restore'),binary=path.join(sourceRoot,'target/release/baal-journal-restore.exe'),available=fs.existsSync(binary),opts={skip:!available};
function fixture(t){const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'journal-native-'));t.after(()=>{assert.equal(path.dirname(path.resolve(runtimeDir)),path.resolve(os.tmpdir()));fs.rmSync(runtimeDir,{recursive:true,force:true});});
 const binding={schemaVersion:1,mode:'canonical_js_journal_restore_v1',binaryHash:sha(fs.readFileSync(binary)),engineHash:journalEngineIdentity(sourceRoot),runtimeLibraryHash:sha(fs.readFileSync(path.join(sourceRoot,'target/release/libunwind.dll')))},dir=path.join(runtimeDir,'journal-native-builds',binding.binaryHash);fs.mkdirSync(dir,{recursive:true});for(const f of ['baal-journal-restore.exe','libunwind.dll'])fs.copyFileSync(path.join(sourceRoot,'target/release',f),path.join(dir,f));
 const snapshot=path.join(runtimeDir,'journal-native-sources',binding.engineHash);for(const name of journalSourceFiles){const target=path.join(snapshot,name);fs.mkdirSync(path.dirname(target),{recursive:true});fs.copyFileSync(path.join(sourceRoot,name),target);}
 const file=path.join(runtimeDir,'source.jsonl');fs.writeFileSync(file,'');const input={runtimeDir,binding,file,receiptDir:path.join(runtimeDir,'receipts')};return{...input,input,restore:extra=>openNativeJournal({...input,...extra}),read:extra=>readNativeJournal({...input,endByte:fs.statSync(file).size,expectedSha:sha(fs.readFileSync(file)),...extra})};
}
test('full canonical numeric/string/key corpus restores exact legacy state and hash',opts,async t=>{
 const f=fixture(t),j=new Journal(f.file),values=[0,-0,Number.MIN_VALUE,Number.MAX_VALUE,1e-7,1e-6,1e20,1e21,.1+.2,Math.PI,NaN,Infinity,Number.MAX_SAFE_INTEGER,-Number.MAX_SAFE_INTEGER,Number.MAX_SAFE_INTEGER+1,1000000000000000100];
 let x=1234567;for(let n=0;n<2000;n++){x=(x*1664525+1013904223)>>>0;values.push((x-2147483648)*10**((n%80)-40));}
 const state={values,strings:['é💥','\ud800','\udfff','\ud800\ud800','\u0000\b\f\n\r\t','\u2028\u2029','"\\/'],keys:{2:'second',0:'first',4294967295:'ordinary','01':'alsoordinary'}};
 j.commit(state,{type:'fixture'});j.commit({...state,last:true,largeObject:Object.fromEntries(Array.from({length:40},(_,i)=>['field'+i,i]))},{type:'fixture2'});const restored=await f.restore(),legacy=new Journal(f.file);assert.deepEqual(restored.last,legacy.last);assert.equal(restored.previous,legacy.previous);assert.equal(restored.sequence,legacy.sequence);assert.ok(restored instanceof Journal);assert.equal(restored.nativeRestore.engineHash,f.binding.engineHash);assert.equal(restored.nativeRestore.sourceHash,sha(fs.readFileSync(f.file)));assert.equal(restored.nativeRestore.hashMode,'computed');assert.equal(restored.nativeRestore.prefixHashMs,0);
});
test('computed-hash mode returns the verified source digest without a caller pre-hash',opts,async t=>{
 const f=fixture(t),j=new Journal(f.file);j.commit({cash:'99.985000',basis:'0.000000',fills:2},{type:'paper'});const result=await readNativeJournal({runtimeDir:f.runtimeDir,binding:f.binding,file:f.file,endByte:fs.statSync(f.file).size,receiptDir:f.receiptDir});assert.equal(result.sourceHash,sha(fs.readFileSync(f.file)));assert.equal(result.sourceBytes,fs.statSync(f.file).size);assert.equal(result.records,1);assert.equal(result.hashMode,'computed');assert.equal(result.prefixHashMs,0);
});
test('archived sources alone cannot grant computed mode to a different pinned binary or library',opts,t=>{
 const f=fixture(t);
 assert.equal(nativeJournalRuntimeHashMode(f.runtimeDir,f.binding),'computed');
 assert.equal(nativeJournalRuntimeHashMode(f.runtimeDir,{...f.binding,binaryHash:'a'.repeat(64)}),'explicit');
 assert.equal(nativeJournalRuntimeHashMode(f.runtimeDir,{...f.binding,runtimeLibraryHash:'b'.repeat(64)}),'explicit');
});
test('matching archived legacy source still selects explicit hash mode',opts,t=>{
 const f=fixture(t),old=path.join(f.runtimeDir,'legacy-engine-source'),archive=path.join(f.runtimeDir,'journal-native-sources');fs.cpSync(sourceRoot,old,{recursive:true});fs.appendFileSync(path.join(old,'src/main.rs'),'\n// legacy restore CLI\n');
 const engineHash=journalEngineIdentity(old),snapshot=path.join(archive,engineHash);for(const name of journalSourceFiles){const target=path.join(snapshot,name);fs.mkdirSync(path.dirname(target),{recursive:true});fs.copyFileSync(path.join(old,name),target);}
 const legacyBinding={...f.binding,engineHash};assert.equal(nativeJournalHashMode(legacyBinding,snapshot),'computed');assert.equal(nativeJournalRuntimeHashMode(f.runtimeDir,legacyBinding),'explicit');
});
test('runtime snapshot selects computed mode even when checkout differs; invalid snapshots use explicit fallback',opts,async t=>{
 const f=fixture(t),snapshot=path.join(f.runtimeDir,'journal-native-sources',f.binding.engineHash),checkout=path.join(f.runtimeDir,'different-checkout');
 assert.equal(nativeJournalRuntimeHashMode(f.runtimeDir,f.binding),'computed');assert.equal(nativeJournalHashMode(f.binding,sourceRoot),'computed');
 for(const name of journalSourceFiles){const target=path.join(checkout,name);fs.mkdirSync(path.dirname(target),{recursive:true});fs.copyFileSync(path.join(snapshot,name),target);}
 fs.appendFileSync(path.join(checkout,'src/main.rs'),'// checkout differs\n');assert.equal(nativeJournalHashMode(f.binding,checkout),'explicit');assert.equal(nativeJournalRuntimeHashMode(f.runtimeDir,f.binding),'computed');
 assert.equal(nativeJournalRuntimeHashMode(path.join(f.runtimeDir,'missing-runtime'),f.binding),'explicit');assert.equal(nativeJournalRuntimeHashMode(f.runtimeDir,{...f.binding,engineHash:'a'.repeat(64)}),'explicit');
 fs.appendFileSync(path.join(snapshot,'src/main.rs'),'// damaged snapshot\n');assert.equal(nativeJournalRuntimeHashMode(f.runtimeDir,f.binding),'explicit');
 const j=new Journal(f.file);j.commit({cash:'99.985000',fills:2},{type:'fallback'});
 let result=await readNativeJournal({runtimeDir:f.runtimeDir,binding:f.binding,file:f.file,endByte:fs.statSync(f.file).size,receiptDir:f.receiptDir});assert.equal(result.hashMode,'legacy_explicit');assert.equal(result.sourceHash,sha(fs.readFileSync(f.file)));
 fs.rmSync(snapshot,{recursive:true,force:true});result=await readNativeJournal({runtimeDir:f.runtimeDir,binding:f.binding,file:f.file,endByte:fs.statSync(f.file).size,receiptDir:f.receiptDir});assert.equal(result.hashMode,'legacy_explicit');assert.equal(result.sourceHash,sha(fs.readFileSync(f.file)));
});
test('legacy append interface emits identical next bytes after native restoration',opts,async t=>{
 const f=fixture(t),j=new Journal(f.file);j.commit({cash:'99.985000',basis:'0.000000',fills:2},{type:'paper'});const copy=path.join(f.runtimeDir,'copy');fs.copyFileSync(f.file,copy);const legacy=new Journal(copy),native=await f.restore(),now=Date.now;
 Date.now=()=>1234567890000;const NativeDate=globalThis.Date;globalThis.Date=class extends NativeDate{constructor(...args){super(...(args.length?args:[1234567890000]));}};
 try{for(const h of [legacy,native])h.commit({cash:'99.985000',fills:2},{type:'same_append'});}finally{globalThis.Date=NativeDate;Date.now=now;}
 assert.deepEqual(fs.readFileSync(copy),fs.readFileSync(f.file));assert.deepEqual(new Journal(f.file).last,native.last);
});
test('noncanonical nested values cannot validate using a raw-only forged hash',opts,async t=>{
 const f=fixture(t);for(const state of ['{"x":1.0}','{"x":-0}','{"x":1e-07}','{"x":1e400}','{"x":1,"x":2}','{"2":2,"0":0}','{"a":1,"0":0}','{"x":"\\u0061"}','{"x":"\\/"}','{"x":"\\ud83d\\udca5"}']){
  const body='{"sequence":1,"previous":"","at":"2026-10-04T00:00:00.000Z","event":{"type":"fixture"},"state":'+state+'}',raw=body.slice(0,-1)+',"hash":"'+sha(body)+'"}\n';fs.writeFileSync(f.file,raw);assert.throws(()=>new Journal(f.file),/integrity/);await assert.rejects(f.read(),/JOURNAL_CANONICAL_REQUIRED/);assert.equal(fs.readFileSync(f.file,'utf8'),raw);
 }
});
test('partial/hash/sequence corruption and explicit prefix mismatches reject without mutation',opts,async t=>{
 const f=fixture(t),j=new Journal(f.file);j.commit({value:1},{type:'fixture'});const bytes=fs.readFileSync(f.file);
 fs.writeFileSync(f.file,bytes.subarray(0,bytes.length-1));await assert.rejects(f.read(),/PARTIAL/);fs.writeFileSync(f.file,bytes);await assert.rejects(f.read({expectedSha:'a'.repeat(64)}),/PREFIX_MISMATCH/);
 fs.writeFileSync(f.file,bytes.toString().replace('"value":1','"value":2'));await assert.rejects(f.read(),/HASH_INVALID/);fs.writeFileSync(f.file,bytes.toString().replace('"sequence":1','"sequence":2'));await assert.rejects(f.read(),/CHAIN_INVALID/);
});
test('empty and blank-line prefixes preserve the existing Journal bootstrap semantics',opts,async t=>{
 const f=fixture(t);let h=await f.restore();assert.equal(h.sequence,0);assert.equal(h.last,null);fs.writeFileSync(f.file,'\n');h=await f.restore();assert.equal(h.sequence,0);assert.equal(h.previous,'');
 h.commit({cash:'100.000000'},{type:'first'});fs.appendFileSync(f.file,'\n\n');assert.deepEqual((await f.restore()).last,new Journal(f.file).last);
});
test('changed native library and stopped observer never supply an alternate checkpoint',opts,async t=>{
 const f=fixture(t),j=new Journal(f.file);j.commit({cash:'100.000000'},{type:'fixture'});const saved=fs.readFileSync(f.file);await assert.rejects(f.restore({canOperate:()=>false}),/STOPPED/);
 const controller=new AbortController();controller.abort();await assert.rejects(f.restore({signal:controller.signal}),/STOPPED/);assert.deepEqual(fs.readFileSync(f.file),saved);
 const lib=path.join(f.runtimeDir,'journal-native-builds',f.binding.binaryHash,'libunwind.dll');fs.appendFileSync(lib,'changed');await assert.rejects(f.read(),/BINARY_CHANGED/);assert.deepEqual(fs.readFileSync(f.file),saved);
});
test('immutable native output cannot be overwritten and same-size source replacement rejects restoration',opts,async t=>{
 const f=fixture(t),j=new Journal(f.file);j.commit({cash:'100.000000'},{type:'fixture'});const result=await f.read(),out=result.receiptFile.replace(/\.receipt\.json$/,''),savedOutput=fs.readFileSync(out),savedSource=fs.readFileSync(f.file);
 const exe=path.join(f.runtimeDir,'journal-native-builds',f.binding.binaryHash,'baal-journal-restore.exe'),repeat=spawnSync(exe,['--journal',f.file,'--end-byte',String(savedSource.length),'--expected-sha',sha(savedSource),'--out',out],{windowsHide:true,encoding:'utf8'});
 assert.equal(repeat.status,1);assert.match(repeat.stderr,/JOURNAL_OUTPUT_EXISTS/);assert.deepEqual(fs.readFileSync(out),savedOutput);
 let calls=0;await assert.rejects(f.restore({canOperate(){calls++;if(calls===2)fs.writeFileSync(f.file,savedSource.toString().replace('100.000000','101.000000'));return true;}}),/STOP_OR_SOURCE_CHANGED/);assert.equal(fs.readFileSync(f.file).length,savedSource.length);
});
test('stop arriving while the native restorer runs leaves the source unchanged',opts,async t=>{
 const f=fixture(t),j=new Journal(f.file);j.commit({payload:'x'.repeat(2000000)},{type:'fixture'});const saved=fs.readFileSync(f.file),controller=new AbortController(),opening=f.restore({signal:controller.signal});setTimeout(()=>controller.abort(),5);await assert.rejects(opening,/STOPPED/);assert.deepEqual(fs.readFileSync(f.file),saved);
});
test('direct native reads reject a concurrent append to their verified prefix',opts,async t=>{
 const f=fixture(t),j=new Journal(f.file);j.commit({payload:'x'.repeat(8000000)},{type:'fixture'});const initial=fs.statSync(f.file).size,opening=f.read({expectedSha:undefined});setTimeout(()=>fs.appendFileSync(f.file,'\n'),0);await assert.rejects(opening,/STOP_OR_SOURCE_CHANGED/);assert.equal(fs.statSync(f.file).size,initial+1);
});
