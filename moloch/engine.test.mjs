import test from 'node:test';import assert from 'node:assert/strict';
import {createMoloch,advanceMoloch,reportMoloch,assertMoloch} from './engine.mjs';
import {payloadHash} from '../execution/ledger.mjs';
const base=Date.parse('2026-10-13T00:00:00Z'),at=n=>new Date(base+n*1000).toISOString();
const book=(n,ex='101',seq=1)=>({type:'book',receivedAt:at(n),book:{tournamentId:'cup',marketId:ex,exchangeId:ex,sequence:String(seq),requestStartedAt:at(n),receivedAt:at(n),sourceAt:at(n),bids:[{price:'0.450000',quantity:1000}],asks:[{price:'0.455000',quantity:1000}]}});
const trade=(n,id,p='0.440000',quantity=100,executed=n)=>({type:'trade',receivedAt:at(n),executedAt:at(executed),id,tournamentId:'cup',marketId:'101',exchangeId:'101',yesPrice:p,quantity});
const tick=n=>({type:'tick',receivedAt:at(n)});
test('paired spread ignores uncalibrated imbalance but funds both legs and rejects fee-drained packets',()=>{
  const e=book(0);e.book.bids[0].quantity=1000000;e.book.asks[0].quantity=1;
  const paired=createMoloch();advanceMoloch(paired,e);assert.equal(paired.orders.length,2);
  const micro=createMoloch({quoteMode:'microprice'});advanceMoloch(micro,e);assert.equal(micro.orders.length,1);
  const scarce=createMoloch({maximumCommitted:'0.500000'});advanceMoloch(scarce,e);assert.equal(scarce.orders.length,0);
  const fees=createMoloch({feePerShare:'0.002000'});advanceMoloch(fees,e);assert.equal(fees.orders.length,0);
});
test('inside quotes improve wide books without crossing, losing fee margin or changing strict fills',()=>{
  const e=book(0);e.book.asks[0].price='0.465000';const s=createMoloch({insideTicks:1,quantity:1});advanceMoloch(s,e);
  assert.equal(s.orders.find(q=>q.side==='YES').price,'0.455000');assert.equal(s.orders.find(q=>q.side==='NO').price,'0.540000');
  advanceMoloch(s,trade(4,'touch-inside','0.455000'));assert.equal(s.fills.length,0);advanceMoloch(s,trade(5,'old-bid','0.450000'));assert.equal(s.fills.length,1);assertMoloch(s);
  const narrow=createMoloch({insideTicks:3});advanceMoloch(narrow,book(0));assert.equal(narrow.orders.find(q=>q.side==='YES').price,'0.450000');
  const fees=createMoloch({insideTicks:1,feePerShare:'0.002000'});advanceMoloch(fees,e);assert.equal(fees.orders.find(q=>q.side==='YES').price,'0.450000');
});
test('two-tick inside entry leans toward depth value; inventory exit remains covered and noncrossing',()=>{
  const e=book(0);e.book.asks[0].price='0.460000';e.book.bids[0].quantity=1;e.book.asks[0].quantity=1000000;
  const s=createMoloch({insideTicks:1});advanceMoloch(s,e);assert.equal(s.orders.find(q=>q.side==='YES').price,'0.450000');assert.equal(s.orders.find(q=>q.side==='NO').price,'0.545000');assertMoloch(s);
  const owned=createMoloch({insideTicks:1,quantity:1});advanceMoloch(owned,e);advanceMoloch(owned,trade(4,'owned-entry','0.440000'));advanceMoloch(owned,tick(8));
  const checkpoint=book(9,'101',20);checkpoint.book.asks[0].price='0.460000';advanceMoloch(owned,checkpoint);advanceMoloch(owned,{type:'coverage',receivedAt:at(10),tournamentId:'cup',marketId:'101',exchangeId:'101',from:at(3),to:at(9),complete:true,projectedThroughSequence:'20'});
  const exit=owned.orders.find(q=>q.action==='sell');assert.equal(exit.price,'0.455000');advanceMoloch(owned,trade(14,'exit-touch-original','0.460000'));assert.equal(owned.realized,'0.005000');assertMoloch(owned);
});
test('inventory improvement cannot turn a profitable passive exit into a break-even sale',()=>{
  const s=createMoloch({insideTicks:1,quantity:1}),entry=book(0);entry.book.asks[0].price='0.465000';advanceMoloch(s,entry);advanceMoloch(s,trade(4,'cost-455','0.450000'));advanceMoloch(s,tick(8));
  const checkpoint=book(9,'101',20);checkpoint.book.asks[0].price='0.460000';advanceMoloch(s,checkpoint);advanceMoloch(s,{type:'coverage',receivedAt:at(10),tournamentId:'cup',marketId:'101',exchangeId:'101',from:at(3),to:at(9),complete:true,projectedThroughSequence:'20'});
  assert.equal(s.orders.find(q=>q.action==='sell').price,'0.460000');advanceMoloch(s,trade(14,'exit-not-through','0.460000'));assert.equal(s.fills.length,1);advanceMoloch(s,trade(15,'exit-through','0.465000'));assert.equal(s.realized,'0.005000');assertMoloch(s);
});
test('early renewal requires a fresh unchanged quote and preserves old funding through cancellation',()=>{
  const s=createMoloch({renewBeforeMs:45000,maximumMarketCost:'20.000000'});advanceMoloch(s,book(0));
  advanceMoloch(s,tick(59));assert.equal(s.counts.earlyRenewals,undefined);
  advanceMoloch(s,book(81,'101',2));assert.equal(s.counts.earlyRenewals,1);assert.equal(s.orders[0].retired,false);assert.ok(Number(reportMoloch(s).reservedCash)>=4.975);
  advanceMoloch(s,tick(84));advanceMoloch(s,tick(88));
  assert.ok(s.orders.length>2);assert.equal(s.orders[0].retired,false);assertMoloch(s);
  assert.throws(()=>createMoloch({renewBeforeMs:120000}),/RENEWAL_WINDOW/);
});

