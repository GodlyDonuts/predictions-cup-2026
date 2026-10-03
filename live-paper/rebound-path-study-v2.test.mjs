import test from 'node:test';import assert from 'node:assert/strict';import {compareReboundPaths} from './rebound-path-study-v2.mjs';
import {compareReboundPaths as compareFrozenV1} from './rebound-path-study.mjs';
const base=Date.parse('2026-10-03T00:00:00Z'),at=ms=>new Date(base+ms).toISOString();
function fixture(){let n=0;const prices=[],books=[];
 const quote=(time,bid=.435,ask=.44,start=time-100)=>{const sequence=++n,row={sequence,hash:'price-'+sequence,at:at(time),event:{snapshot:{requestStartedAt:at(start),receivedAt:at(time-10),missingIds:[],quotes:[{marketId:'354',exchangeId:'1043',option:'YES',crossed:false,bestBid:bid,bestAsk:ask}]}}};prices.push(row);return row;};
 const book=(time,bid=.435,ask=.44,quantity=1000,version='v'+time)=>books.push({sequence:books.length+1,hash:'book-'+time,kind:'book',at:at(time),payload:{book:{marketId:'354',exchangeId:'1043',versionKey:version,requestStartedAt:at(time-100),receivedAt:at(time-10),bids:[{price:bid.toFixed(6),quantity}],asks:[{price:ask.toFixed(6),quantity}]}}});
 const first=quote(1000),events=[{id:'354:1',marketId:'354',exchangeId:'1043',priceSequence:1,priceHash:first.hash,availableAt:first.at,receivedAt:first.event.snapshot.receivedAt,thresholds:['0.010000'],reference:{anchor:'0.450000',bidDrop:'0.015000'},quote:{entryAsk:'0.440000'}}];
 return {quote,book,prices,books,events,run:()=>compareReboundPaths({events,prices,books,cutoff:at(1000000)})};
}
test('confirmed rebound cannot enter on its confirmation quote; exact caps and both latencies remain causal',()=>{
 const f=fixture();f.quote(3000,.43,.435);f.quote(6000,.435,.44);f.quote(8000,.44,.445);f.quote(10000,.435,.44);f.quote(65000,.45,.455);f.quote(72000,.45,.455);f.quote(76000,.45,.455);
 const r=f.run(),c=r.cases[0];assert.equal(c.branches.rebound.signal.availableAt,at(6000));
 for(const latency of [1000,3000]){const q=c.branches.rebound.trials[latency].quote;assert.equal(q.entryAvailableAt,at(10000),'the confirmation and later expensive ask cannot fill');assert.equal(q.entryPrice,'0.440000');assert.equal(q.exits[60000].rawMarkout,'0.010000');}
 assert.equal(c.branches.immediate.trials[1000].quote.entryAvailableAt,at(3000));assert.equal(r.paperFills,0);assert.equal(r.competitionOrders,0);
});
test('depth confirmation needs size and a later independent version/request before entry and fixed-horizon exit',()=>{
 const f=fixture();f.quote(3000,.43,.435);f.quote(6000,.435,.44);f.book(6500,.435,.44,99);f.book(7000);f.book(8000);f.book(9000);f.book(10500);f.book(70500,.45,.455);f.book(75000,.45,.455);
 const c=f.run().cases[0],one=c.branches.rebound.trials[1000].depth,three=c.branches.rebound.trials[3000].depth;
 assert.equal(one.preparedAt,at(7000));assert.equal(one.entryAvailableAt,at(9000));assert.equal(three.entryAvailableAt,at(10500));
 assert.equal(one.exits[60000].rawMarkout,'0.010000');assert.equal(one.exits[60000].adverseMarkout,'0.000000');assert.equal(three.exits[60000].status,'observed');
 assert.equal(one.exits[180000].status,'right_censored');
});
test('unconfirmed and invalidated opportunities remain in all strategy denominators',()=>{
 const f=fixture();f.quote(3000,.435,.44);f.books.push({sequence:1,hash:'boundary',kind:'invalidation',at:at(4000),payload:{marketId:'354'}});f.quote(70000,.45,.455);
 const g=f.run().groups.development;assert.equal(g.opportunities,1);assert.equal(g.strategies.rebound.signals,0);assert.equal(g.strategies.rebound.outcomes[1000][60000].quote.opportunities,1);assert.equal(g.strategies.rebound.outcomes[1000][60000].quote.statuses.source_boundary,1);
 assert.equal(g.strategies.immediate.outcomes[1000][60000].quote.statuses.source_boundary,1);
});
test('mutated future baseline outcomes cannot change rebound signals; source identity and clocks are checked',()=>{
 const f=fixture();f.quote(3000,.43,.435);f.quote(6000,.435,.44);const before=f.run();f.events[0].quote.rawMarkout='999.000000';f.events[0].quote.status='observed';assert.deepEqual(f.run(),before);
 f.events[0].priceHash='foreign';assert.throws(()=>f.run(),/OPPORTUNITY_SOURCE/);f.events[0].priceHash='price-1';f.prices[1].event.snapshot.requestStartedAt=at(4000);assert.throws(()=>f.run(),/PRICE_CLOCK/);
});
test('a late publication keeps the original opportunity in denominators with both entries revoked',()=>{
 const f=fixture();f.prices[0].at=at(22000);f.events[0].availableAt=at(22000);f.quote(30000,.44,.445);
 const g=f.run().groups.development;assert.equal(g.opportunities,1);
 for(const name of ['immediate','rebound'])assert.equal(g.strategies[name].outcomes[1000][60000].quote.statuses.stale_publication,1);
});

