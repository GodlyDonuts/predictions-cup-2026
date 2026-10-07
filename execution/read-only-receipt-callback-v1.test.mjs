import test from 'node:test';import assert from 'node:assert/strict';
import {readOnlyReceiptCallbackV1} from './read-only-receipt-callback-v1.mjs';import {createScopedRestReceiptsV6} from './scoped-rest-receipts-v6.mjs';
test('actual native recorder accepts GET-only host without a financial fetch callback',async()=>{
 const T='bda92870-621e-47b0-bc3c-3602c5c26f55',host={status:()=>({executionAuthority:false})},p=createScopedRestReceiptsV6({scope:{tournamentId:T,tournamentSlug:'midterm-elections',marketIds:['384']},fetchFn:async()=>new Response(JSON.stringify({id:T})),onFetchStart:e=>readOnlyReceiptCallbackV1(host,e)});
 try{const r=await p.fetch('https://sig.thesuper.market/api/v1/tournaments/midterm-elections');assert.equal((await r.json()).id,T);}finally{p.close();}
 assert.throws(()=>readOnlyReceiptCallbackV1(host,{method:'POST'}),/WRITE_RECEIPT_CALLBACK_REQUIRED/);
 let called=false;assert.equal(readOnlyReceiptCallbackV1({onFetchStart:e=>{called=e.method==='POST';return true;}},{method:'POST'}),true);assert.equal(called,true);
});

