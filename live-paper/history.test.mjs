import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';
import {historyEndpoint,allowedHistoryEndpoint,normalizeHistory,historicalReference} from './history.mjs';import {ReadClient} from './runtime.mjs';
const policy=JSON.parse(fs.readFileSync(new URL('./dip-policy.json',import.meta.url))),tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
const spec={exchangeId:'1066',marketId:'377'},received='2026-10-02T06:30:01Z',started='2026-10-02T06:30:00Z';
const candle={time:'2026-10-02T06:28:00Z',open:.5,high:.51,low:.49,close:.5,vwap:.5,volume:100,tradeCount:2};
const response=()=>({endpoint:historyEndpoint(spec.exchangeId,started),requestStartedAt:started,receivedAt:received,data:{exchangeId:'1066',marketId:'377',resolution:'1m',from:'2026-10-02T06:00:00Z',to:started,candles:[candle],coverage:{complete:true,projectedThroughSequence:100}}});
test('history is tournament scoped, sparse and closed; future/in-progress or duplicate candles fail',()=>{
 const r=response(),h=normalizeHistory(r,spec);assert.equal(h.candles.length,1);assert.equal(h.executable,false);assert.equal(h.sparse,true);
 for(const change of [r=>r.data.marketId='999',r=>r.data.candles.push(candle),r=>r.data.candles[0]={...candle,time:started},r=>r.data.candles[0]={...candle,vwap:.9}]){const bad=structuredClone(r);change(bad);assert.throws(()=>normalizeHistory(bad,spec));}
 const empty=response();empty.data.candles=[];assert.deepEqual(normalizeHistory(empty,spec).candles,[]);
 const roundoff=response();roundoff.data.candles[0]={...candle,open:.5,low:.5,high:.5,close:.5,vwap:.5000000000000001};assert.equal(normalizeHistory(roundoff,spec).candles.length,1);
});
test('history request shares serialized admission and rejects unscoped queries',async()=>{
 const endpoint=historyEndpoint('1066',started);assert.equal(allowedHistoryEndpoint(endpoint),true);assert.equal(allowedHistoryEndpoint(endpoint.replace('&resolution=1m','')),false);
 let now=0,reads=0;const api=new ReadClient('invented-test-key',{now:()=>now,sleepFn:async ms=>{now+=ms;},fetchFn:async()=>{reads++;return {ok:true,status:200,json:async()=>({})};}});await Promise.all([api.request(endpoint),api.request(endpoint)]);assert.equal(reads,2);assert.ok(now>=6000);await assert.rejects(api.request('/exchanges/1066/price-history'),/allowlist/);
});
test('reference is lagged, needs historical span, and refuses future receipt or stale benchmark',()=>{
 const at='2026-10-02T06:30:00Z',now=Date.parse(at),books=Array.from({length:9},(_,i)=>({at:new Date(now-3600000+i*400000).toISOString(),receivedAt:new Date(now-3600000+i*400000).toISOString(),mid:.5}));
 const r=historicalReference(null,books,at,policy);assert.equal(r.ready,true);assert.equal(r.center,.5);
 const plus=[...books,{at:'2026-10-02T06:40:00Z',receivedAt:'2026-10-02T06:40:00Z',mid:.99}];assert.deepEqual(historicalReference(null,plus,at,policy),r);
 assert.equal(historicalReference(null,books.slice(0,2),at,policy).ready,false);
 assert.equal(historicalReference(null,books,'2026-10-02T08:30:00Z',policy).reason,'historical_reference_stale');
});
