import test from 'node:test';
import assert from 'node:assert/strict';
import {comparisonNameValid,comparisonWalletViews,ageComparisonWallet} from './app/comparison-view.mjs';
const at='2026-10-02T13:00:00.000Z',manifest={schemaVersion:1,cohort:'entry-common-v1',bankHash:'a'.repeat(64)};
const fixture=()=>({mode:'prospective_common_feed_entry_comparison_paper',cohort:manifest.cohort,bankHash:manifest.bankHash,status:'running',updatedAt:at,cutoff:at,
 sharedExecution:{feeBps:0},maxMarkAgeMs:80000,networkRequests:0,competitionOrders:0,marketIds:['377','354','277','305'],
 strategies:Object.fromEntries(['historical_dip','shock_rebound'].map(key=>[key,{cash:'1000.000000',realizedPnl:'-0.050000',referencePnl:'-1.000000',liquidationPnl:'-1.100000',closedTrades:1,
  uniqueObservations:5,acceptedSignals:1,contaminationCounts:{account_exposure_or_orders:2,unexpected:'must-not-export'},fills:2,recentFills:[],pending:[],positions:[{instrument:'1043:YES',quantity:4,cost:'0.440000',quoteReceivedAt:'2026-10-02T12:59:59.000Z',observationAgeMs:1000,unsoldShares:0,estimatedProceeds:'0.420000'}],privateField:'must-not-export'}]))});
test('matched projections preserve losses and separate wallets without leaking raw state',()=>{
 const v=comparisonWalletViews(fixture(),manifest,Date.parse(at));assert.deepEqual(v.map(x=>x.id),['comparison-dip','comparison-rebound']);
 assert.equal(v[0].metrics.realizedPnl,'-0.050000');assert.equal(v[0].performance.closedRoundTrips,1);assert.equal(v[0].comparison.contamination.account_exposure_or_orders,2);assert.ok(!JSON.stringify(v).includes('must-not-export'));
});

test('client clock expires fetched marks during polling failure without erasing realized accounting',()=>{
 const v=comparisonWalletViews(fixture(),manifest,Date.parse(at))[0];
 assert.equal(v.metrics.liquidationPnl,'-1.100000');
 for(const now of [Date.parse(at)+60001,Date.parse(at)-1,NaN]){
  const aged=ageComparisonWallet(v,now);assert.equal(aged.metrics.liquidationPnl,null);assert.equal(aged.positions[0].estimatedProceeds,null);assert.equal(aged.metrics.realizedPnl,'-0.050000');
 }
 const short=structuredClone(v);short.comparison.maxMarkAgeMs=20000;
 assert.equal(ageComparisonWallet(short,Date.parse(at)+20000).metrics.referencePnl,null);
 assert.equal(v.metrics.liquidationPnl,'-1.100000');
 const failed=structuredClone(v);failed.status='failed';assert.equal(ageComparisonWallet(failed,Date.parse(at)).positions[0].liquidationValue,null);
});

test('client aging does not count server projection delay twice',()=>{
 const r=fixture();r.maxMarkAgeMs=20000;
 const v=comparisonWalletViews(r,manifest,Date.parse(at)+10000)[0];
 assert.equal(v.positions[0].quoteAgeMs,11000);
 const next=ageComparisonWallet(v,Date.parse(at)+11000);
 assert.equal(next.positions[0].quoteAgeMs,12000);assert.equal(next.metrics.liquidationPnl,'-1.100000');
});
test('stale or failed comparison retains accounting and withholds current market values',()=>{
 for(const r of [fixture(),{...fixture(),status:'failed'}]){const v=comparisonWalletViews(r,manifest,Date.parse(at)+60001)[0];assert.equal(v.metrics.realizedPnl,'-0.050000');assert.equal(v.metrics.liquidationPnl,null);assert.equal(v.positions[0].liquidationValue,null);assert.equal(v.positions[0].quoteInvalidated,true);}
});
test('fixed local cohort identity rejects path traversal, bank mismatch and foreign execution reports',()=>{
 for(const n of ['../secret','C:/secret','con','com9','LPT1','a/b'])assert.equal(comparisonNameValid(n),false);
 assert.throws(()=>comparisonWalletViews(fixture(),{...manifest,bankHash:'b'.repeat(64)}));
 assert.throws(()=>comparisonWalletViews({...fixture(),competitionOrders:1},manifest));
 assert.throws(()=>comparisonWalletViews({...fixture(),strategies:{}},manifest));
});
