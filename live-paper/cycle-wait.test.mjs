import test from 'node:test';import assert from 'node:assert/strict';import {collectionCycleWait} from './cycle-wait.mjs';
const input={cycleStartedAt:100000,now:110000,cycleIntervalMs:30000};
test('flat collection retains original remaining cycle floor; overrun retains one-second idle',()=>{
 assert.deepEqual(collectionCycleWait(input),{mode:'normal',delayMs:20000});assert.equal(collectionCycleWait({...input,now:150000}).delayMs,1000);
});
test('owned inventory or pending intent removes only the idle floor and flat recovery restores it',()=>{
 for(const managedMarketIds of [['277'],['277','305']])assert.deepEqual(collectionCycleWait({...input,managedMarketIds}),{mode:'owned_risk',delayMs:0});
 assert.equal(collectionCycleWait({...input,managedMarketIds:[]}).delayMs,20000);
});
test('clock reversal, invalid risk and fractional timing cannot manufacture a fast cycle',()=>{
 for(const change of [{now:99999},{now:NaN},{now:110000.5},{cycleIntervalMs:0},{managedMarketIds:null},{managedMarketIds:['0']},{managedMarketIds:[277]}])assert.throws(()=>collectionCycleWait({...input,...change}),/INVALID/);
});
