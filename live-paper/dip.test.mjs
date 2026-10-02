import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';
import {initialState,normalizeBook,stepDip,validateConfig} from './model.mjs';import {dipSignal,diversification,validateDipPolicy,createDip,advanceDip} from './dip-shadow.mjs';import {digest} from './model.mjs';import {verifySource} from './ml-shadow.mjs';
const c=JSON.parse(fs.readFileSync(new URL('./fee-free-config.json',import.meta.url))),p=JSON.parse(fs.readFileSync(new URL('./dip-policy.json',import.meta.url)));
function book(at,bid,ask,sequence){return normalizeBook({contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'1066',option:'YES',asOf:{at,sequence},bids:[{price:bid,quantity:10000}],asks:[{price:ask,quantity:10000}]}]}}]},'377',c.tournamentId,at,at,20000);}
const settings={reference:.6,allowEntry:true,minimumEdge:.02,takeProfit:'0.010000',horizonMs:3600000,maxMarketCost:'50.000000'};
test('dip takes profit only through subsequent depth, conserves cash/FIFO, fees zero, control stays flat',()=>{
 let s=initialState(c,'dip-fixture','2026-10-02T06:00:00Z');s=stepDip(s,book('2026-10-02T06:00:01Z',.45,.46,1),c,settings);assert.equal(s.strategies.mean_reversion.fills.length,0);
 s=stepDip(s,book('2026-10-02T06:00:03Z',.45,.46,2),c,settings);assert.equal(s.strategies.mean_reversion.fills.length,1);
 s=stepDip(s,book('2026-10-02T06:00:05Z',.49,.50,3),c,settings);assert.equal(s.strategies.mean_reversion.pending['1066:YES'].action,'sell');
 s=stepDip(s,book('2026-10-02T06:00:07Z',.49,.50,4),c,{...settings,allowEntry:false});assert.equal(s.strategies.mean_reversion.fills.length,2);assert.equal(s.strategies.mean_reversion.fees,'0.000000');assert.equal(s.strategies.mean_reversion.realized,'0.200000');assert.equal(s.strategies.buy_hold_benchmark.fills.length,0);
});
test('a vanished discount revokes a pending entry before it can fill',()=>{
 let s=stepDip(initialState(c,'revocation','2026-10-02T06:00:00Z'),book('2026-10-02T06:00:01Z',.45,.46,1),c,settings);
 s=stepDip(s,book('2026-10-02T06:00:03Z',.45,.46,2),c,{...settings,allowEntry:false,reference:null});assert.equal(s.strategies.mean_reversion.fills.length,0);assert.deepEqual(s.strategies.mean_reversion.pending,{});
});
test('fresh authoritative REST receipt can carry an old last-event clock; legacy freshness is preserved',()=>{
 const source='2026-10-02T05:00:00Z',received='2026-10-02T06:00:01Z',raw={contexts:[{type:'tournament',tournament:{id:c.tournamentId},orderbook:{exchanges:[{exchangeId:'1066',option:'YES',asOf:{at:source,sequence:1},bids:[{price:.45,quantity:10000}],asks:[{price:.46,quantity:10000}]}]}}]};
 assert.throws(()=>normalizeBook(raw,'377',c.tournamentId,received,received,20000),/stale/);
 const b=normalizeBook(raw,'377',c.tournamentId,received,received,20000,new Set(),'authoritative_receipt_v1');assert.ok(b.sourceEventAgeMs>3600000);
 let s=stepDip(initialState(c,'quiet-book','2026-10-02T06:00:00Z'),b,c,settings);
 const next='2026-10-02T06:00:03Z',b2=normalizeBook(raw,'377',c.tournamentId,next,next,20000,new Set(),'authoritative_receipt_v1');s=stepDip(s,b2,c,settings);assert.equal(s.strategies.mean_reversion.fills.length,1);
 s=stepDip(s,{...b2,receivedAt:'2026-10-02T06:00:05Z',requestStartedAt:'2026-10-02T06:00:05Z'},c,settings);assert.equal(s.strategies.mean_reversion.fills.length,1);
 assert.throws(()=>normalizeBook(raw,'377',c.tournamentId,next,'2026-10-02T05:59:00Z',20000,new Set(),'authoritative_receipt_v1'),/Slow/);
 assert.throws(()=>normalizeBook(raw,'377',c.tournamentId,next,next,20000,new Set(['1066']),'authoritative_receipt_v1'),/real account/);
});
test('historical discount gates price/spread/regime and grouping limits concurrent market bets',()=>{
 const ref={ready:true,center:.6,robustDeviation:.01};assert.equal(dipSignal(ref,.56,.55,p).admitted,true);assert.equal(dipSignal(ref,.56,.5,p).admitted,false);assert.equal(dipSignal(ref,.2,.19,p).admitted,false);
 const bank={policy:p,identities:{a:{state:'FL',office:'Federal'},b:{state:'FL',office:'Federal'},c:{state:'FL',office:'Federal'},d:{state:'NC',office:'Federal'}}},state={wallet:{books:{a:{marketId:'a'},b:{marketId:'b'}},strategies:{mean_reversion:{positions:{a:{},b:{}},pending:{}}}}};assert.equal(diversification(state,bank,'c').reason,'state_concentration');assert.equal(diversification(state,bank,'d').allowed,true);
 assert.throws(()=>validateDipPolicy({...p,maxUnknownIdentity:10}));assert.throws(()=>validateConfig({...c,marketIds:Array.from({length:9},(_,i)=>String(i+1))}));assert.throws(()=>validateConfig({...c,strategyUniverse:'diversified-dip-v1',maxExposure:'20000.000000'}));
});
test('history seeding cannot trade before cutoff, source prefix is bound and resume is idempotent',()=>{
 const at='2026-10-02T06:00:00Z',body={sequence:1,previous:'',at,event:{type:'started'},state:{}},raw=JSON.stringify({...body,hash:digest(body)})+'\n',universe={tournamentId:c.tournamentId,exchanges:[{marketId:'377',exchangeId:'1066',option:'YES'}]},cfg={...c,marketIds:['377'],strategyUniverse:'diversified-dip-v1'};
 const {bank,state}=createDip(raw,cfg,universe,p,{377:{state:'AK',office:'Federal'}});assert.equal(state.wallet.observations,0);assert.deepEqual(advanceDip(state,verifySource(raw),bank),state);
 const replacement={...body,state:{changed:true}},bad=JSON.stringify({...replacement,hash:digest(replacement)})+'\n';assert.throws(()=>advanceDip(state,verifySource(bad),bank),/source\/bank/);
});
test('scaled allocation is a separately bounded profile with one-percent market cost and half-bank reserve',()=>{
 const scaled={...p,profile:'scale',maxOpenMarkets:100,maxPerState:4,maxPerOffice:50,maxMarketCost:'1000.000000'},config={...c,strategyUniverse:'diversified-dip-scale-v1',orderShares:1000,maxPositionShares:2000,maxExposure:'50000.000000',cashReserve:'50000.000000'};
 assert.equal(validateDipPolicy(scaled),scaled);assert.equal(validateConfig(config),config);assert.throws(()=>validateDipPolicy({...scaled,maxOpenMarkets:101}));assert.throws(()=>validateDipPolicy({...scaled,maxMarketCost:'2000.000000'}));assert.throws(()=>validateConfig({...config,cashReserve:'10000.000000'}));assert.throws(()=>validateConfig({...config,maxExposure:'100000.000000'}));
});
