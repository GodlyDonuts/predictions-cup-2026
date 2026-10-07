import fs from 'node:fs';import path from 'node:path';import test from 'node:test';import assert from 'node:assert/strict';import {createHash} from 'node:crypto';
import {replayBaalGenericNoJournalV4,baalGenericNoControllerV4Identity} from './baal-generic-no-controller-v4.mjs';
import {reconcileBlockedAdmissionV1} from './blocked-admission-reconciliation-v1.mjs';
const rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),file=path.join(rt,'baal-generic-live-runs','baal-scaled-no-20261006-human','generic-v2-20261007-root','controller','baal-generic-no-v4-journal.jsonl');
test('actual historical actor plus hypothetical unchanged later account closes only the blocked intent',{skip:!fs.existsSync(file)},()=>{
 const bytes=fs.readFileSync(file),rows=bytes.toString().trimEnd().split('\n').map(JSON.parse),replay=replayBaalGenericNoJournalV4({bodyText:bytes.toString(),expectedCodeIdentity:baalGenericNoControllerV4Identity(),custodySeed:rows[0].event.input.custodySeed}),state=replay.state,o=state.orders.at(-1),after=structuredClone(o.before),at='2026-10-07T01:40:00.000Z';
 const receipts=[after.cash,after.positions,...after.openOrdersPages,...after.scopedOrdersPages];for(const [i,r]of receipts.entries()){r.requestStartedAt=new Date(Date.parse(at)-8000+i*1000).toISOString();r.receivedAt=new Date(Date.parse(at)-7500+i*1000).toISOString();}
 const input={state,events:rows.map(x=>x.event.input),timings:fs.readFileSync(path.join(rt,'read-timings.jsonl'),'utf8').trimEnd().split('\n').map(JSON.parse),after,at},result=reconcileBlockedAdmissionV1(input);
 assert.equal(result.state.orders.at(-1).phase,'abandoned');assert.equal(result.proof.clearsStop,false);assert.deepEqual(result.state.inventory,state.inventory);assert.deepEqual(result.state.orders.at(-1).body,o.body);assert.ok(fs.readFileSync(file).equals(bytes));
 const changed=structuredClone(after),r=changed.scopedOrdersPages[0],data=JSON.parse(r.bodyText);data.data.push({...data.data[0],id:999999999});r.bodyText=JSON.stringify(data);r.sourceBodyHash=createHash('sha256').update(r.bodyText).digest('hex');assert.throws(()=>reconcileBlockedAdmissionV1({...input,after:changed}));
});

