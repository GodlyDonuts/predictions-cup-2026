import test from 'node:test';import assert from 'node:assert/strict';import fs from 'node:fs';
import {digest} from './model.mjs';import {BID_ANCHOR_POLICY,bidAnchorSignal,createBidAnchorState,advanceBidAnchor,bidAnchorReport} from './bid-anchor.mjs';
const at=n=>new Date(Date.parse('2026-10-03T00:00:00Z')+n).toISOString();
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
const book=(n,bid='0.450000',ask='0.455000',quantity=1000)=>({marketId:'1',exchangeId:'101',instrument:'101:YES',tournamentId:tid,receivedAt:at(n),requestStartedAt:at(n-100),sourceAt:at(n-50),sequence:String(n),versionKey:n+':'+at(n-50),bids:[{price:bid,quantity}],asks:[{price:ask,quantity}]});
const points=()=>[0,60000,120000,180000,240000].map(n=>({at:at(n),minute:n/60000,bid:'0.450000'}));
const bank=()=>({cutoff:at(250000),universe:{tournamentId:tid,exchanges:[{marketId:'1',exchangeId:'101',option:'YES'}]},policy:structuredClone(BID_ANCHOR_POLICY),config:{...JSON.parse(fs.readFileSync(new URL('./config.json',import.meta.url))),tournamentId:tid,strategyUniverse:'diversified-dip-v1',marketIds:['1'],initialCash:'100.000000',cashReserve:'90.000000',maxExposure:'10.000000',feeBps:0,slippageTicks:1,depthFractionBps:100,orderShares:1,maxPositionShares:1,stopLoss:'0.020000'}});
const row=b=>({kind:'book',at:b.receivedAt,sequence:Number(b.sequence),hash:'source-'+b.sequence,payload:{book:b}});
test('prior executable bid anchor admits cost-adjusted concession, ordinary spread crossing rejects',()=>{
 const signal=bidAnchorSignal(points(),book(300000,'0.425000','0.430000'));assert.equal(signal.admitted,true);assert.equal(signal.concession,'0.010000');
 assert.equal(bidAnchorSignal(points(),book(300000)).admitted,false);
 const withFuture=[...points(),{at:at(360000),bid:'0.990000'}];assert.deepEqual(bidAnchorSignal(withFuture,book(300000,'0.425000','0.430000')),signal);
});
test('gaps, unstable anchors, sparse history, wide spread, collapse and thin entry reject',()=>{
 assert.equal(bidAnchorSignal(points().slice(0,4),book(300000)).reason,'history_minutes');
 assert.equal(bidAnchorSignal(points(),book(500000)).reason,'history_gap');
 const unstable=points();unstable[0].bid='0.400000';assert.equal(bidAnchorSignal(unstable,book(300000)).reason,'unstable_anchor');
 assert.equal(bidAnchorSignal(points(),book(300000,'0.400000','0.430000')).reason,'spread');
 assert.equal(bidAnchorSignal(points(),book(300000,'0.400000','0.410000')).reason,'bid_collapse');
 assert.equal(bidAnchorSignal(points(),book(300000,'0.425000','0.430000',99)).reason,'entry_depth');
});
test('frozen prospective wallet uses delayed price-capped fills and exact FIFO; latency wallets remain distinct',()=>{
 const b=bank(),s=createBidAnchorState(b);for(const n of [0,60000,120000,180000,240000])advanceBidAnchor(s,row(book(n)),b);
 advanceBidAnchor(s,row(book(300000,'0.425000','0.430000')),b);assert.equal(s.signals.length,1);assert.equal(s.wallets[1000].state.strategies.mean_reversion.fills.length,0);
 advanceBidAnchor(s,row(book(301200,'0.425000','0.430000')),b);assert.equal(s.wallets[1000].state.strategies.mean_reversion.fills.length,1);assert.equal(s.wallets[3000].state.strategies.mean_reversion.fills.length,0);
 advanceBidAnchor(s,row(book(304000,'0.425000','0.430000')),b);assert.equal(s.wallets[3000].state.strategies.mean_reversion.fills.length,1);
 advanceBidAnchor(s,row(book(310000,'0.460000','0.465000')),b);advanceBidAnchor(s,row(book(314000,'0.460000','0.465000')),b);
 const r=bidAnchorReport(s,b,at(314000));for(const w of Object.values(r.wallets)){assert.equal(w.realizedPnl,'0.020000');assert.equal(w.fees,'0.000000');assert.equal(w.positions.length,0);}assert.equal(r.profitabilityEstablished,false);
});
test('same version cannot multiply signals, boundaries revoke buys and reset anchors, foreign scope rejects',()=>{
 const b=bank(),s=createBidAnchorState(b);for(const n of [0,60000,120000,180000,240000])advanceBidAnchor(s,row(book(n)),b);const q=row(book(300000,'0.425000','0.430000'));advanceBidAnchor(s,q,b);advanceBidAnchor(s,q,b);assert.equal(s.signals.length,1);
 advanceBidAnchor(s,{kind:'invalidation',at:at(300100),payload:{marketId:'1'}},b);assert.equal(s.histories['1'],undefined);assert.equal(Object.keys(s.wallets[1000].state.strategies.mean_reversion.pending).length,0);
 assert.throws(()=>advanceBidAnchor(s,row({...book(301000),marketId:'2'}),b),/SCOPE/);
 const changed={...b,policy:{...b.policy,cooldownMs:1}};assert.throws(()=>createBidAnchorState(changed),/POLICY/);
 assert.equal(s.bankHash,digest(b));
});
test('risk exit reprices to usable deeper bid and waits for a later request; loss may exceed stop',()=>{
 const b=bank(),s=createBidAnchorState(b);for(const n of [0,60000,120000,180000,240000])advanceBidAnchor(s,row(book(n)),b);
 advanceBidAnchor(s,row(book(300000,'0.425000','0.430000')),b);advanceBidAnchor(s,row(book(304000,'0.425000','0.430000')),b);
 const thin=n=>({...book(n,'0.420000','0.425000'),bids:[{price:'0.420000',quantity:60},{price:'0.410000',quantity:1000}]});
 advanceBidAnchor(s,row(thin(310000)),b);const st=s.wallets[1000].state.strategies.mean_reversion;assert.equal(st.pending['101:YES'].limit,'0.405000');assert.equal(st.fills.length,1);
 advanceBidAnchor(s,row(thin(314000)),b);const r=bidAnchorReport(s,b,at(314000));assert.equal(r.wallets[1000].realizedPnl,'-0.030000');assert.equal(r.wallets[1000].positions.length,0);
});
