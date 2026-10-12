import test from 'node:test';import assert from 'node:assert/strict';import {marketableAllocationCostV1 as cost} from './marketable-allocation-cost-v1.mjs';
const a=[{key:'a',quantity:2,observedRawPrice:'0.550000'},{key:'b',quantity:3,observedRawPrice:'0.575000'}],input={limit:'0.585000',quantity:5,allocations:a};
test('per-rung cash differs from the full worst-limit reservation',()=>{const r=cost(input);assert.equal(r.modeledPrincipal,'2.850000');assert.equal(r.worstLimitReservation,'2.925000');assert.equal(r.reservationPremium,'0.075000');});
test('no-stress scenario remains explicit and cannot be labeled actual execution',()=>{const r=cost({...input,adverseTicks:0});assert.equal(r.modeledPrincipal,'2.825000');assert.equal(r.actualFillProven,false);});
test('missing, duplicated, oversized and unfillable depth reject',()=>{for(const x of [{allocations:a.slice(0,1)},{allocations:[a[0],a[0]]},{quantity:4},{adverseTicks:3}])assert.throws(()=>cost({...input,...x}));});
