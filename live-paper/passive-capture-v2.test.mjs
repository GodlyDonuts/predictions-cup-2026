import test from 'node:test';import assert from 'node:assert/strict';
import {createPassiveCapture,consumePassiveCapture} from './passive-capture-v2.mjs';
import {createPassiveCapture as createOld,consumePassiveCapture as consumeOld} from './passive-capture.mjs';
import {timestamp} from '../feed/contract.mjs';
const base=Date.parse('2026-10-02T15:00:00.000Z'),at=n=>new Date(base+n).toISOString(),tid='bda92870-621e-47b0-bc3c-3602c5c26f55',topic='tournament:'+tid+':market:377';
const raw=(sequence,n,expiry)=>({exchangeId:'1066',asOf:{sequence,at:at(n)},nextExpiryAt:expiry===null?null:at(expiry),expiryKnown:true,bids:[{price:.40,quantity:1000}],asks:[{price:.42,quantity:1000}]});
function fixture(old=false){const bank={scope:{tournamentId:tid,markets:[{marketId:'377',exchangeId:'1066'}]},sourceHash:'a'.repeat(64),cutoff:at(0),sessionId:'fixture'},create=old?createOld:createPassiveCapture,consume=old?consumeOld:consumePassiveCapture;let s=create(bank),index=0;
 return {get state(){return s;},apply(kind,n,extra={}){const row={format:'sig-feed-capture',schemaVersion:1,captureIndex:index++,topic,epoch:1,kind,observedAt:at(n),...extra},out=consume(s,row);s=out.state;return out;}};}
function establishExpired(f){f.apply('recovery_started',100,{generation:1});f.apply('snapshot',200,{receivedAt:at(190),requestStartedAt:at(100),books:[raw(10,180,null)]});f.apply('recovery_finished',210,{generation:1,booksReady:true});
 f.apply('event',1000,{receivedAt:at(990),event:'market_batch',payload:{delivery:{model:'best-effort-authoritative-resync',revision:100,previousRevision:99,sourceSequenceFrom:null,sourceSequenceThrough:null},books:[raw(20,980,2000)],trades:[],bookDirty:[],marketSettled:[],resyncRequired:false}});
 f.apply('invalidation',2100,{reason:'EXPIRY',generation:2});}
function recover(f,{sequence=30,sourceAt=2280,expiry=null,received=2290,request=2200}={}){f.apply('recovery_started',2200,{generation:3});f.apply('snapshot',2300,{receivedAt:at(received),requestStartedAt:at(request),books:[raw(sequence,sourceAt,expiry)]});return f.apply('recovery_finished',2400,{generation:3,booksReady:true});}
test('reproduces frozen v1 permanent expiry barrier and recovers only on newer valid snapshot in v2',()=>{
 const old=fixture(true);establishExpired(old);recover(old);assert.equal(old.state.contexts['377'].ready,false);assert.equal(old.state.contexts['377'].knownExpiry.at,timestamp(at(2000)));
 const f=fixture();establishExpired(f);const out=recover(f);assert.equal(f.state.contexts['377'].ready,true);assert.equal(f.state.contexts['377'].knownExpiry,null);
 assert.equal(out.records.find(x=>x.event.kind==='book').event.book.sequence,'30');
});
test('older, same-version and pre-expiry REST engine versions cannot clear safety barrier',()=>{
 // REST expiry fields are not promised; only verified engine-time advancement
 // beyond the old expiry can supersede it. Never treat a REST null as known none.
 for(const args of [{sequence:19,sourceAt:970},{sequence:20,sourceAt:980,expiry:null},{sequence:30,sourceAt:1999}]){const f=fixture();establishExpired(f);recover(f,args);assert.equal(f.state.contexts['377'].ready,false);}
});
test('unverified HTTP start never clears expiry',()=>{const f=fixture();establishExpired(f);recover(f,{request:2199});assert.equal(f.state.contexts['377'].ready,false);assert.equal(f.state.contexts['377'].knownExpiry.at,timestamp(at(2000)));});
test('buffered newer push wins recovery and keeps its own expiry and source version',()=>{const f=fixture();establishExpired(f);f.apply('recovery_started',2200,{generation:3});
 f.apply('event',2250,{receivedAt:at(2249),event:'market_batch',payload:{delivery:{model:'best-effort-authoritative-resync',revision:101,previousRevision:100,sourceSequenceFrom:null,sourceSequenceThrough:null},books:[raw(40,2240,3400)],trades:[],bookDirty:[],marketSettled:[],resyncRequired:false}});
 f.apply('snapshot',2300,{receivedAt:at(2290),requestStartedAt:at(2200),books:[raw(30,2230,null)]});const out=f.apply('recovery_finished',2400,{generation:3,booksReady:true});
 assert.equal(f.state.contexts['377'].ready,true);assert.equal(f.state.contexts['377'].knownExpiry.at,timestamp(at(3400)));assert.equal(out.records.find(x=>x.event.kind==='book').event.book.sequence,'40');
});
test('same-version stream expiry conflict cannot erase an expired barrier',()=>{const f=fixture();establishExpired(f);
 const out=f.apply('event',2150,{receivedAt:at(2149),event:'market_batch',payload:{delivery:{model:'best-effort-authoritative-resync',revision:101,previousRevision:100,sourceSequenceFrom:null,sourceSequenceThrough:null},books:[raw(20,980,null)],trades:[],bookDirty:[],marketSettled:[],resyncRequired:false}});
 assert.equal(f.state.contexts['377'].knownExpiry.at,timestamp(at(2000)));assert.equal(f.state.contexts['377'].ready,false);assert.equal(out.records.some(x=>x.event.kind==='book'),false);
});
test('currently expired buffered authoritative push remains invalid at recovery confirmation',()=>{const f=fixture();establishExpired(f);f.apply('recovery_started',2200,{generation:3});
 f.apply('event',2250,{receivedAt:at(2249),event:'market_batch',payload:{delivery:{model:'best-effort-authoritative-resync',revision:101,previousRevision:100,sourceSequenceFrom:null,sourceSequenceThrough:null},books:[raw(40,2240,2300)],trades:[],bookDirty:[],marketSettled:[],resyncRequired:false}});
 f.apply('snapshot',2300,{receivedAt:at(2290),requestStartedAt:at(2200),books:[raw(30,2230,null)]});f.apply('recovery_finished',2400,{generation:3,booksReady:true});assert.equal(f.state.contexts['377'].ready,false);
});
