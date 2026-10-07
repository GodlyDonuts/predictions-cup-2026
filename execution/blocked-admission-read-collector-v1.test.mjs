import test from 'node:test';import assert from 'node:assert/strict';import {collectBlockedAdmissionAccountV1} from './blocked-admission-read-collector-v1.mjs';
test('blocked recovery uses four existing-client GETs and stops before further reads on ownership loss',async()=>{
 for(const loseAfter of [99,1]){let calls=0;const args={scope:{tournamentId:'T',tournamentSlug:'midterm-elections'},intent:{orderId:null,actualFetchCount:0,body:{expirationDate:'2026-10-07T00:00:00Z',exchangeId:'1073'}},now:()=>Date.parse('2026-10-07T00:01:00Z'),canRead:()=>calls<loseAfter,receiptOf:r=>r,api:{dispatch:async s=>{assert.equal(s.method,'GET');s.beforeDispatch();s.beforeNetwork();calls++;return{bodyText:JSON.stringify({pagination:{hasMore:false,nextCursor:null}})};}}};
  if(loseAfter===99){const r=await collectBlockedAdmissionAccountV1(args);assert.equal(calls,4);assert.equal(r.scopedOrdersPages.length,1);}else{await assert.rejects(collectBlockedAdmissionAccountV1(args),/OWNER_OR_STOP_PROVENANCE/);assert.equal(calls,1);}
 }
});

