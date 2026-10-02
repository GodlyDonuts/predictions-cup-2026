import test from 'node:test';
import assert from 'node:assert/strict';
import {STREAM_DIP_POLICY,createStreamDip,warmStreamDip,advanceStreamDip,streamDipReport,streamDipHash} from './stream-dip.mjs';
const base=Date.parse('2026-10-02T15:00:00.000Z'),at=n=>new Date(base+n).toISOString();
const tid='bda92870-621e-47b0-bc3c-3602c5c26f55';
const bank=()=>({schemaVersion:1,mode:'stream_native_dip_paper',tournamentId:tid,inputBankHash:'a'.repeat(64),cutoff:at(0),startEventId:0,
 markets:[{marketId:'377',exchangeId:'1066',state:'FL',office:'Senate'}],policy:structuredClone(STREAM_DIP_POLICY)});
function book(id,n,{bid='.390000',ask='.400000',quantity=1000,host=n,source=n,seq=id,origin='stream'}={}){
 const canonical=x=>Number(x).toFixed(6);return {kind:'book',eventId:id,marketId:'377',exchangeId:'1066',tournamentId:tid,origin,
 quoteReceivedAt:at(n),confirmedAt:at(n),availableAt:at(host),requestStartedAt:origin==='rest_recovery'?at(n-100):null,
 nextExpiryAt:null,decisionEligible:true,continuity:{ready:true,continuous:true},
 book:{marketId:'377',exchangeId:'1066',instrument:'1066:YES',sequence:String(seq),sourceAt:at(source),versionKey:seq+':'+at(source),
 bids:[{price:canonical(bid),quantity}],asks:[{price:canonical(ask),quantity}]}};}
function fixture(){const b=bank(),warm=Array.from({length:9},(_,i)=>book(i+1,-(40-i*5)*60000,{bid:'.420000',ask:'.430000'}));
 return {b,s:warmStreamDip(createStreamDip(b),warm,b)};}
