import test from 'node:test';import assert from 'node:assert/strict';import {streamActivity,selectActivePairs,relativeFlowSide,currentPriceCandidates} from './research/stream-activity.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const at=n=>new Date(Date.parse('2026-10-14T00:00:00Z')+n*1000).toISOString(),pairs=[{marketId:'101',exchangeId:'101'}];
const row=(n,options={})=>({event:{kind:'broadcast',source:'official_sig_realtime',event:'market_batch',marketId:'101',receivedAt:at(n),duplicate:false,gap:false,payload:{resyncRequired:false,books:[{exchangeId:'101',asOf:{at:at(n),sequence:n},bids:[{price:0.45,quantity:100}],asks:[{price:0.46,quantity:100}]}],trades:[],...options}}});
const trade={id:'trade',sequence:1,exchangeId:'101',marketId:'101',quantity:100,price:0.455,executedAt:at(1)};
test('stream activity deduplicates native trades and excludes old executions from recent ranking',()=>{
 const r=row(2,{trades:[trade,trade]}),c=streamActivity([r],at(3),pairs)[0];assert.equal(c.recentShares,'100');assert.equal(c.recentTrades,1);assert.equal(c.activityScore,0.5);assert.equal(c.meanObservedTradeLagSeconds,1);assert.equal(c.spread,'0.010000');
 const old=streamActivity([r,row(200)],at(201),pairs)[0];assert.equal(old.observedTrades,1);assert.equal(old.recentTrades,0);assert.equal(old.activityScore,null);
});
test('stale books, gaps and conflicting native trade facts cannot produce an eligible rank',()=>{
 const r=row(2,{trades:[trade]});assert.equal(streamActivity([r],at(13),pairs)[0].eligible,false);
 const gap=row(3);gap.event.gap=true;assert.equal(streamActivity([r,gap],at(4),pairs)[0].eligible,false);
 gap.event.payload.books=[];assert.equal(streamActivity([r,gap],at(4),pairs)[0].eligible,false);
 assert.throws(()=>streamActivity([r,row(3,{trades:[{...trade,quantity:101}]})],at(4),pairs),/TRADE_CONFLICT/);
 assert.throws(()=>streamActivity([row(2,{trades:[{...trade,executedAt:at(3)}]})],at(4),pairs),/TRADE_FACT/);
});
test('trade-count selection rejects stale candidates and does not let queue turnover override frequency',()=>{
 const candidates=[{exchangeId:'1',eligible:true,recentTrades:2,activityScore:100},{exchangeId:'2',eligible:true,recentTrades:8,activityScore:1},{exchangeId:'3',eligible:false,recentTrades:100,activityScore:1000}];
 assert.equal(selectActivePairs(candidates)[0].exchangeId,'2');assert.throws(()=>selectActivePairs(candidates,5),/SELECTION_COUNT/);assert.deepEqual(selectActivePairs([{exchangeId:'4',eligible:true,recentTrades:1}]),[]);
});
test('price-relative flow never classifies with a future, stale or post-match book',()=>{
 const b=row(0).event.payload.books[0],t={...trade,price:0.45,sequence:2};assert.equal(relativeFlowSide(t,[{book:b}]),'bid');assert.equal(relativeFlowSide({...t,price:0.46},[{book:b}]),'ask');
 assert.equal(relativeFlowSide(t,[{book:row(2).event.payload.books[0]}]),'unknown');assert.equal(relativeFlowSide({...t,sequence:0},[{book:b}]),'unknown');assert.equal(relativeFlowSide({...t,executedAt:at(10)},[{book:b}]),'unknown');
 const floating={...b,bids:[{price:0.545,quantity:100}],asks:[{price:0.55,quantity:100}]};assert.equal(relativeFlowSide({...t,price:0.5449999999999999},[{book:floating}]),'bid');assert.throws(()=>relativeFlowSide({...t,price:0.547},[{book:floating}]),/PRICE_TICK/);
});
test('balanced-flow selection rejects one-sided high activity',()=>{
 const c=[{exchangeId:'1',eligible:true,recentTrades:100,balancedFlowScore:0},{exchangeId:'2',eligible:true,recentTrades:4,balancedFlowScore:1}];assert.equal(selectActivePairs(c,1,'balancedFlow')[0].exchangeId,'2');
});
test('current-price selector cannot combine prints from expired bid and ask regimes',()=>{
 const events=[row(0),row(5,{trades:[{...trade,id:'bid',sequence:4,price:.45,executedAt:at(4)}]}),row(10,{trades:[{...trade,id:'ask',sequence:9,price:.46,executedAt:at(9)}]})];
 const chain=list=>{let previous='';return list.map((r,i)=>{const body={sequence:i+1,previous,event:r.event},sealed={...body,hash:payloadHash(body)};previous=sealed.hash;return sealed;});};
 const valid=chain(events);assert.ok(currentPriceCandidates(valid,at(10),pairs)[0].balancedFlowScore>0);
 const moved=row(11);moved.event.payload.books[0].bids[0].price=.455;moved.event.payload.books[0].asks[0].price=.465;
 const c=currentPriceCandidates(chain([...events,moved]),at(11),pairs)[0];assert.ok(c.rawBalancedFlowScore>0);assert.equal(c.balancedFlowScore,0);assert.equal(c.currentPriceRegimeCompatibleTrades,0);assert.deepEqual(selectActivePairs([c],1,'balancedFlow'),[]);
 assert.throws(()=>currentPriceCandidates([{...valid[0],hash:'edited'}],at(10),pairs),/CHAIN/);
});
