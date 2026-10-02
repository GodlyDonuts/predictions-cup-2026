import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {openPassiveWatch} from './passive-watch.mjs';

const base=Date.parse('2026-10-02T10:00:00Z'),at=n=>new Date(base+n).toISOString(),tournamentId='bda92870-621e-47b0-bc3c-3602c5c26f55';
function fixture(t){
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-passive-watch-'));
  t.after(()=>{const target=path.resolve(dir);if(path.dirname(target)!==path.resolve(os.tmpdir())||!path.basename(target).startsWith('sig-passive-watch-'))throw Error('Unsafe test cleanup');fs.rmSync(target,{recursive:true,force:true});});
  const file='realtime-fixture.jsonl',scope={tournamentId,markets:[{marketId:'377',exchangeId:'1066'}]},topic=`tournament:${tournamentId}:market:377`;
  fs.writeFileSync(path.join(dir,'realtime-session.json'),JSON.stringify({file,scope}));
  let index=0;const push=(kind,extra={})=>fs.appendFileSync(path.join(dir,file),JSON.stringify({format:'sig-feed-capture',schemaVersion:1,captureIndex:index++,kind,topic,epoch:1,observedAt:at(100),...extra})+'\n');
  push('config',{epoch:0,observedAt:at(-100),staleMs:20000,scope:{tournamentId,marketId:'377',exchangeIds:['1066'],topic}});
  const options={now:()=>base,codeIdentity:{fixture:'v1'}};
  const recover=(finished=true)=>{
    push('subscribed',{observedAt:at(-20)});push('invalidation',{observedAt:at(-10),reason:'INITIAL'});push('recovery_started',{observedAt:at(0),generation:1});
    push('snapshot',{receivedAt:at(100),requestStartedAt:at(10),books:[{exchangeId:'1066',asOf:{sequence:1,at:at(50)},nextExpiryAt:null,bids:[{price:.4,quantity:100}],asks:[{price:.42,quantity:100}]}]});
    if(finished)push('recovery_finished',{booksReady:true,generation:1});
  };
  return {dir,file,push,options,recover};
}
test('prospective observer resumes its own exact journal without resetting quote accounting',t=>{
  const f=fixture(t),w=openPassiveWatch(f.dir,{...f.options,initialize:true});w.consume();f.recover();w.consume();
  const before=w.status();assert.equal(before.markets[0].quotePairs,1);assert.equal(before.simulatedFills,null);assert.equal(before.ordersSubmitted,0);
  const resumed=openPassiveWatch(f.dir,f.options);assert.deepEqual(resumed.status(),before);assert.equal(resumed.consume(),0);
  assert.throws(()=>openPassiveWatch(f.dir,{...f.options,initialize:true}),/Refusing/);
});
test('a captured snapshot alone creates no quote or recovered state',t=>{
  const f=fixture(t),w=openPassiveWatch(f.dir,{...f.options,initialize:true});f.recover(false);w.consume();
  assert.equal(w.status().markets[0].quotePairs,0);assert.equal(w.status().markets[0].ready,false);
});
test('changed prefix, code and incomplete journal fail closed without replacing evidence',t=>{
  const f=fixture(t),w=openPassiveWatch(f.dir,{...f.options,initialize:true});w.consume();f.recover();w.consume();
  assert.throws(()=>openPassiveWatch(f.dir,{...f.options,codeIdentity:{fixture:'v2'}}),/identity changed/);
  const file=path.join(f.dir,'passive-journal.jsonl'),original=fs.readFileSync(file);fs.appendFileSync(file,'{');
  assert.throws(()=>openPassiveWatch(f.dir,f.options),/Incomplete/);fs.writeFileSync(file,original);
  const source=path.join(f.dir,f.file),bytes=fs.readFileSync(source);bytes[0]=32;fs.writeFileSync(source,bytes);
  assert.throws(()=>openPassiveWatch(f.dir,f.options),/prefix changed/);
});
test('partial capture tails wait for completion and a new feed session requires a separate cohort',t=>{
  const f=fixture(t),w=openPassiveWatch(f.dir,{...f.options,initialize:true});w.consume();
  fs.appendFileSync(path.join(f.dir,f.file),'{');assert.equal(w.consume(),0);
  fs.writeFileSync(path.join(f.dir,'realtime-session.json'),JSON.stringify({file:'realtime-new.jsonl'}));assert.equal(w.sessionCurrent(),false);
});
test('worker failure publishes failed readiness and preserves its bank and original journal',t=>{
  const f=fixture(t);f.push('event',{event:'unsupported',observedAt:at(100)});
  const run=spawnSync(process.execPath,[fileURLToPath(new URL('./passive-watch.mjs',import.meta.url)),'--init','--once'],{env:{...process.env,SIG_PAPER_DIR:f.dir},encoding:'utf8'});
  assert.equal(run.status,1);const stats=JSON.parse(fs.readFileSync(path.join(f.dir,'passive-stats.json')));
  assert.equal(stats.runtime.status,'failed');assert(stats.markets.every(m=>!m.ready));
  assert(fs.existsSync(path.join(f.dir,'passive-bank.json')));assert(fs.statSync(path.join(f.dir,'passive-journal.jsonl')).size>0);
  assert.equal(fs.existsSync(path.join(f.dir,'passive-watch.lock')),false);
});
