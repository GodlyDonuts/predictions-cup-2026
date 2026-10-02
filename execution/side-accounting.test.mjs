import test from 'node:test';
import assert from 'node:assert/strict';
import {effectiveFillPrice, projectBoundBuyFills} from './side-accounting.mjs';

const binding = {orderId:'1001', exchangeId:'36', marketId:'26', tournamentId:'test-scope',
  action:'buy', side:'no', quantity:6, sideRelativeLimit:'0.160000', createdAt:'2026-10-02T20:00:00.000Z'};
const fill = {id:5001, orderId:'1001', exchangeId:'36', marketId:'26', tournamentId:'test-scope',
  side:'no', quantity:-3, price:0.84, filledAt:'2026-10-02T20:00:01.000Z'};
const options = {source:'realtime_account_batch', readAt:'2026-10-02T20:00:02.000Z', complete:false};
const project = (rows=[fill], b=binding, o=options) => projectBoundBuyFills(b,rows,o);

test('NO complement is exact and signed quantity remains distinct from positive acquired units',()=>{
  const p=project();assert.equal(p.principalCost,'0.480000');assert.equal(p.maximumRemainingReservation,'0.480000');
  assert.equal(p.signedPositionDelta,-3);assert.equal(p.filledQuantity,3);assert.equal(p.fills[0].rawPrice,'0.840000');
  assert.equal(p.fills[0].effectivePrice,'0.160000');assert.equal(p.authoritativeReconciliation,false);
});
test('REST NO and unknown source cannot be assigned a price basis by caller inference',()=>{
  assert.throws(()=>project([fill],binding,{...options,source:'rest_order_fills'}),/UNVERIFIED_REST_NO_PRICE_BASIS/);
  assert.throws(()=>effectiveFillPrice('.84','no','guessed_side_relative'),/UNKNOWN_FILL_SOURCE/);
  assert.equal(effectiveFillPrice('0.84','yes','rest_order_fills'),'0.840000');
});
test('repeated deliveries do not multiply costs and new fills extend immutable history',()=>{
  const p=project([fill,structuredClone(fill)]);assert.equal(p.fills.length,1);
  const next={...fill,id:5002,quantity:-2,price:0.845,filledAt:'2026-10-02T20:00:02.000Z'};
  const q=project([next,fill],binding,{...options,readAt:'2026-10-02T20:00:03.000Z',prior:p});
  assert.equal(q.principalCost,'0.790000');assert.equal(q.maximumRemainingReservation,'0.160000');
  assert.equal(q.filledQuantity,5);assert.deepEqual(project([fill,fill]).projectionHash,p.projectionHash);
});
test('changed or removed history, forged projection and clock regression fail',()=>{
  const p=project();
  assert.throws(()=>project([fill,{...fill,price:0.845}]),/CONFLICTING_DUPLICATE_FILL/);
  assert.throws(()=>project([],binding,{...options,prior:p}),/FILL_HISTORY_CHANGED/);
  assert.throws(()=>project([fill],binding,{...options,prior:{...p,principalCost:'0.000000'}}),/INVALID_PRIOR_PROJECTION/);
  assert.throws(()=>project([fill],binding,{...options,readAt:'2026-10-02T20:00:01.000Z',prior:p}),/CLOCK_REGRESSION/);
  assert.throws(()=>project([fill],{...binding,quantity:7},{...options,prior:p}),/PRIOR_BINDING_MISMATCH/);
});
test('side sign, scope, order identity, null price and future/pre-order fills fail',()=>{
  for(const change of [{side:'yes'},{quantity:3},{quantity:0},{tournamentId:'other'},{orderId:'1002'},
    {exchangeId:'37'},{marketId:'27'},{price:null},{filledAt:'2026-10-02T19:59:59.000Z'},
    {filledAt:'2026-10-02T20:00:03.000Z'}])assert.throws(()=>project([{...fill,...change}]));
});
test('NO limit uses complement and total fills cannot exceed the bound',()=>{
  assert.throws(()=>project([{...fill,price:0.835}]),/OUTSIDE_SIDE_RELATIVE/);
  assert.throws(()=>project([{...fill,quantity:-7}]),/OVERFILLED/);
  assert.throws(()=>project([fill],{...binding,action:'sell'}),/INVALID_BUY_BINDING/);
});
test('YES cost is unchanged and empty projection has no negative zero',()=>{
  const b={...binding,side:'yes',sideRelativeLimit:'0.845000'},p=project([{...fill,side:'yes',quantity:3}],b,{...options,source:'rest_order_fills'});
  assert.equal(p.principalCost,'2.520000');assert.equal(p.signedPositionDelta,3);
  assert.equal(Object.is(project([]).signedPositionDelta,-0),false);
});
test('complete history cannot silently revert to incomplete',()=>{
  const p=project([fill],binding,{...options,complete:true});
  assert.throws(()=>project([fill],binding,{...options,prior:p}),/COMPLETE_HISTORY_REGRESSION/);
});
test('known numeric complementary-tick artifact retains provenance and uses exact effective cost',()=>{
  const raw=0.32999999999999996,p=project([{...fill,price:raw}],{...binding,sideRelativeLimit:'0.675000'});
  assert.equal(p.principalCost,'2.010000');assert.equal(p.fills[0].effectivePrice,'0.670000');
  assert.equal(p.fills[0].rawApiPrice,String(raw));assert.equal(p.fills[0].priceRepresentation,'binary_tick_complement');
  assert.throws(()=>project([{...fill,price:.330000000001}],{...binding,sideRelativeLimit:'0.675000'}),/UNRECOGNIZED/);
});
