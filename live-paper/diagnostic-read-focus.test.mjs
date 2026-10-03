import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {createHash} from 'node:crypto';
import {createDiagnosticFocusState,advanceDiagnosticFocus,projectDiagnosticFocus,diagnosticDepthFocus,openDiagnosticFocus} from './diagnostic-read-focus.mjs';
import {canonicalJson} from '../execution/ledger.mjs';import {digest} from './model.mjs';import {mergePaperReadFocus} from './bulk-dip-read-focus.mjs';import {prioritizeAnchorReads} from './bid-anchor-read-focus.mjs';
const base=Date.parse('2026-10-03T00:00:00Z'),at=n=>new Date(base+n).toISOString(),hash=x=>createHash('sha256').update(canonicalJson(x)).digest('hex');
const universe={tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',catalogueObservedAt:at(0),exchanges:[{marketId:'354',exchangeId:'1043',option:'YES'}]};
function fixture(){const s=createDiagnosticFocusState(universe),books=[],prices=[];
 const book=(time,kind='session_start',marketId=null)=>{const b={schemaVersion:1,kind,sequence:s.book.sequence+1,previousHash:s.book.hash,sessionId:'test-session',tournamentId:universe.tournamentId,scopeHash:s.bookScopeHash,at:at(time),payload:kind==='invalidation'?{marketId}:{}};const row={...b,hash:hash(b)};books.push(row);advanceDiagnosticFocus(s,'book',row);return row;};
 const price=(time,bid=.6,ask=.605,{request=time-10,missing=false}={})=>{const snapshot={tournamentId:universe.tournamentId,universeHash:s.universe.hash,requestStartedAt:at(request),receivedAt:at(time),sourceTimestamp:null,depthAvailable:false,executable:false,quotes:missing?[]:[{marketId:'354',exchangeId:'1043',option:'YES',latestPrice:bid,bestBid:bid,bestAsk:ask,spread:ask-bid,crossed:bid>=ask}],missingIds:missing?['1043']:[]};const b={schemaVersion:1,sequence:s.price.sequence+1,previous:s.price.hash,at:at(time+1),state:null,event:{type:'market_price_observation',snapshot}},row={...b,hash:digest(b)};prices.push(row);advanceDiagnosticFocus(s,'price',row);return row;};
 const warm=()=>{book(0);for(let i=0;i<5;i++)price(1000+i*60000);};
 const project=now=>projectDiagnosticFocus(s,{pid:process.pid,id:'test-owner'},'edge-test',base+now);
 return {s,books,prices,book,price,warm,project};
}
test('one-cent diagnostics request depth without altering the stronger frozen trading rule',()=>{
 const f=fixture();f.warm();f.price(301000,.585,.59);const p=f.project(301010);assert.equal(p.candidates.length,1);assert.equal(p.candidates[0].grossDiscount,'0.010000');assert.equal(p.candidates[0].depthVerified,false);assert.equal(p.executionAuthority,false);assert.equal(p.networkRequests,0);assert.equal(p.competitionOrders,0);
 assert.equal(p.candidates[0].sourceSequence,6);assert.equal(p.candidates[0].anchor,'0.600000');
});
test('same-minute quotes cannot pad history; gaps and stale candidates cannot request reads',()=>{
 const f=fixture();f.book(0);for(let i=0;i<10;i++)f.price(1000+i*100);f.price(10000,.585,.59);assert.equal(f.project(10010).candidates.length,0);
 const g=fixture();g.warm();g.price(400000,.585,.59);assert.equal(g.project(400010).candidates.length,0);
 const h=fixture();h.warm();h.price(301000,.585,.59);assert.equal(h.project(322000).candidates.length,0);
});
test('invalidation/session stop/missing coverage revoke history and straddling requests cannot revive it',()=>{
 for(const kind of ['invalidation','session_stop']){const f=fixture();f.warm();f.price(301000,.585,.59);f.book(302000,kind,'354');f.price(303000,.585,.59,{request:301500});assert.equal(f.project(303010).candidates.length,0);assert.equal(Object.keys(f.s.history).length,0);}
 const f=fixture();f.warm();f.price(301000,.585,.59);f.price(302000,.585,.59,{missing:true});assert.equal(f.project(302010).candidates.length,0);assert.equal(Object.keys(f.s.history).length,0);
});
test('source chains and scope are validated before history admission',()=>{
 const f=fixture();f.warm();const b=structuredClone(f.prices.at(-1));b.hash='a'.repeat(64);assert.throws(()=>advanceDiagnosticFocus(f.s,'price',b),/PRICE_CHAIN/);
 const wrong=structuredClone(f.books[0]);wrong.tournamentId='other';const{hash:h,...body}=wrong;wrong.hash=hash(body);assert.throws(()=>advanceDiagnosticFocus(createDiagnosticFocusState(universe),'book',wrong),/BOOK_CHAIN/);
});
test('collector canonical universe retains the original scope hash rather than hashing it twice',()=>{
 const f=fixture();f.warm();f.price(301000,.585,.59);const canonical=createDiagnosticFocusState(f.s.universe);for(const item of [...f.books.map(row=>({kind:'book',row})),...f.prices.map(row=>({kind:'price',row}))].sort((a,b)=>Date.parse(a.row.at)-Date.parse(b.row.at)))advanceDiagnosticFocus(canonical,item.kind,item.row);assert.equal(canonical.universe.hash,f.s.universe.hash);assert.equal(projectDiagnosticFocus(canonical,{pid:process.pid,id:'test-owner'},'collector-diagnostic',base+301010).candidates.length,1);assert.throws(()=>createDiagnosticFocusState({...f.s.universe,hash:'a'.repeat(64)}),/UNIVERSE_HASH/);
});
test('validator rejects tampered/future/expired/foreign/authoritative hints and incorrect collector ownership',()=>{
 const f=fixture();f.warm();f.price(301000,.585,.59);const p=f.project(301010);
 const check=(x,now=base+301020)=>diagnosticDepthFocus(x,f.s.universe,{pid:process.pid,lockId:'test-owner',now}),rehash=x=>{const{projectionHash,...body}=x;return{...body,projectionHash:hash(body)}};
 assert.deepEqual(check(p).markets,[{marketId:'354',phase:'depth_required'}]);
 for(const mutate of [x=>x.candidates[0].exchangeId='999',x=>x.executionAuthority=true,x=>x.candidates[0].depthVerified=true,x=>x.at=at(400000),x=>x.lockId='other',x=>x.pid+=1,x=>x.sourceClosed=true,x=>x.candidates[0].expiresAt=at(301001)]){const x=structuredClone(p);mutate(x);assert.equal(check(rehash(x)),null);}
 const tampered=structuredClone(p);tampered.candidates[0].anchor='0.700000';assert.equal(check(tampered),null);assert.equal(check(p,base+322000),null);
});
test('held and pending markets keep priority; diagnostics reuse at most two existing slots',()=>{
 const focus=mergePaperReadFocus({markets:[{marketId:'1',phase:'held'},{marketId:'2',phase:'entry_pending'}]},{markets:[{marketId:'3',phase:'depth_required'},{marketId:'4',phase:'depth_required'}]});
 const plan={reads:[{marketId:'10',reason:'discovery'},{marketId:'11',reason:'discovery'},{marketId:'12',reason:'discovery'}],entryMarketIds:['10','11','12']},next=prioritizeAnchorReads(plan,focus);
 assert.equal(next.reads.length,plan.reads.length);assert.deepEqual(next.bidAnchorReadFocus.marketIds,['1','2']);assert.equal(next.bidAnchorReadFocus.requestsAdded,0);assert.equal(next.bidAnchorReadFocus.executionAuthority,false);
});
test('tail consumption preserves partial records and rejects source truncation',()=>{
 const f=fixture();f.warm();const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-diagnostic-tail-'));
 try{const bookFile=path.join(dir,'public-books.jsonl'),priceFile=path.join(dir,'market-prices.jsonl');fs.writeFileSync(bookFile,f.books.map(x=>JSON.stringify(x)+'\n').join(''));fs.writeFileSync(priceFile,f.prices.map(x=>JSON.stringify(x)+'\n').join(''));const host=openDiagnosticFocus(dir,universe);f.price(301000,.585,.59);const line=JSON.stringify(f.prices.at(-1))+'\n';fs.appendFileSync(priceFile,line.slice(0,30));host.consume();assert.equal(host.project({pid:process.pid,id:'test-owner'},'edge-test',base+301010).candidates.length,0);fs.appendFileSync(priceFile,line.slice(30));host.consume();assert.equal(host.project({pid:process.pid,id:'test-owner'},'edge-test',base+301010).candidates.length,1);fs.truncateSync(priceFile,0);assert.throws(()=>host.consume(),/SOURCE_REPLACED/);
 }finally{assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));assert.ok(path.basename(dir).startsWith('sig-diagnostic-tail-'));fs.rmSync(dir,{recursive:true});}
});
test('deferred initialization yields to STOP and never exposes a partial bootstrap',async()=>{
 const f=fixture();f.book(0);for(let i=0;i<140;i++)f.price(1000+i*100);const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-diagnostic-async-'));
 try{fs.writeFileSync(path.join(dir,'public-books.jsonl'),f.books.map(x=>JSON.stringify(x)+'\n').join(''));fs.writeFileSync(path.join(dir,'market-prices.jsonl'),f.prices.map(x=>JSON.stringify(x)+'\n').join(''));
  const host=openDiagnosticFocus(dir,universe,{deferBootstrap:true});assert.throws(()=>host.consume(),/BOOTSTRAP_PENDING/);let stopped=false;setImmediate(()=>{stopped=true;});await assert.rejects(host.initialize({stopped:()=>stopped}),/STOPPED/);assert.throws(()=>host.project({pid:process.pid,id:'test-owner'},'collector-diagnostic',base+20000),/BOOTSTRAP_PENDING/);
 }finally{assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));assert.ok(path.basename(dir).startsWith('sig-diagnostic-async-'));fs.rmSync(dir,{recursive:true});}
});
test('deferred initialization validates full chains before admitting hints and spread widening revokes them',async()=>{
 const f=fixture();f.warm();f.price(301000,.585,.59);const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-diagnostic-ready-'));
 try{fs.writeFileSync(path.join(dir,'public-books.jsonl'),f.books.map(x=>JSON.stringify(x)+'\n').join(''));fs.writeFileSync(path.join(dir,'market-prices.jsonl'),f.prices.map(x=>JSON.stringify(x)+'\n').join(''));const host=openDiagnosticFocus(dir,universe,{deferBootstrap:true});await host.initialize();assert.equal(host.project({pid:process.pid,id:'test-owner'},'collector-diagnostic',base+301010).candidates.length,1);f.price(302000,.565,.59);fs.appendFileSync(path.join(dir,'market-prices.jsonl'),JSON.stringify(f.prices.at(-1))+'\n');host.consume();assert.equal(host.project({pid:process.pid,id:'test-owner'},'collector-diagnostic',base+302010).candidates.length,0);
 }finally{assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));assert.ok(path.basename(dir).startsWith('sig-diagnostic-ready-'));fs.rmSync(dir,{recursive:true});}
});
