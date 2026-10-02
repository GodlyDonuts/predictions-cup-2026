import test from 'node:test';
import assert from 'node:assert/strict';
import {createStreamBookAdapter,advanceStreamBookAdapter,streamBookReport} from './stream-book-adapter.mjs';
const base=Date.parse('2026-10-02T15:00:00.000Z'),at=n=>new Date(base+n).toISOString(),tid='bda92870-621e-47b0-bc3c-3602c5c26f55',topic='tournament:'+tid+':market:377';
const raw=(sequence,n)=>({exchangeId:'1066',asOf:{sequence,at:at(n)},nextExpiryAt:null,expiryKnown:true,bids:[{price:.40,quantity:1000}],asks:[{price:.42,quantity:1000}]});
const batch=(revision,previousRevision,books)=>({event:'market_batch',payload:{delivery:{model:'best-effort-authoritative-resync',revision,previousRevision,sourceSequenceFrom:null,sourceSequenceThrough:null},books,trades:[],bookDirty:[],marketSettled:[],resyncRequired:false}});
function fixture(prefix=0){
 const bank={schemaVersion:1,mode:'verified_stream_book_input',scope:{tournamentId:tid,markets:[{marketId:'377',exchangeId:'1066'}]},cutoff:at(prefix?800:0),sourceHash:'a'.repeat(64),sessionId:'fixture',sourcePrefixRecords:prefix};
 let state=createStreamBookAdapter(bank);const rows=[];
 return {bank,rows,get state(){return state;},apply(kind,n,extra={},host=n){const row={format:'sig-feed-capture',schemaVersion:1,captureIndex:rows.length,topic,epoch:1,kind,observedAt:at(n),...extra};rows.push({row,availableAt:at(host)});const out=advanceStreamBookAdapter(state,row,bank,{availableAt:at(host)});state=out.state;return out;}};
}
function recover(f){f.apply('recovery_started',100,{generation:1});f.apply('snapshot',200,{receivedAt:at(190),requestStartedAt:at(100),books:[raw(10,180)]});return f.apply('recovery_finished',210,{booksReady:true,generation:1});}
test('recovery preserves original REST receipt and request instead of confirmation time',()=>{
 const f=fixture(),events=recover(f).events,b=events.find(e=>e.kind==='book');assert.equal(b.origin,'rest_recovery');assert.equal(b.quoteReceivedAt,at(190));assert.equal(b.requestStartedAt,at(100));assert.equal(b.confirmedAt,at(210));assert.equal(b.availableAt,at(210));assert.equal(b.decisionEligible,true);
 assert.equal('requestStartedAt' in b.book,false);assert.equal('receivedAt' in b.book,false);
});
test('a stream event has no fictional HTTP start and duplicate broadcasts cannot refresh it',()=>{
 const f=fixture();recover(f);let out=f.apply('event',1000,{receivedAt:at(900),...batch(100,99,[raw(20,850)])},1200),b=out.events.find(e=>e.kind==='book');
 assert.equal(b.origin,'stream');assert.equal(b.requestStartedAt,null);assert.equal(b.quoteReceivedAt,at(900));assert.equal(b.availableAt,at(1200));
 out=f.apply('event',2000,{receivedAt:at(2000),...batch(101,100,[raw(20,850)])});assert.equal(out.events.filter(e=>e.kind==='book').length,0);
 assert.equal(streamBookReport(f.state,f.bank,at(20900)).markets[0].ready,false);
});
test('buffered stream winning recovery retains its earlier receipt with no REST request',()=>{
 const f=fixture();f.apply('recovery_started',100,{generation:1});f.apply('event',400,{receivedAt:at(350),...batch(100,99,[raw(30,340)])});
 f.apply('snapshot',500,{receivedAt:at(490),requestStartedAt:at(100),books:[raw(20,450)]});const b=f.apply('recovery_finished',510,{booksReady:true,generation:1}).events.find(e=>e.kind==='book');
 assert.equal(b.origin,'buffered_stream_recovery');assert.equal(b.requestStartedAt,null);assert.equal(b.quoteReceivedAt,at(350));assert.equal(b.confirmedAt,at(510));
});
test('late consumer retains evidence but cannot backdate fresh eligibility; replay uses exact saved clocks',()=>{
 const f=fixture();recover(f);const b=f.apply('event',1000,{receivedAt:at(900),...batch(100,99,[raw(20,850)])},30000).events.find(e=>e.kind==='book');
 assert.equal(b.decisionEligible,false);assert.equal(b.availableAt,at(30000));assert.equal(b.quoteReceivedAt,at(900));
 const replay=f.rows.reduce((s,r)=>advanceStreamBookAdapter(s,r.row,f.bank,{availableAt:r.availableAt}).state,createStreamBookAdapter(f.bank));assert.deepEqual(replay,f.state);
});
test('all invalidations are forwarded, clear readiness and do not make trades into fills',()=>{
 const f=fixture();recover(f);f.apply('event',900,{receivedAt:at(900),...batch(100,99,[raw(20,850)])});
 const out=f.apply('event',1000,{receivedAt:at(1000),...batch(105,104,[raw(22,950)])});assert.ok(out.events.some(e=>e.kind==='invalidate'));assert.equal(out.events.some(e=>e.kind==='book'),false);
 const r=streamBookReport(f.state,f.bank,at(1001));assert.equal(r.markets[0].ready,false);assert.equal(r.simulatedFills,null);assert.equal(r.ordersSubmitted,0);
});
test('historical prefix warms validator without emitting a backdated decision stream',()=>{
 const f=fixture(3),out=recover(f);assert.equal(out.events.length,0);assert.equal(f.state.counters.acceptedBooks,0);assert.equal(streamBookReport(f.state,f.bank,at(211)).markets[0].ready,false);
 assert.ok(f.apply('event',900,{receivedAt:at(900),...batch(100,99,[raw(20,850)])}).events.some(e=>e.decisionEligible));
});
test('scope, sequence and host clock errors fail without mutating input',()=>{
 const f=fixture();recover(f);const before=structuredClone(f.state),row={...f.rows.at(-1).row,captureIndex:3,observedAt:at(300)};
 assert.throws(()=>advanceStreamBookAdapter(f.state,row,f.bank,{availableAt:at(299)}),/CLOCK/);
 assert.throws(()=>advanceStreamBookAdapter(f.state,{...row,captureIndex:5},f.bank,{availableAt:at(300)}),/reordered/);
 assert.throws(()=>advanceStreamBookAdapter(f.state,{...row,topic:topic.replace('377','999')},f.bank,{availableAt:at(300)}),/Unknown/);assert.deepEqual(before,f.state);
});
