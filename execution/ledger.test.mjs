import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {openLedger,replayJournal,acquireOwnerLease,canonicalJson,payloadHash,atomicJson} from './ledger.mjs';

const scope='bda92870-621e-47b0-bc3c-3602c5c26f55';
const input=()=>({intentId:'intent-1',idempotencyKey:'key-1',method:'POST',path:'/orders',body:{idempotencyKey:'key-1',exchangeId:'1066',side:'yes',action:'buy',quantity:1,price:.45,expirationDate:null,tournamentId:scope},attribution:{runId:'run-1',strategyId:'dip-v2',decisionId:'decision-1'}});
const handles=new Map();
function fixture(t){const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-execution-ledger-'));handles.set(dir,[]);t.after(()=>{for(const handle of handles.get(dir))handle.close();handles.delete(dir);const resolved=path.resolve(dir);assert.equal(path.dirname(resolved),path.resolve(os.tmpdir()));assert.ok(path.basename(resolved).startsWith('sig-execution-ledger-'));fs.rmSync(resolved,{recursive:true,force:true});});return dir;}
function open(dir,extra={}){const ledger=openLedger({dir,ownerId:'codex',runId:'run-1',scope,isStopped:()=>false,...extra});handles.get(dir)?.push(ledger);return ledger;}
const raw=dir=>fs.readFileSync(path.join(dir,'execution-journal.jsonl'),'utf8');
function rehash(record){const {hash,...body}=record;return {...body,hash:createHash('sha256').update(canonicalJson(body)).digest('hex')};}

test('prepare and dispatch are durable before return and replay exact immutable order bytes',t=>{
 const dir=fixture(t),ledger=open(dir);t.after(()=>ledger.close());
 const prepared=ledger.prepareIntent(input());assert.equal(prepared.status,'prepared');assert.equal(prepared.payloadHash,payloadHash(input().body));
 assert.equal(replayJournal(raw(dir),{scope}).intents['intent-1'].bodyJson,prepared.bodyJson);
 prepared.body.price=.99;prepared.bodyJson='changed';
 const dispatch=ledger.markDispatching('intent-1');assert.equal(dispatch.body.price,.45);assert.equal(dispatch.attempts,1);
 assert.equal(replayJournal(raw(dir)).intents['intent-1'].status,'dispatching');
 assert.equal(JSON.parse(fs.readFileSync(ledger.projectionFile)).sequence,2);
 ledger.recordOutcome('intent-1',{status:'acknowledged',orderId:'official-1',reasonCode:'accepted'});
 ledger.recordOutcome('intent-1',{status:'reconciled',orderId:'official-1'});
 ledger.recordOutcome('intent-1',{status:'closed',orderId:'official-1',reasonCode:'filled'});
 assert.equal(replayJournal(raw(dir)).intents['intent-1'].status,'closed');
});

test('same preparation is idempotent; changed terms or reused keys cannot create another intent',t=>{
 const ledger=open(fixture(t));t.after(()=>ledger.close());ledger.prepareIntent(input());
 ledger.prepareIntent(input());assert.equal(ledger.snapshot().sequence,1);
 const changed=input();changed.body.price=.5;assert.throws(()=>ledger.prepareIntent(changed),{code:'INTENT_CONFLICT'});
 assert.throws(()=>ledger.prepareIntent({...input(),intentId:'intent-2'}),{code:'INTENT_CONFLICT'});
 assert.throws(()=>ledger.markDispatching('absent'),{code:'UNKNOWN_INTENT'});
 assert.throws(()=>ledger.prepareIntent({...input(),payloadHash:'bad'}),{code:'INTENT_CONFLICT'});
});

test('ambiguous retry preserves original payload and requires explicit permission and due time',t=>{
 const dir=fixture(t);let now=Date.parse('2026-10-02T06:00:00.000Z');
 let ledger=open(dir,{now:()=>now});const prepared=ledger.prepareIntent(input());ledger.markDispatching('intent-1');
 ledger.recordOutcome('intent-1',{status:'uncertain',reasonCode:'request_in_flight',retryAllowed:true,earliestRetryAt:new Date(now+90000).toISOString()});ledger.close();
 ledger=open(dir,{runId:'run-2',now:()=>now});t.after(()=>ledger.close());
 assert.throws(()=>ledger.markDispatching('intent-1'),{code:'INVALID_TRANSITION'});
 assert.throws(()=>ledger.markDispatching('intent-1',{retry:true}),{code:'INVALID_TRANSITION'});
 now+=90000;const retry=ledger.markDispatching('intent-1',{retry:true});
 assert.equal(retry.bodyJson,prepared.bodyJson);assert.equal(retry.idempotencyKey,prepared.idempotencyKey);assert.equal(retry.payloadHash,prepared.payloadHash);assert.equal(retry.attempts,2);
});

test('crash after dispatch remains unresolved on replay and cannot silently retry',t=>{
 const dir=fixture(t);let ledger=open(dir);ledger.prepareIntent(input());ledger.markDispatching('intent-1');ledger.close();
 ledger=open(dir,{runId:'restart'});t.after(()=>ledger.close());assert.equal(ledger.getIntent('intent-1').status,'dispatching');
 assert.throws(()=>ledger.markDispatching('intent-1',{retry:true}),{code:'INVALID_TRANSITION'});
 ledger.recordOutcome('intent-1',{status:'reconciled',orderId:'found-after-crash'});
 assert.equal(ledger.getIntent('intent-1').orderId,'found-after-crash');
});

test('semantic uncertainty and missing order IDs fail closed; raw outcomes are rejected',t=>{
 const ledger=open(fixture(t));t.after(()=>ledger.close());ledger.prepareIntent(input());ledger.markDispatching('intent-1');
 assert.throws(()=>ledger.recordOutcome('intent-1',{status:'acknowledged',orderId:null}),{code:'INVALID_INPUT'});
 assert.throws(()=>ledger.recordOutcome('intent-1',{status:'uncertain',reasonCode:'semantic_mismatch',retryAllowed:true}),{code:'INVALID_INPUT'});
 assert.throws(()=>ledger.recordOutcome('intent-1',{status:'uncertain',response:{balance:999}}),{code:'INVALID_INPUT'});
 ledger.recordOutcome('intent-1',{status:'uncertain',reasonCode:'semantic_mismatch',retryAllowed:false});
 assert.throws(()=>ledger.markDispatching('intent-1',{retry:true}),{code:'INVALID_TRANSITION'});
 assert.throws(()=>ledger.recordOutcome('intent-1',{status:'uncertain',reasonCode:'network_error',retryAllowed:true}),{code:'INVALID_TRANSITION'});
});

test('STOP callback blocks dispatch before a new durable dispatch record',t=>{
 let stopped=false;const ledger=open(fixture(t),{isStopped:()=>stopped});t.after(()=>ledger.close());ledger.prepareIntent(input());stopped=true;
 assert.throws(()=>ledger.markDispatching('intent-1'),{code:'STOPPED'});assert.equal(ledger.snapshot().sequence,1);assert.equal(ledger.getIntent('intent-1').status,'prepared');
});

test('STOP appearing during the durability barrier leaves an unresolved record and no dispatch permission',t=>{
 let checks=0;const ledger=open(fixture(t),{isStopped:()=>++checks>1});t.after(()=>ledger.close());ledger.prepareIntent(input());
 assert.throws(()=>ledger.markDispatching('intent-1'),{code:'STOPPED'});assert.equal(ledger.getIntent('intent-1').status,'dispatching');assert.equal(ledger.snapshot().sequence,2);
});

test('crash tail is preserved and refused without silently truncating journal evidence',t=>{
 const dir=fixture(t),ledger=open(dir);ledger.prepareIntent(input());ledger.close();
 fs.appendFileSync(path.join(dir,'execution-journal.jsonl'),'{"sequence":2');const before=raw(dir);
 assert.throws(()=>open(dir),{code:'JOURNAL_INCOMPLETE'});assert.equal(raw(dir),before);
 assert.equal(fs.existsSync(path.join(dir,'execution-owner.lock')),false,'failed open releases only its own lease');
});

test('hash corruption, sequence edits, duplicate preparation and wrong scope are rejected',t=>{
 const dir=fixture(t),ledger=open(dir);ledger.prepareIntent(input());ledger.close();const original=JSON.parse(raw(dir));
 const changed=structuredClone(original);changed.event.intent.bodyJson=changed.event.intent.bodyJson.replace('0.45','0.95');assert.throws(()=>replayJournal(JSON.stringify(changed)+'\n'),{code:'JOURNAL_CORRUPT'});
 const skipped=rehash({...original,sequence:2});assert.throws(()=>replayJournal(JSON.stringify(skipped)+'\n'),{code:'JOURNAL_CORRUPT'});
 const duplicate=rehash({...original,sequence:2,previous:original.hash});assert.throws(()=>replayJournal(raw(dir)+JSON.stringify(duplicate)+'\n'),{code:'INTENT_CONFLICT'});
 assert.throws(()=>replayJournal(raw(dir),{scope:'different-tournament'}),{code:'SCOPE_MISMATCH'});
});

test('projection replacement is atomic and restart rebuilds it from the authoritative journal',t=>{
 const dir=fixture(t),ledger=open(dir);ledger.prepareIntent(input());ledger.close();
 const projection=path.join(dir,'execution-state.json');fs.writeFileSync(projection,'interrupted-or-stale-projection');
 const restarted=open(dir,{runId:'restart'});t.after(()=>restarted.close());assert.equal(JSON.parse(fs.readFileSync(projection)).intents['intent-1'].status,'prepared');
 atomicJson(path.join(dir,'standalone.json'),{value:1});atomicJson(path.join(dir,'standalone.json'),{value:2});
 assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir,'standalone.json'))),{value:2});assert.equal(fs.readdirSync(dir).some(x=>x.endsWith('.tmp')),false);
});