const step=(s,e,b,host=e.availableAt)=>advanceStreamDip(s,[e],b,{availableAt:host});
test('minute-balanced history is causal and warming cannot submit trades',()=>{
 const {b,s}=fixture();assert.equal(s.histories['377'].length,9);assert.equal(s.wallets.latency_1000.outcomes.length,0);
 const warm=Array.from({length:100},(_,i)=>book(i+1,-60000+i,{bid:'.420000',ask:'.430000'}));
 assert.equal(warmStreamDip(createStreamDip(b),warm,b).histories['377'].length,1);
 assert.throws(()=>warmStreamDip(s,[book(1,1)],b),/FUTURE_WARM/);
 const next=step(s,book(1,1000),b);assert.equal(next.wallets.latency_1000.outcomes.length,1);
 assert.equal(next.wallets.latency_1000.fills.length,0);assert.throws(()=>warmStreamDip(next,[],b),/WARM_AFTER_START/);
});
test('later independent source and latency required; no duplicate depth consumption',()=>{
 const {b,s}=fixture();let x=step(s,book(1,1000),b);x=step(x,book(2,1500),b);
 assert.equal(x.wallets.latency_1000.fills.length,0);x=step(x,book(3,2500,{quantity:300}),b);
 assert.equal(x.wallets.latency_1000.positions['377'].quantity,3);assert.equal(x.wallets.latency_3000.fills.length,0);
 const duplicate=step(x,book(4,2700,{seq:3,source:2500,quantity:300}),b);assert.equal(duplicate.wallets.latency_1000.positions['377'].quantity,3);
 x=step(x,book(4,4500),b);assert.equal(x.wallets.latency_1000.positions['377'].quantity,10);assert.equal(x.wallets.latency_3000.positions['377'].quantity,10);
 assert.equal(x.wallets.latency_1000.cumulativeBuys,'4.000000');
});
test('partial loss exits release exact cost; closed trades count only when flat',()=>{
 const {b,s}=fixture();let x=step(s,book(1,1000),b);x=step(x,book(2,2500),b);
 x=step(x,book(3,4000,{bid:'.360000',ask:'.370000'}),b);assert.equal(x.wallets.latency_1000.pending['377'].side,'sell');
 x=step(x,book(4,5500,{bid:'.360000',ask:'.370000',quantity:300}),b);
 assert.equal(x.wallets.latency_1000.realizedPnl,'-0.135000');assert.equal(x.wallets.latency_1000.closedTrades,0);
 x=step(x,book(5,7000,{bid:'.360000',ask:'.370000'}),b);assert.equal(x.wallets.latency_1000.closedTrades,1);
 assert.equal(x.wallets.latency_1000.realizedPnl,'-0.450000');assert.equal(x.wallets.latency_1000.cash,'99.550000');
});
test('delayed consumption preserves adverse in-window fills and prevents backdated decisions',()=>{
 const {b,s}=fixture();let x=step(s,book(1,1000),b);
 x=advanceStreamDip(x,[book(2,1500),book(3,2500)],b,{availableAt:at(40000)});
 assert.equal(x.wallets.latency_1000.positions['377'].quantity,10);assert.equal(x.wallets.latency_3000.outcomes[0].status,'expired');
 assert.equal(x.wallets.latency_3000.outcomes[0].censoring,'no_independent_in_window_quote');
 assert.equal(x.wallets.latency_1000.outcomes.length,1);assert.equal(x.wallets.latency_1000.gateCounts.stale_or_invalid,1);
});
test('expired orders distinguish observed non-execution from unobserved windows',()=>{
 const {b,s}=fixture();let x=step(s,book(1,1000),b);x=step(x,book(2,5000,{ask:'.410000'}),b);
 x=advanceStreamDip(x,[],b,{availableAt:at(31000)});
 assert.equal(x.wallets.latency_1000.outcomes[0].censoring,'observed_without_full_execution');
 const y=advanceStreamDip(step(s,book(1,1000),b),[],b,{availableAt:at(31000)});
 assert.equal(y.wallets.latency_1000.outcomes[0].censoring,'no_independent_in_window_quote');
});
test('invalidation preserves inventory and makes liquidation unavailable',()=>{
 const {b,s}=fixture();let x=step(s,book(1,1000),b);x=step(x,book(2,2500),b);
 x=step(x,{kind:'invalidate',eventId:3,marketId:'377',exchangeId:'1066',tournamentId:tid,confirmedAt:at(3000),availableAt:at(3000)},b);
 const r=streamDipReport(x,b,at(3000)).wallets.latency_1000;assert.equal(r.positions[0].quantity,10);assert.equal(r.totalPnl,null);assert.equal(r.realizedPnl,'0.000000');
});
test('stream clocks never masquerade as REST requests and frozen identities are enforced',()=>{
 const {b,s}=fixture();const e=book(1,1000);e.requestStartedAt=at(900);assert.throws(()=>step(s,e,b),/REQUEST_CLOCK/);
 const changed=structuredClone(b);changed.policy.feeBps=1;assert.throws(()=>step(s,book(1,1000),changed),/POLICY_CHANGED/);
 const original=streamDipHash(s);step(s,book(1,1000),b);assert.equal(streamDipHash(s),original);
});
test('real recovery request precision is retained without becoming a push request',()=>{
 const {b,s}=fixture(),e=book(1,1000,{origin:'rest_recovery'});e.requestStartedAt='2026-10-02T15:00:00.900123400Z';
 assert.equal(step(s,e,b).observations,1);e.requestStartedAt='2026-10-02T15:00:01.000000001Z';assert.throws(()=>step(s,e,b),/REQUEST_CLOCK/);
});
test('conditional dip uses a fixed lower limit and never fills above it',()=>{
 const {b,s}=fixture();let x=step(s,book(1,1000,{bid:'.395000',ask:'.405000'}),b);
 assert.equal(x.wallets.latency_1000.pending['377'].limit,'0.400000');assert.equal(x.wallets.latency_1000.pending['377'].reason,'conditional_one_tick_lower');
 x=step(x,book(2,2500,{bid:'.395000',ask:'.405000'}),b);assert.equal(x.wallets.latency_1000.fills.length,0);
 x=step(x,book(3,4000),b);assert.equal(x.wallets.latency_1000.fills[0].price,'0.400000');
});
test('input cannot spend more than a hundred or turn a foreign tournament into a trade',()=>{
 const {b,s}=fixture();const e=book(1,1000);e.tournamentId='foreign';assert.throws(()=>step(s,e,b),/SCOPE/);
 const x=structuredClone(s);x.wallets.latency_1000.cumulativeBuys='100.000000';
 const y=step(x,book(1,1000),b);assert.equal(y.wallets.latency_1000.outcomes.length,0);assert.equal(y.wallets.latency_1000.gateCounts.budget,1);
});
