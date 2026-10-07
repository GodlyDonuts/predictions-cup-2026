import test from 'node:test';import assert from 'node:assert/strict';
import {blockedNetworkProofV1,blockedNetworkProofMatchesV1} from './blocked-network-proof-v1.mjs';
import {payloadHash} from './core.mjs';
test('pre-network proof binds exact original body and cannot stand for a fetched order',()=>{
 const body={exchangeId:'1073',quantity:29},intent={body,bodyHash:payloadHash(body),createdAt:'2026-10-07T00:00:00Z',orderId:null,actualFetchCount:0};
 const proof=blockedNetworkProofV1({spec:{method:'POST',endpoint:'/orders',bodyJson:JSON.stringify(body),beforeNetwork:()=>{}},error:Error('EXECUTION_AUTHORITY_UNAVAILABLE'),at:'2026-10-07T00:00:01Z'});
 assert.equal(blockedNetworkProofMatchesV1(proof,intent),true);
 for(const change of[{actualFetchCount:1},{orderId:'1'},{body:{exchangeId:'1073',quantity:30}}])assert.equal(blockedNetworkProofMatchesV1(proof,{...intent,...change}),false);
 assert.equal(blockedNetworkProofMatchesV1({...proof,physicalFetchReached:true},intent),false);
 assert.equal(blockedNetworkProofMatchesV1({...proof,at:'2026-10-06T23:59:59Z'},intent),false);
});

