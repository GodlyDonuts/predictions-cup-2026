import test from 'node:test';import assert from 'node:assert/strict';
import {RealtimeBooks} from '../live-paper/realtime-books.mjs';import {makerPushQuoteGuardV1} from './maker-push-quote-guard-v1.mjs';
import {scope,start,at,book,batch,snapshot} from '../feed/test/helpers.mjs';
function fixture(){let now=start;const bridge=new RealtimeBooks({tournamentId:scope.tournamentId,markets:[{marketId:'277',exchangeId:'701'}]},{now:()=>now});
  const subscribe=connectionId=>bridge.subscribed({marketId:'277',connectionId,receivedAt:at(now-start)});
  const recover=connectionId=>{const t=bridge.beginSnapshot({marketId:'277',connectionId});assert.equal(bridge.completeSnapshot(t,{endpoint:t.endpoint,
    requestStartedAt:at(now-start),receivedAt:at(now-start),data:snapshot([book(701)])}),true);};
  const quotes=[{quoteId:'q1',marketId:'277',exchangeId:'701',sourceBinding:{stream:{connectionId:'c1',epoch:1}}},{quoteId:'other',marketId:'278',exchangeId:'702'}];
  const guard=()=>makerPushQuoteGuardV1({view:bridge.view('277'),at:at(now-start),openQuotes:quotes});
  subscribe('c1');recover('c1');return{bridge,quotes,guard,subscribe,recover,advance:ms=>now=start+ms,
    send:(payload,connectionId='c1')=>bridge.ingest({marketId:'277',connectionId,event:'market_batch',payload,receivedAt:at(now-start)})};}
test('healthy current book supports paper only; original clocks retained and unchanged quotes keep queue',()=>{
  const f=fixture(),r=f.guard();assert.equal(r.paperProposalsAllowed,true);assert.deepEqual(r.withdrawQuoteIds,[]);
  assert.equal(r.book.receivedAt,at(0));assert.equal(Object.hasOwn(r.book,'requestStartedAt'),false);assert.equal(r.executionAuthority,false);
  f.advance(1);f.send(batch(1,0,[book(701,110,1)]));assert.deepEqual(f.guard().withdrawQuoteIds,[]);
});
test('topic gap requests affected cancellations without releasing money or touching unrelated markets',()=>{
  const f=fixture();f.send(batch(10,0,[book(701,110)]));f.send(batch(20,15,[book(701,120)]));const r=f.guard();
  assert.equal(r.paperProposalsAllowed,false);assert.deepEqual(r.withdrawQuoteIds,['q1']);assert.ok(r.reasons.includes('GAP'));
  assert.equal(r.cashReservationReleaseAllowed,false);assert.equal(r.cancellationConfirmed,false);
});
test('recovered new epoch cannot keep old epoch quotes supported; old callbacks cannot restore them',()=>{
  const f=fixture();f.bridge.disconnected({marketId:'277',connectionId:'c1'});f.subscribe('c2');f.recover('c2');
  assert.equal(f.guard().paperProposalsAllowed,true);assert.deepEqual(f.guard().withdrawQuoteIds,['q1']);
  assert.equal(f.send(batch(30,0,[book(701,130)]),'c1'),false);assert.deepEqual(f.guard().withdrawQuoteIds,['q1']);
});
test('quiet duplicate messages do not renew book freshness; no executable depth stays unavailable',()=>{
  const f=fixture();f.advance(9000);f.send(batch(1,0,[book(701)]));f.advance(10000);
  assert.equal(f.guard().paperProposalsAllowed,false);assert.deepEqual(f.guard().withdrawQuoteIds,['q1']);
  const g=fixture();g.send(batch(1,0,[book(701,101,0,{bids:[],asks:[]})]));assert.equal(g.guard().paperProposalsAllowed,false);
});
test('unavailable host view requests scoped withdrawal with no invented observation',()=>{
  const f=fixture();const r=makerPushQuoteGuardV1({view:null,scope:{marketId:'277',exchangeId:'701'},at:at(0),openQuotes:f.quotes});
  assert.deepEqual(r.withdrawQuoteIds,['q1']);assert.equal(r.viewAvailable,false);assert.equal(r.book,null);
  assert.equal(r.streamBinding.viewHash,null);assert.equal(r.cashReservationReleaseAllowed,false);
  assert.throws(()=>makerPushQuoteGuardV1({view:null,at:at(0),openQuotes:f.quotes}),/UNAVAILABLE_SCOPE/);
});