test('reproduced v1 late-boundary bug is corrected without discarding original opportunities',()=>{
 const f=fixture();f.quote(62000,.455,.46);f.book(62000,.455,.46);
 f.books.push({sequence:2,hash:'boundary',kind:'invalidation',at:at(70000),payload:{marketId:'354'}});
 const legacy=compareFrozenV1({events:f.events,prices:f.prices,books:f.books,cutoff:at(1000000)});
 const fixed=f.run();
 for(const latency of [1000,3000]){
  assert.equal(legacy.cases[0].branches.immediate.trials[latency].quote.stage,'source_boundary');
  assert.equal(fixed.cases[0].branches.immediate.trials[latency].quote.stage,'entry_not_observed');
  assert.equal(fixed.cases[0].branches.immediate.trials[latency].depth.stage,'confirmation_not_observed');
 }
 assert.equal(fixed.groups.development.opportunities,legacy.groups.development.opportunities);
});

test('a boundary after expiry respects the deadline even if no earlier observation advanced it',()=>{
 const f=fixture();f.books.push({sequence:1,hash:'boundary',kind:'invalidation',at:at(130000),payload:{marketId:'354'}});
 const c=f.run().cases[0];assert.equal(c.watch.status,'confirmation_not_observed');
 assert.equal(c.branches.immediate.trials[1000].quote.stage,'entry_not_observed');
 assert.equal(c.branches.immediate.trials[1000].depth.stage,'confirmation_not_observed');
});

test('repeated missing quotes and invalidations latch their first terminal reason',()=>{
 const f=fixture();f.quote(3000,.455,.46);f.prices.at(-1).event.snapshot={requestStartedAt:at(2900),receivedAt:at(2990),missingIds:['1043'],quotes:[]};
 f.books.push({sequence:1,hash:'boundary',kind:'invalidation',at:at(70000),payload:{marketId:'354'}});
 const c=f.run().cases[0];assert.equal(c.watch.status,'missing_quote');
 assert.equal(c.branches.immediate.trials[1000].quote.stage,'missing_quote');
 assert.equal(c.branches.immediate.trials[1000].depth.stage,'missing_quote');
});

test('completed labels stay immutable while only unfinished horizons are revoked',()=>{
 const f=fixture();f.quote(3000);f.quote(65000,.45,.455);
 f.books.push({sequence:1,hash:'boundary',kind:'invalidation',at:at(100000),payload:{marketId:'354'}});
 const exits=f.run().cases[0].branches.immediate.trials[1000].quote.exits;
 assert.equal(exits[60000].status,'observed');assert.equal(exits[60000].rawMarkout,'0.010000');
 assert.equal(exits[180000].status,'source_boundary');
});
