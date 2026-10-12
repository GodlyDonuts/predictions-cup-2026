import test from 'node:test';
import assert from 'node:assert/strict';
import {MAKER_SCHEDULER_POLICY_V1, createMakerSchedulerV1, planMakerQuoteV1, beginMakerDispatchV1, finishMakerDispatchV1} from './maker-quote-scheduler-v1.mjs';
const at = '2026-10-12T22:10:00.000Z', earlier = '2026-10-12T22:09:55.000Z';
const quote = (extra = {}) => ({exchangeId:'101', side:'yes', action:'buy', quantity:2, price:'0.450000',
  sourceAt:earlier, validUntil:'2026-10-12T22:10:10.000Z', netEdgePerShare:'0.010000', reduceOnly:false, ...extra});
const order = (extra = {}) => ({...quote(), orderId:'1', status:'open', remainingQuantity:2, createdAt:earlier, ...extra});
const input = (extra = {}) => ({at, account:{observedAt:at, complete:true, orders:[]}, desired:[quote()],
  budget:{availableWrites:1, nextWriteAt:at}, entriesEnabled:true, exitsEnabled:true, cancelEnabled:true, ...extra});
const plan = (i, s = createMakerSchedulerV1()) => planMakerQuoteV1(s, i);

test('identical resting quotes preserve queue; partial fills do not recreate original quantity implicitly', () => {
  assert.equal(plan(input({account:{observedAt:at,complete:true,orders:[order()]}})).kind,'hold');
  assert.equal(plan(input({account:{observedAt:at,complete:true,orders:[order({status:'partial',remainingQuantity:1})]},
    desired:[quote({quantity:1})]})).kind,'hold');
});
test('stale or negative-edge quotes withdraw before eligible new entries', () => {
  const i=input({account:{observedAt:at,complete:true,orders:[order()]},desired:[quote({netEdgePerShare:'-0.005000'}),quote({exchangeId:'102'})]});
  assert.equal(plan(i).reason,'withdraw_obsolete_quote');
  i.desired[0]=quote({sourceAt:'2026-10-12T22:09:00.000Z'});
  assert.equal(plan(i).kind,'cancel');
});
test('risk reduction bypasses quote age; favorable repricing respects age hysteresis', () => {
  const recent=order({createdAt:'2026-10-12T22:09:59.000Z'});
  const i=input({account:{observedAt:at,complete:true,orders:[recent]},desired:[quote({price:'0.445000'})]});
  assert.equal(plan(i).reason,'withdraw_more_aggressive_quote');
  i.desired=[quote({price:'0.455000'})]; assert.equal(plan(i).kind,'hold');
  i.account.orders[0]=order(); assert.equal(plan(i).reason,'reprice_after_confirmed_cancel');
});
test('unknown acceptance blocks all writes, including after a restarted serialized state', () => {
  const s=beginMakerDispatchV1(createMakerSchedulerV1(),plan(input()));
  const uncertain=finishMakerDispatchV1(s,{dispatchKey:s.pending.dispatchKey,outcome:'unknown'});
  assert.equal(plan(input(),JSON.parse(JSON.stringify(uncertain))).reason,'write_outcome_pending_reconciliation');
  assert.throws(()=>beginMakerDispatchV1(s,plan(input())),/STALE_DISPATCH/);
});
test('cancel must be confirmed and fresh remaining fills reconciled before replacement', () => {
  const i=input({account:{observedAt:at,complete:true,orders:[order()]},desired:[quote({price:'0.460000'})]});
  const d=plan(i),s=beginMakerDispatchV1(createMakerSchedulerV1(),d);
  assert.equal(plan(i,s).kind,'hold');
  const next='2026-10-12T22:10:03.000Z';
  const done=finishMakerDispatchV1(s,{dispatchKey:d.dispatchKey,outcome:'confirmed',reconciledAt:next});
  assert.equal(plan({...i,at:next},done).reason,'account_refresh_required');
  const refreshed={...i,at:next,account:{observedAt:next,complete:true,orders:[order({status:'canceled'})]},
    desired:[quote({price:'0.460000',quantity:1})]}; // caller accounts for one fill during cancellation
  const replacement=plan(refreshed,done); assert.equal(replacement.kind,'submit'); assert.equal(replacement.quote.quantity,1);
});
test('inventory exits beat entry; entries ranked by net edge per funded price', () => {
  const exit=quote({exchangeId:'103',action:'sell',reduceOnly:true,netEdgePerShare:'-0.020000'});
  assert.equal(plan(input({desired:[quote(),exit]})).quotes[0].action,'sell');
  const cheap=quote({exchangeId:'102',price:'0.200000',netEdgePerShare:'0.006000'});
  assert.equal(plan(input({desired:[quote(),cheap]})).quotes[0].exchangeId,'102');
});
test('eight independent quotes consume one batch proposal without introducing owned crosses', () => {
  const desired=Array.from({length:12},(_,i)=>quote({exchangeId:String(101+i)}));
  const d=plan(input({desired})); assert.equal(d.kind,'submit_batch'); assert.equal(d.quotes.length,8);
  assert.equal(d.endpoint,'/orders/batch'); assert.equal(d.atomicAcceptance,false);
  const s=beginMakerDispatchV1(createMakerSchedulerV1(),d);
  assert.equal(plan(input({desired}),s).kind,'hold');
  const crossing=[quote({price:'0.600000'}),quote({side:'no',price:'0.400000'})];
  assert.equal(plan(input({desired:crossing})).kind,'submit');
  const bracket=[quote({price:'0.450000'}),quote({action:'sell',price:'0.460000',reduceOnly:true})];
  assert.equal(plan(input({desired:bracket})).quotes.length,2);
  assert.equal(planMakerQuoteV1(createMakerSchedulerV1(),input({desired}),{...MAKER_SCHEDULER_POLICY_V1,maximumSubmitBatch:1}).kind,'submit');
});
test('complementary YES and NO orders cannot cross our own resting quote', () => {
  const existing=order({price:'0.600000'}),opposite=quote({side:'no',price:'0.400000'});
  const i=input({account:{observedAt:at,complete:true,orders:[existing]},desired:[quote({price:'0.600000'}),opposite]});
  const d=plan(i); assert.equal(d.reason,'prevent_owned_quote_cross'); assert.equal(d.orderId,'1');
  i.desired[1]=quote({side:'no',price:'0.395000'}); assert.equal(plan(i).kind,'submit');
});
test('rolling budget wait and retry-after hint never create an independent burst', () => {
  assert.equal(plan(input({budget:{availableWrites:0,nextWriteAt:at}})).reason,'shared_write_budget_wait');
  assert.equal(plan(input({budget:{availableWrites:24,nextWriteAt:'2026-10-12T22:11:00.000Z'}})).kind,'hold');
  assert.throws(()=>plan(input({budget:{availableWrites:25,nextWriteAt:at}})),/BUDGET/);
});
test('disabled entry drains buys but allows caller-authorized covered inventory exits', () => {
  assert.equal(plan(input({entriesEnabled:false})).kind,'hold');
  assert.equal(plan(input({entriesEnabled:false,desired:[quote({action:'sell',reduceOnly:true})]})).kind,'submit');
  assert.equal(plan(input({entriesEnabled:false,account:{observedAt:at,complete:true,orders:[order()]}})).kind,'cancel');
});
test('future/incomplete snapshots, unknown open status, naked sells and modified decisions fail closed', () => {
  assert.equal(plan(input({account:{observedAt:'2026-10-12T22:10:01.000Z',complete:true,orders:[]}})).reason,'account_refresh_required');
  assert.throws(()=>plan(input({account:{observedAt:at,complete:false,orders:[]}})),/COMPLETE/);
  assert.equal(plan(input({account:{observedAt:at,complete:true,orders:[order({status:'cancel_pending'})]}})).reason,'owned_order_outcome_unknown');
  assert.throws(()=>plan(input({desired:[quote({action:'sell'})]})),/COVERED_SELL/);
  const d=plan(input()); d.quote.quantity=9; assert.throws(()=>beginMakerDispatchV1(createMakerSchedulerV1(),d),/DISPATCH_CHANGED/);
});
test('expired evidence and duplicate slots rejected; open quote cap preserves existing liabilities', () => {
  assert.equal(plan(input({desired:[quote({validUntil:at})]})).kind,'hold');
  assert.throws(()=>plan(input({desired:[quote(),quote()]})),/DUPLICATE_DESIRED/);
  const orders=Array.from({length:8},(_,i)=>order({orderId:String(i+1),exchangeId:String(101+i)}));
  const desired=orders.map(q=>quote({exchangeId:q.exchangeId})); desired.push(quote({exchangeId:'999'}));
  assert.equal(plan(input({account:{observedAt:at,complete:true,orders},desired})).kind,'hold');
  desired.push(quote({exchangeId:'1000',action:'sell',reduceOnly:true}));
  assert.equal(plan(input({account:{observedAt:at,complete:true,orders},desired})).reason,'free_quote_slot_for_inventory_exit');
});