test('paired strict-through fills reconcile exact cash, fees and engine FIFO netting',()=>{
  const s=createMoloch({quantity:1,feePerShare:'0.000100'});advanceMoloch(s,book(0));
  advanceMoloch(s,trade(4,'a'));advanceMoloch(s,trade(5,'b','0.465000'));
  const r=reportMoloch(s);assert.equal(r.fills,2);assert.equal(r.cash,'1000.004800');assert.equal(r.realized,'0.004800');assert.equal(r.fees,'0.000200');assert.equal(r.ownedCost,'0.000000');assert.equal(s.redemptions[0].mechanism,'engine_opposite_buy_fifo_netting');assert.equal(s.redemptions[0].counterfactual,true);assert.equal(r.profitabilityEstablished,false);assertMoloch(s);
});
test('pre-activation and at-touch prints never fabricate queue fills; duplicate IDs cannot fill twice',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,trade(1,'early'));advanceMoloch(s,trade(4,'touch','0.450000'));assert.equal(s.fills.length,0);
  advanceMoloch(s,trade(5,'through'));advanceMoloch(s,trade(6,'through','0.440000',100,5));assert.equal(s.fills.length,1);assert.equal(s.counts.duplicateTrades,1);
  assert.throws(()=>advanceMoloch(s,trade(7,'through','0.435000',100,5)),/TRADE_CONFLICT/);
});
test('conditional FIFO touch volume must exhaust visible queue before bounded fill credit',()=>{
  const s=createMoloch({fillModel:'conditionalFifo',quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'queue-first','0.450000',999));const q=s.orders.find(q=>q.side==='YES');assert.equal(q.queueAhead,'1');assert.equal(s.fills.length,0);
  advanceMoloch(s,trade(5,'queue-first','0.450000',999,4));assert.equal(q.queueAhead,'1');advanceMoloch(s,trade(6,'queue-cleared','0.450000',101));assert.equal(s.fills.length,1);assert.equal(s.fills[0].conditionalQueueFill,true);assert.equal(reportMoloch(s).profitabilityEstablished,false);assertMoloch(s);
  const half=createMoloch({fillModel:'conditionalFifo',touchAttributionBps:5000,quantity:1});advanceMoloch(half,book(0));advanceMoloch(half,trade(4,'half-first','0.450000',999));advanceMoloch(half,trade(6,'half-next','0.450000',101));assert.equal(half.fills.length,0);
});
test('drain disables new buys but retains exact funds until delayed fills and full coverage resolve',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));const next=createMoloch({...s.policy,entriesEnabled:false}).policy;s.policy=next;s.policyHash=payloadHash(next);advanceMoloch(s,tick(20));assert.equal(s.orders[0].cancelEffectiveAt,base+23000);assert.equal(s.orders[0].retired,false);
  advanceMoloch(s,tick(23));advanceMoloch(s,book(30,'101',20));advanceMoloch(s,{type:'coverage',receivedAt:at(31),tournamentId:'cup',marketId:'101',exchangeId:'101',from:at(3),to:at(30),complete:true,projectedThroughSequence:'20'});assert.equal(s.orders.length,2);assert.ok(s.orders.every(q=>q.retired));assert.equal(s.cash,'1000.000000');assertMoloch(s);
});
test('atomic scope invalidation cannot admit another retiring market between gap events',()=>{
  const s=createMoloch({entriesEnabled:false});advanceMoloch(s,book(0,'101'));advanceMoloch(s,book(1,'102'));s.policy=createMoloch({...s.policy,entriesEnabled:true}).policy;s.policyHash=payloadHash(s.policy);advanceMoloch(s,{type:'gap',exchangeIds:['101','102'],receivedAt:at(2)});assert.equal(s.orders.length,0);assert.equal(s.books['101'].invalid,true);assert.equal(s.books['102'].invalid,true);
});
test('conditional FIFO ignores cancellation credit and places new equal-price depth behind an active quote',()=>{
  const s=createMoloch({fillModel:'conditionalFifo',quantity:1});advanceMoloch(s,book(0));const lower=book(4,'101',2);lower.book.bids[0].quantity=1;advanceMoloch(s,lower);const q=s.orders.find(q=>q.side==='YES');assert.equal(q.queueAhead,'1000');const higher=book(5,'101',3);higher.book.bids[0].quantity=100;advanceMoloch(s,higher);assert.equal(q.queueAhead,'1000');
  advanceMoloch(s,trade(6,'residual-hundred','0.450000',1100));assert.equal(s.fills.length,1);assertMoloch(s);
  const before=createMoloch({fillModel:'conditionalFifo'});advanceMoloch(before,book(0));const early=book(2,'101',2);early.book.bids[0].quantity=1500;advanceMoloch(before,early);assert.equal(before.orders.find(q=>q.side==='YES').queueAhead,'1500');
});
test('REST and stream timestamp precision differences deduplicate without masking changed trade facts',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'shared'));
  const e=trade(5,'shared','0.440000',100,4);e.executedAt=e.executedAt.replace('.000Z','.000000000Z');advanceMoloch(s,e);assert.equal(s.fills.length,1);assert.equal(s.counts.duplicateTrades,1);
  assert.throws(()=>advanceMoloch(s,{...e,receivedAt:at(6),quantity:101}),/TRADE_CONFLICT/);
});
test('delayed pushed book cannot become fresh or clear a quote-end barrier by receipt time',()=>{
  const s=createMoloch();const e=book(0);e.receivedAt=at(20);e.book.receivedAt=at(20);e.book.requestStartedAt=at(20);e.book.transport='official_sig_realtime';advanceMoloch(s,e);assert.equal(s.orders.length,0);
});
test('cancel latency stops active quotes without freeing unresolved funding or globally blocking another market',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,{type:'gap',exchangeId:'101',receivedAt:at(4)});
  advanceMoloch(s,tick(7));advanceMoloch(s,book(10,'102'));
  assert.ok(s.orders.some(q=>q.exchangeId==='102'));assert.equal(s.orders[0].retired,false);assert.equal(s.orders[0].cancelEffectiveAt,base+7000);assert.ok(Number(reportMoloch(s).reservedCash)>0);
});
test('delayed pre-cancel match uses reserved capital; only scoped full coverage and post-end checkpoint retire it',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,{type:'gap',exchangeId:'101',receivedAt:at(4)});advanceMoloch(s,tick(7));
  advanceMoloch(s,trade(8,'delayed','0.440000',100,6));assert.equal(s.fills.length,1);
  advanceMoloch(s,book(12,'101',20));
  const coverage={type:'coverage',receivedAt:at(13),tournamentId:'cup',marketId:'101',exchangeId:'101',from:at(3),to:at(12),complete:true,projectedThroughSequence:'19'};
  const pending=s.orders.find(q=>q.side==='NO');
  advanceMoloch(s,coverage);assert.equal(pending.retired,false);
  advanceMoloch(s,{...coverage,receivedAt:at(14),projectedThroughSequence:'20'});assert.equal(pending.retired,true);assertMoloch(s);
});
test('funding caps, stale liquidation, receipt causality and immutable policy are enforced',()=>{
  const s=createMoloch({quoteMode:'microprice',maximumCommitted:'0.500000',maximumMarketCost:'0.500000',quantity:5});advanceMoloch(s,book(0));assert.ok(Number(reportMoloch(s).reservedCash)<=.5);
  advanceMoloch(s,trade(4,'a'));advanceMoloch(s,tick(20));assert.equal(reportMoloch(s).netLiquidationPnl,null);
  assert.throws(()=>advanceMoloch(s,book(2)),/RECEIPT_ORDER/);s.policy.quantity=99;assert.throws(()=>advanceMoloch(s,tick(21)),/POLICY_CHANGED/);
});
test('fractional participation accumulates observed volume without enlarging a single print',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'a','0.440000',50));assert.equal(s.fills.length,0);advanceMoloch(s,trade(5,'b','0.440000',50));assert.equal(s.fills.length,1);assert.equal(s.fills[0].quantity,1);
});
test('opposite acquisition liability cannot pledge the same inventory to an exit',()=>{
  const s=createMoloch({quantity:5});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'a','0.440000',100));advanceMoloch(s,book(8,'101',2));
  assert.equal(s.orders.filter(q=>q.action==='sell').length,0);assertMoloch(s);
});
test('actual processing clock controls activation; delayed historical prints cannot fill a newly processed quote',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,{...book(0),processedAt:at(5)});
  assert.equal(s.orders[0].activationAt,base+8000);
  advanceMoloch(s,{...trade(6,'historical','0.440000',100,4),processedAt:at(7)});assert.equal(s.fills.length,0);
  advanceMoloch(s,{...trade(9,'future'),processedAt:at(10)});assert.equal(s.fills.length,1);
});
test('duplicate delivery still advances processing chronology and budget accounting',()=>{
  const s=createMoloch();advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'a'));advanceMoloch(s,trade(20,'a','0.440000',100,4));
  assert.equal(s.lastAt,at(20));assert.throws(()=>advanceMoloch(s,tick(19)),/RECEIPT_ORDER/);
});
test('funding reserves per-share fee rounding for partial fills',()=>{
  const s=createMoloch({quoteMode:'microprice',initialCash:'1.635164',maximumCommitted:'1.635164',maximumMarketCost:'1.635164',quantity:3,feeBps:1});
  advanceMoloch(s,book(0));const no=s.orders.find(q=>q.side==='NO');assert.equal(no.quantity,2);
  for(let n=4;n<=5;n++)advanceMoloch(s,trade(n,'partial'+n,'0.465000',100));assertMoloch(s);
  assert.ok(Number(s.cash)>=Number(reportMoloch(s).reservedCash));
});
test('candle replay is explicitly hypothetical release timing and cannot manufacture book depth or fills',()=>{
  const s=createMoloch();advanceMoloch(s,{type:'candle',receivedAt:at(3600),availableAt:at(3600),bucketStartedAt:at(0),exchangeId:'101',close:.45,historicalReleaseClockAssumed:true});
  assert.equal(s.orders.length,0);assert.equal(s.fills.length,0);assert.equal(s.priceHistory['101'].observations,1);assert.equal(s.cash,'1000.000000');
});
test('admission freshness does not shorten resting lifetime, but prolonged silence drains quotes',()=>{
  const s=createMoloch({quantity:1});advanceMoloch(s,book(0));advanceMoloch(s,tick(12));
  assert.equal(s.orders.length,2);assert.ok(s.orders.every(q=>q.cancelEffectiveAt===undefined));
  advanceMoloch(s,trade(13,'later-through'));assert.equal(s.fills.length,1);
  const flat=createMoloch({quantity:1});advanceMoloch(flat,book(0));advanceMoloch(flat,tick(61));assert.ok(flat.orders.some(q=>q.cancelEffectiveAt!==undefined));
});

