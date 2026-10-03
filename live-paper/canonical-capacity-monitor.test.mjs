import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {createHash} from 'node:crypto';
import {digest} from './model.mjs';import {createBookPublication} from './book-publication.mjs';
import {capacityCodeIdentity,openCanonicalCapacityMonitor} from './canonical-capacity-monitor.mjs';
import {evaluateCanonicalNoCapacity} from './canonical-no-capacity.mjs';import {fixture} from './canonical-no-capacity.test.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex');
test('collector publication -> durable capacity -> exact resume; no duplicate frame or stale-account reinterpretation',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'canonical-capacity-')),u={tournamentId:'bda92870-621e-47b0-bc3c-3602c5c26f55',exchanges:[{marketId:'381',exchangeId:'1070',option:'YES'},{marketId:'382',exchangeId:'1071',option:'YES'}]};
 let time=Date.parse('2026-10-03T21:59:52.000Z');const source=path.join(dir,'public-books.jsonl'),publication=createBookPublication({file:source,universe:u,sessionId:'test-collector',now:()=>time});
 try{const f=fixture(),cohort='nh-canonical-capacity-test',d=path.join(dir,'canonical-capacity-cohorts',cohort);fs.mkdirSync(d,{recursive:true});f.review.tournamentId=u.tournamentId;
  const bytes=fs.readFileSync(source),p=publication.status(),bank={schemaVersion:1,name:cohort,tournamentId:u.tournamentId,cutoff:new Date(time).toISOString(),review:f.review,policy:f.policy,
   codeIdentity:capacityCodeIdentity(),sourcePrefixBytes:bytes.length,sourcePrefixHash:sha(bytes),sourcePrefixSequence:p.sequence,sourcePrefixJournalHash:p.hash};
  fs.writeFileSync(path.join(d,'bank.json'),JSON.stringify(bank));fs.writeFileSync(path.join(d,'journal.jsonl'),'');fs.writeFileSync(path.join(dir,'canonical-capacity-active.json'),JSON.stringify({cohort,bankHash:digest(bank)}));
  const account={positions:{at:f.account.positionsAt,rows:[]},orders:{at:f.account.ordersAt,rows:[]},errors:{}};fs.writeFileSync(path.join(dir,'official-account.json'),JSON.stringify(account));
  const w=openCanonicalCapacityMonitor({runtimeDir:dir,universe:u,now:()=>time});
  const publish=(book,seq)=>{time=Date.parse(book.publishedAt);const response={endpoint:'/markets/'+book.marketId+'/orderbook?tournamentId='+u.tournamentId+'&depth=20',requestStartedAt:book.requestStartedAt,receivedAt:book.receivedAt,data:{marketId:book.marketId,contexts:[{type:'tournament',tournament:{id:u.tournamentId},orderbook:{exchanges:[{exchangeId:book.exchangeId,option:'YES',asOf:{sequence:seq,at:book.sourceAt},bids:book.bids.map(l=>({...l,price:Number(l.price)})),asks:book.asks.map(l=>({...l,price:Number(l.price)}))}]}}]}};assert.equal(publication.publishBook({marketId:book.marketId,response}),true);};
  publish(f.books[0],1);assert.equal(w.inspect(publication.status()),false);publish(f.books[1],2);assert.equal(w.inspect(publication.status()),true);
  assert.equal(w.status().journalSequence,1);assert.equal(w.status().last.matchedQuantity,'6');assert.equal(w.inspect(publication.status()),false);
  const row=JSON.parse(fs.readFileSync(path.join(d,'journal.jsonl'),'utf8').trim());const expected=evaluateCanonicalNoCapacity({review:f.review,policy:f.policy,books:f.review.legs.map(l=>publication.status().books[l.exchangeId+':YES']),account:f.account,now:row.at});assert.deepEqual(row.result,expected);
  const restored=openCanonicalCapacityMonitor({runtimeDir:dir,universe:u,now:()=>time});assert.equal(restored.status().journalHash,w.status().journalHash);assert.deepEqual(restored.status().last,w.status().last);assert.equal(restored.inspect(publication.status()),false);
  const original=fs.readFileSync(path.join(d,'journal.jsonl'));fs.writeFileSync(path.join(d,'journal.jsonl'),Buffer.from(original.toString().replace('5.930000','5.940000')));assert.throws(()=>restored.inspect(publication.status()),/JOURNAL_CHANGED/);
 }finally{publication.close('STOP');fs.rmSync(dir,{recursive:true,force:true});}
});
test('uninitialized monitor has no side effects, no network and no additional process',()=>{const dir=fs.mkdtempSync(path.join(os.tmpdir(),'canonical-capacity-none-'));try{assert.equal(openCanonicalCapacityMonitor({runtimeDir:dir,universe:{}}),null);assert.deepEqual(fs.readdirSync(dir),[]);}finally{fs.rmSync(dir,{recursive:true,force:true});}});
