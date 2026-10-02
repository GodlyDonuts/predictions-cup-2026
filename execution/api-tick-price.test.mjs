import test from'node:test';import assert from'node:assert/strict';import{normalizeOfficialFillTick}from'./api-tick-price.mjs';
test('observed legacy NO JSON complement artifacts resolve to exact official ticks',()=>{
  for(const [raw,expected]of [[0.07499999999999996,'0.075000'],[0.32999999999999996,'0.330000'],[0.31499999999999995,'0.315000'],[0.06499999999999995,'0.065000']]){
    const n=normalizeOfficialFillTick(raw);assert.equal(n.price,expected);assert.match(n.representation,/^binary_/);
  }
});
test('every official tick and its exact double complement map unambiguously',()=>{
  for(let i=0;i<=200;i++){const p=i/200;assert.equal(normalizeOfficialFillTick(p).tick,i);assert.equal(normalizeOfficialFillTick(1-(1-p)).tick,i);assert.equal(normalizeOfficialFillTick(1-(200-i)/200).tick,i);}
});
test('off-tick numbers, even tiny deviations, are not rounded onto a tick',()=>{
  for(const p of [.315000001,.31500000000000006,.07500000000000001,.314999999999,NaN,Infinity,-0,-.005,1.005])assert.throws(()=>normalizeOfficialFillTick(p));
});
test('exact string prices cannot borrow numeric floating-point exceptions',()=>{
  assert.equal(normalizeOfficialFillTick('0.315').price,'0.315000');
  for(const p of ['0.31499999999999995','0.07500000000000001','0.315001','1.005','-0.005'])assert.throws(()=>normalizeOfficialFillTick(p));
});
