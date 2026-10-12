import test from 'node:test';import assert from 'node:assert/strict';import {statePollBiasV1 as pool} from './state-poll-bias-v1.mjs';
const model={biasPoints:-1,standardDeviationPoints:8,trainedThroughYear:2022},races=[{key:'2020:A',year:2020,state:'A',error:-7}];
test('sparse local errors shrink toward global bias with unchanged variance',()=>{const r=pool({model,races,state:'A'});assert.equal(r.biasPoints,-2);assert.equal(r.standardDeviationPoints,8);assert.equal(r.varianceShrunk,false);});
test('missing state uses global model instead of extrapolating another state',()=>{assert.equal(pool({model,races,state:'B'}).biasPoints,-1);});
test('duplicate races and future outcomes cannot become prior evidence',()=>{assert.throws(()=>pool({model,races:[...races,races[0]],state:'A'}));assert.throws(()=>pool({model,races:[{...races[0],year:2024}],state:'A'}));});
