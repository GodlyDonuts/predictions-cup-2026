import fs from 'node:fs';import path from 'node:path';import os from 'node:os';import test from 'node:test';import assert from 'node:assert/strict';
import {createBookPublication} from './book-publication.mjs';import {digest} from './model.mjs';import {validateUniverse} from './breadth.mjs';
import {createMarkoutProtocol,studyDipMarkouts,verifyMarkoutProtocol} from './dip-markout-study.mjs';
const universe=JSON.parse(fs.readFileSync(new URL('./price-universe.json',import.meta.url))),u=validateUniverse(universe,universe.tournamentId),spec=universe.exchanges.find(x=>x.marketId==='354'),base=Date.parse('2026-10-03T00:00:00Z'),at=n=>new Date(base+n).toISOString();
function fixture(t){const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-markout-'));let n=0,version=0,sequence=0,previous='',records=[];
 const file=path.join(dir,'books.jsonl'),writer=createBookPublication({file,universe,sessionId:'markout-test',now:()=>base+n});
 t.after(()=>{writer.close();assert.equal(path.dirname(dir),os.tmpdir());assert.ok(path.basename(dir).startsWith('sig-markout-'));fs.rmSync(dir,{recursive:true,force:true});});
 return {price(time,bid=.45,ask=.455,start=time-100){n=time;const snapshot={tournamentId:universe.tournamentId,universeHash:u.hash,requestStartedAt:at(start),receivedAt:at(time-10),sourceTimestamp:null,depthAvailable:false,executable:false,quotes:[{exchangeId:spec.exchangeId,marketId:spec.marketId,option:'YES',latestPrice:bid,bestBid:bid,bestAsk:ask,spread:ask-bid,crossed:false}],missingIds:[]},body={sequence:++sequence,previous,at:at(time),event:{type:'market_price_observation',snapshot},state:null},hash=digest(body);records.push({...body,hash});previous=hash;},
 book(time,bid=.435,ask=.44,quantity=1000){n=time;assert.ok(writer.publishBook({marketId:spec.marketId,response:{endpoint:`/markets/${spec.marketId}/orderbook?tournamentId=${universe.tournamentId}&depth=20`,requestStartedAt:at(time-100),receivedAt:at(time-10),data:{contexts:[{type:'tournament',tournament:{id:universe.tournamentId},orderbook:{exchanges:[{exchangeId:spec.exchangeId,option:'YES',asOf:{sequence:++version,at:at(time-20)},bids:[{price:bid,quantity}],asks:[{price:ask,quantity}]}]}}]}}}));},
 invalidate(time,marketId=spec.marketId){n=time;writer.invalidate({marketId,reason:'MANUAL_INVALIDATION'});},
 inputs(){return {priceBytes:Buffer.from(records.map(x=>JSON.stringify(x)+'\n').join('')),bookBytes:fs.readFileSync(file),universe,bank:createMarkoutProtocol(at(1000000),universe)};},
 seed(){for(const time of [1000,61000,121000,181000,241000])this.price(time);}
 };
}
test('dip events need causal prior history; quote markouts and later depth pairs keep raw/adverse prices separate',t=>{
 const f=fixture(t);f.seed();f.price(300000,.435,.44);f.book(300500);f.book(301000);f.book(302000);f.book(304000);
 for(const time of [350000,400000,450000,500000,550000])f.price(time,.44,.445);
 f.price(600200,.45,.455);f.book(604500,.45,.455);const result=studyDipMarkouts(f.inputs()),e=result.events[0];
 assert.equal(result.events.length,1);assert.deepEqual(e.thresholds,['0.005000','0.010000']);assert.equal(e.quote.status,'observed');assert.equal(e.quote.rawMarkout,'0.010000');assert.equal(e.quote.adverseMarkout,'0.000000');
 assert.equal(e.depth[1000].entrySequence,4,'the same or too-early depth cannot match entry');assert.equal(e.depth[3000].entrySequence,5);assert.equal(e.depth[1000].status,'observed');assert.equal(e.depth[1000].rawMarkout,'0.010000');assert.equal(e.depth[1000].adverseMarkout,'0.000000');
 assert.equal(result.groups.development['0.010000'].quoteMarkouts.events,1);assert.equal(result.paperFills,0);assert.equal(result.competitionOrders,0);assert.equal(result.profitabilityEstablished,false);
});
test('market invalidation and in-flight prices fence references and keep outcomes revoked',t=>{
 const f=fixture(t);f.seed();f.price(300000,.435,.44);f.invalidate(320000);f.price(320100,.45,.455,319900);f.price(600200,.45,.455);
 const result=studyDipMarkouts(f.inputs());assert.equal(result.events.length,1);assert.equal(result.events[0].quote.status,'source_boundary');assert.equal(result.events[0].depth[1000].status,'source_boundary');assert.ok(result.counts.price_request_before_boundary>0);
});
test('source gaps, missing depth and right-censoring are retained instead of selecting complete paths',t=>{
 const f=fixture(t);f.seed();f.price(300000,.435,.44);f.book(300500,.435,.44,99);f.price(450000,.45,.455);
 const result=studyDipMarkouts(f.inputs());assert.equal(result.events[0].quote.status,'coverage_gap');assert.equal(result.events[0].depth[1000].status,'confirmation_not_observed');assert.equal(result.groups.development['0.010000'].depthMarkouts[1000].observedPrices.count,0);assert.equal(result.groups.development['0.010000'].depthMarkouts[1000].events,1);
});
test('chronological purge separates near-cutoff events; prospective events use original frozen protocol',t=>{
 const f=fixture(t);f.seed();f.price(300000,.435,.44);const inputs=f.inputs();inputs.bank=createMarkoutProtocol(at(290000),universe);const result=studyDipMarkouts(inputs);assert.equal(result.events[0].phase,'prospective');assert.equal(result.events[0].quote.status,'right_censored');
 inputs.bank=createMarkoutProtocol(at(350000),universe);assert.equal(studyDipMarkouts(inputs).events[0].phase,'purged_boundary');
 inputs.bank=createMarkoutProtocol(at(1000000),universe);assert.equal(studyDipMarkouts(inputs).events[0].phase,'development');
 const changed=structuredClone(inputs.bank);changed.protocol.grossConcessions.push('0.001000');assert.throws(()=>verifyMarkoutProtocol(changed,universe),/PROTOCOL/);
 const missingIdentity=structuredClone(inputs.bank);missingIdentity.codeIdentity={};assert.throws(()=>verifyMarkoutProtocol(missingIdentity,universe),/FROZEN_CODE/);
});
test('both original source chains reject consumed mutations and do not count an incomplete tail',t=>{
 const f=fixture(t);f.seed();f.price(300000,.435,.44);const inputs=f.inputs(),changed={...inputs,priceBytes:Buffer.from(inputs.priceBytes)};changed.priceBytes[changed.priceBytes.indexOf(Buffer.from('0.45'))+3]=54;
 assert.throws(()=>studyDipMarkouts(changed),/PRICE_CHAIN/);const bookChanged={...inputs,bookBytes:Buffer.from(inputs.bookBytes)};bookChanged.bookBytes[bookChanged.bookBytes.indexOf(Buffer.from('markout-test'))+1]=120;assert.throws(()=>studyDipMarkouts(bookChanged),/PUBLIC_BOOK_CHAIN_INVALID/);
 const tail=studyDipMarkouts({...inputs,priceBytes:Buffer.concat([inputs.priceBytes,Buffer.from('{partial')])});assert.equal(tail.sources.price.incompleteTailBytes,8);
});
