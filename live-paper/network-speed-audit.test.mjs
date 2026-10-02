import test from 'node:test';import assert from 'node:assert/strict';import {auditNetworkSpeed} from './network-speed-audit.mjs';
const iso=n=>new Date(n).toISOString(),row=(admit,finish=admit,queue=admit)=>({schemaVersion:1,kind:'book',marketId:'1',queuedAt:iso(queue),admittedAt:iso(admit),finishedAt:iso(finish),queueWaitMs:0,admissionWaitMs:admit-queue,headersMs:finish-admit,bodyMs:0,serviceMs:finish-admit,totalMs:finish-queue,statusCode:200,outcome:'success'}),raw=rows=>rows.map(JSON.stringify).join('\n')+'\n';
test('only completed pre-cutoff and new post-cutoff requests enter their separate windows',()=>{
 const a=auditNetworkSpeed(raw([row(1000),row(9000,11000,8000),row(12000,12500,10000),row(14000)]),iso(10000),{beforeWindowMs:5000});assert.equal(a.before.records,0);assert.equal(a.after.records,2);assert.equal(a.minimumCompletionGapMs,1500);assert.equal(a.maximumRequestsInInclusiveMinute,2);
});
test('inclusive rolling minute catches a boundary pair and incomplete trailing data is excluded',()=>{
 const a=auditNetworkSpeed(raw([row(0),row(60000),row(61001)])+'{',iso(0));assert.equal(a.maximumRequestsInInclusiveMinute,2);assert.equal(a.partialTailExcluded,true);assert.equal(a.after.records,3);assert.equal(a.networkRequests,0);
});
test('empty input remains unmeasured and unsafe metadata cannot pass the curated timing schema',()=>{
 const a=auditNetworkSpeed('',iso(0));assert.equal(a.minimumCompletionGapMs,null);assert.equal(a.firstNewAdmissionAt,null);assert.throws(()=>auditNetworkSpeed(raw([{...row(0),Authorization:'secret'}]),iso(0)));assert.throws(()=>auditNetworkSpeed('', 'invalid'));
});
