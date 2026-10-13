import test from 'node:test';import assert from 'node:assert/strict';import {makerProjectionRecoveryRequestsV1 as map} from './maker-projection-recovery-requests-v1.mjs';
import {createMakerProjectionBarrierV1,observeMakerDeadlineBookV1} from '../paper/maker-projection-barrier-v1.mjs';
import {validateQuoteRecoveryRequest} from './quote-projection-recovery.mjs';
const tid='11111111-1111-4111-8111-111111111111',at=n=>new Date(Date.parse('2026-10-12T23:20:00.000Z')+n*1000).toISOString();
const universe={tournamentId:tid,exchanges:[{marketId:'11',exchangeId:'101'}]};
function quote(id=1){const scope={tournamentId:tid,marketId:'11',exchangeId:'101'};let projection=createMakerProjectionBarrierV1({scope,activationAt:at(4),endAt:at(11)});
  projection=observeMakerDeadlineBookV1(projection,{...scope,sequence:'110',requestStartedAt:at(12),receivedAt:at(13),sourceAt:at(12)});
  return{id,marketId:'11',exchangeId:'101',activationAt:Date.parse(at(4)),expiresAt:Date.parse(at(120)),cancelEffectiveAt:Date.parse(at(11)),closedAt:null,projection};}
test('numeric quote clocks and new projection barrier map to original exact recovery schema',()=>{
  const r=map({quotes:[quote()],universe,at:at(20)});assert.equal(r.requests.length,1);assert.equal(r.requests[0].from,at(4));
  assert.equal(r.requests[0].to,at(11));assert.equal(r.requests[0].requiredCheckpoint,'110');
  assert.equal(validateQuoteRecoveryRequest(r.requests[0],universe,Date.parse(at(20))),r.requests[0]);assert.equal(r.networkRequests,0);
});
test('same two-leg interval deduplicates without dropping source quotes or extra recovery windows',()=>{
  const quotes=[quote(1),quote(2)];const r=map({quotes,universe,at:at(20)});assert.equal(r.requests.length,1);assert.equal(quotes.length,2);assert.equal(quotes[0].closedAt,null);
  const many=Array.from({length:6},(_,i)=>{const q=quote(i+1);q.projection.barrier.sequence=String(110+i);return q;});
  assert.equal(map({quotes:many,universe,at:at(20)}).requests.length,6);assert.equal(r.truncated,false);
});
test('missing barrier is visible; mismatched scope/deadline/engine clock fail closed',()=>{
  const q=quote();q.projection.barrier=null;assert.equal(map({quotes:[q],universe,at:at(20)}).blocked[0].reason,'post_deadline_engine_barrier_missing');
  const wrong=quote();wrong.projection.scope.tournamentId='foreign';assert.throws(()=>map({quotes:[wrong],universe,at:at(20)}),/SCOPE_BINDING/);
  const changed=quote();changed.cancelEffectiveAt=Date.parse(at(10));assert.throws(()=>map({quotes:[changed],universe,at:at(20)}),/LIFETIME_BINDING/);
  const stale=quote();stale.projection.barrier.sourceAt=at(10);assert.throws(()=>map({quotes:[stale],universe,at:at(20)}),/DEADLINE_BOOK/);
});
