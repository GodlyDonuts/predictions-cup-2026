import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {createBookPublication,createBookPublicationState,applyBookPublication} from './book-publication.mjs';
import {readNativePublicationPrefix,nativeExecutable} from './rust-backend.mjs';
import {canonicalJson} from '../execution/ledger.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),hash=v=>sha(canonicalJson(v));
const base=Date.parse('2026-10-04T11:00:00Z'),at=n=>new Date(base+n).toISOString();
const universe={tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',exchanges:[{marketId:'2',exchangeId:'102',option:'YES'},{marketId:'1',exchangeId:'101',option:'YES'}]};
function fixture(t){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-native-publication-')),journal=path.join(dir,'source.jsonl'),scope=path.join(dir,'scope.json');fs.writeFileSync(scope,JSON.stringify(universe));let clock=0;
 const w=createBookPublication({file:journal,universe,sessionId:'correctness-source-1',now:()=>base+clock});
 t.after(()=>{w.close('STOP');assert.equal(path.dirname(dir),os.tmpdir());assert.ok(path.basename(dir).startsWith('sig-native-publication-'));fs.rmSync(dir,{recursive:true,force:true});});
 const book=(market,n,{seq=1,source=n+100,bid=.895,ask=.9,start=n-100}={})=>{clock=n;assert.equal(w.publishBook({marketId:market,response:{endpoint:'/markets/'+market+'/orderbook?tournamentId='+universe.tournamentId+'&depth=20',requestStartedAt:at(start),receivedAt:at(n),data:{contexts:[{type:'tournament',tournament:{id:universe.tournamentId},orderbook:{exchanges:[{exchangeId:market==='1'?'101':'102',option:'YES',asOf:{sequence:seq,at:at(source)},bids:[{price:bid,quantity:10000}],asks:[{price:ask,quantity:10000}]}]}}]}}}),true);};
 book('1',1000);book('2',1500);clock=1600;w.invalidate({marketId:'1',reason:'MANUAL_INVALIDATION'});book('1',2200,{source:1100});book('1',3000,{seq:2,source:3100,bid:.9,ask:.905});clock=4000;w.close('STOP');
 const rows=fs.readFileSync(journal,'utf8').trimEnd().split('\n').map(JSON.parse);
 return{dir,journal,scope,rows};
}
function rechain(rows){let previous='';return rows.map((r,i)=>{const {hash:old,...body}=structuredClone(r);body.sequence=i+1;body.previousHash=previous;const record={...body,hash:hash(body)};previous=record.hash;return record;});}
function js(rows){let s=createBookPublicationState(universe);for(const row of rows)s=applyBookPublication(s,row);return s;}
async function native(f,rows,name,raw){const bytes=Buffer.from(raw??rows.map(r=>JSON.stringify(r)+'\n').join('')),journal=path.join(f.dir,name+'.jsonl'),out=path.join(f.dir,name+'.json');fs.writeFileSync(journal,bytes);return readNativePublicationPrefix({journal,scope:f.scope,endByte:bytes.length,expectedSha:sha(bytes),out});}

test('native prefix preserves exact state across unchanged versions, market barriers, source boundaries and later sessions',async t=>{
 const f=fixture(t);for(const[count,name]of [[3,'open'],[5,'market-barrier'],[7,'closed']]){const rows=f.rows.slice(0,count),r=await native(f,rows,name);assert.deepEqual(r.state,js(rows));assert.equal(r.networkRequests,0);assert.equal(r.competitionOrders,0);}
 const later=structuredClone(f.rows);const start=structuredClone(later[0]);start.sessionId='correctness-source-2';start.at=at(4500);later.push(start);const b=structuredClone(later[1]);b.sessionId=start.sessionId;b.at=at(5000);Object.assign(b.payload.book,{requestStartedAt:at(4900),receivedAt:at(5000),sourceAt:at(5100).slice(0,-1)+'000000Z',sequence:'3',versionKey:'3:'+at(5100).slice(0,-1)+'000000Z',sourceEventAgeMs:-100});later.push(b);const rows=rechain(later),r=await native(f,rows,'reopened');assert.deepEqual(r.state,js(rows));assert.equal(r.state.sessions.length,2);assert.equal(r.state.closed,false);
});
test('native canonical hashes agree with JavaScript for safe integer JSON 1.0, old source events and nanosecond/year precision',async t=>{
 const f=fixture(t),rows=f.rows.slice(0,2);let raw=rows.map(r=>JSON.stringify(r)+'\n').join('').replaceAll('"schemaVersion":1','"schemaVersion":1.0').replaceAll('"quantity":10000','"quantity":10000.0');assert.deepEqual((await native(f,rows,'integer-json',raw)).state,js(JSON.parse('['+raw.trim().split('\n').join(',')+']')));
 const old=structuredClone(rows);old[1].payload.book.sourceAt='1970-01-01T00:00:00.000000001Z';old[1].payload.book.versionKey='1:'+old[1].payload.book.sourceAt;old[1].payload.book.sourceEventAgeMs=Date.parse(old[1].payload.book.receivedAt);const oldRows=rechain(old);assert.deepEqual((await native(f,oldRows,'old-event')).state,js(oldRows));
 const precise=structuredClone(rows),b=precise[1].payload.book;b.sourceAt='9999-12-31T12:00:00.000000001Z';b.receivedAt=b.requestStartedAt='9999-12-31T12:00:00.000Z';b.versionKey='1:'+b.sourceAt;b.sourceEventAgeMs=0;precise[1].at=b.receivedAt;const exact=rechain(precise);assert.deepEqual((await native(f,exact,'year-nanos')).state,js(exact));
});
test('native admission rejects the same malformed complete records as the original validator',async t=>{
 const f=fixture(t);const cases=[
  ['extra-root',r=>r[1].rawAccount='PRIVATE'],['scope',r=>r[1].scopeHash='0'.repeat(64)],['tournament',r=>r[1].tournamentId='wrong'],['session',r=>r[1].sessionId='wrong'],['session-ascii',r=>r[1].sessionId='not valid'],['clock-rollback',r=>r[2].at=at(999)],
  ['identity',r=>r[1].payload.book.exchangeId='102'],['instrument',r=>r[1].payload.book.instrument='102:YES'],['book-extra',r=>r[1].payload.book.rawAccount='PRIVATE'],['version-string',r=>r[1].payload.book.sequence='01'],['version-unsafe',r=>r[1].payload.book.sequence='9007199254740992'],['version-key',r=>r[1].payload.book.versionKey='bad'],
  ['noncanonical-ms',r=>r[1].payload.book.receivedAt=at(1000).replace('.000','')],['bad-calendar',r=>r[1].payload.book.receivedAt='2026-02-30T11:00:00.000Z'],['leap-second',r=>r[1].payload.book.receivedAt='2026-10-04T11:00:60.000Z'],['source-offset',r=>r[1].payload.book.sourceAt=r[1].payload.book.sourceAt.replace('Z','+00:00')],['source-too-future',r=>{const b=r[1].payload.book;b.sourceAt=at(4000).slice(0,-1)+'000000Z';b.versionKey='1:'+b.sourceAt;b.sourceEventAgeMs=-3000;}],
  ['event-age',r=>r[1].payload.book.sourceEventAgeMs=0],['slow-receipt',r=>r[1].payload.book.requestStartedAt=at(-20001)],['level-extra',r=>r[1].payload.book.bids[0].private='PRIVATE'],['depth-empty',r=>r[1].payload.book.bids=[]],['depth-21',r=>r[1].payload.book.bids=Array.from({length:21},(_,i)=>({price:(.895-i*.005).toFixed(6),quantity:100}))],['tick',r=>r[1].payload.book.bids[0].price='0.896000'],['crossed',r=>r[1].payload.book.bids[0].price='0.900000'],['quantity-fraction',r=>r[1].payload.book.bids[0].quantity=1.5],['quantity-unsafe',r=>r[1].payload.book.bids[0].quantity=9007199254740992],
  ['invalid-reason',r=>r[3].payload.reason='UNKNOWN'],['invalid-market',r=>r[3].payload.marketId='99'],['barrier',r=>r[4].payload.book.requestStartedAt=at(1599)],['version-conflict',r=>r[4].payload.book.bids[0].price='0.890000'],['source-rollback',r=>{const b=r[5].payload.book;b.sourceAt=at(1000).slice(0,-1)+'000000Z';b.versionKey='2:'+b.sourceAt;b.sourceEventAgeMs=2000;}],['version-rollback',r=>{r[5].payload.book.sequence='0';r[5].payload.book.versionKey='0:'+r[5].payload.book.sourceAt;}],['stop-reason',r=>r[6].payload.reason='UNKNOWN']
 ];
 for(const[name,change]of cases){const changed=structuredClone(f.rows);change(changed);const rows=rechain(changed);assert.throws(()=>js(rows),name);await assert.rejects(native(f,rows,'invalid-'+name),undefined,name);assert.equal(fs.existsSync(path.join(f.dir,'invalid-'+name+'.json')),false,name);}
});
test('prefix byte/hash, scope bytes, binary identity and immutable output are independent bindings',async t=>{
 const f=fixture(t),bytes=fs.readFileSync(f.journal),out=path.join(f.dir,'bound.json');const options={journal:f.journal,scope:f.scope,endByte:bytes.length,expectedSha:sha(bytes),out};
 await assert.rejects(readNativePublicationPrefix({...options,expectedSha:'0'.repeat(64)}),/SOURCE_PREFIX_HASH_MISMATCH/);await assert.rejects(readNativePublicationPrefix({...options,endByte:bytes.length-1,expectedSha:sha(bytes.subarray(0,-1))}),/PARTIAL/);await assert.rejects(readNativePublicationPrefix({...options,pinnedBinaryHash:'0'.repeat(64)}),/binary identity/);
 const r=await readNativePublicationPrefix({...options,pinnedBinaryHash:sha(fs.readFileSync(nativeExecutable))});assert.equal(r.sourceHash,options.expectedSha);await assert.rejects(readNativePublicationPrefix(options),/overwrite/);const proof=JSON.parse(fs.readFileSync(out+'.receipt.json'));assert.equal(proof.outputHash,sha(fs.readFileSync(out)));assert.equal(proof.binaryHash,sha(fs.readFileSync(nativeExecutable)));
});