test('one owner holds the runtime even when another caller changes scope or run identity',t=>{
 const dir=fixture(t),lease=acquireOwnerLease({dir,ownerId:'codex',runId:'run-1',scope});
 assert.throws(()=>acquireOwnerLease({dir,ownerId:'researcher',runId:'run-2',scope:'another-scope'}),{code:'OWNER_LOCKED'});
 assert.equal(JSON.parse(fs.readFileSync(lease.file)).pid,process.pid);lease.release();lease.release();
 const replacement=acquireOwnerLease({dir,ownerId:'codex',runId:'run-2',scope});replacement.release();
});

test('a stale or foreign lock is never stolen and a changed owner lock is never removed',t=>{
 const dir=fixture(t),file=path.join(dir,'execution-owner.lock');fs.writeFileSync(file,JSON.stringify({pid:2147483647,ownerId:'old-owner'}));
 assert.throws(()=>open(dir),{code:'OWNER_LOCKED'});assert.equal(JSON.parse(fs.readFileSync(file)).ownerId,'old-owner');fs.unlinkSync(file);
 const lease=acquireOwnerLease({dir,ownerId:'codex',runId:'run-1',scope});const other={...lease.identity,token:'different-token'};fs.writeFileSync(file,JSON.stringify(other));
 assert.throws(()=>lease.release(),{code:'OWNER_CHANGED'});assert.deepEqual(JSON.parse(fs.readFileSync(file)),other);
});

