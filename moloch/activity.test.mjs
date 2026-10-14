import test from 'node:test';import assert from 'node:assert/strict';import {ActivityStudy} from './activity.mjs';import {payloadHash} from '../execution/ledger.mjs';
const at=n=>new Date(Date.parse('2026-10-13T22:00:00Z')+n*1000).toISOString();
const setup=()=>{const s=new ActivityStudy();let sequence=0,previous='';return{s,send(payload,n,{gap=false}={}){const event={kind:'broadcast',source:'official_sig_realtime',marketId:'1',event:'market_batch',receivedAt:at(n),gap,payload},body={sequence:++sequence,previous,event},row={...body,hash:payloadHash(body)};previous=row.hash;s.row(row);return row;}};};
const book=(n,seq,bid=.4,ask=.42)=>({exchangeId:'2',asOf:{sequence:seq,at:at(n)},bids:[{price:bid,quantity:100}],asks:[{price:ask,quantity:200}]});
const batch=(books,trades=[])=>({books,trades,resyncRequired:false});
const trade=(n,id='trade')=>({id,sequence:10,exchangeId:'2',marketId:'1',tournamentId:'cup',price:.4,quantity:50,executedAt:at(n)});
test('future book in a trade batch cannot invent a prior quote or a wallet profit',()=>{
  const {s,send}=setup();send(batch([book(6,11)],[trade(4)]),6);send(batch([book(40,12,.39,.41)]),40);const r=s.report(at(40))[0];assert.equal(r.observedTrades,1);assert.equal(r.conditionalMarkouts,0);assert.equal(r.meanConditionalLiquidationDeltaPerShare,null);assert.equal(r.bidCompatibleVolume,0);assert.equal(r.askCompatibleVolume,0);
});
test('conditional markout uses a prior observable regime and a genuinely later executable book',()=>{
  const {s,send}=setup();send(batch([book(0,1)]),0);send(batch([book(6,11)],[trade(4)]),6);send(batch([book(20,12,.395,.415)]),20);assert.equal(s.report(at(20))[0].conditionalMarkouts,0);
  send(batch([book(35,13,.39,.41)]),35);const r=s.report(at(35))[0];assert.equal(r.conditionalMarkouts,1);assert.equal(r.meanConditionalLiquidationDeltaPerShare,-.01);assert.equal(r.gapObserved,false);assert.equal(r.bidCompatibleVolume,50);assert.equal(r.askCompatibleVolume,0);assert.equal(r.bothSideQueueTurnover,0);
});
test('a revision gap prevents quoted admission but retains activity priority for authoritative recovery',()=>{
  const {s,send}=setup();send(batch([book(0,1)]),0);send(batch([book(6,11)],[trade(4)]),6);send(batch([book(35,12,.45,.47)]),35,{gap:true});const r=s.report(at(35))[0];assert.equal(r.gapObserved,true);assert.equal(r.eligibleForFurtherReconciliation,false);assert.equal(r.conditionalMarkouts,0);assert.equal(r.quoteReadyFromObservation,false);assert.equal(r.authoritativeResyncRequired,true);assert.ok(r.reconciliationPriority>0);
});
test('study rejects event edits in a preserved hash chain',()=>{
  const a=setup();const row=a.send(batch([book(0,1)]),0);const s=new ActivityStudy();assert.throws(()=>s.row({...row,event:{}}),/CHAIN/);
});

test('post-gap observations recover local price compatibility while retaining historical warnings',()=>{
 const {s,send}=setup();send(batch([book(0,1)]),0);send(batch([book(5,3)]),5,{gap:true});send(batch([book(10,11)],[trade(9,'post-gap-buy')]),10);const sell={...trade(12,'post-gap-sell'),sequence:12,price:.42};send(batch([book(13,13)],[sell]),13);send(batch([book(45,14)]),45);const r=s.report(at(45))[0];assert.equal(r.gapObserved,true);assert.equal(r.quoteReadyFromObservation,false);assert.equal(r.eligibleForFurtherReconciliation,false);assert.equal(r.bidCompatibleVolume,50);assert.equal(r.askCompatibleVolume,50);assert.equal(r.conditionalMarkouts,2);assert.ok(r.roundTripReconciliationPriority>0);
});

test('one-tick two-sided joins remain discoverable without crediting one-sided traffic',()=>{
 const {s,send}=setup();const narrow=(n,seq)=>book(n,seq,.4,.405);send(batch([narrow(0,1)]),0);send(batch([narrow(5,11)],[trade(4,'bid')]),5);assert.equal(s.report(at(5))[0].roundTripReconciliationPriority,0);send(batch([narrow(10,13)],[{...trade(9,'ask'),price:.405,sequence:12}]),10);const r=s.report(at(10))[0];assert.ok(r.roundTripReconciliationPriority>0);assert.equal(r.insideRoomTicks,0);assert.equal(r.quoteReadyFromObservation,false);
});
