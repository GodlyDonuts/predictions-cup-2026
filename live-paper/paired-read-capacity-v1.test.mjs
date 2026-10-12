import test from 'node:test';
import assert from 'node:assert/strict';
import {pairedReadCapacityDelayV1 as delay} from './paired-read-capacity-v1.mjs';
const now=100000;
const rows=(n,t)=>Array.from({length:n},()=>({admittedAt:new Date(t).toISOString()}));
test('full packet fits without increasing the rolling budget',()=>{
 assert.equal(delay(rows(49,now),now),0);
 assert.equal(delay(rows(50,now),now),60001);
});
test('waits until enough admissions expire, preserving inclusive boundary',()=>{
 const xs=[...rows(10,now-60000),...rows(50,now-10000)];
 assert.equal(delay(xs,now),50001);
 assert.equal(delay(xs,now+50001),0);
});
test('invalid and future timings cannot justify capacity',()=>{
 assert.throws(()=>delay(rows(1,now+1),now),/CLOCK/);
 assert.throws(()=>delay([{admittedAt:'invalid'}],now),/CLOCK/);
});

