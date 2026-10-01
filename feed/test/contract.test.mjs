import test from 'node:test';
import assert from 'node:assert/strict';
import {config,parseEvent,parseSnapshot,compareVersion,timestamp,DELIVERY_MODEL,instantNs,snapshotPath} from '../contract.mjs';
import {scope,at,book,batch,trade,snapshot,clone} from './helpers.mjs';
const cfg=config(scope);
test('source-shaped batch normalized, unknown fields omitted',()=>{
 const raw=batch(1,0,undefined,{authorization:'Bearer forbidden',trades:[trade()]});const p=parseEvent('market_batch',raw,cfg).payload;
 assert.equal(p.delivery.model,DELIVERY_MODEL);assert.equal(p.trades[0].exchangeId,'701');assert.equal('authorization' in p,false);assert.equal('correlationId' in p.delivery,false);
});
test('compact resync accepts missing arrays and rejects incorrect model',()=>{
 const p=batch();const v=parseEvent('market_batch',{delivery:p.delivery,resyncRequired:true},cfg).payload;assert.deepEqual(v.books,[]);
 p.delivery.model='future';assert.throws(()=>parseEvent('market_batch',p,cfg),/UNKNOWN_DELIVERY/);
});
test('null-sequence trade forces explicit unordered representation',()=>{
 const p=batch(1,0,undefined,{trades:[trade(null,null)]});assert.equal(parseEvent('market_batch',p,cfg).payload.trades[0].sequence,null);
 p.trades[0].id='101';assert.throws(()=>parseEvent('market_batch',p,cfg),/UNORDERED/);
});
test('topic sequence is independent of engine sequence',()=>{
 const p=batch(99,90);p.delivery.sourceSequenceFrom=100000;p.delivery.sourceSequenceThrough=500000;
 assert.equal(parseEvent('market_batch',p,cfg).payload.delivery.revision,99);
});
test('versions compare sequence before exact timestamp instants',()=>{
 assert.equal(compareVersion({sequence:2,at:at(0)},{sequence:1,at:at(1000)}),1);
 assert.equal(compareVersion({sequence:2,at:'2026-10-01T21:00:00.000002Z'},{sequence:2,at:'2026-10-01T21:00:00.000001Z'}),1);
 assert.equal(instantNs('2026-10-01T22:00:00.000001+01:00'),instantNs('2026-10-01T21:00:00.000001Z'));
 assert.equal(compareVersion(null,null),0);assert.equal(compareVersion(null,book().asOf),-1);
});
for(const bad of ['2026-02-31T00:00:00Z','2026-01-01T24:00:00Z','2026-01-01T00:00:61Z','2026-01-01T00:00:00+99:00','not-time'])test(`invalid timestamp rejected: ${bad}`,()=>assert.throws(()=>timestamp(bad)));
test('calendar leap year and nanoseconds retained',()=>{assert.equal(timestamp('2024-02-29T21:00:00.123456789Z'),'2024-02-29T21:00:00.123456789Z');assert.throws(()=>timestamp('2025-02-29T21:00:00Z'));});
test('bad schema variants fail closed',()=>{
 for(const edit of [p=>p.trades=null,p=>p.delivery.revision=0,p=>p.delivery.previousRevision=1,p=>p.delivery.revision=Number.MAX_SAFE_INTEGER+1,p=>p.delivery.sourceSequenceFrom=200,p=>p.books.push(book(701)),p=>p.books[0].asOf=null,p=>delete p.books[0].nextExpiryAt,p=>p.books[0].bids[0].price=NaN,p=>p.books[0].asks[0].quantity=-1,p=>p.books[0].bids.push({price:.5,quantity:1}),p=>p.books[0].exchangeId=999]){
 const p=batch();edit(p);assert.throws(()=>parseEvent('market_batch',p,cfg));
 }
});
test('foreign context and account events rejected',()=>{
 assert.throws(()=>parseEvent('account_batch',{},cfg),/UNSUPPORTED/);
 const p=batch(1,0,undefined,{trades:[trade()]});p.trades[0].tournamentId='22222222-2222-4222-8222-222222222222';assert.throws(()=>parseEvent('market_batch',p,cfg),/CONTEXT/);
});
test('authoritative context selects matching tournament rather than top-level default',()=>{
 const p=snapshot();p.exchanges=[book(999)];assert.equal(parseSnapshot(p,cfg).length,2);
 p.contexts[0].tournament.id='22222222-2222-4222-8222-222222222222';assert.throws(()=>parseSnapshot(p,cfg),/CONTEXT/);
});
test('snapshot completeness and unique exchange requirements',()=>{
 for(const books of [[book(701)],[book(701),book(701)],[book(701),book(999)]])assert.throws(()=>parseSnapshot(snapshot(books),cfg));
 const p=snapshot();p.contexts.push(clone(p.contexts[0]));assert.throws(()=>parseSnapshot(p,cfg),/CONTEXT/);
});
test('REST permits null version and omitted expiry; pushed stream requires version',()=>{
 const b=book(701);b.asOf=null;delete b.nextExpiryAt;const p=parseSnapshot(snapshot([b,book(702)]),cfg);assert.equal(p[0].asOf,null);assert.equal(p[0].expiryKnown,false);
 assert.match(snapshotPath(cfg),/^\/markets\/277\/orderbook\?tournamentId=.*&depth=200$/);
});
test('scope validation rejects duplicate IDs and malformed tournament IDs',()=>{
 assert.throws(()=>config({...scope,exchangeIds:['701','701']}));assert.throws(()=>config({...scope,tournamentId:'secret'}));
});
