import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {collectorJournalCodeIdentity,collectorNativeJournalConfig} from '../live-paper/collector-journal-native.mjs';
import {collectorPublicationCodeIdentity,collectorNativePublicationConfig} from '../live-paper/collector-publication-native.mjs';
import {nativeJournalBinding,journalEngineIdentity} from '../live-paper/native-journal-restore.mjs';
import {nativePublicationBinding} from '../live-paper/native-publication.mjs';
import {frozenPaperModule} from '../live-paper/frozen-paper-runtime.mjs';
import {digest} from '../live-paper/model.mjs';
const workspace=process.cwd(),runtimeDir=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),sha=b=>createHash('sha256').update(b).digest('hex');
const marker=JSON.parse(fs.readFileSync(path.join(runtimeDir,'COLLECTOR_STOP')));if(marker.source!=='transport_binding_repair_20261005')throw Error('Transport repair provenance missing');
const originalLedger=fs.readFileSync('data/frozen-ledger-before-rename-20261005.mjs');if(sha(originalLedger)!=='523ee3fd04f995983ee3ffc03b5ee8dbdf629a718ba91c17d08026fe2411b5c4')throw Error('Original immutable ledger bytes differ');
function graph(entry){const files={},bytes={},pending=[entry];while(pending.length){const key=path.posix.normalize(pending.pop()),file=path.resolve(workspace,key);if(!file.startsWith(path.resolve(workspace)+path.sep)||!file.endsWith('.mjs'))throw Error('Frozen graph scope');if(files[key])continue;
 const b=key==='execution/ledger.mjs'?originalLedger:fs.readFileSync(file),source=b.toString();files[key]=sha(b);bytes[key]=b;
 for(const pattern of [/(?:\bfrom\s*|\bimport\s*(?:\(\s*)?)['"](\.[^'"]+\.mjs)['"]/g,/\bnew\s+URL\(\s*['"](\.[^'"]+\.mjs)['"]\s*,\s*import\.meta\.url/g])for(const match of source.matchAll(pattern))pending.push(path.posix.join(path.posix.dirname(key),match[1]));
 }return{files:Object.fromEntries(Object.entries(files).sort(([a],[b])=>a.localeCompare(b))),bytes};}
const indexFile=path.join(runtimeDir,'frozen-code/index.json'),index=JSON.parse(fs.readFileSync(indexFile)),archives=[];
for(const [pointer,entry]of [['probability-receipt-active.json','live-paper/probability-receipt-host-v3.mjs'],['calibrated-poll-paper-active.json','live-paper/probability-receipt-host.mjs']]){
 const active=JSON.parse(fs.readFileSync(path.join(runtimeDir,pointer))),dir=path.join(runtimeDir,'probability-receipt-cohorts',active.cohort),bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json'))),bankHash=payloadHash(bank),g=graph(entry);
 if(digest(bank)!==active.bankHash)throw Error('Active model identity differs');
 if(index.banks[bankHash]){await frozenPaperModule({runtimeDir,bank,modulePath:entry,fallback:{}});archives.push({cohort:active.cohort,bankHash:active.bankHash,canonicalIndexKey:bankHash,snapshot:index.banks[bankHash].snapshot,existingArchiveVerified:true,originalCodePreserved:true,bankChanged:false,accountingReset:false});continue;}
 for(const [relative,expected]of Object.entries(bank.codeIdentity))if(g.files[path.posix.normalize(path.posix.join('live-paper',relative))]!==expected)throw Error('Original model identity not preserved');
 const snapshot=payloadHash(g.files),root=path.join(runtimeDir,'frozen-code',snapshot);if(fs.existsSync(root))throw Error('Unexpected archive exists');fs.mkdirSync(root,{recursive:true,mode:0o700});
 for(const [name,b]of Object.entries(g.bytes)){const target=path.join(root,name);fs.mkdirSync(path.dirname(target),{recursive:true,mode:0o700});fs.writeFileSync(target,b,{flag:'wx',mode:0o600});}
 fs.writeFileSync(path.join(root,'manifest.json'),JSON.stringify({schemaVersion:1,files:g.files},null,2)+'\n',{flag:'wx',mode:0o600});index.banks[bankHash]={snapshot};archives.push({cohort:active.cohort,bankHash:active.bankHash,canonicalIndexKey:bankHash,snapshot,files:Object.keys(g.files).length,originalCodePreserved:true,bankChanged:false,accountingReset:false});
}
atomicJson(indexFile,index);
for(const x of archives){const bank=JSON.parse(fs.readFileSync(path.join(runtimeDir,'probability-receipt-cohorts',x.cohort,'bank.json')));await frozenPaperModule({runtimeDir,bank,modulePath:bank.schemaVersion===4?'live-paper/probability-receipt-host-v3.mjs':'live-paper/probability-receipt-host.mjs',fallback:{}});}
const universe=JSON.parse(fs.readFileSync('live-paper/price-universe.json')),transport=[];
for(const [name,identity]of [['collector-native-journal.json',collectorJournalCodeIdentity()],['collector-native-publication.json',collectorPublicationCodeIdentity()]]){
 const file=path.join(runtimeDir,name),bytes=fs.readFileSync(file),prior=JSON.parse(bytes);
 if(name.includes('journal')){nativeJournalBinding(runtimeDir,prior.nativeBackend);if(journalEngineIdentity(path.join(runtimeDir,'journal-native-sources',prior.nativeBackend.engineHash))!==prior.nativeBackend.engineHash)throw Error('Native source changed');}else nativePublicationBinding(runtimeDir,prior.nativeBackend);
 fs.writeFileSync(file+'.before-rename-20261005',bytes,{flag:'wx',mode:0o600});const next={...prior,codeIdentity:identity,...(name.includes('journal')?{sourceSnapshot:payloadHash(identity)}:{})};atomicJson(file,next);
 transport.push({file:name,previousHash:sha(bytes),updatedHash:sha(fs.readFileSync(file)),compiledBindingUnchanged:payloadHash(prior.nativeBackend)===payloadHash(next.nativeBackend),identityFiles:Object.keys(identity).length});
}
collectorNativeJournalConfig(runtimeDir);collectorNativePublicationConfig(runtimeDir,universe);
fs.writeFileSync('data/transport-binding-repair-20261005.json',JSON.stringify({at:new Date().toISOString(),archives,transport,activationChanged:false,originalJournalRewritten:false,modelBanksChanged:false,accountingReset:false,networkRequests:0,competitionOrders:0},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({archives:archives.length,transportBindings:transport.length,modelBanksChanged:false,accountingReset:false}));
