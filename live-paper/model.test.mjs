import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {initialState,normalizeBook,observe,report,assertState,sampleEquity,validateConfig} from './model.mjs';
const config=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url)));
const start=Date.parse('2026-10-01T23:00:00Z');
function book(n,{bid=.4,ask=.41,qty=10000,seq=n}={}){const at=new Date(start+n*30000).toISOString();return{marketId:'377',instrument:'9:YES',exchangeId:'9',sequence:String(seq),sourceAt:at,receivedAt:at,requestStartedAt:at,versionKey:String(seq)+':'+at,bids:[{price:String(bid),quantity:qty}],asks:[{price:String(ask),quantity:qty}]};}
test('next-snapshot taker control pays spread, adverse tick and fees; no decision-time fills',()=>{
 let s=initialState(config,'test',new Date(start).toISOString());s=observe(s,book(1),config);assert.equal(s.strategies.buy_hold_benchmark.fills.length,0);s=observe(s,book(2),config);
 const v=s.strategies.buy_hold_benchmark;assert.equal(v.fills.length,1);assert.equal(v.fills[0].price,'0.415000');assert.equal(v.fills[0].quantity,10);assert.equal(v.cash,'99995.845850');assert.equal(v.fees,'0.004150');assertState(s,config);
 const r=report(s,config,book(2).receivedAt);assert.equal(r.strategies.buy_hold_benchmark.referencePnl,'-0.104150');assert.equal(r.strategies.buy_hold_benchmark.liquidationPnl,'-0.208100');assert.equal(r.ordersSentToCompetition,0);
});
test('risk/depth restrictions underfill and never spend reserve',()=>{
 const c={...config,cashReserve:'99999.000000'};let s=initialState(c,'r',new Date(start).toISOString());s=observe(s,book(1),c);s=observe(s,book(2,{qty:150}),c);const v=s.strategies.buy_hold_benchmark;assert.equal(v.positions['9:YES'].quantity,1);assert.equal(v.fills[0].quantity,1);assertState(s,c);
});
test('zero depth fraction capacity, price movement and stale intent cannot manufacture fills',()=>{
 let s=initialState(config,'r',new Date(start).toISOString());s=observe(s,book(1),config);s=observe(s,book(2,{ask:.43}),config);assert.equal(s.strategies.buy_hold_benchmark.fills.length,0);s=observe(s,book(3,{qty:50}),config);assert.equal(s.strategies.buy_hold_benchmark.fills.length,0);s=observe(s,book(7),config);assert.equal(s.strategies.buy_hold_benchmark.fills.length,0);assert.equal(s.strategies.buy_hold_benchmark.expiredIntents,1);
});
test('lagged mean-reversion enters after warmup, exits with conserved basis and loss',()=>{
 const c={...config,warmupObservations:3,entryZ:.5,minEdge:'0.005000',stopLoss:'0.030000'};let s=initialState(c,'r',new Date(start).toISOString());
 for(const[n,bid,ask]of [[1,.5,.51],[2,.51,.52],[3,.49,.5],[4,.45,.46],[5,.45,.46],[6,.4,.41],[7,.4,.41]])s=observe(s,book(n,{bid,ask}),c);
 const v=s.strategies.mean_reversion;assert.equal(v.fills[0].action,'buy');assert.equal(v.fills[1].action,'sell');assert.equal(v.realized,'-0.708600');assert.equal(v.cash,'99999.291400');assertState(s,c);
});
test('stale aggregate marks and insufficient liquidation depth produce unavailable equity',()=>{
 let s=initialState(config,'r',new Date(start).toISOString());s=observe(s,book(1),config);s=observe(s,book(2),config);let r=report(s,config,new Date(start+999999).toISOString());assert.equal(r.strategies.buy_hold_benchmark.referenceEquity,null);
 s=observe(s,book(3,{qty:50}),config);r=report(s,config,book(3).receivedAt);assert.equal(r.strategies.buy_hold_benchmark.estimatedLiquidationEquity,null);assert.equal(r.strategies.buy_hold_benchmark.positions[0].unsoldShares,10);
});
test('rollback, config change and damaged accounting fail closed; state changes isolated',()=>{
 const s=initialState(config,'r',new Date(start).toISOString());const t=observe(s,book(1),config);assert.equal(s.observations,0);assert.throws(()=>observe(t,book(2,{seq:0}),config),/rollback/);assert.throws(()=>assertState(t,{...config,feeBps:50}),/identical/);
 const damaged=structuredClone(t);damaged.strategies.mean_reversion.cash='1.000000';assert.throws(()=>assertState(damaged,config),/conservation/);
 assert.throws(()=>validateConfig({...config,readIntervalMs:100}),/limits/);
});
test('fresh exact context normalization rejects foreign tournament, unknown versions and real exclusions',()=>{
 const at=book(1).receivedAt;const raw={contexts:[{type:'tournament',tournament:{id:config.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',asOf:{sequence:1,at},bids:[{price:.4,quantity:100}],asks:[{price:.41,quantity:100}]}]}}]};
 const b=normalizeBook(raw,'377',config.tournamentId,at,at,20000);assert.equal(b.bids[0].price,'0.400000');assert.throws(()=>normalizeBook(raw,'377','wrong',at,at,20000),/context/);assert.throws(()=>normalizeBook(raw,'377',config.tournamentId,at,at,20000,new Set(['9'])),/real account/);
 raw.contexts[0].orderbook.exchanges[0].asOf=null;assert.throws(()=>normalizeBook(raw,'377',config.tournamentId,at,at,20000),/version/);
});
test('equity sample persists actual observed drawdown without inferring complete coverage',()=>{
 let s=initialState(config,'r',new Date(start).toISOString());s=observe(s,book(1),config);sampleEquity(s,config,book(1).receivedAt);s=observe(s,book(2),config);sampleEquity(s,config,book(2).receivedAt);assert.equal(s.strategies.buy_hold_benchmark.equityCurve.length,2);assert.equal(s.strategies.buy_hold_benchmark.maxDrawdown,'0.104150');
});
