import test from 'node:test';import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {payloadHash} from '../execution/ledger.mjs';import {bookExportEvents,replayEvents,run} from './run.mjs';
test('bounded export verifies each record and hash chain, not just the export checksum',()=>{
  const b={kind:'book',sequence:10,previousHash:'prefix',payload:{book:{receivedAt:'2026-10-13T00:00:00.000Z'}}},row={...b,hash:payloadHash(b)},body={rows:[row]},document={...body,exportHash:payloadHash(body)};
  assert.equal(bookExportEvents(document).length,1);
  const bad=structuredClone(body);bad.rows[0].sequence++;assert.throws(()=>bookExportEvents({...bad,exportHash:payloadHash(bad)}),/RECORD_CHAIN/);
});
test('file runner persists exact event chain, refuses overwrite and never performs network traffic',async t=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-run-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const input=path.join(dir,'input.ndjson'),output=path.join(dir,'run');fs.writeFileSync(input,JSON.stringify({type:'tick',receivedAt:'2026-10-13T00:00:00.000Z'})+'\n');
  const result=await run(['--input',input,'--output',output]);assert.equal(result.events,1);assert.equal(result.networkRequests,0);
  const snapshot=JSON.parse(fs.readFileSync(path.join(output,'state.json'))),{hash,...body}=snapshot;assert.equal(hash,payloadHash(body));
  assert.equal(snapshot.state.bot,'Moloch');await assert.rejects(run(['--input',input,'--output',output]),/OUTPUT_EXISTS/);
});
test('noncausal input is rejected rather than sorted using future information',()=>{
  assert.throws(()=>replayEvents([{type:'tick',receivedAt:'2026-10-13T00:00:01.000Z'},{type:'tick',receivedAt:'2026-10-13T00:00:00.000Z'}]),/RECEIPT_ORDER/);
});
test('prospective tail skips historical data and records actual processing clock',async t=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'moloch-tail-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const input=path.join(dir,'input.ndjson'),output=path.join(dir,'paper');
  fs.writeFileSync(input,JSON.stringify({type:'tick',receivedAt:'2026-10-12T00:00:00.000Z'})+'\n');
  const timer=setTimeout(()=>fs.appendFileSync(input,JSON.stringify({type:'tick',receivedAt:new Date().toISOString()})+'\n'),100);
  t.after(()=>clearTimeout(timer));const result=await run(['--input',input,'--output',output,'--follow','--seconds','0.6']);
  assert.equal(result.preCutoffEventsSkipped,1);assert.equal(result.events,1);
  const row=JSON.parse(fs.readFileSync(path.join(output,'events.jsonl'),'utf8'));assert.ok(Date.parse(row.event.processedAt)>=Date.parse(row.event.receivedAt));
});
