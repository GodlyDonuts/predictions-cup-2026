import test from 'node:test';
import assert from 'node:assert/strict';
import {Api} from './api.mjs';
test('read-only client rejects placement, cancellation, and credential redirects',async()=>{
 const api=new Api('https://sig.thesuper.market/api/v1','test-key',0);
 await assert.rejects(api.request('/orders','POST',{}),/read-only/);
 await assert.rejects(api.request('/orders/1','DELETE'),/read-only/);
 assert.throws(()=>new Api('https://example.com','test-key'),/authorized SIG host/);
});
test('pagination reads all pages and refuses a truncated cursor contract',async()=>{
 const api=new Api('https://sig.thesuper.market/api/v1','test-key',0);const calls=[];
 api.request=async path=>{calls.push(path);return calls.length===1?{data:[1],pagination:{hasMore:true,nextCursor:'a/b'}}:{data:[2],pagination:{hasMore:false}};};
 assert.deepEqual(await api.pages('/markets?limit=100'),[1,2]);
 assert.equal(calls[1],'/markets?limit=100&cursor=a%2Fb');
 api.request=async()=>({data:[],pagination:{hasMore:true,nextCursor:null}});
 await assert.rejects(api.pages('/markets?limit=100'),/Incomplete/);
});
