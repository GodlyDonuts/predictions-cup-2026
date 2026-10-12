import test from 'node:test';import assert from 'node:assert/strict';
import {createMakerSchedulerV1,planMakerQuoteV1,beginMakerDispatchV1} from './maker-quote-scheduler-v1.mjs';
import {bindMakerBatchV1,assertMakerBatchBindingV1,inspectMakerBatchOutcomeV1} from './maker-batch-outcome-v1.mjs';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {CoordinatedAccountClientV2} from '../live-paper/coordinated-account-client-v2.mjs';
const at='2026-10-12T22:20:00.000Z',tournamentId='550e8400-e29b-41d4-a716-446655440000';
function fixture(count=2){const desired=Array.from({length:count},(_,i)=>String(101+i)).map(exchangeId=>({exchangeId,side:'yes',action:'buy',quantity:2,price:'0.450000',
  sourceAt:at,validUntil:'2026-10-12T22:20:10.000Z',netEdgePerShare:'0.010000',reduceOnly:false}));
  const decision=planMakerQuoteV1(createMakerSchedulerV1(),{at,account:{observedAt:at,complete:true,orders:[]},desired,
    budget:{availableWrites:1,nextWriteAt:at},entriesEnabled:true,exitsEnabled:true,cancelEnabled:true});
  const request=bindMakerBatchV1({decision,tournamentId,idempotencyKey:'maker-batch-test-only'});
  return{request,decision};}
const accepted=(i,extra={})=>({index:i,ok:true,status:200,data:{orderId:1001+i,exchangeId:String(101+i),open:true,
  remainingQuantity:2,action:'buy',side:'yes',price:.45,quantity:2,quantityTraded:0,totalCost:0,fillPrice:null,all:null,...extra}});
const failure=(i,status,code)=>({index:i,ok:false,status,data:{code,error:'fixture'}});
const response=(status,results)=>({status,data:{results}});

