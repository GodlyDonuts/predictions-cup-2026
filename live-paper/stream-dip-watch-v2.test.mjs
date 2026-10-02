import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';import path from 'node:path';import os from 'node:os';import {createHash} from 'node:crypto';
import {createStreamBookAdapter,advanceStreamBookAdapter,streamDigest} from './stream-book-adapter-v2.mjs';
import {streamBookCodeIdentity} from './stream-book-watch-v2.mjs';
import {openStreamDipWatch} from './stream-dip-watch-v2.mjs';
import {streamDipHash} from './stream-dip.mjs';
import {openStreamDipWatch as openOld} from './stream-dip-watch.mjs';
const sha=x=>createHash('sha256').update(x).digest('hex'),base=Date.parse('2026-10-02T15:00:00.000Z'),at=n=>new Date(base+n).toISOString(),tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
function fixture(){const root=fs.mkdtempSync(path.join(os.tmpdir(),'sig-stream-dip-')),runtimeDir=path.join(root,'runtime'),inputCohort='input-books',dir=path.join(runtimeDir,'stream-book-v2-cohorts',inputCohort);fs.mkdirSync(dir,{recursive:true});
 const sourceFile='realtime-fixture.jsonl',source=path.join(runtimeDir,sourceFile),inputJournal=path.join(dir,'journal.jsonl'),topic='tournament:'+tid+':market:377',scope={tournamentId:tid,markets:[{marketId:'377',exchangeId:'1066'}]};let clock=0,index=0,seq=10,revision=100;
 const row=(kind,n,extra)=>({format:'sig-feed-capture',schemaVersion:1,captureIndex:index++,topic,epoch:1,kind,observedAt:at(n),...extra});
 const raw=(n,{bid=.42,ask=.43,quantity=1000}={})=>({exchangeId:'1066',asOf:{sequence:seq++,at:at(n)},nextExpiryAt:null,expiryKnown:true,bids:[{price:bid,quantity}],asks:[{price:ask,quantity}]});
 const t=-41*60000,initial=[row('recovery_started',t,{generation:1}),row('snapshot',t+100,{receivedAt:at(t+90),requestStartedAt:at(t),books:[raw(t+80)]}),row('recovery_finished',t+110,{booksReady:true,generation:1})];
 const sourcePrefix=Buffer.from(initial.map(x=>JSON.stringify(x)+'\n').join('')),bank={schemaVersion:1,mode:'verified_stream_book_input_v2',cohort:inputCohort,sourceFile,scope,cutoff:at(t+120),sourceHash:sha(sourcePrefix),sourcePrefixRecords:3,sourcePrefixBytes:sourcePrefix.length,sessionId:'fixture',codeIdentity:streamBookCodeIdentity()};
 fs.writeFileSync(source,sourcePrefix);fs.writeFileSync(path.join(dir,'bank.json'),JSON.stringify(bank));fs.writeFileSync(path.join(runtimeDir,'realtime-session.json'),JSON.stringify({file:sourceFile,scope}));fs.writeFileSync(path.join(runtimeDir,'stream-book-gateway.lock'),String(process.pid));
 let adapter=createStreamBookAdapter(bank);for(const r of initial)adapter=advanceStreamBookAdapter(adapter,r,bank,{availableAt:bank.cutoff}).state;
 let count=1,previous=streamDigest(bank);const genesis={sequence:count,previous,bankHash:streamDigest(bank),kind:'prefix',stateHash:streamDigest(adapter)};previous=streamDigest(genesis);fs.writeFileSync(inputJournal,JSON.stringify({...genesis,hash:previous})+'\n');
 const stats=()=>fs.writeFileSync(path.join(dir,'stats.json'),JSON.stringify({status:'running',cohort:inputCohort,bankHash:streamDigest(bank),pid:process.pid,updatedAt:at(clock)}));
 function appendRow(r){const n=Date.parse(r.observedAt)-base,bytes=Buffer.from(JSON.stringify(r)+'\n');fs.appendFileSync(source,bytes);
  const out=advanceStreamBookAdapter(adapter,r,bank,{availableAt:at(n)});adapter=out.state;const record={sequence:++count,previous,bankHash:streamDigest(bank),kind:'capture',sourceCaptureIndex:r.captureIndex,sourceLineHash:sha(bytes),availableAt:at(n),events:out.events,stateHash:streamDigest(adapter)};
  previous=streamDigest(record);fs.appendFileSync(inputJournal,JSON.stringify({...record,hash:previous})+'\n');clock=Math.max(clock,n);stats();}
 function append(n,prices){appendRow(row('event',n,{receivedAt:at(n),event:'market_batch',payload:{delivery:{model:'best-effort-authoritative-resync',revision:revision++,previousRevision:revision-2,sourceSequenceFrom:null,sourceSequenceThrough:null},books:[raw(n-1,prices)],trades:[],bookDirty:[],marketSettled:[],resyncRequired:false}}));}
 for(let i=0;i<9;i++){const n=-(40-i*5)*60000,generation=i+2;appendRow(row('recovery_started',n-100,{generation}));
  appendRow(row('snapshot',n-50,{receivedAt:at(n-51),requestStartedAt:at(n-100),books:[raw(n-60)]}));appendRow(row('recovery_finished',n,{booksReady:true,generation}));}
 clock=0;stats();
 return {root,runtimeDir,inputCohort,source,inputJournal,bank,setClock(n){clock=n;stats();},append,open(options={}){return openStreamDipWatch({runtimeDir,cohort:'stream-dip-test',inputCohort,initialize:true,markets:[{marketId:'377',exchangeId:'1066',state:'FL',office:'Senate'}],now:()=>base+clock,workspaceDir:root,...options});},cleanup(){assert.ok(path.resolve(root).startsWith(path.resolve(os.tmpdir())+path.sep));fs.rmSync(root,{recursive:true,force:true});}};
}
test('live consumer uses actual availability, retains partial accounting and replays restart exactly',()=>{
 const f=fixture();let w;try{w=f.open();assert.equal(w.status().warmBooks,10);assert.equal(w.status().wallets.latency_1000.orders,0);
  f.append(1000,{bid:.39,ask:.40});f.setClock(1250);w.consume();assert.equal(w.status().wallets.latency_1000.pending[0].at,at(1250));
  f.append(2500,{bid:.39,ask:.40,quantity:300});f.setClock(2800);w.consume();assert.equal(w.status().wallets.latency_1000.positions[0].quantity,3);
  const before=w.status();w.close();w=f.open({initialize:false});const after=w.status();assert.equal(after.stateHash,before.stateHash);assert.equal(after.journalHash,before.journalHash);
  f.append(5000,{bid:.39,ask:.40});w.consume();assert.equal(w.status().wallets.latency_1000.positions[0].quantity,10);assert.equal(w.status().wallets.latency_1000.realizedPnl,'0.000000');
 }finally{w?.close();f.cleanup();}});