test('participation experiments preserve legacy quote assumptions and tag new quotes',()=>{
  const legacy=createMoloch();advanceMoloch(legacy,book(0));legacy.policy={...legacy.policy,participationBps:1000};legacy.policyHash=payloadHash(legacy.policy);advanceMoloch(legacy,trade(4,'legacy-credit','0.440000',20));assert.equal(legacy.fills.length,0);assert.equal(legacy.orders.find(q=>q.side==='YES').participationCredit,2000);
  const next=createMoloch({participationBps:1000});advanceMoloch(next,book(0));assert.equal(next.orders[0].participationBps,1000);advanceMoloch(next,trade(4,'new-credit','0.440000',20));assert.equal(next.fills[0].quantity,2);assertMoloch(next);
});

test('funded opposite quote survives a full entry and closes the spread without a cancel barrier',()=>{
 const s=createMoloch({retainProfitableHedge:true});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'first-leg','0.440000',500));const hedge=s.orders.find(q=>q.side==='NO');assert.equal(hedge.cancelEffectiveAt,undefined);assert.equal(s.orders.length,2);advanceMoloch(s,trade(8,'second-leg','0.460000',500));assert.equal(s.redemptions[0].quantity,5);assert.equal(s.realized,'0.025000');assertMoloch(s);
 const prior=createMoloch();advanceMoloch(prior,book(0));advanceMoloch(prior,trade(4,'prior-first','0.440000',500));assert.ok(prior.orders.find(q=>q.side==='NO').cancelEffectiveAt);advanceMoloch(prior,trade(8,'prior-second','0.460000',500));assert.equal(prior.fills.length,1);
});
test('oversized opposite quote cancels after a partial entry and never gains a second share pledge',()=>{
 const s=createMoloch({retainProfitableHedge:true});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'partial-entry','0.440000',100));assert.equal(s.fills[0].quantity,1);assert.equal(s.orders.find(q=>q.side==='NO').cancelEffectiveAt,base+7000);advanceMoloch(s,book(8,'101',20));assert.equal(s.orders.some(q=>q.action==='sell'),false);advanceMoloch(s,{type:'coverage',receivedAt:at(9),tournamentId:'cup',marketId:'101',exchangeId:'101',from:at(3),to:at(8),complete:true,projectedThroughSequence:'20'});advanceMoloch(s,tick(12));assert.equal(s.orders.find(q=>q.action==='sell').quantity,1);assertMoloch(s);
});
test('gaps and explicit drains still cancel retained inventory hedges',()=>{
 for(const drain of [false,true]){const s=createMoloch({retainProfitableHedge:true});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'owned','0.440000',500));if(drain){s.policy={...s.policy,entriesEnabled:false};s.policyHash=payloadHash(s.policy);advanceMoloch(s,tick(8));}else advanceMoloch(s,{type:'gap',exchangeId:'101',receivedAt:at(8)});assert.ok(s.orders.find(q=>q.side==='NO').cancelEffectiveAt);assert.ok(Number(reportMoloch(s).reservedCash)>0);assertMoloch(s);}
 assert.throws(()=>createMoloch({retainProfitableHedge:'yes'}),/RETAIN_HEDGE/);
});

