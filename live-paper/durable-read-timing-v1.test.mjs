import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import test from 'node:test';import assert from 'node:assert/strict';import {appendReadTimingV1} from './durable-read-timing-v1.mjs';
const event={schemaVersion:1,kind:'orders',marketId:null,queuedAt:'2026-10-07T00:00:00Z',admittedAt:null,finishedAt:'2026-10-07T00:00:01Z',queueWaitMs:0,admissionWaitMs:1000,headersMs:null,bodyMs:null,serviceMs:null,totalMs:1000,statusCode:null,outcome:'blocked'};
test('Windows sharing conflict before open retries without losing or duplicating the event',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-timing-')),file=path.join(dir,'timing.jsonl');let opens=0,pauses=0;const io={...fs,openSync(...a){if(++opens<3)throw Object.assign(Error('private'),{code:'EBUSY'});return fs.openSync(...a);}};
 appendReadTimingV1({file,diagnosticFile:path.join(dir,'failure.json'),event,filesystem:io,pause:()=>pauses++});assert.equal(opens,3);assert.equal(pauses,2);assert.equal(fs.readFileSync(file,'utf8'),JSON.stringify(event)+'\n');
});
test('longer transient sharing lock uses bounded exponential open retries',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-timing-backoff-')),file=path.join(dir,'timing.jsonl'),delays=[];let opens=0;const io={...fs,openSync(...a){if(++opens<6)throw Object.assign(Error('private'),{code:'EBUSY'});return fs.openSync(...a);}};
 appendReadTimingV1({file,diagnosticFile:path.join(dir,'failure.json'),event,filesystem:io,pause:ms=>delays.push(ms)});assert.equal(opens,6);assert.deepEqual(delays,[20,40,80,160,320]);assert.equal(fs.readFileSync(file,'utf8'),JSON.stringify(event)+'\n');
});
test('persistent open lock fails closed after finite retries without writing a payload',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-timing-lock-')),file=path.join(dir,'timing.jsonl'),diagnosticFile=path.join(dir,'failure.json'),delays=[];let opens=0,writes=0;const io={...fs,openSync(){opens++;throw Object.assign(Error('private'),{code:'EBUSY'});},writeSync(){writes++;throw Error('must not write');}};
 assert.throws(()=>appendReadTimingV1({file,diagnosticFile,event,filesystem:io,pause:ms=>delays.push(ms)}),e=>e.code==='EBUSY'&&e.durableRecordingFailed);assert.equal(opens,7);assert.equal(writes,0);assert.deepEqual(delays,[20,40,80,160,320,500]);assert.equal(JSON.parse(fs.readFileSync(diagnosticFile,'utf8')).bytesWritten,0);assert.equal(JSON.parse(fs.readFileSync(diagnosticFile,'utf8')).code,'EBUSY');assert.equal(fs.existsSync(file),false);
});
test('partial writes fail once, retain a sanitized reason and never retry the payload',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-timing-partial-')),file=path.join(dir,'timing.jsonl'),diagnosticFile=path.join(dir,'failure.json');let calls=0;const io={...fs,writeSync(fd,b,offset,length){if(++calls===1)return fs.writeSync(fd,b,offset,10);throw Object.assign(Error('private account data'),{code:'EIO'});}};
 assert.throws(()=>appendReadTimingV1({file,diagnosticFile,event,filesystem:io}),e=>e.durableRecordingFailed);assert.equal(calls,2);assert.equal(fs.statSync(file).size,10);const d=fs.readFileSync(diagnosticFile,'utf8');assert.equal(JSON.parse(d).bytesWritten,10);assert.ok(!d.includes('private'));
});
test('invalid instrumentation records its specific cause without copying extra sensitive fields',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-timing-invalid-')),diagnosticFile=path.join(dir,'failure.json'),file=path.join(dir,'timing.jsonl');let opens=0;const io={...fs,openSync(){opens++;throw Object.assign(Error('private'),{code:'EBUSY'});}};assert.throws(()=>appendReadTimingV1({file,diagnosticFile,event:{...event,Authorization:'private token'},filesystem:io}));assert.equal(opens,0);assert.equal(JSON.parse(fs.readFileSync(diagnosticFile)).code,'TIMING_SCHEMA');assert.ok(!fs.readFileSync(diagnosticFile,'utf8').includes('private'));
});
