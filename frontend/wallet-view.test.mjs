import test from 'node:test';
import assert from 'node:assert/strict';
import {paperWalletView,paperCohorts,walletConnection} from './app/wallet-view.mjs';
import {GET} from './app/api/wallets/route.js';

const at='2026-10-02T10:00:00.000Z';
const report=()=>({updatedAt:at,status:'running',strategy:{cash:'99990.000000',realizedPnl:'1.000000',referencePnl:'-4.000000',referenceEquity:'99996.000000',liquidationPnl:null,estimatedLiquidationEquity:null,fills:3,positions:[{instrument:'1:YES',label:'Fixture',quantity:20,cost:'10.000000',enteredAt:at,observationAgeMs:15000,quoteAgeMs:15000,quoteReceivedAt:at,quoteInvalidated:false,unsoldShares:12,estimatedProceeds:'3.000000',privateField:'must-not-export'}],recentFills:[],pending:[]}});

test('paper projection preserves open losses and explains incomplete liquidation without exposing extra fields',()=>{
 const v=paperWalletView(report(),'dip','Conservative dip');
 assert.equal(v.metrics.realizedPnl,'1.000000');assert.equal(v.metrics.referencePnl,'-4.000000');assert.equal(v.metrics.liquidationPnl,null);
 assert.equal(v.positions[0].unsoldShares,12);assert.equal(v.positions[0].observationAgeMs,15000);assert.equal(v.positions[0].liquidationValue,null);assert.equal(v.positions[0].estimatedProceeds,'3.000000');
 assert.ok(!JSON.stringify(v).includes('must-not-export'));
 const complete=report();complete.strategy.positions[0].unsoldShares=0;assert.equal(paperWalletView(complete,'dip','Dip').positions[0].liquidationValue,'3.000000');
 complete.strategy.positions[0].observationAgeMs=null;complete.strategy.positions[0].quoteAgeMs=300000;const stale=paperWalletView(complete,'dip','Dip').positions[0];assert.equal(stale.liquidationValue,null);assert.equal(stale.quoteAgeMs,300000);
 complete.strategy.positions[0].observationAgeMs=1000;complete.strategy.positions[0].quoteInvalidated=true;assert.equal(paperWalletView(complete,'dip','Dip').positions[0].liquidationValue,null);
});

test('official connection cannot say fresh for missing sections, errors, stale or future clocks',()=>{
 const now=Date.parse(at),full={official:true,available:true,collectorStatus:'running',sourceTimes:Object.fromEntries(['positions','orders','fills','pnl'].map(k=>[k,at])),health:{status:'fresh'},errors:{}};
 assert.equal(walletConnection(full,now).healthy,true);
 const partial=structuredClone(full);delete partial.sourceTimes.orders;assert.equal(walletConnection(partial,now).status,'partial');
 const future=structuredClone(full);future.sourceTimes.orders='2026-10-02T10:01:00.000Z';assert.equal(walletConnection(future,now).status,'invalid_clock');assert.equal(walletConnection(future,now).healthy,false);
 assert.equal(walletConnection({...full,errors:{fills:{reason:'unavailable'}}},now).status,'degraded');
 assert.equal(walletConnection({...full,collectorStatus:'failed'},now).status,'degraded');assert.equal(walletConnection(full,now+300001).status,'stale');
});

test('wallet route includes responsive paper and uses only fixed loopback endpoints',async t=>{
 const calls=[];t.mock.method(globalThis,'fetch',async url=>{calls.push(String(url));assert.match(String(url),/^http:\/\/127\.0\.0\.1:8787\/api\/(account|execution|dip|dip-scale|dip-responsive|shock)$/);return {ok:true,json:async()=>String(url).endsWith('/account')?{available:true,positions:[],pending:[],trades:[],health:{status:'partial'}}:String(url).endsWith('/execution')?{status:'monitoring',ordersSubmitted:2,fills:[{}],privateField:'must-not-export'}:report()};});
 const result=await (await GET()).json();assert.deepEqual(result.wallets.map(x=>x.id),['official',...paperCohorts.map(([id])=>id)]);assert.equal(calls.length,6);assert.ok(calls.includes('http://127.0.0.1:8787/api/shock'));assert.ok(calls.includes('http://127.0.0.1:8787/api/dip-responsive'));assert.equal(result.execution.ordersSubmitted,2);assert.equal(result.execution.attributedFills,1);assert.ok(!JSON.stringify(result).includes('must-not-export'));
 assert.equal(result.wallets.find(x=>x.id==='dip-responsive').metrics.referencePnl,'-4.000000');
});
