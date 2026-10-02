import test from 'node:test';
import assert from 'node:assert/strict';
import {limitEntryWalletViews} from './app/limit-entry-view.mjs';
import {ageComparisonWallet} from './app/comparison-view.mjs';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {GET} from './app/api/wallets/route.js';

const at='2026-10-02T15:00:00.000Z',now=Date.parse(at),hash='a'.repeat(64);
function fixture(){
 const wallet={cash:'99.000000',realizedPnl:'0.005000',referencePnl:'0.015000',referenceEquity:'100.015000',liquidationPnl:'0.010000',estimatedLiquidationEquity:'100.010000',closedTrades:2,
  positions:[{instrument:'x',quantity:10,cost:'1.000000',quoteReceivedAt:at,observationAgeMs:0,quoteInvalidated:false,estimatedProceeds:'1.010000',unsoldShares:0}],
  recentFills:[{instrument:'x',quantity:10,action:'buy',price:'0.100000',fee:'0.000000',at}],
  pending:[{instrument:'y',quantity:3,remaining:3,action:'buy',limit:'0.105000',createdAt:at}],entryOutcomes:{orders:4,expiredUnobserved:1,expiredObserved:1}};
 return {manifest:{schemaVersion:1,cohort:'limit-test',bankHash:hash},report:{mode:'prospective_price_protected_limit_entry_paper',cohort:'limit-test',bankHash:hash,
  execution:{feeBps:0,entryLifetimeMs:30000,maximumQuoteAgeMs:20000},networkRequests:0,competitionOrders:0,updatedAt:at,status:'running',cutoff:at,marketIds:['377','354'],
  strategies:{immediate_dip:structuredClone(wallet),price_protected_hybrid:structuredClone(wallet)}}};
}
test('two independent paper wallets retain trades, pending actual limit and observation outcomes',()=>{
 const f=fixture(),views=limitEntryWalletViews(f.report,f.manifest,now);
 assert.deepEqual(views.map(v=>v.id),['limit-immediate','limit-hybrid']);
 assert.equal(views[1].comparison.kind,'limit_entry');assert.equal(views[1].comparison.entryOutcomes.expiredUnobserved,1);
 assert.equal(views[0].trades[0].fee,'0.000000');assert.equal(views[0].pending[0].limit,'0.105000');assert.equal(views[0].positions[0].liquidationValue,'1.010000');
});
test('browser ages actual quote freshness without fresh API response',()=>{
 const f=fixture(),v=limitEntryWalletViews(f.report,f.manifest,now)[0],aged=ageComparisonWallet(v,now+20001);
 assert.equal(aged.metrics.liquidationPnl,null);assert.equal(aged.positions[0].liquidationValue,null);assert.equal(aged.metrics.realizedPnl,'0.005000');
 assert.equal(v.positions[0].liquidationValue,'1.010000');
});
test('failed worker preserves accounting but withholds all current marked values',()=>{
 const f=fixture();f.report.status='failed';const v=limitEntryWalletViews(f.report,f.manifest,now)[0];
 assert.equal(v.metrics.referencePnl,null);assert.equal(v.metrics.realizedPnl,'0.005000');assert.equal(v.metrics.cash,'99.000000');
});
test('reject identity, fee, source and execution contract mismatches',()=>{
 for(const modify of [f=>f.manifest.cohort='../escape',f=>f.report.bankHash='b'.repeat(64),f=>f.report.cohort='foreign',f=>f.report.competitionOrders=1,
  f=>f.report.networkRequests=1,f=>f.report.execution.feeBps=1,f=>f.report.execution.entryLifetimeMs=60000,f=>f.report.execution.maximumQuoteAgeMs=60000,
  f=>delete f.report.strategies.price_protected_hybrid]){const f=fixture();modify(f);assert.throws(()=>limitEntryWalletViews(f.report,f.manifest,now));}
});
test('route displays activated cohort and retains unavailable tabs if its report fails',async t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-limit-view-')),previous=process.env.SIG_PAPER_DIR;process.env.SIG_PAPER_DIR=dir;
 t.after(()=>{if(previous===undefined)delete process.env.SIG_PAPER_DIR;else process.env.SIG_PAPER_DIR=previous;fs.rmSync(dir,{recursive:true,force:true});});
 t.mock.method(globalThis,'fetch',async url=>{assert.match(String(url),/^http:\/\/127\.0\.0\.1:8787\/api\/(account|execution|dip|dip-scale|dip-responsive|shock)$/);return {ok:false};});
 const f=fixture(),folder=path.join(dir,'limit-entry-cohorts',f.manifest.cohort);fs.mkdirSync(folder,{recursive:true});
 fs.writeFileSync(path.join(dir,'limit-entry-active.json'),JSON.stringify(f.manifest));fs.writeFileSync(path.join(folder,'stats.json'),JSON.stringify(f.report));
 let views=(await (await GET()).json()).wallets.filter(w=>w.id.startsWith('limit-'));assert.equal(views.length,2);assert.equal(views[0].available,true);
 fs.writeFileSync(path.join(folder,'stats.json'),'{');views=(await (await GET()).json()).wallets.filter(w=>w.id.startsWith('limit-'));
 assert.equal(views.length,2);assert.ok(views.every(w=>w.available===false));
});
