import test from 'node:test';import assert from 'node:assert/strict';import {predict} from './ml-bank.mjs';
test('exported GPU network uses training normalization and unscales target exactly',()=>{
 const m={family:'gpu_mlp',width:2,hidden:2,means:[1,10],scales:[1,2],inputWeights:[[2,-2],[3,4]],hiddenBias:[1,-1],outputWeights:[.2,.5],outputBias:-.1,targetMean:.01,targetScale:.02};
 // x normalized to [0, 1], hidden activations [4, 3], normalized target 2.2.
 assert.ok(Math.abs(predict(m,[1,12])-.054)<1e-12);
 assert.throws(()=>predict({...m,scales:[1,0]},[1,12]),/values/);
 assert.throws(()=>predict({...m,inputWeights:[[1],[2]]},[1,12]),/values/);
 assert.throws(()=>predict(m,[1,NaN]),/values/);
});
