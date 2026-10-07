import test from 'node:test';import assert from 'node:assert/strict';
import {auditBlockedAdmissionV1} from './blocked-admission-audit-v1.mjs';import {payloadHash} from './core.mjs';
const at=n=>new Date(Date.UTC(2026,9,7,0,0,n)).toISOString();
function input(){const body={quantity:1,expirationDate:at(5)},o={phase:'uncertain',operation:'place',orderId:null,ack:null,proof:null,cancelPending:false,fetchStarts:[],fences:[],receipts:[],body,bodyHash:payloadHash(body),createdAt:at(0)};return{state:{orders:[o]},events:[{kind:'admitted',operation:'place',at:at(2)},{kind:'error',code:'EXECUTION_AUTHORITY_UNAVAILABLE',at:at(4)}],timings:[{schemaVersion:1,kind:'orders',marketId:null,queuedAt:at(1),admittedAt:null,finishedAt:at(3),queueWaitMs:0,admissionWaitMs:2000,headersMs:null,bodyMs:null,serviceMs:null,totalMs:2000,statusCode:null,outcome:'blocked'}],now:Date.parse(at(6))};}
test('historical gate audit requires unique complete blocked timing and no physical evidence',()=>{
 const x=input();assert.equal(auditBlockedAdmissionV1(x).clearsStop,false);
 for(const field of ['fetchStarts','fences','receipts']){const y=input();y.state.orders[0][field]=[{}];assert.throws(()=>auditBlockedAdmissionV1(y));}
 for(const change of[{outcome:'success'},{admittedAt:at(2)},{statusCode:200},{serviceMs:1}]){const y=input();Object.assign(y.timings[0],change);assert.throws(()=>auditBlockedAdmissionV1(y));}
 const y=input();y.timings.push({...y.timings[0]});assert.throws(()=>auditBlockedAdmissionV1(y),/UNIQUE_ORDER_TIMING/);
});