test('one singleton and existing cohort cannot be overwritten',()=>{const f=fixture();let w;try{w=f.open();assert.throws(()=>f.open(),/EEXIST/);w.close();assert.throws(()=>f.open(),/COHORT_EXISTS/);}finally{w?.close();f.cleanup();}});
test('version 1 and version 2 consumers share the same singleton lock',()=>{const f=fixture();let w;try{w=f.open();assert.throws(()=>openOld({runtimeDir:f.runtimeDir,cohort:'old-paper',inputCohort:'old-input',initialize:true}),/EEXIST/);assert.equal(w.owned(),true);}finally{w?.close();f.cleanup();}});
test('STOP and session rotation prevent new ingestion without dropping wallet state',()=>{for(const kind of ['stop','session']){const f=fixture();let w;try{w=f.open();const before=w.status().stateHash;
 if(kind==='stop')fs.writeFileSync(path.join(f.runtimeDir,'STREAM_DIP_V2_STOP'),'test');else fs.writeFileSync(path.join(f.runtimeDir,'realtime-session.json'),JSON.stringify({file:'realtime-other.jsonl',scope:f.bank.scope}));
 assert.throws(()=>w.consume(),/STOPPED|SESSION_ENDED/);assert.equal(w.status().stateHash,before);
 }finally{w?.close();f.cleanup();}}});
