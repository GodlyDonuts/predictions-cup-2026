import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {createBookPublication} from './book-publication-v2.mjs';import {createNativeBookPublication} from './book-publication-v3.mjs';
import {collectorNativePublicationConfig,collectorPublicationCodeIdentity} from './collector-publication-native.mjs';
import {createBookPublicationState} from './book-publication-v2.mjs';
const u={tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',exchanges:[{marketId:'381',exchangeId:'1070',option:'YES'},{marketId:'382',exchangeId:'1071',option:'YES'}]};
const deploymentFile=new URL('../data/probability-native-deployment-20261004.json',import.meta.url),sourceRoot=process.env.LOCALAPPDATA?path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'):null;
const deployment=fs.existsSync(deploymentFile)?JSON.parse(fs.readFileSync(deploymentFile)):null,nativeBackend=deployment?.nativeBackend;
const available=sourceRoot&&nativeBackend&&fs.existsSync(path.join(sourceRoot,'native-builds',nativeBackend.binaryHash,'baal-backend.exe'));
function fixture(t){const runtimeDir=fs.mkdtempSync(path.join(os.tmpdir(),'native-collector-'));t.after(()=>fs.rmSync(runtimeDir,{recursive:true,force:true}));
 const dir=path.join(runtimeDir,'native-builds',nativeBackend.binaryHash);fs.mkdirSync(dir,{recursive:true});for(const f of ['baal-backend.exe','libunwind.dll'])fs.copyFileSync(path.join(sourceRoot,'native-builds',nativeBackend.binaryHash,f),path.join(dir,f));
 let clock=Date.parse('2026-10-04T02:00:00.000Z');const response=(marketId,n)=>{const exchangeId=marketId==='381'?'1070':'1071';return{endpoint:'/markets/'+marketId+'/orderbook?tournamentId='+u.tournamentId+'&depth=20',requestStartedAt:new Date(clock-100).toISOString(),receivedAt:new Date(clock).toISOString(),data:{marketId,contexts:[{type:'tournament',tournament:{id:u.tournamentId},orderbook:{exchanges:[{exchangeId,option:'YES',asOf:{sequence:n,at:new Date(clock).toISOString()},bids:[{price:.4,quantity:1000}],asks:[{price:.42,quantity:500}]}]}}]}};};
 const file=path.join(runtimeDir,'source'),seed=createBookPublication({file,universe:u,sessionId:'prefix-session',now:()=>clock});clock+=200;seed.publishBook({marketId:'381',response:response('381',1)});clock+=200;seed.publishBook({marketId:'382',response:response('382',2)});seed.close('STOP');clock+=1000;
 const options={file,universe:u,sessionId:'next-session',runtimeDir,nativeBackend,receiptDir:path.join(runtimeDir,'receipts'),now:()=>clock};
 return{runtimeDir,file,response,options,advance(n=200){clock+=n;},native:opts=>createNativeBookPublication({...options,...opts})};
}
const options={skip:!available};
test('native collector restore emits exact legacy journal bytes and immutable states through publish/invalidation/stop',options,async t=>{
 const f=fixture(t),old=path.join(f.runtimeDir,'legacy');fs.copyFileSync(f.file,old);const legacy=createBookPublication({...f.options,file:old}),native=await f.native();t.after(()=>{legacy.close('STOP');native.close('STOP');});
 assert.deepEqual(native.status(),legacy.status());assert.equal(native.startup().sourceRecords,4);assert.equal(native.startup().engineHash,nativeBackend.engineHash);
 for(let n=3;n<30;n++){f.advance();const marketId=n%2?'381':'382',response=f.response(marketId,n);assert.equal(native.publishBook({marketId,response}),legacy.publishBook({marketId,response}));
  if(n%5===0){f.advance(1);for(const w of [legacy,native])w.invalidate({marketId:n%10===0?null:marketId,reason:'MANUAL_INVALIDATION'});}
  assert.deepEqual(native.status(),legacy.status());assert.deepEqual(fs.readFileSync(f.file),fs.readFileSync(old));
 }
 assert.throws(()=>native.status().scope.exchanges.push({}));assert.throws(()=>Object.values(native.status().books)[0].bids[0].quantity=1);
 for(const w of [legacy,native])w.close('STOP');assert.deepEqual(fs.readFileSync(f.file),fs.readFileSync(old));
});
test('damaged original prefix and a changed native library never append a replacement session',options,async t=>{
 const f=fixture(t),saved=fs.readFileSync(f.file);fs.writeFileSync(f.file,saved.subarray(0,saved.length-1));const partial=fs.readFileSync(f.file);await assert.rejects(f.native(),/PUBLIC_BOOK_PARTIAL_OR_OVERSIZE_RECORD/);assert.deepEqual(fs.readFileSync(f.file),partial);
 const rows=saved.toString().trimEnd().split('\n').map(JSON.parse);rows[1].hash='a'.repeat(64);const corrupt=Buffer.from(rows.map(x=>JSON.stringify(x)+'\n').join(''));fs.writeFileSync(f.file,corrupt);await assert.rejects(f.native(),/PUBLIC_BOOK_CHAIN_INVALID/);assert.deepEqual(fs.readFileSync(f.file),corrupt);
 fs.writeFileSync(f.file,saved);const library=path.join(f.runtimeDir,'native-builds',nativeBackend.binaryHash,'libunwind.dll');fs.appendFileSync(library,'changed');await assert.rejects(f.native(),/CHAIN_INVALID/);assert.deepEqual(fs.readFileSync(f.file),saved);
});
test('one asynchronous native writer owns the path; stop during restoration adds no record and releases the lease',options,async t=>{
 const f=fixture(t),first=f.native();await assert.rejects(f.native(),/WRITER_EXISTS/);const w=await first;w.close('STOP');f.advance(1000);const saved=fs.readFileSync(f.file);let run=true;
 const opening=f.native({sessionId:'stop-during-restore',canOperate:()=>run});run=false;await assert.rejects(opening,/STOPPED/);assert.deepEqual(fs.readFileSync(f.file),saved);
 const recovered=await f.native({sessionId:'after-stop'});recovered.close('STOP');
});
test('native startup cannot hide a durable append failure or keep a second writer alive',options,async t=>{
 const f=fixture(t),io={...fs,writeSync(){throw Error('disk failure');}},saved=fs.readFileSync(f.file);await assert.rejects(f.native({io}),/DURABILITY_FAILURE/);assert.deepEqual(fs.readFileSync(f.file),saved);
 const w=await f.native();w.close('STOP');
});
test('explicit collector binding rejects a changed source closure or tournament scope',options,t=>{
 const f=fixture(t),file=path.join(f.runtimeDir,'collector-native-publication.json'),c={schemaVersion:1,mode:'native_publication_collector_v1',scopeHash:createBookPublicationState(u).scopeHash,codeIdentity:collectorPublicationCodeIdentity(),nativeBackend};
 assert.equal(collectorNativePublicationConfig(f.runtimeDir,u),null);fs.writeFileSync(file,JSON.stringify(c));assert.deepEqual(collectorNativePublicationConfig(f.runtimeDir,u),c);
 c.scopeHash='a'.repeat(64);fs.writeFileSync(file,JSON.stringify(c));assert.throws(()=>collectorNativePublicationConfig(f.runtimeDir,u),/BINDING_CHANGED/);c.scopeHash=createBookPublicationState(u).scopeHash;delete c.codeIdentity['live-paper/book-publication-v3.mjs'];fs.writeFileSync(file,JSON.stringify(c));assert.throws(()=>collectorNativePublicationConfig(f.runtimeDir,u),/BINDING_CHANGED/);
});
