import test from 'node:test';
import assert from 'node:assert/strict';
import {validateUniverse,priceBatch,priceEndpoint,normalizePrices,updateCoverage} from './breadth.mjs';
import {ReadClient} from './runtime.mjs';
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
const u=validateUniverse({tournamentId:tid,exchanges:Array.from({length:237},(_,i)=>({exchangeId:String(i+1),marketId:String(i+1000),option:'YES'}))},tid);
const batch=priceBatch(u,0);
function response(){return {endpoint:priceEndpoint(batch,tid),requestStartedAt:'2026-10-02T01:00:00Z',receivedAt:'2026-10-02T01:00:02Z',data:{data:batch.map(x=>({...x,latestPrice:.4,bestBid:.39,bestAsk:.41,spread:.02})),missingIds:[]}};}
test('universe rotation is bounded, tournament scoped and exhaustive',()=>{
 assert.deepEqual([0,1,2].map(i=>priceBatch(u,i).length),[100,100,37]);
 assert.equal(new Set([0,1,2].flatMap(i=>priceBatch(u,i).map(x=>x.exchangeId))).size,237);
 assert.deepEqual(priceBatch(u,3),batch);
 assert.throws(()=>validateUniverse({...u,tournamentId:'wrong'},tid));
 assert.throws(()=>validateUniverse({...u,exchanges:[u.exchanges[0],u.exchanges[0]]},tid));
});
test('bulk coverage rejects incomplete, duplicate and mismapped exchange identity',()=>{
 for(const mutate of [r=>r.data.data.pop(),r=>r.data.data.push(r.data.data[0]),r=>r.data.data[0].marketId='9',r=>r.data.data[0].option='NO',r=>r.data.data[0].bestBid='0.39',r=>r.data.data[0].latestPrice=NaN,r=>r.data.missingIds.push('999'),r=>r.endpoint=r.endpoint.replace(tid,'wrong')]){
  const r=response();mutate(r);assert.throws(()=>normalizePrices(r,batch,u));
 }
 const r=response();r.data.data[0].bestAsk=null;r.data.data[0].spread=null;
 const s=normalizePrices(r,batch,u);assert.equal(s.depthAvailable,false);assert.equal(s.sourceTimestamp,null);assert.equal(s.executable,false);
});
test('missing IDs remove cached quotes, collection times are retained per exchange',()=>{
 const s=normalizePrices(response(),batch,u),old=updateCoverage(null,s,u,0);
 const r=response();r.data.missingIds.push(r.data.data.shift().exchangeId);r.receivedAt='2026-10-02T01:02:00Z';
 const next=updateCoverage(old,normalizePrices(r,batch,u),u,0);
 assert.equal(Object.keys(old.quotes).length,100);assert.equal(Object.keys(next.quotes).length,99);assert.equal(next.quotes['1'],undefined);assert.equal(next.quotes['2'].receivedAt,r.receivedAt);
 assert.equal(next.nextBatch,1);assert.equal(next.executable,false);
});
test('bulk admission rejects unscoped, overlarge and duplicate batches and shares GET pacing',async()=>{
 let now=0;const times=[];const api=new ReadClient('FAKE',{now:()=>now,sleepFn:async ms=>{now+=ms;},fetchFn:async()=>{times.push(now);return {ok:true,json:async()=>({})};}});
 for(const endpoint of ['/exchanges/prices?ids=1',priceEndpoint([...batch,batch[0]],tid),priceEndpoint([batch[0],batch[0]],tid)])await assert.rejects(api.request(endpoint),/allowlist/);
 await Promise.all([api.request(priceEndpoint(batch,tid)),api.request('/tournaments/midterm-elections')]);assert.deepEqual(times,[0,6000]);api.close();
});