test('credentials, raw account fields and non-JSON values cannot enter intent persistence',t=>{
 const ledger=open(fixture(t));t.after(()=>ledger.close());
 const credentials=input();credentials.body.authorization='secret';assert.throws(()=>ledger.prepareIntent(credentials),{code:'INVALID_INPUT'});
 assert.throws(()=>ledger.prepareIntent({...input(),attribution:{balance:999}}),{code:'INVALID_INPUT'});
 assert.throws(()=>ledger.prepareIntent({...input(),headers:{Authorization:'secret'}}),{code:'INVALID_INPUT'});
 assert.throws(()=>canonicalJson({value:undefined}),{code:'INVALID_INPUT'});assert.throws(()=>canonicalJson({value:NaN}),{code:'INVALID_INPUT'});
 assert.equal(ledger.snapshot().sequence,0);
});

test('an acknowledged order cannot become retryable through a later uncertain outcome',t=>{
 const ledger=open(fixture(t));t.after(()=>ledger.close());ledger.prepareIntent(input());ledger.markDispatching('intent-1');
 ledger.recordOutcome('intent-1',{status:'acknowledged',orderId:'official-1'});
 ledger.recordOutcome('intent-1',{status:'uncertain',reasonCode:'reconciliation_unavailable',retryAllowed:false});
 assert.throws(()=>ledger.recordOutcome('intent-1',{status:'uncertain',reasonCode:'network_error',retryAllowed:true}),{code:'INVALID_TRANSITION'});
 assert.throws(()=>ledger.markDispatching('intent-1',{retry:true}),{code:'INVALID_TRANSITION'});
});

test('order tournament must match lease scope before any preparation is persisted',t=>{
 const ledger=open(fixture(t));t.after(()=>ledger.close());const foreign=input();foreign.body.tournamentId='different-tournament';
 assert.throws(()=>ledger.prepareIntent(foreign),{code:'SCOPE_MISMATCH'});assert.equal(ledger.snapshot().sequence,0);
});

test('fsync failure denies dispatch permission and poisons the handle until restart',t=>{
 const dir=fixture(t),ledger=open(dir);ledger.prepareIntent(input());
 const sync=t.mock.method(fs,'fsyncSync',()=>{const error=Error('injected storage failure');error.code='EIO';throw error;});
 try{assert.throws(()=>ledger.markDispatching('intent-1'),{code:'EIO'});assert.throws(()=>ledger.markDispatching('intent-1'),{code:'DURABILITY_FAILURE'});}finally{sync.mock.restore();}
 ledger.close();const restarted=open(dir,{runId:'restart'});t.after(()=>restarted.close());
 assert.equal(restarted.getIntent('intent-1').status,'dispatching');assert.throws(()=>restarted.markDispatching('intent-1',{retry:true}),{code:'INVALID_TRANSITION'});
});

test('projection failure after append denies dispatch and replay recovers the durable event',t=>{
 const dir=fixture(t),ledger=open(dir);ledger.prepareIntent(input());
 const rename=t.mock.method(fs,'renameSync',()=>{const error=Error('injected projection failure');error.code='EIO';throw error;});
 try{assert.throws(()=>ledger.markDispatching('intent-1'),{code:'EIO'});}finally{rename.mock.restore();}
 assert.equal(replayJournal(raw(dir)).intents['intent-1'].status,'dispatching');
 assert.throws(()=>ledger.markDispatching('intent-1'),{code:'DURABILITY_FAILURE'});ledger.close();
 const restarted=open(dir,{runId:'restart'});t.after(()=>restarted.close());assert.equal(JSON.parse(fs.readFileSync(restarted.projectionFile)).intents['intent-1'].status,'dispatching');
});
