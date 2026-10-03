import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {bulkDipWalletViews} from './app/bulk-dip-view.mjs';
import {ageComparisonWallet} from './app/comparison-view.mjs';
const at='2026-10-03T18:00:00.000Z';
function fixture(){
 const bank={schemaVersion:2,hostVersion:2,name:'bulk-test-v2',mode:'market_wide_bulk_reference_paper',cutoff:at,
  config:{feeBps:0,maxDataAgeMs:20000},policy:{latenciesMs:[1000,3000]},universe:{exchanges:[{marketId:'259',exchangeId:'948',option:'YES'}]}};
 const hash=createHash('sha256').update(JSON.stringify(bank)).digest('hex'),manifest={cohort:bank.name,bankHash:hash};
 const wallet={cash:'99.585000',fees:'0.000000',fills:1,closedTrades:0,realizedPnl:'0.000000',referencePnl:'-0.010000',liquidationPnl:'-0.020000',estimatedLiquidationEquity:'99.980000',
  positions:[{instrument:'259:YES',label:'Wrong party YES',quantity:1,cost:'0.415000',quoteReceivedAt:at,observationAgeMs:0,unsoldShares:0,estimatedProceeds:'0.395000'}],
  pending:[{instrument:'259:YES',label:'Wrong party YES',quantity:1,action:'sell',limit:'0.425000',createdAt:at}],
  recentFills:[{instrument:'259:YES',label:'Wrong party YES',quantity:1,action:'buy',price:'0.415000',fee:'0.000000',at}]};
 const report={cohort:bank.name,bankHash:hash,hostVersion:2,mode:bank.mode,cutoff:at,updatedAt:at,failed:false,sourceClosed:false,networkRequests:0,competitionOrders:0,
  bulkReference:{marketCount:1,candidates:[],counts:{}},bookSequence:10,priceSequence:20,counts:{insufficient_concession:7},wallets:{1000:structuredClone(wallet),3000:structuredClone(wallet)}};
 return {bank,manifest,report,options:{workerAlive:true,now:Date.parse(at)}};
}
test('separate latency wallets preserve losses and replace all partisan labels with scoped YES identities',()=>{
 const f=fixture(),original=structuredClone(f.report),v=bulkDipWalletViews(f.report,f.manifest,f.bank,f.options);
 assert.deepEqual(v.map(x=>x.id),['bulk-dip-1000','bulk-dip-3000']);
 for(const w of v){assert.equal(w.status,'running');assert.equal(w.metrics.liquidationPnl,'-0.020000');assert.equal(w.feeBps,0);
  for(const row of [...w.positions,...w.pending,...w.trades])assert.equal(row.label,'Market 259 · YES');
  assert.equal(w.bulkDip.candidateCount,0);assert.equal(w.bulkDip.gates.insufficient_concession,7);}
 assert.deepEqual(f.report,original);
});
test('mismatched banks, scope, fees, source identity and missing wallets never fall back to historical cohorts',()=>{
 const f=fixture(),view=(report=f.report,manifest=f.manifest,bank=f.bank)=>bulkDipWalletViews(report,manifest,bank,f.options);
 assert.throws(()=>view({...f.report,bankHash:'a'.repeat(64)}));assert.throws(()=>view(f.report,{...f.manifest,cohort:'../other'}));
 assert.throws(()=>view(f.report,f.manifest,{...f.bank,config:{...f.bank.config,feeBps:10}}));
 assert.throws(()=>view({...f.report,cutoff:'2026-10-04T00:00:00Z'}));assert.throws(()=>view({...f.report,wallets:{1000:f.report.wallets[1000]}}));
 for(const group of ['positions','pending','recentFills']){const r=structuredClone(f.report);r.wallets[1000][group][0].instrument='777:YES';assert.throws(()=>view(r));}
 const fee=structuredClone(f.report);fee.wallets[1000].fees='0.010000';assert.throws(()=>view(fee));
});
test('dead worker, failed or closed source and browser-aged depth cannot advertise executable marks',()=>{
 const f=fixture();
 for(const [r,o] of [[f.report,{...f.options,workerAlive:false}],[{...f.report,failed:true},f.options],[{...f.report,sourceClosed:true},f.options],[f.report,{...f.options,now:Date.parse(at)+60001}]]){
  const [v]=bulkDipWalletViews(r,f.manifest,f.bank,o);assert.notEqual(v.status,'running');assert.equal(v.metrics.liquidationPnl,null);assert.equal(v.positions[0].liquidationValue,null);assert.equal(v.metrics.realizedPnl,'0.000000');
 }
 const [v]=bulkDipWalletViews(f.report,f.manifest,f.bank,f.options),aged=ageComparisonWallet(v,Date.parse(at)+21000);
 assert.equal(aged.positions[0].liquidationValue,null);assert.equal(aged.metrics.liquidationPnl,null);
});
