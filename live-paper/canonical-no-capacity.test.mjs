import test from 'node:test';import assert from 'node:assert/strict';
import {CANONICAL_NO_POLICY,evaluateCanonicalNoCapacity as evaluate} from './canonical-no-capacity.mjs';
import {planCapacityPair} from './canonical-capacity-monitor.mjs';
const h='a'.repeat(64),now='2026-10-03T22:00:00.000Z';
export const fixture=()=>({now,policy:{...CANONICAL_NO_POLICY},
 review:{schemaVersion:1,id:'nh-no-v1',kind:'conditional_no_pair_review',tournamentId:'contest',evidenceHash:h,
  recordedAt:'2026-10-03T21:30:00.000Z',expiresAt:'2026-10-04T03:30:00.000Z',
  assumptions:['Ordinary current Senate ballot D and R winner affiliations are exclusive.'],
  legs:[{marketId:'381',exchangeId:'1070',outcome:'NO',rulesHash:h},{marketId:'382',exchangeId:'1071',outcome:'NO',rulesHash:h}],
  scenarios:[{id:'D',category:'ordinary',payouts:[0,1]},{id:'R',category:'ordinary',payouts:[1,0]},
   {id:'other',category:'ordinary',payouts:[1,1]},{id:'fusion_or_override',category:'exception',payouts:[0,0]}]},
 account:{schemaVersion:1,positionsAt:'2026-10-03T21:59:50.000Z',ordersAt:'2026-10-03T21:59:51.000Z',excludedExchangeIds:[]},
 books:[['381','1070','0.805000','0.810000',240],['382','1071','0.220000','0.235000',635]].map(([marketId,exchangeId,bid,ask,q],i)=>({tournamentId:'contest',marketId,exchangeId,
  instrument:exchangeId+':YES',freshnessPolicy:'authoritative_receipt_v1',publicationSequence:100+i,publicationHash:h,
  requestStartedAt:`2026-10-03T21:59:${i?'57':'54'}.000Z`,receivedAt:`2026-10-03T21:59:${i?'57':'54'}.200Z`,
  publishedAt:`2026-10-03T21:59:${i?'57':'54'}.201Z`,sequence:String(i+1),sourceAt:`2026-10-03T21:59:${i?'57':'54'}.300000000Z`,versionKey:(i+1)+`:2026-10-03T21:59:${i?'57':'54'}.300000000Z`,
  bids:[{price:bid,quantity:q},{price:i?'0.215000':'0.800000',quantity:20000}],asks:[{price:ask,quantity:1000}]}))});
test('canonical YES bids, not YES asks, determine explicit NO capacity and exception losses',()=>{
 const f=fixture(),before=JSON.stringify(f),r=evaluate(f);
 assert.equal(r.matchedQuantity,'6');assert.equal(r.acquisitionCost,'5.930000');assert.equal(r.conditionalOrdinaryMinimumProfit,'0.070000');
 assert.equal(r.maximumSingleLegCostAtRisk,'4.710000');assert.equal(r.stressedMinimumProfit,'-5.930000');
 assert.equal(r.nativeNoObserved,false);assert.equal(r.fills,null);assert.equal(r.executionAuthority,false);assert.equal(r.profitabilityEstablished,false);
 assert.equal(JSON.stringify(f),before);f.books[0].asks[0].price='0.900000';assert.equal(evaluate(f).acquisitionCost,r.acquisitionCost);
});
test('full displayed capacity differs from one-percent pilot capacity',()=>{const f=fixture();f.policy={...f.policy,depthFractionBps:10000,maxCost:'25000',maxUnmatchedCost:'20000'};
 const r=evaluate(f);assert.equal(r.matchedQuantity,'635');assert.equal(r.acquisitionCost,'627.450000');assert.equal(r.conditionalOrdinaryMinimumProfit,'7.550000');
 assert.equal(r.stopReason,'marginal_edge');assert.equal(r.stressedMinimumProfit,'-627.450000');});
