import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import test from 'node:test';import assert from 'node:assert/strict';
import {DiagnosticAccountClientV3,safeFetchFailureV1} from './diagnostic-account-client-v3.mjs';
const networkBudget={schemaVersion:4,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,executionOwner:'Codex',handoffId:'human-to-baal-offline-test',ackCommentId:1,writesEnabled:true,authorityKind:'direct_human_assignment',assignmentCommentId:2},coordination={kind:'baal_single_client_rolling_policy_v1',readCompletionGapMs:100,writeCompletionGapMs:2500,maxReadsPerMinute:60,maxWritesPerMinute:24};
test('same serialized client exposes safe connection cause and preserves recording failures',async t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-network-diag-'));let fatal=false,clock=100000,calls=0;
 const c=new DiagnosticAccountClientV3('synthetic-offline-only',{diagnosticFile:path.join(dir,'diagnostic.json'),admissionFile:path.join(dir,'admissions.json'),networkBudget,coordination,now:()=>clock,sleepFn:async n=>clock+=n,fetchFn:async()=>{calls++;if(fatal)throw Object.assign(Error('private body secret'),{code:'REST_RECEIPT_START_RECORDING_FAILED',durableRecordingFailed:true,mayHaveExecuted:false});throw Object.assign(new TypeError('private credentials'),{cause:{code:'UND_ERR_CONNECT_TIMEOUT'}});}});t.after(()=>c.close());
 await assert.rejects(c.request('/tournaments/midterm-elections'),e=>e.recoverable===true);let d=JSON.parse(fs.readFileSync(c.diagnosticFile));assert.equal(d.failures[0].code,'UND_ERR_CONNECT_TIMEOUT');assert.ok(!JSON.stringify(d).includes('private'));fatal=true;
 await assert.rejects(c.request('/tournaments/midterm-elections'),e=>e.fatal===true&&e.durableRecordingFailed===true);assert.equal(calls,2);assert.equal(JSON.parse(fs.readFileSync(path.join(dir,'admissions.json'))).reads.length,2);
 assert.equal(safeFetchFailureV1({code:'SECRET_TOKEN_VALUE',message:'private'}).code,'UNCLASSIFIED_FETCH_FAILURE');
});

