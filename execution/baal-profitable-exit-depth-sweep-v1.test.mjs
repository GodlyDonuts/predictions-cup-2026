import test from 'node:test';import assert from 'node:assert/strict';
import {selectBaalProfitableExitDepthSweepV1 as select} from './baal-profitable-exit-depth-sweep-v1.mjs';
const owned={remainingLots:[{quantity:100,entryPrice:'0.100000'}],remainingOwnedQuantity:100};
test('aggregates distinct executable rungs at a single protected worst price',()=>{
 const r=select({...owned,yesAsks:[{price:'0.885000',quantity:20},{price:'0.890000',quantity:20}]});
 assert.equal(r.selected.quantity,40);assert.equal(r.selected.protectedLimit,'0.110000');assert.equal(r.selected.protectedNetProfitIfFilled,'0.400000');assert.equal(r.orderEligible,false);
});
test('maximizes dollar gain rather than blindly choosing most shares',()=>{
 const r=select({...owned,yesAsks:[{price:'0.880000',quantity:40},{price:'0.895000',quantity:60}]});
 assert.equal(r.selected.quantity,40);assert.equal(r.selected.protectedNetProfitIfFilled,'0.800000');
});
test('cannot average past an expensive first FIFO lot',()=>{
 const r=select({remainingLots:[{quantity:1,entryPrice:'0.120000'},{quantity:99,entryPrice:'0.010000'}],remainingOwnedQuantity:100,yesAsks:[{price:'0.890000',quantity:100}]});assert.equal(r.selected,null);
});
test('duplicate or reversed prices cannot double count capacity',()=>{
 for(const asks of[[{price:'0.890000',quantity:10},{price:'0.890000',quantity:10}],[{price:'0.895000',quantity:10},{price:'0.890000',quantity:10}]])assert.equal(select({...owned,yesAsks:asks}).selected,null);
});
test('large displayed quantity retains exact audit depth and obeys original caps',()=>{
 const r=select({...owned,yesAsks:[{price:'0.885000',quantity:Number.MAX_SAFE_INTEGER},{price:'0.890000',quantity:Number.MAX_SAFE_INTEGER}]});assert.equal(r.selected.quantity,100);assert.equal(r.candidates[1].displayedAggregateQuantity,'18014398509481982');assert.equal(r.selected.orderNotional,'11.500000');
});
