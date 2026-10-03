import test from 'node:test';
import assert from 'node:assert/strict';
import { evaluatePayoutCapacity as evaluate } from './payout-capacity.mjs';
const h = 'a'.repeat(64), now = '2026-10-03T21:00:00.000Z';
export function fixture() {
  const legs = [{marketId:'1',exchangeId:'101',outcome:'NO',rulesHash:h},{marketId:'2',exchangeId:'102',outcome:'NO',rulesHash:h}];
  return { now, policy:{maxAgeMs:5000,maxSkewMs:1000,maxReviewAgeMs:3600000,maxQuantity:100,
    maxCost:'100.00',maxUnmatchedCost:'100.00',minMarginalEdge:'0.01',minTotalProfit:'0.01'},
    review:{id:'pair-1',kind:'reviewed_conditional_scenarios',tournamentId:'contest',evidenceHash:h,
      completeUnderAssumptions:true,reviewedAt:'2026-10-03T20:30:00.000Z',expiresAt:'2026-10-03T22:00:00.000Z',
      assumptions:['Only D, R or neither wins; overlapping party affiliations excluded by reviewed hypothesis.'],legs,
      scenarios:[{id:'D',payouts:[0,1]},{id:'R',payouts:[1,0]},{id:'other',payouts:[1,1]}]},
    books:legs.map((l,i)=>({...l,tournamentId:'contest',nativeOutcome:true,status:'active',sourceHash:h,version:`v${i}`,
      requestStartedAt:'2026-10-03T20:59:59.500Z',receivedAt:'2026-10-03T20:59:59.700Z',
      tickSize:'0.005',adversePricePerShare:'0.005',feePerShare:'0.001',fixedFee:'0.10',
      asks:[{price:i?'0.50':'0.40',quantity:100}]}))};
}
test('exact conditional costs and flags, no mutation',()=>{
 const f=fixture(), before=JSON.stringify(f),r=evaluate(f);
 assert.equal(r.matchedQuantity,'100'); assert.equal(r.acquisitionCost,'91.400000');
 assert.equal(r.conditionalMinimumProfit,'8.600000');assert.equal(r.maximumSingleLegCostAtRisk,'50.700000');
 assert.equal(r.scenarioProfits[2].profit,'108.600000');assert.equal(r.acceptedForResearch,true);
 assert.equal(r.executionAuthority,false);assert.equal(r.fills,null);assert.equal(JSON.stringify(f),before);
});
test('fusion scenario removes conditional floor',()=>{const f=fixture();f.review.scenarios.push({id:'fusion',payouts:[0,0]});assert.equal(evaluate(f).acceptedForResearch,false);});
test('fees never silently default',()=>{const f=fixture();delete f.books[0].feePerShare;assert.throws(()=>evaluate(f),/INVALID_DECIMAL/);});
test('budget includes fixed costs exactly',()=>{const f=fixture();f.policy.maxCost='9.32';assert.equal(evaluate(f).matchedQuantity,'10');f.policy.maxCost='9.319999';assert.equal(evaluate(f).matchedQuantity,'9');});
test('unmatched cap includes that legs fixed and variable costs',()=>{const f=fixture();f.policy.maxUnmatchedCost='5.16';assert.equal(evaluate(f).matchedQuantity,'10');f.policy.maxUnmatchedCost='5.159999';assert.equal(evaluate(f).matchedQuantity,'9');});
test('depth breakpoints stop at unprofitable marginal level',()=>{const f=fixture();f.books[0].asks=[{price:'0.40',quantity:7},{price:'0.45',quantity:3},{price:'0.49',quantity:100}];const r=evaluate(f);assert.equal(r.matchedQuantity,'10');assert.deepEqual(r.segments.map(x=>x.quantity),['7','3']);assert.equal(r.acquisitionCost,'9.470000');});
test('second leg bounds available capacity',()=>{const f=fixture();f.books[1].asks[0].quantity=3;assert.equal(evaluate(f).matchedQuantity,'3');});
test('fixed fee can make positive marginal opportunity negative in total',()=>{const f=fixture();f.books.forEach(b=>b.fixedFee='20');f.policy.maxCost='200';const r=evaluate(f);assert.equal(r.reason,'TOTAL_PROFIT');assert.equal(r.acceptedForResearch,false);assert.equal(r.conditionalMinimumProfit,'-31.200000');});
test('same-exchange YES/NO at ask sum above one rejected',()=>{const f=fixture();f.review.legs[1]={...f.review.legs[0],outcome:'YES'};f.books[1]={...f.books[1],...f.review.legs[1]};f.review.scenarios=f.review.scenarios.slice(0,2);f.books[0].asks[0].price='0.505';f.books[1].asks[0].price='0.505';assert.equal(evaluate(f).matchedQuantity,'0');});
test('duplicate native leg rejected',()=>{const f=fixture();f.review.legs[1]={...f.review.legs[0]};assert.throws(()=>evaluate(f),/DUPLICATE_LEG/);});
test('bad scope and fabricated complement rejected',()=>{for(const [key,value]of[['exchangeId','999'],['marketId','999'],['outcome','YES'],['tournamentId','other'],['rulesHash','b'.repeat(64)],['nativeOutcome',false],['status','settled']]){const f=fixture();f.books[0][key]=value;assert.throws(()=>evaluate(f),/BOOK_SCOPE/);}});
test('future, reversed and old request clocks rejected',()=>{for(const patch of [{receivedAt:'2026-10-03T21:00:01.000Z'},{requestStartedAt:'2026-10-03T20:59:59.900Z'},{requestStartedAt:'2026-10-03T20:59:54.999Z'}]){const f=fixture();Object.assign(f.books[0],patch);assert.throws(()=>evaluate(f),/STALE_BOOK/);}});
test('whole request/receipt window skew checked',()=>{const f=fixture();f.books[0].requestStartedAt='2026-10-03T20:59:58.000Z';assert.throws(()=>evaluate(f),/SKEWED_BOOKS/);});
test('expired and future review rejected',()=>{for(const patch of [{expiresAt:now},{reviewedAt:'2026-10-03T21:00:01.000Z'},{reviewedAt:'2026-10-03T19:00:00.000Z'}]){const f=fixture();Object.assign(f.review,patch);assert.throws(()=>evaluate(f),/EXPIRED_REVIEW/);}});
test('timestamps require strict canonical dates',()=>{for(const v of ['2026-02-30T21:00:00.000Z','today','2026-10-03T21:00:00Z']){const f=fixture();f.now=v;assert.throws(()=>evaluate(f),/INVALID_CLOCK/);}});
test('depth invalid ordering, duplicates, empty, off-tick and zero quantity rejected',()=>{for(const asks of [[],[{price:'0.401',quantity:1}],[{price:'0.40',quantity:0}],[{price:'0.40',quantity:2},{price:'0.40',quantity:3}],[{price:'0.50',quantity:2},{price:'0.40',quantity:3}]]){const f=fixture();f.books[0].asks=asks;assert.throws(()=>evaluate(f),/INVALID_DEPTH/);}});
test('numeric prices/exponents/excess precision rejected',()=>{for(const v of [0.4,'4e-1','0.4000001','-0.4','NaN','01.0']){const f=fixture();f.books[0].asks[0].price=v;assert.throws(()=>evaluate(f),/INVALID_DECIMAL/);}});
test('fractional, unsafe and negative quantity rejected',()=>{for(const q of [1.1,Number.MAX_SAFE_INTEGER+1,-1]){const f=fixture();f.books[0].asks[0].quantity=q;assert.throws(()=>evaluate(f),/INVALID_INTEGER/);}});
test('missing assumptions, scenarios and duplicate scenario IDs rejected',()=>{for(const patch of [{completeUnderAssumptions:false},{assumptions:[]},{scenarios:[]},{scenarios:[{id:'a',payouts:[0,1]},{id:'a',payouts:[1,0]}]},{scenarios:[{id:'a',payouts:[0.5,1]}]}]){const f=fixture();Object.assign(f.review,patch);assert.throws(()=>evaluate(f));}});
test('input fingerprint changes with book source',()=>{const f=fixture(),a=evaluate(f);f.books[0].sourceHash='b'.repeat(64);assert.notEqual(evaluate(f).inputHash,a.inputHash);});
test('zero maximum quantity produces no cost',()=>{const f=fixture();f.policy.maxQuantity=0;const r=evaluate(f);assert.equal(r.acquisitionCost,'0.000000');assert.equal(r.acceptedForResearch,false);});
test('big integer arithmetic survives values beyond safe money integers',()=>{const f=fixture();f.policy.maxQuantity=10000000000000;f.policy.maxCost='9999999999999';f.policy.maxUnmatchedCost='9999999999999';f.books.forEach(b=>b.asks[0].quantity=10000000000000);const r=evaluate(f);assert.equal(r.acquisitionCost,'9120000000000.200000');assert.equal(r.conditionalMinimumProfit,'879999999999.800000');});
test('exhaustive small budget sweep agrees with brute force',()=>{for(let cents=0;cents<2000;cents++){const f=fixture();f.policy.maxCost=(cents/100).toFixed(2);const r=evaluate(f);let q=0;for(let n=1;n<=100;n++)if(200000n+912000n*BigInt(n)<=BigInt(cents)*10000n)q=n;assert.equal(r.matchedQuantity,String(q));}});

test('one exchange cannot bind two different markets',()=>{const f=fixture();f.review.legs[1].exchangeId=f.review.legs[0].exchangeId;f.review.legs[1].outcome='YES';assert.throws(()=>evaluate(f),/EXCHANGE_MARKET_CONFLICT/);});
test('adverse price outside binary domain rejected',()=>{const f=fixture();f.books[0].adversePricePerShare='0.605';assert.throws(()=>evaluate(f),/INVALID_ADVERSE_PRICE/);});

test('same native YES/NO cannot both pay one',()=>{const f=fixture();f.review.legs[1]={...f.review.legs[0],outcome:'YES'};f.books[1]={...f.books[1],...f.review.legs[1]};assert.throws(()=>evaluate(f),/CONTRADICTORY_NATIVE_PAYOUT/);});