test('bounded original-quote overshoot can net a partial packet and exits residual inventory without new hedge buys',()=>{
 const s=createMoloch({quantity:10,retainProfitableHedge:true,maximumHedgeOvershoot:3});advanceMoloch(s,book(0));advanceMoloch(s,trade(4,'partial-no-1','0.460000',100));advanceMoloch(s,{...trade(4.1,'partial-no-6','0.460000',600)});advanceMoloch(s,tick(8));const yes=s.orders.find(q=>q.side==='YES');assert.equal(yes.cancelEffectiveAt,undefined);assert.equal(s.orders.length,2);advanceMoloch(s,trade(9,'hedge-and-residual','0.440000',1000));assert.equal(s.redemptions[0].quantity,7);assert.equal(s.realized,'0.035000');assert.equal(s.positions['101'].YES.reduce((n,l)=>n+l.quantity,0),3);assertMoloch(s);
 const marked=book(10,'101',30);marked.book.bids[0].price='0.440000';marked.book.asks[0].price='0.445000';advanceMoloch(s,marked);assert.equal(reportMoloch(s).netLiquidationPnl,'0.005000');advanceMoloch(s,{type:'coverage',receivedAt:at(11),tournamentId:'cup',marketId:'101',exchangeId:'101',from:at(3),to:at(10),complete:true,projectedThroughSequence:'30'});advanceMoloch(s,trade(14,'residual-covered-sale','0.450000',300));assert.equal(s.realized,'0.020000');assert.equal(s.positions['101'].YES.length,0);assertMoloch(s);
 assert.throws(()=>createMoloch({quantity:10,maximumHedgeOvershoot:11}),/HEDGE_OVERSHOOT/);
});
