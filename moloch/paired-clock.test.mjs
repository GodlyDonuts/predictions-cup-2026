import test from 'node:test';import assert from 'node:assert/strict';
import {pairedClock} from './paired-clock.mjs';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {run} from './run.mjs';import {payloadHash} from '../execution/ledger.mjs';
const event={type:'tick',receivedAt:'2026-10-14T00:00:00.000Z'};
test('both consumers wait for identical events and receive one actual clock sample',async()=>{
 let calls=0,ready=false;const c=pairedClock(()=>{calls++;return '2026-10-14T00:00:01.000Z';});
 const a=c.clock(0)(event,1).then(v=>{ready=true;return v;});await Promise.resolve();assert.equal(ready,false);assert.equal(calls,0);
 const times=await Promise.all([a,c.clock(1)(event,1)]);assert.equal(times[0],times[1]);assert.equal(calls,1);
});
test('different events reject both consumers rather than comparing different histories',async()=>{
 const c=pairedClock(),a=c.clock(0)(event,1),b=c.clock(1)({...event,type:'gap'},1);
 const r=await Promise.allSettled([a,b]);assert.ok(r.every(x=>x.status==='rejected'&&/EVENT_MISMATCH/.test(x.reason.message)));
});
test('abort releases a waiting consumer and an earlier clock never backdates execution',async()=>{
 const c=pairedClock(),a=c.clock(0)(event,1);c.abort(Error('source stopped'));await assert.rejects(a,/source stopped/);
 const d=pairedClock(()=> '2026-10-13T00:00:00.000Z');const r=await Promise.allSettled([d.clock(0)(event,1),d.clock(1)(event,1)]);assert.ok(r.every(x=>x.status==='rejected'&&/CAUSALITY/.test(x.reason.message)));
});
test('actual journal consumers persist identical processing clocks and entry intents',async()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-paired-')),input=path.join(dir,'source.jsonl'),clock=pairedClock();fs.writeFileSync(input,'');
 try {
  const jobs=[0,1].map(i=>run(['--input',input,'--output',path.join(dir,String(i)),'--format','journal','--follow','--seconds','0.6'],{processingClock:clock.clock(i)}));
  const at=new Date().toISOString(),e={type:'book',receivedAt:at,book:{tournamentId:'cup',marketId:'101',exchangeId:'101',sequence:'1',requestStartedAt:at,receivedAt:at,sourceAt:at,bids:[{price:'0.450000',quantity:100}],asks:[{price:'0.465000',quantity:100}]}};
  const body={sequence:1,previous:'',event:e};fs.writeFileSync(input,JSON.stringify({...body,hash:payloadHash(body)})+'\n');
  await Promise.all(jobs);
  const snapshots=[0,1].map(i=>JSON.parse(fs.readFileSync(path.join(dir,String(i),'state.json'))));
  assert.equal(snapshots[0].sequence,1);assert.equal(snapshots[1].sequence,1);
  assert.equal(payloadHash(snapshots[0].state),payloadHash(snapshots[1].state));
  const rows=[0,1].map(i=>JSON.parse(fs.readFileSync(path.join(dir,String(i),'events.jsonl'),'utf8').trim()));
  assert.equal(rows[0].event.processedAt,rows[1].event.processedAt);assert.ok(snapshots[0].state.orders.length>0);
 } finally {
  if(path.dirname(fs.realpathSync(dir))!==fs.realpathSync(os.tmpdir())||!path.basename(dir).startsWith('moloch-paired-'))throw Error('TEST_CLEANUP_PATH');
  fs.rmSync(dir,{recursive:true,force:true});
 }
});
