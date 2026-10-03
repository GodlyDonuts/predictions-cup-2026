import test from 'node:test';
import assert from 'node:assert/strict';
import {comparePassivePaths} from './passive-entry-study.mjs';
const base=Date.parse('2026-10-03T00:00:00Z'),at=n=>new Date(base+n).toISOString();
function fixture(){
 const first={sequence:1,hash:'price-1',at:at(1000),event:{snapshot:{requestStartedAt:at(900),receivedAt:at(990),missingIds:[],quotes:[{marketId:'354',exchangeId:'1043',option:'YES',bestBid:.435,bestAsk:.44,crossed:false}]}}};
 const prices=[first],books=[],events=[{id:'354:1',marketId:'354',exchangeId:'1043',priceSequence:1,priceHash:first.hash,availableAt:first.at,receivedAt:first.event.snapshot.receivedAt,thresholds:['0.010000'],quote:{entryAsk:'0.440000'}}];
 const book=(time,bid=.425,ask=.435,quantity=1000,version='v'+time,request=time-100)=>books.push({sequence:books.length+1,hash:'book-'+time,kind:'book',at:at(time),payload:{book:{marketId:'354',exchangeId:'1043',requestStartedAt:at(request),receivedAt:at(time-10),versionKey:version,bids:[{price:bid.toFixed(6),quantity}],asks:[{price:ask.toFixed(6),quantity}]}}});
 const boundary=time=>books.push({sequence:books.length+1,hash:'boundary',kind:'invalidation',at:at(time),payload:{marketId:'354'}});
 return {prices,books,events,book,boundary,run:()=>comparePassivePaths({prices,books,events,cutoff:at(1000000)})};
}
test('resting entry requires admission plus two independent price-through observations; cost stays its limit',()=>{
 const f=fixture();f.book(3000);f.book(5000);f.book(6000,.415,.42);f.book(7000,.415,.42);f.book(70000,.44,.445);
 const r=f.run(),c=r.cases[0],bid=c.strategies.rest_bid[1000],improve=c.strategies.improve_bid[1000],cross=c.strategies.cross_ask[1000];
 assert.equal(bid.entryPrice,'0.425000');assert.equal(bid.entry.availableAt,at(7000));assert.equal(bid.crossing.availableAt,at(6000));
 assert.equal(improve.entryPrice,'0.430000');assert.equal(cross.entryPrice,'0.440000');
 assert.equal(bid.exits[60000].netMarkout,'0.010000');assert.equal(improve.exits[60000].netMarkout,'0.005000');assert.equal(cross.exits[60000].netMarkout,'-0.005000');
 assert.equal(r.paperFills,0);assert.equal(r.competitionOrders,0);assert.equal(r.networkRequests,0);
 assert.equal(c.strategies.rest_bid[3000].stage,'post_only_would_cross','a delayed arrival cannot retroactively rest and fill');
});
test('a touch or displayed queue disappearance alone never creates a maker entry',()=>{
 const f=fixture();f.book(3000);f.book(5000);f.book(6000,.42,.425);f.book(7000,.42,.425);
 let r=f.run();assert.equal(r.groups.development.strategies.rest_bid[1000].entries,0);
 const q=fixture();q.book(3000);q.book(5000);q.book(6000,.425,.435,900);q.book(7000,.425,.435,100);
 r=q.run();for(const name of ['rest_bid','improve_bid'])assert.equal(r.groups.development.strategies[name][1000].entries,0);
});
test('repeated versions and requests already in flight at the first crossing cannot confirm a fill',()=>{
 const f=fixture();f.book(3000);f.book(5000);f.book(6000,.415,.42,1000,'cross');f.book(7000,.415,.42,1000,'cross');f.book(8000,.415,.42,1000,'second',5900);
 assert.equal(f.run().groups.development.strategies.rest_bid[1000].entries,0);
 f.book(9000,.415,.42,1000,'independent');assert.equal(f.run().cases[0].strategies.rest_bid[1000].entry.availableAt,at(9000));
});
test('every crossing needs one-percent size and an uninterrupted qualifying episode',()=>{
 const f=fixture();f.book(3000);f.book(5000);f.book(6000,.415,.42,99);f.book(7000,.415,.42);f.book(8000,.415,.42,99);
 assert.equal(f.run().groups.development.strategies.rest_bid[1000].entries,0);
 f.book(9000,.415,.42);assert.equal(f.run().groups.development.strategies.rest_bid[1000].entries,0);
 f.book(10000,.415,.42);assert.equal(f.run().groups.development.strategies.rest_bid[1000].entries,1);
});
test('entry and exit caps, adverse ticks and independent exit timing are enforced',()=>{
 const f=fixture();f.book(3000);f.book(5000,.435,.445);f.book(6500,.435,.445);f.book(8000,.425,.435);f.book(65000,.45,.455);f.book(70000,.45,.455);
 const cell=f.run().cases[0].strategies.cross_ask[1000];assert.equal(cell.entry.availableAt,at(8000),'adverse ask above the original cap cannot enter');
 assert.equal(cell.entryPrice,'0.440000');assert.equal(cell.exits[60000].source.availableAt,at(70000));assert.equal(cell.exits[60000].exitPrice,'0.445000');assert.equal(cell.exits[60000].netMarkout,'0.005000');
});
test('expiry reasons remain latched through later boundaries; observed exits stay immutable',()=>{
 const f=fixture();f.book(3000);f.book(5000);f.boundary(130000);const r=f.run();
 assert.equal(r.cases[0].strategies.rest_bid[1000].stage,'price_through_not_observed');
 assert.equal(r.groups.development.strategies.rest_bid[1000].outcomes[60000].opportunities,1);
 const done=fixture();done.book(3000);done.book(5000);done.book(6000,.415,.42);done.book(7000,.415,.42);done.book(70000,.44,.445);done.boundary(100000);
 const exits=done.run().cases[0].strategies.rest_bid[1000].exits;assert.equal(exits[60000].status,'observed');assert.equal(exits[180000].status,'source_boundary');
});
test('stale publications revoke pending entries and source clocks/scope never silently fall back',()=>{
 const f=fixture();f.book(3000);f.book(30000);f.books.at(-1).payload.book.receivedAt=at(3100);f.books.at(-1).payload.book.requestStartedAt=at(3000);
 assert.equal(f.run().cases[0].strategies.rest_bid[1000].stage,'stale_book');
 const bad=fixture();bad.book(3000);bad.books[0].payload.book.exchangeId='999';assert.throws(()=>bad.run(),/BOOK_SCOPE/);
 bad.books[0].payload.book.exchangeId='1043';bad.books[0].payload.book.requestStartedAt=at(4000);assert.throws(()=>bad.run(),/BOOK_CLOCK/);
});
test('future baseline labels cannot alter admissions; missing opportunities and empty regimes remain visible',()=>{
 const f=fixture();f.book(3000);const before=f.run();f.events[0].quote.rawMarkout='999.000000';f.events[0].quote.status='observed';assert.deepEqual(f.run(),before);
 f.events[0].priceHash='foreign';assert.throws(()=>f.run(),/OPPORTUNITY_SOURCE/);
 const none=fixture();none.events[0].thresholds=['0.020000'];none.book(3000);const r=none.run();assert.equal(r.groups.development.opportunities,0);assert.equal(r.groups.prospective.opportunities,0);
});
