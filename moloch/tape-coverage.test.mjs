import test from 'node:test';import assert from 'node:assert/strict';import {TapeCoverage} from './tape-coverage.mjs';
const from='2026-10-14T00:00:00Z',to='2026-10-14T00:01:00Z';
const page=(cursor,next,seq=100)=>({kind:'trades',source:'official_sig_api_v1',status:200,marketId:'1',exchangeId:'2',tournamentId:'cup',requestStartedAt:to,receivedAt:'2026-10-14T00:01:01Z',endpoint:`/exchanges/2/trades?tournamentId=cup&from=${from}&to=${to}${cursor?'&cursor='+cursor:''}`,data:{marketId:'1',exchangeId:'2',from,to,data:[],pagination:{hasMore:next!==null,nextCursor:next},coverage:{complete:true,projectedThroughSequence:seq}}});
test('coverage requires full cursor chain, original window and minimum page checkpoint',()=>{
 const c=new TapeCoverage();assert.deepEqual(c.row(page(null,'a',100)),[]);assert.deepEqual(c.row(page('a','b',105)),[]);const [e]=c.row(page('b',null,103));assert.equal(e.type,'coverage');assert.equal(e.projectedThroughSequence,'100');assert.equal(e.sourcePagination.pages,3);assert.equal(e.sourcePagination.receiptHashes.length,3);
 assert.deepEqual(new TapeCoverage().row(page('b',null)),[]);
});
test('missing, reordered, changed-window and cyclic pages never authenticate coverage',()=>{
 const c=new TapeCoverage();c.row(page(null,'a'));assert.throws(()=>c.row(page('b',null)),/CURSOR_CHAIN/);assert.throws(()=>c.row(page('a','a')),/CURSOR_LOOP/);
 const changed=page(null,'a');changed.data.from=to;assert.throws(()=>new TapeCoverage().row(changed),/SERVED_WINDOW/);
 const c2=new TapeCoverage();const first=page(null,'a');first.data.coverage.projectedThroughSequence=null;c2.row(first);assert.deepEqual(c2.row(page('a',null)),[]);
});
