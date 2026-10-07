import test from 'node:test';import assert from 'node:assert/strict';import {payloadHash} from './core.mjs';import {provenUnfetchedFencesV1 as valid} from './proven-unfetched-fences-v1.mjs';
test('a preserved fence requires matching later no-fetch proof; absence of fetch alone is insufficient',()=>{
 const body={quantity:29},o={body,bodyHash:payloadHash(body),fences:[{at:'2026-10-07T00:00:00Z'}],fetchStarts:[],receipts:[]};assert.equal(valid(o),false);
 o.blockedNetworkProofs=[{kind:'serialized_client_before_network_block_v1',method:'POST',endpoint:'/orders',physicalFetchReached:false,requestBodyHash:o.bodyHash,at:'2026-10-07T00:00:01Z'}];assert.equal(valid(o),true);assert.equal(valid({...o,fetchStarts:[{}]}),false);assert.equal(valid({...o,body:{quantity:30}}),false);
});

