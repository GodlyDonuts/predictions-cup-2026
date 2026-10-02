import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {openStreamBookWatch,runStreamBookWatch} from './stream-book-watch-v2.mjs';
import {streamDigest} from './stream-book-adapter-v2.mjs';
import {openStreamBookWatch as openOld} from './stream-book-watch.mjs';
const base=Date.parse('2026-10-02T15:00:00.000Z'),at=n=>new Date(base+n).toISOString(),tid='bda92870-621e-47b0-bc3c-3602c5c26f55',topic='tournament:'+tid+':market:377';
const book=(sequence,n)=>({exchangeId:'1066',asOf:{sequence,at:at(n)},nextExpiryAt:null,expiryKnown:true,bids:[{price:.4,quantity:1000}],asks:[{price:.42,quantity:1000}]});
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'stream-watch-')),source=path.join(dir,'realtime-fixture.jsonl'),workspaceDir=path.join(dir,'workspace');fs.mkdirSync(workspaceDir);
 const session={file:'realtime-fixture.jsonl',scope:{tournamentId:tid,markets:[{marketId:'377',exchangeId:'1066'}]}};fs.writeFileSync(path.join(dir,'realtime-session.json'),JSON.stringify(session));
 let count=0,clock=800;const opened=[];
 const make=(kind,n,extra={})=>({format:'sig-feed-capture',schemaVersion:1,captureIndex:count++,topic,epoch:1,kind,observedAt:at(n),...extra});
 const append=row=>fs.appendFileSync(source,JSON.stringify(row)+'\n');
 append(make('recovery_started',100,{generation:1}));append(make('snapshot',200,{receivedAt:at(190),requestStartedAt:at(100),books:[book(10,180)]}));append(make('recovery_finished',210,{booksReady:true,generation:1}));
 const options={runtimeDir:dir,cohort:'test',workspaceDir,now:()=>base+clock};
 const event=(n=1000,seq=20,rev=100)=>make('event',n,{receivedAt:at(n),...{event:'market_batch',payload:{delivery:{model:'best-effort-authoritative-resync',revision:rev,previousRevision:rev-1,sourceSequenceFrom:null,sourceSequenceThrough:null},books:[book(seq,n)],trades:[],bookDirty:[],marketSettled:[],resyncRequired:false}},privateSentinel:'DO_NOT_EXPORT'});
 t.after(()=>{for(const w of opened)w.close();fs.rmSync(dir,{recursive:true,force:true});});
 return {dir,source,options,event,append,make,setNow:n=>clock=n,cohortDir:path.join(dir,'stream-book-v2-cohorts/test'),open:(initialize=false,cohort='test')=>{const w=openStreamBookWatch({...options,cohort,initialize});opened.push(w);return w;}};
}
const journal=f=>fs.readFileSync(path.join(f.cohortDir,'journal.jsonl'),'utf8').trim().split('\n').map(JSON.parse);
test('immutable prefix warms silently; actual consumption clock and sanitized book journal replay exactly',t=>{
 const f=fixture(t),w=f.open(true);assert.equal(w.status().counters.acceptedBooks,0);assert.equal(journal(f).length,1);
 f.append(f.event());f.setNow(1250);assert.equal(w.consume(),1);const status=w.status(),saved=fs.readFileSync(path.join(f.cohortDir,'journal.jsonl'),'utf8');
 assert.equal(status.counters.streamBooks,1);assert.equal(status.markets[0].availableAt,at(1250));assert.equal(status.markets[0].quoteReceivedAt,at(1000));
 assert.equal(saved.includes('DO_NOT_EXPORT'),false);const event=journal(f)[1].events.find(e=>e.kind==='book');assert.equal(event.requestStartedAt,null);w.close();
 f.setNow(50000);const resumed=f.open();assert.equal(resumed.status().journalHash,status.journalHash);assert.equal(resumed.status().markets[0].availableAt,at(1250));assert.equal(resumed.status().markets[0].ready,false);
 assert.equal(fs.readFileSync(path.join(f.cohortDir,'journal.jsonl'),'utf8'),saved);
});
test('one global lock prevents duplicate cohorts and initialization never overwrites old accounting',t=>{
 const f=fixture(t),w=f.open(true),before=fs.readFileSync(path.join(f.cohortDir,'bank.json'));
 assert.throws(()=>f.open(true,'other'),/EEXIST/);assert.equal(w.owned(),true);w.close();assert.throws(()=>f.open(true),/COHORT_EXISTS/);assert.deepEqual(fs.readFileSync(path.join(f.cohortDir,'bank.json')),before);
});
test('version 1 and version 2 gateways share the same singleton lock',t=>{const f=fixture(t),w=f.open(true);
 assert.throws(()=>openOld({...f.options,cohort:'old-source',initialize:true}),/EEXIST/);assert.equal(w.owned(),true);
});
test('incomplete source tail is retried after restart without duplicate or lost events',t=>{
 const f=fixture(t),w=f.open(true),bytes=Buffer.from(JSON.stringify(f.event())+'\n');f.setNow(1250);fs.appendFileSync(f.source,bytes.subarray(0,50));assert.equal(w.consume(),0);w.close();
 const resumed=f.open();assert.equal(resumed.consume(),0);fs.appendFileSync(f.source,bytes.subarray(50));assert.equal(resumed.consume(),1);assert.equal(resumed.consume(),0);assert.equal(journal(f).length,2);
});
test('rewritten source, changed bank and rehashed output tampering fail closed',async t=>{
 for(const change of ['source','bank','journal'])await t.test(change,t=>{
  const f=fixture(t),w=f.open(true);f.append(f.event());f.setNow(1250);w.consume();w.close();
  if(change==='source'){const b=fs.readFileSync(f.source,'utf8');fs.writeFileSync(f.source,b.replace('DO_NOT_EXPORT','DO_NOT_EXPOSE'));}
  if(change==='bank'){const file=path.join(f.cohortDir,'bank.json'),bank=JSON.parse(fs.readFileSync(file));bank.cutoff=at(799);fs.writeFileSync(file,JSON.stringify(bank));}
  if(change==='journal'){const file=path.join(f.cohortDir,'journal.jsonl'),j=journal(f);j[1].events[0].availableAt=at(1200);const {hash,...body}=j[1];j[1].hash=streamDigest(body);fs.writeFileSync(file,j.map(JSON.stringify).join('\n')+'\n');}
  assert.throws(()=>f.open(),/STREAM_(SOURCE_CHANGED|JOURNAL_CHAIN|REPLAY_MISMATCH)/);assert.equal(fs.existsSync(path.join(f.dir,'stream-book-gateway.lock')),false);
 });
});
test('active source replacement and truncated checkpoint tails stop processing',t=>{
 const f=fixture(t),w=f.open(true);fs.renameSync(f.source,f.source+'.old');fs.copyFileSync(f.source+'.old',f.source);assert.throws(()=>w.consume(),/REPLACED/);assert.throws(()=>w.consume(),/FAILED_CLOSED/);w.close();
 fs.appendFileSync(path.join(f.cohortDir,'journal.jsonl'),'{');assert.throws(()=>f.open(),/INCOMPLETE/);
});
test('STOP and session rotation cannot admit new records; no credentials or API requests',async t=>{
 const f=fixture(t),w=f.open(true);f.append(f.event());f.setNow(1250);const before=fs.readFileSync(path.join(f.cohortDir,'journal.jsonl'));
 fs.writeFileSync(path.join(f.dir,'STREAM_BOOK_V2_STOP'),'');assert.throws(()=>w.consume(),/STOPPED/);assert.deepEqual(fs.readFileSync(path.join(f.cohortDir,'journal.jsonl')),before);w.close();fs.unlinkSync(path.join(f.dir,'STREAM_BOOK_V2_STOP'));
 let requests=0;t.mock.method(globalThis,'fetch',()=>{requests++;throw Error('NETWORK_FORBIDDEN');});
 await runStreamBookWatch({...f.options,once:true});assert.equal(requests,0);const stats=JSON.parse(fs.readFileSync(path.join(f.cohortDir,'stats.json')));assert.equal(stats.status,'running');assert.equal(stats.networkRequests,0);
 const resumed=f.open(),session=JSON.parse(fs.readFileSync(path.join(f.dir,'realtime-session.json')));session.file='realtime-new.jsonl';fs.writeFileSync(path.join(f.dir,'realtime-session.json'),JSON.stringify(session));assert.equal(resumed.current(),false);assert.throws(()=>resumed.consume(),/SESSION_ENDED/);
});
test('failed write cannot promote a source checkpoint and startup write failure removes its new lock',t=>{
 const f=fixture(t),original=fs.writeFileSync;t.mock.method(fs,'writeFileSync',function(file,...args){if(typeof file==='number')throw Error('Injected');return original.call(this,file,...args);});
 assert.throws(()=>f.open(true),/Injected/);assert.equal(fs.existsSync(path.join(f.dir,'stream-book-gateway.lock')),false);t.mock.restoreAll();
 const w=f.open(true);f.append(f.event());f.setNow(1250);const before=w.status();t.mock.method(fs,'writeSync',()=>{throw Error('Injected');});assert.throws(()=>w.consume(),/Injected/);
 assert.equal(w.status().sourceCaptureIndex,before.sourceCaptureIndex);assert.equal(w.status().journalHash,before.journalHash);
});