test('incomplete input line is retried and rehashed output cannot invent evidence',()=>{const f=fixture();let w;try{w=f.open();const before=w.status();fs.appendFileSync(f.inputJournal,'{');assert.equal(w.consume(),0);assert.equal(w.status().journalHash,before.journalHash);
 fs.truncateSync(f.inputJournal,fs.statSync(f.inputJournal).size-1);f.append(1000,{bid:.39,ask:.40});const lines=fs.readFileSync(f.inputJournal,'utf8').trim().split('\n'),last=JSON.parse(lines.at(-1));last.events=[];const {hash:unused,...body}=last;last.hash=streamDigest(body);lines[lines.length-1]=JSON.stringify(last);fs.writeFileSync(f.inputJournal,lines.join('\n')+'\n');
 assert.throws(()=>w.consume(),/INPUT_REPLAY/);assert.equal(w.status().stateHash,before.stateHash);
 }finally{w?.close();f.cleanup();}});
test('stale upstream health forbids decisions while retaining verified source',()=>{const f=fixture();let w;try{w=f.open();f.append(1000,{bid:.39,ask:.40});f.setClock(1250);fs.writeFileSync(path.join(f.runtimeDir,'stream-book-v2-cohorts/input-books/stats.json'),JSON.stringify({status:'failed'}));w.consume();assert.equal(w.status().wallets.latency_1000.orders,0);assert.equal(w.status().lastEventId>0,true);
 }finally{w?.close();f.cleanup();}});
test('failed journal append never promotes source offsets or wallet accounting',()=>{const f=fixture();let w;const original=fs.writeSync;try{w=f.open();const before=w.status();f.append(1000,{bid:.39,ask:.40});
 fs.writeSync=()=>{throw Error('injected write failure');};assert.throws(()=>w.consume());fs.writeSync=original;
 assert.equal(w.status().stateHash,before.stateHash);assert.equal(w.status().inputSequence,before.inputSequence);assert.throws(()=>w.consume(),/FAILED_CLOSED/);
 w.close();w=f.open({initialize:false});w.consume();assert.equal(w.status().wallets.latency_1000.orders,1);
 }finally{fs.writeSync=original;w?.close();f.cleanup();}});
test('read-only full replay works beside the live lock and never writes or consumes',()=>{const f=fixture();let w,a;try{w=f.open();f.append(1000,{bid:.39,ask:.40});w.consume();const live=w.status(),lock=fs.readFileSync(path.join(f.runtimeDir,'stream-dip.lock'));
 a=f.open({initialize:false,readOnly:true});const audit=a.status();assert.equal(audit.stateHash,live.stateHash);assert.equal(audit.journalHash,live.journalHash);assert.equal(audit.runtimeWrites,0);assert.equal(audit.networkRequests,0);assert.equal(audit.pid,null);
 assert.throws(()=>a.consume(),/READ_ONLY/);a.close();assert.deepEqual(fs.readFileSync(path.join(f.runtimeDir,'stream-dip.lock')),lock);
 }finally{a?.close();w?.close();f.cleanup();}});
test('rehashed wallet checkpoint tampering fails replay and preserves the original writer lock',()=>{const f=fixture();let w;try{w=f.open();f.append(1000,{bid:.39,ask:.40});w.consume();const file=path.join(f.runtimeDir,'stream-dip-v2-cohorts/stream-dip-test/journal.jsonl'),lines=fs.readFileSync(file,'utf8').trim().split('\n'),last=JSON.parse(lines.at(-1));last.stateHash='f'.repeat(64);const {hash:unused,...body}=last;last.hash=streamDipHash(body);lines[lines.length-1]=JSON.stringify(last);fs.writeFileSync(file,lines.join('\n')+'\n');
 assert.throws(()=>f.open({initialize:false,readOnly:true}),/REPLAY_MISMATCH/);assert.equal(fs.readFileSync(path.join(f.runtimeDir,'stream-dip.lock'),'utf8'),String(process.pid));
 }finally{w?.close();f.cleanup();}});
