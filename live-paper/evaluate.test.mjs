import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {digest} from './model.mjs';
import {evaluateJournal,validatePlan} from './evaluate.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),base=Date.parse('2026-10-01T23:00:00Z');
const at=s=>new Date(base+s*1000).toISOString();
const plan=()=>({schemaVersion:1,createdAt:at(0),startAt:at(60),endAt:at(180),baseConfig:c,baseConfigHash:digest(c),minimumClosedTrades:100,variants:[{name:'base',overrides:{},configHash:digest(c)},{name:'stress',overrides:{feeBps:25,slippageTicks:2},configHash:digest({...c,feeBps:25,slippageTicks:2})}]});
function journal(times){let previous='';return times.map((seconds,i)=>{
 const time=at(seconds),event={type:'cycle',clockAt:time,exclusionObservedAt:time,accountExclusions:[],sourceSnapshots:[{endpoint:'/markets/377/orderbook?tournamentId='+c.tournamentId,requestStartedAt:time,receivedAt:time,data:{contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'9',option:'YES',asOf:{at:time,sequence:i},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]}]}}]}}]},body={sequence:i+1,previous,at:time,event,state:{}},hash=createHash('sha256').update(JSON.stringify(body)).digest('hex');previous=hash;return JSON.stringify({...body,hash});}).join('\n')+'\n';}
test('future window has no inherited history, control fills, or manufactured performance',()=>{
 const r=evaluateJournal(journal([0,30]),plan());assert.equal(r.status,'awaiting_in_window_capture');assert.deepEqual(r.variants,[]);assert.equal(r.profitabilityEstablished,false);
});
test('start inclusive/end exclusive starts flat and reports every frozen cost variant',()=>{
 const r=evaluateJournal(journal([0,30,60,90,180,210]),plan());assert.equal(r.windowComplete,true);assert.equal(r.variants.length,2);
 for(const v of r.variants){assert.equal(v.observations,2);assert.equal(v.strategies.buy_hold_benchmark.fills,1);assert.equal(v.strategies.mean_reversion.intents,0);assert.equal(v.sampleStatus,'insufficient_closed_trades');assert.equal(v.strategies.buy_hold_benchmark.openShares,10);}
 assert.ok(Number(r.variants[1].strategies.buy_hold_benchmark.cash)<Number(r.variants[0].strategies.buy_hold_benchmark.cash));assert.notEqual(r.originalSourceHash,r.variants[0].derivedWindowHash);
});
test('immutable configuration checks and whole source integrity fail closed',()=>{
 const p=plan();p.variants[0].overrides.entryZ=.1;assert.throws(()=>validatePlan(p),/sensitivity/);const late=plan();late.createdAt=late.startAt;assert.throws(()=>validatePlan(late),/frozen/);
 const damaged=journal([0,60,90]).replace('10000','9999');assert.throws(()=>evaluateJournal(damaged,plan()),/integrity/);
});
test('final cutoff preserves stale unresolved inventory rather than inventing a terminal sale',()=>{
 const p=plan();p.endAt=at(360);const r=evaluateJournal(journal([0,60,90,400]),p);
 for(const v of r.variants){assert.equal(v.strategies.buy_hold_benchmark.openShares,10);assert.equal(v.strategies.buy_hold_benchmark.realizedPnl,'0.000000');assert.equal(v.strategies.buy_hold_benchmark.referencePnl,null);assert.equal(v.strategies.buy_hold_benchmark.liquidationPnl,null);assert.equal(v.strategies.buy_hold_benchmark.unresolvedPositions[0].unsoldShares,10);}
});
