import test from 'node:test';import assert from 'node:assert/strict';
import {rankBaalOwnedExitReadHintsV1 as rank} from './baal-owned-exit-priority-v1.mjs';
const now=Date.parse('2026-10-06T21:00:00Z'),H='a'.repeat(64);
const slot=(ex,q,cost)=>({proof:{exchangeId:ex,marketId:ex,remainingOwnedQuantity:q,remainingLots:[{quantity:q,entryPrice:cost}]}});
const quote=(ex,ask)=>({exchangeId:ex,marketId:ex,option:'YES',bestAsk:ask,crossed:false,sourceHash:H,
 requestStartedAt:new Date(now-1000).toISOString(),receivedAt:new Date(now-500).toISOString()});
const args=()=>({scope:{tournamentId:'T'},universeHash:H,now,owned:[slot('1',2,'0.125000'),slot('2',29,'0.140000')],
 hints:{tournamentId:'T',universeHash:H,quoteCaptures:[quote('1',.85),quote('2',.85)]}});
test('larger quoted dollar profit gets scarce read capacity before a smaller equally profitable unit',()=>{
 const r=rank(args());assert.deepEqual(r.ranked.map(x=>x.exchangeId),['2','1']);
 assert.equal(r.ranked[0].upperBoundProfitWithoutDepth,'0.290000');assert.equal(r.depthVerified,false);assert.equal(r.orderEligible,false);
});
test('stale, duplicate and foreign quote cannot prioritize a financial read',()=>{
 for(const mutate of[a=>{a.hints.quoteCaptures[1].tournamentId='foreign';},a=>{a.hints.quoteCaptures[1].knownAt=new Date(now+1).toISOString();},a=>{a.hints.quoteCaptures[1].requestStartedAt=new Date(now-21000).toISOString();},a=>{a.hints.quoteCaptures.push({...a.hints.quoteCaptures[1]});}]){
  const a=args();mutate(a);assert.deepEqual(rank(a).ranked.map(x=>x.exchangeId),['1']);
 }
});
test('expensive first FIFO lot cannot be skipped or averaged into a profitable hint',()=>{
 const a=args();a.owned=[{proof:{exchangeId:'2',marketId:'2',remainingOwnedQuantity:29,
  remainingLots:[{quantity:1,entryPrice:'0.200000'},{quantity:28,entryPrice:'0.050000'}]}}];
 assert.equal(rank(a).ranked.length,0);
});
