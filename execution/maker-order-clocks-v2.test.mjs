import test from 'node:test';import assert from 'node:assert/strict';
import {createMakerSchedulerV1,MAKER_SCHEDULER_POLICY_V1} from './maker-quote-scheduler-v1.mjs';
import {planMakerQuoteV2,bindMakerBatchV2,assertMakerBatchBindingV2,assertMakerBatchFirstDispatchV2,inspectMakerBatchOutcomeV2} from './maker-order-clocks-v2.mjs';
const at=n=>new Date(Date.parse('2026-10-12T23:30:00.000Z')+n*1000).toISOString(),tournamentId='11111111-1111-4111-8111-111111111111';
const input=()=>({at:at(0),account:{observedAt:at(0),complete:true,orders:[]},desired:['101','102'].map(exchangeId=>({exchangeId,side:'yes',action:'buy',
  quantity:1,price:'0.450000',sourceAt:at(0),validUntil:at(10),admissionValidUntil:at(10),orderExpiresAt:at(120),netEdgePerShare:'0.010000',reduceOnly:false})),
  budget:{availableWrites:1,nextWriteAt:at(0)},entriesEnabled:true,exitsEnabled:true,cancelEnabled:true});
const fixture=()=>{const i=input(),decision=planMakerQuoteV2(createMakerSchedulerV1(),i),request=bindMakerBatchV2({decision,tournamentId,idempotencyKey:'clock-test'});return{i,decision,request};};
test('batch wire has full resting lifetime while original decision keeps ten-second admission deadline',()=>{
  const{decision,request}=fixture();assert.equal(decision.quotes[0].validUntil,at(10));
  assert.equal(request.body.orders[0].expirationDate,at(120));assert.equal(request.decisionKey,decision.dispatchKey);
  assert.equal(request.sourceDecision.quotes[0].admissionValidUntil,at(10));assert.equal(MAKER_SCHEDULER_POLICY_V1.minimumNetEntryEdge,'0.005000');
});
test('queue wait cannot use long server expiry to admit stale first request',()=>{
  const{request}=fixture();assert.equal(assertMakerBatchFirstDispatchV2(request,at(9)),request);
  assert.throws(()=>assertMakerBatchFirstDispatchV2(request,at(10)),/FIRST_DISPATCH_STALE/);
  // Existing unknown requests retain their original identity for reconciliation.
  assert.equal(assertMakerBatchBindingV2(request),request);
  assert.equal(inspectMakerBatchOutcomeV2(request,{status:503,data:{error:{code:'SERVICE_UNAVAILABLE'}}}).accountReconciliationRequired,true);
});
test('too-short physical order life is rejected, but short remaining admission can allow a long-lived order',()=>{
  const i=input();i.desired=i.desired.map(q=>({...q,validUntil:at(1),admissionValidUntil:at(1)}));
  assert.equal(planMakerQuoteV2(createMakerSchedulerV1(),i).kind,'submit_batch');
  i.desired=i.desired.map(q=>({...q,orderExpiresAt:at(3)}));assert.equal(planMakerQuoteV2(createMakerSchedulerV1(),i).kind,'hold');
});
test('changing source deadline, server expiry, or admission alias breaks exact binding',()=>{
  const{request}=fixture();const changed=JSON.parse(JSON.stringify(request));changed.body.orders[0].expirationDate=at(240);
  assert.throws(()=>assertMakerBatchBindingV2(changed),/BINDING_CHANGED/);
  const i=input();i.desired[0].admissionValidUntil=at(11);assert.throws(()=>planMakerQuoteV2(createMakerSchedulerV1(),i),/ADMISSION_ALIAS/);
});
