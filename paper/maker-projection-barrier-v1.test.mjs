import test from 'node:test';import assert from 'node:assert/strict';
import {createMakerProjectionBarrierV1 as create,observeMakerDeadlineBookV1 as book,observeMakerProjectionPageV1 as page,
  makerProjectionRetirementV1 as report} from './maker-projection-barrier-v1.mjs';
const scope={tournamentId:'11111111-1111-4111-8111-111111111111',marketId:'11',exchangeId:'101'},at=n=>new Date(Date.parse('2026-10-12T23:20:00.000Z')+n*1000).toISOString();
const initial=()=>create({scope,activationAt:at(4),endAt:at(11)});
const b=(extra={})=>({...scope,sequence:'110',requestStartedAt:at(12),receivedAt:at(13),sourceAt:at(12),...extra});
const p=(extra={})=>({...scope,from:at(0),to:at(20),requestStartedAt:at(20),receivedAt:at(21),requestCursor:null,
  pagination:{hasMore:false,nextCursor:null},coverage:{complete:true,projectedThroughSequence:110},...extra});
test('complete lagged time-range page does not retire quote or release reservation',()=>{
  let s=book(initial(),b());s=page(s,p({coverage:{complete:true,projectedThroughSequence:90}}));
  assert.equal(report(s).canRetire,false);assert.equal(report(s).cashReleaseAuthority,false);
  s=page(s,p());assert.equal(report(s).canRetire,true);
});
test('even a high projection checkpoint needs an actual post-deadline book',()=>{
  let s=page(initial(),p({coverage:{complete:true,projectedThroughSequence:999}}));assert.equal(report(s).canRetire,false);
  s=book(s,b({requestStartedAt:at(10),sourceAt:at(12)}));assert.equal(report(s).canRetire,false);
  s=book(s,b({sourceAt:at(10)}));assert.equal(report(s).canRetire,false);
  s=book(s,b());assert.equal(report(s).canRetire,true);
});
test('all pages need original cursor chain and caught-up minimum checkpoint',()=>{
  let s=book(initial(),b());s=page(s,p({pagination:{hasMore:true,nextCursor:'a'},coverage:{complete:true,projectedThroughSequence:90}}));
  assert.equal(report(s).canRetire,false);s=page(s,p({requestCursor:'a'}));assert.equal(report(s).canRetire,false);
  const orphan=page(book(initial(),b()),p({requestCursor:'missing'}));assert.equal(report(orphan).canRetire,false);
  let fresh=page(book(initial(),b()),p({pagination:{hasMore:true,nextCursor:'a'}}));fresh=page(fresh,p({requestCursor:'a'}));
  assert.equal(report(fresh).canRetire,true);
});
test('missing time interval/null checkpoint/foreign scope cannot establish retirement',()=>{
  let s=book(initial(),b());s=page(s,p({from:at(0),to:at(7)}));s=page(s,p({from:at(8),to:at(20)}));assert.equal(report(s).canRetire,false);
  s=page(s,p({from:at(7),to:at(8)}));assert.equal(report(s).canRetire,true);
  assert.equal(report(page(book(initial(),b()),p({coverage:{complete:true,projectedThroughSequence:null}}))).canRetire,false);
  assert.throws(()=>page(initial(),p({exchangeId:'999'})),/SCOPE/);
});