test('explicit tournament, ordered immutable body and one write are bound to durable decision',()=>{
  const{request,decision}=fixture();assert.equal(request.writeAdmissionsRequired,1);
  assert.equal(request.atomicAcceptance,false);assert.ok(request.body.orders.every(q=>q.tournamentId===tournamentId));
  assert.equal(request.decisionKey,decision.dispatchKey);assert.equal(beginMakerDispatchV1(createMakerSchedulerV1(),decision).pending.dispatchKey,request.decisionKey);
  assert.throws(()=>request.body.orders[0].price=.5,TypeError);
  const changed=JSON.parse(JSON.stringify(request));changed.body.orders.reverse();assert.throws(()=>assertMakerBatchBindingV1(changed),/REQUEST_CHANGED/);
  assert.throws(()=>bindMakerBatchV1({decision,tournamentId:'default',idempotencyKey:'x'}),/TOURNAMENT/);
});
test('all accepted placements require actual order/fill reconciliation, never claim realized profit',()=>{
  const{request}=fixture(),r=inspectMakerBatchOutcomeV1(request,response(200,[accepted(0),accepted(1)]));
  assert.equal(r.disposition,'reconcile_accepted_orders');assert.equal(r.finalEnvelope,true);
  assert.equal(r.accountReconciliationRequired,true);assert.equal(r.retrySameKeyAndOrderedBodyOnly,false);
  assert.deepEqual(r.knownOrderIds,['1001','1002']);assert.equal(r.profitabilityEstablished,false);
});
test('completed mixed207 distinguishes accepted identities from definitive rejection',()=>{
  const{request}=fixture(),r=inspectMakerBatchOutcomeV1(request,response(207,[accepted(0),failure(1,400,'VALIDATION_ERROR')]));
  assert.equal(r.finalEnvelope,true);assert.equal(r.items[0].kind,'accepted_needs_reconciliation');
  assert.equal(r.items[1].kind,'rejected');assert.equal(r.accountReconciliationRequired,true);
});
test('mixed207 with a transient item keeps ordered payload/key and pending batch fence',()=>{
  const{request}=fixture(),r=inspectMakerBatchOutcomeV1(request,response(207,[accepted(0),failure(1,429,'RATE_LIMITED')]));
  assert.equal(r.finalEnvelope,false);assert.equal(r.retrySameKeyAndOrderedBodyOnly,true);
  assert.deepEqual(r.unresolvedIndexes,[1]);assert.deepEqual(r.knownOrderIds,['1001']);
});
test('503 with no results may have placed earlier orders; missing indexes stay unresolved',()=>{
  const{request}=fixture(),r=inspectMakerBatchOutcomeV1(request,{status:503,data:{error:{code:'SERVICE_UNAVAILABLE'}}});
  assert.equal(r.disposition,'reconcile_or_retry_identical_batch');assert.deepEqual(r.unresolvedIndexes,[0,1]);
  assert.equal(r.accountReconciliationRequired,true);
  const partial=inspectMakerBatchOutcomeV1(request,response(503,[accepted(0)]));
  assert.deepEqual(partial.knownOrderIds,['1001']);assert.deepEqual(partial.unresolvedIndexes,[1]);
});
test('unknown502, top-level409 conflict, and expired item keys do not share one rejection rule',()=>{
  const{request}=fixture();const unknown=inspectMakerBatchOutcomeV1(request,response(502,[failure(0,502,'ORDER_STATUS_UNKNOWN'),failure(1,502,'ORDER_STATUS_UNKNOWN')]));
  assert.deepEqual(unknown.unresolvedIndexes,[0,1]);assert.equal(unknown.accountReconciliationRequired,true);
  assert.equal(inspectMakerBatchOutcomeV1(request,{status:409,data:{error:{code:'CONFLICT'}}}).retrySameKeyAndOrderedBodyOnly,true);
  const expired=inspectMakerBatchOutcomeV1(request,response(422,[failure(0,409,'IDEMPOTENCY_KEY_EXPIRED'),failure(1,409,'IDEMPOTENCY_KEY_EXPIRED')]));
  assert.equal(expired.disposition,'definitive_rejection');
});
test('engine canonicalization mismatch preserves returned identity and blocks reinterpretation',()=>{
  const{request}=fixture(),r=inspectMakerBatchOutcomeV1(request,response(200,[accepted(0,{action:'sell',side:'no',price:.55}),accepted(1)]));
  assert.equal(r.items[0].kind,'unknown');assert.equal(r.items[0].reason,'canonical_action_mismatch');
  assert.deepEqual(r.knownOrderIds,['1001','1002']);assert.equal(r.finalEnvelope,false);
});
test('missing order IDs, duplicate indexes and foreign exchanges retain all returned identities',()=>{
  const{request}=fixture();assert.equal(inspectMakerBatchOutcomeV1(request,response(200,[accepted(0,{orderId:null}),accepted(1)])).finalEnvelope,false);
  const dup=inspectMakerBatchOutcomeV1(request,response(200,[accepted(0),accepted(0,{orderId:9999}),accepted(1)]));
  assert.equal(dup.malformedEnvelope,true);assert.equal(dup.finalEnvelope,false);assert.deepEqual(dup.knownOrderIds,['1001','9999','1002']);
  assert.equal(inspectMakerBatchOutcomeV1(request,response(200,[accepted(0,{exchangeId:'999'}),accepted(1)])).items[0].kind,'unknown');
});
test('documented upfront validation errors reject all items, while malformed422 does not',()=>{
  const{request}=fixture();assert.equal(inspectMakerBatchOutcomeV1(request,{status:400,data:{error:{code:'VALIDATION_ERROR'}}}).disposition,'definitive_rejection');
  assert.equal(inspectMakerBatchOutcomeV1(request,{status:422,data:{}}).accountReconciliationRequired,true);
});
test('eight quote orders pass through ONE existing shared-client write admission and both guarded boundaries',async t=>{
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'maker-batch-shared-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const{request}=fixture(8);let clock=Date.parse(at),fetches=0,admitted=false,networkGuarded=false;
  const beforeDispatch=()=>{assert.equal(admitted,false);admitted=true;};
  const beforeNetwork=()=>{assert.equal(admitted,true);networkGuarded=true;};
  const client=new CoordinatedAccountClientV2('synthetic-offline-only',{
    admissionFile:path.join(dir,'admissions.json'),coordination:{kind:'baal_single_client_rolling_policy_v1',readCompletionGapMs:100,
      writeCompletionGapMs:2500,maxReadsPerMinute:60,maxWritesPerMinute:24},
    networkBudget:{schemaVersion:4,ownerId:'Codex',enabled:true,intervalMs:1000,writeIntervalMs:2500,soleAccountClient:true,
      executionOwner:'Codex',handoffId:'human-to-baal-offline-test',ackCommentId:1,writesEnabled:true,
      authorityKind:'direct_human_assignment',assignmentCommentId:2},
    now:()=>clock,sleepFn:async ms=>{clock+=ms;},
    authorizeDispatch:s=>s.method===request.method&&s.endpoint===request.endpoint&&s.bodyJson===request.bodyJson&&
      s.beforeDispatch===beforeDispatch&&s.beforeNetwork===beforeNetwork,
    fetchFn:async(url,options)=>{assert.equal(networkGuarded,true);assert.equal(url,'https://sig.thesuper.market/api/v1/orders/batch');
      assert.equal(JSON.parse(options.body).orders.length,8);fetches++;return new Response(JSON.stringify({results:Array.from({length:8},(_,i)=>accepted(i))}));},
  });t.after(()=>client.close());
  const response=await client.dispatch({...request,beforeDispatch,beforeNetwork});
  assert.equal(fetches,1);const durable=JSON.parse(fs.readFileSync(path.join(dir,'admissions.json')));
  assert.equal(durable.writes.length,1);assert.equal(durable.reads.length,0);
  assert.equal(inspectMakerBatchOutcomeV1(request,response).knownOrderIds.length,8);
  assert.throws(()=>client.dispatch({...request,beforeDispatch:()=>true,beforeNetwork}),/not authorized/);
});
