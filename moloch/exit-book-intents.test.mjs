import test from 'node:test';import assert from 'node:assert/strict';import {exitBookIntents} from './live.mjs';
test('only pending same-scope AON windows can request a priority book',()=>{
 const scope=[{marketId:'1',exchangeId:'2'}],q={marketId:'1',exchangeId:'2',executionStyle:'inventoryAON',quantity:25,filled:0,activationAt:8000,expiresAt:13000,retired:false};
 assert.deepEqual(exitBookIntents(scope,{orders:[q]},10000,[q]),[{marketId:'1',exchangeId:'2',activationAt:8000,expiresAt:13000}]);
 assert.deepEqual(exitBookIntents(scope,{orders:[{...q,retired:true},{...q,expiresAt:9000},{...q,executionStyle:undefined}]},10000),[]);
 assert.throws(()=>exitBookIntents(scope,{orders:[{...q,marketId:'3'}]},10000),/EXIT_BOOK_SCOPE/);
});