test('exact hard single-leg boundary never borrows profitable opposite proceeds',()=>{const f=fixture();f.policy.maxUnmatchedCost='4.709999';assert.equal(evaluate(f).matchedQuantity,'5');});
test('whole request window blocks seemingly fresh mismatched legs',()=>{const f=fixture();f.books[0].requestStartedAt='2026-10-03T21:59:46.000Z';assert.equal(evaluate(f).reason,'whole_window_skew');f.books[0].requestStartedAt='2026-10-03T21:59:39.999Z';assert.equal(evaluate(f).reason,'stale_request');});
test('account outages, future account snapshots, own positions/orders prevent capacity',()=>{
 const f=fixture();f.account=null;assert.equal(evaluate(f).reason,'account_unavailable');const g=fixture();g.account.ordersAt=now;assert.equal(evaluate(g).reason,'account_not_causal_or_fresh');
 const k=fixture();k.account.excludedExchangeIds=['1071'];assert.equal(evaluate(k).reason,'own_account_exposure');});
test('review must be known before BOTH requests and expire independently of heartbeat',()=>{
 const f=fixture();f.review.recordedAt='2026-10-03T21:59:55.000Z';assert.equal(evaluate(f).reason,'review_not_known_at_request');const g=fixture();g.review.expiresAt=now;assert.equal(evaluate(g).reason,'review_not_current');});
test('exception stresses cannot be omitted or mislabeled ordinary guarantees',()=>{const f=fixture();f.review.scenarios=f.review.scenarios.filter(s=>s.category==='ordinary');assert.throws(()=>evaluate(f),/SCENARIOS/);const g=fixture();g.review.scenarios[3].category='ordinary';assert.throws(()=>evaluate(g),/SCENARIOS/);});
test('native book identity, reversed/future clocks, duplicate and off-tick levels fail',()=>{for(const change of [f=>f.books[0].instrument='1070:NO',f=>f.books[0].tournamentId='other',f=>f.books[0].receivedAt='2026-10-03T22:00:01.000Z',f=>f.books[0].requestStartedAt=now,f=>f.books[0].bids.push(f.books[0].bids[0]),f=>f.books[0].bids[0].price='0.805001',f=>f.books[0].bids[0].quantity=1.5]){const f=fixture();change(f);assert.throws(()=>evaluate(f));}});
test('one-percent floor creates no quantities from tiny or zero depth',()=>{const f=fixture();f.books.forEach(b=>b.bids=b.bids.map(l=>({...l,quantity:99})));assert.equal(evaluate(f).reason,'fractional_depth');});
test('nonzero cost sensitivity changes economics and unknown fees fail',()=>{const f=fixture();delete f.policy.feePerShare;assert.throws(()=>evaluate(f));const g=fixture();g.policy.feePerShare='0.010000';assert.equal(evaluate(g).matchedQuantity,'0');});
test('paired admission replaces slots after retained risk, requires both slots and respects exclusions',()=>{
 const f=fixture(),plan={reads:[{marketId:'363',reason:'bid_anchor_paper_risk'},{marketId:'297',reason:'bid_anchor_paper_risk'},
  {marketId:'4',reason:'discovery'},{marketId:'5',reason:'discovery'}],entryMarketIds:['4','5']},context={cycle:4,now:Date.parse(now)};
 const p=planCapacityPair(plan,f.review,context);assert.equal(p.reads.length,4);assert.deepEqual(p.reads.slice(0,2),plan.reads.slice(0,2));assert.deepEqual(p.reads.slice(2).map(x=>x.marketId),['381','382']);
 assert.equal(p.canonicalCapacityFocus.requestsAdded,0);assert.equal(planCapacityPair(plan,f.review,{...context,cycle:5}),plan);
 assert.equal(planCapacityPair(plan,f.review,{...context,excludedExchangeIds:['1070']}),plan);
 const protectedPlan=structuredClone(plan);protectedPlan.reads[2].reason='future_risk';assert.equal(planCapacityPair(protectedPlan,f.review,context),protectedPlan);
});
test('independent per-unit brute force agrees across joint depth and capital boundaries',()=>{
 for(let cents=0;cents<=1000;cents+=7){const f=fixture();f.policy.maxCost=(cents/100).toFixed(2);const r=evaluate(f);
  let best=0,cost=0n;const a=['200000','205000'].map(BigInt),b=785000n;
  for(let q=1;q<=6;q++){cost+=a[q<=2?0:1]+b;if(cost<=BigInt(cents)*10000n)best=q;}
  assert.equal(r.matchedQuantity,String(best));}
});
