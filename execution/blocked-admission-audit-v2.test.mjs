import test from 'node:test';import assert from 'node:assert/strict';import {auditBlockedAdmissionV2} from './blocked-admission-audit-v2.mjs';import {payloadHash} from './core.mjs';
const at=n=>new Date(Date.UTC(2026,9,7,0,0,n)).toISOString();
function input(){const body={quantity:29,expirationDate:at(5)},o={phase:'uncertain',operation:'place',orderId:null,ack:null,proof:null,cancelPending:false,fetchStarts:[],fences:[{operation:'place'}],receipts:[],body,bodyHash:payloadHash(body),createdAt:at(0)};return{state:{orders:[o]},events:[{kind:'admitted',operation:'place',at:at(2)},{kind:'network_fence',operation:'place',at:at(3)},{kind:'error',code:'EXECUTION_AUTHORITY_UNAVAILABLE',at:at(4)}],timings:[{schemaVersion:1,kind:'orders',marketId:null,queuedAt:at(1),admittedAt:null,finishedAt:at(3),queueWaitMs:0,admissionWaitMs:2000,headersMs:null,bodyMs:null,serviceMs:null,totalMs:2000,statusCode:null,outcome:'blocked'}],now:Date.parse(at(6))};}
test('post-fence audit requires one exact local fence and a blocked pre-fetch timing record',()=>{
 assert.equal(auditBlockedAdmissionV2(input()).clearsStop,false);
 for(const field of ['fetchStarts','receipts']){const x=input();x.state.orders[0][field]=[{}];assert.throws(()=>auditBlockedAdmissionV2(x));}
 for(const fences of[[],[{},{}]]){const x=input();x.state.orders[0].fences=fences;assert.throws(()=>auditBlockedAdmissionV2(x));}
 const x=input();x.timings[0].admittedAt=at(3);assert.throws(()=>auditBlockedAdmissionV2(x));
 const y=input();y.events[1].kind='fetch_start';assert.throws(()=>auditBlockedAdmissionV2(y));
});

