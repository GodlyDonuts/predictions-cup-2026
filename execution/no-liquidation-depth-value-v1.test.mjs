import test from 'node:test';import assert from 'node:assert/strict';import {valueNoLiquidationDepthV1 as value} from './no-liquidation-depth-value-v1.mjs';
test('NO valuation consumes actual rungs rather than extrapolating top quote',()=>{
 const x={quantity:5,remainingCost:'1.000000',yesAsks:[{price:'0.700000',quantity:2},{price:'0.800000',quantity:3}]},r=value(x);assert.equal(r.proceeds,'1.200000');assert.equal(r.unrealizedPnl,'0.200000');assert.equal(r.executionAuthority,false);
 assert.equal(value({...x,slippageTicks:1}).proceeds,'1.175000');const thin=value({...x,quantity:6});assert.equal(thin.complete,false);assert.equal(thin.unrealizedPnl,null);assert.equal(thin.unvaluedQuantity,1);
});
test('duplicate, fractional and unordered depth cannot inflate value',()=>{
 for(const rows of[[{price:'0.7',quantity:2},{price:'0.7',quantity:2}],[{price:'0.8',quantity:2},{price:'0.7',quantity:2}],[{price:'0.7',quantity:1.5}]])assert.throws(()=>value({quantity:2,remainingCost:'0.200000',yesAsks:rows}));
});

