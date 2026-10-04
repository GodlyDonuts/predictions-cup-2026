import test from 'node:test';
import assert from 'node:assert/strict';
import {partitionReplayModels,parallelPortfolioReplay} from './portfolio-replay-pool.mjs';
test('parallel shards retain the complete shared feature-history universe or fall back to one worker',()=>{
 const model=(id,market)=>({id,coverage:[{marketId:market,eligible:true,trainingInstruments:[market+':YES']}]});
 const a=model('a','1'),b=model('b','2');
 assert.deepEqual(partitionReplayModels([a,b],2),[[a,b]]);
 const balanced=[a,a,b,b];assert.equal(partitionReplayModels(balanced,2).length,2);
 assert.deepEqual(partitionReplayModels([],4),[]);
 assert.throws(()=>partitionReplayModels([a],9),/worker count/);
});
test('serialized source lookalikes are rejected before any replay workers are created',async()=>{
 await assert.rejects(parallelPortfolioReplay({s:{records:[],verified:true},modelBank:{selected:[]}},{workers:2}),/Unverified offline/);
});
