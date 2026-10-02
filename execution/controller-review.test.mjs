import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {CompetitionController} from './controller.mjs';
import {createExecutionState,prepareIntent,beginDispatch,recordPlacement,reconcileIntent} from './core.mjs';
import {digest} from '../live-paper/model.mjs';

const TID='bda92870-621e-47b0-bc3c-3602c5c26f55',iso=n=>new Date(n).toISOString();
function fixture(t,{owned=false}={}){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sig-controller-review-'));let now=Date.parse('2026-10-02T10:00:00Z');
 t.after(()=>{const target=path.resolve(dir);assert.equal(path.dirname(target),path.resolve(os.tmpdir()));assert.ok(path.basename(target).startsWith('sig-controller-review-'));fs.rmSync(target,{recursive:true,force:true});});
 const policy={...JSON.parse(fs.readFileSync(new URL('../live-paper/dip-policy.json',import.meta.url))),executionPolicy:'responsive_v2'};
 const bank={policy,identities:{377:{state:'FL',office:'Senate'},378:{state:'GA',office:'Senate'}}};
 const history=()=>({receivedAt:iso(now),coverage:{complete:true},candles:Array.from({length:8},(_,n)=>({time:iso(now-(45-n*5)*60000),vwap:.55}))});
 fs.writeFileSync(path.join(dir,'dip-responsive-bank.json'),JSON.stringify(bank));
 function historyRefresh(){fs.writeFileSync(path.join(dir,'dip-responsive-state.json'),JSON.stringify({bankHash:digest(bank),histories:{700:history(),701:history()},bookHistory:{}}));}
 historyRefresh();
 const c=Object.create(CompetitionController.prototype),submitted=[];
 Object.assign(c,{runtimeDir:dir,now:()=>now,activation:{runId:'sig-pilot-review',maxOrderCost:'25.000000',maxCumulativeBuyCost:'100.000000',depthFractionBps:100,entryExpiresAt:iso(now+3600000)},universe:{exchanges:[{marketId:'377',exchangeId:'700',option:'YES'},{marketId:'378',exchangeId:'701',option:'YES'}]},selected:['377'],markets:{377:{at:iso(now),status:'open'}},state:{...createExecutionState({tournamentId:TID,policy:{maxOrderCost:'25.000000',maxAggregateCost:'100.000000',maxCumulativeBuyCost:'100.000000'}}),control:{schemaVersion:1,exits:{},cancellations:{}}},canWrite:()=>true,publish(){},ledger:{prepareIntent(){}},journal:{commit(){}},commit(result){this.state=result.state;},async submit(intent){submitted.push(intent);}});
 function refresh(){c.cash={tournamentId:TID,at:iso(now),amount:100000};c.account={positions:{tournamentId:TID,at:iso(now),complete:true,rows:Object.entries(c.state.owned).map(([exchangeId,p])=>({exchangeId,marketId:p.marketId,quantity:p.quantity}))},orders:{tournamentId:TID,at:iso(now),complete:true,rows:[]}};c.markets['377'].at=iso(now);}
 function snapshot(exchangeId='700',bid=.495,ask=.5){refresh();return c.snapshot({tournamentId:TID,marketId:'377',exchangeId,at:iso(now),requestStartedAt:iso(now),bids:[{price:String(bid),quantity:10000}],asks:[{price:String(ask),quantity:10000}]});}
 function acknowledge(intent){
  const price=Number(intent.price),bid=intent.action==='sell'?price:Number((price-.005).toFixed(3)),ask=intent.action==='buy'?price:Number((price+.005).toFixed(3));
  let r=beginDispatch(c.state,intent.intentId,{snapshot:snapshot(intent.exchangeId,bid,ask),now});assert.equal(r.ok,true,r.code);c.state=r.state;
  r=recordPlacement(c.state,intent.intentId,{status:200,response:{orderId:100+Object.keys(c.state.intents).length,exchangeId:intent.exchangeId,side:'yes',action:intent.action,quantity:intent.quantity,price:Number(intent.price),open:true,quantityTraded:0,remainingQuantity:intent.quantity,totalCost:0}},now);assert.equal(r.ok,true,r.code);c.state=r.state;return r.intent;
 }
 function terminal(intent,filled){
  const i=c.state.intents[intent.intentId].status==='prepared'?acknowledge(intent):c.state.intents[intent.intentId],quantity=(c.state.owned[i.exchangeId]?.quantity||0)+(i.action==='buy'?filled:-filled),account=snapshot(i.exchangeId);
  account.positions.rows=quantity?[{exchangeId:i.exchangeId,marketId:i.marketId,quantity}]:[];
  const r=reconcileIntent(c.state,i.intentId,{at:iso(now),snapshot:account,order:{id:i.orderId,exchangeId:i.exchangeId,tournamentId:TID,side:'yes',action:i.action,priceLimit:Number(i.price),expirationDate:i.expirationDate,open:false,quantity:i.quantity},fills:{complete:true,orderId:i.orderId,exchangeId:i.exchangeId,tournamentId:TID,totalQuantityFilled:filled,rows:filled?[{id:i.orderId+1000,quantity:filled,price:Number(i.price),side:'yes',filledAt:iso(now)}]:[]}},now);assert.equal(r.ok,true,r.code);c.state=r.state;refresh();
 }
 if(owned){
  const spec={intentId:'fixture-buy',idempotencyKey:'fixture-buy-key',marketId:'377',exchangeId:'700',side:'yes',action:'buy',quantity:20,price:'0.500000',expirationDate:iso(now+90000),attribution:{runId:'sig-pilot-review',strategyId:'fixture',modelHash:digest(bank)}};
  const r=prepareIntent(c.state,spec,snapshot(),now);assert.equal(r.ok,true,r.code);c.state=r.state;terminal(r.intent,20);
 }
 refresh();
 function book({exchangeId='700',bid=.495,ask=.5,bidQuantity=10000}={}){return {endpoint:'/markets/377/orderbook?tournamentId='+TID+'&depth=20',receivedAt:iso(now),requestStartedAt:iso(now),data:{contexts:[{type:'tournament',tournament:{id:TID},orderbook:{exchanges:[{exchangeId,option:'YES',asOf:{at:iso(now),sequence:now},bids:[{price:bid,quantity:bidQuantity}],asks:[{price:ask,quantity:10000}]}]}}]}};}
 function beginBuy(){const spec={intentId:'fixture-pending-buy',idempotencyKey:'fixture-pending-buy-key',marketId:'377',exchangeId:'700',side:'yes',action:'buy',quantity:20,price:'0.500000',expirationDate:iso(now+90000),attribution:{runId:'sig-pilot-review',strategyId:'fixture',modelHash:digest(bank)}};
  const r=prepareIntent(c.state,spec,snapshot(),now);assert.equal(r.ok,true,r.code);c.state=r.state;submitted.push(r.intent);acknowledge(r.intent);return r.intent;}
 return {controller:c,submitted,book,terminal,acknowledge,snapshot,beginBuy,get now(){return now;},advance(ms){now+=ms;refresh();historyRefresh();}};
}

function reconciliationCycle(f,{filled=5,pending=false,marketOpen=true,afterMetadata}={}){
 const c=f.controller,metadata=[];
 Object.assign(c,{bootstrapped:true,canOperate:()=>true,assertLedgers(){},async reconcile(i){if(!pending)f.terminal(i,filled);},transport:{async market(marketId){metadata.push(marketId);afterMetadata?.();return {receivedAt:iso(f.now),data:{contexts:[{status:marketOpen?'open':'closed',settledWith:null}],status:marketOpen?'open':'closed',isComposite:false,isMultiOutcome:false,title:'fixture'}};}}});
 const before=()=>c.beforeBooks({positions:{receivedAt:iso(f.now),data:{positions:Object.entries(c.state.owned).map(([exchangeId,p])=>({exchangeId,marketId:p.marketId,quantity:p.quantity}))}},orders:[],ordersAt:iso(f.now),candidateMarketIds:['378']});
 return {before,metadata};
}

test('terminal BUY reconciliation immediately evaluates the same-cycle owned risk book',async t=>{
 const f=fixture(t);const buy=f.beginBuy();assert.equal(buy.action,'buy');
 const cycle=reconciliationCycle(f,{filled:2});await cycle.before();
 assert.deepEqual(f.controller.selected,['377']);assert.equal(f.controller.reconciliationRiskOnly,true);assert.deepEqual(cycle.metadata,['377']);
 await f.controller.onBook({...f.book({exchangeId:'701'}),endpoint:'/markets/378/orderbook?tournamentId='+TID+'&depth=20'});assert.equal(f.submitted.length,1);
 await f.controller.onBook(f.book({bid:.43,ask:.435}));assert.equal(f.submitted.length,2);assert.equal(f.submitted[1].action,'sell');assert.equal(f.submitted[1].quantity,2);assert.equal(f.submitted[1].attribution.reason,'latched_stop_loss');
});
test('a terminal BUY with no fill, open ambiguity or invalid metadata cannot enter the same-cycle risk path',async t=>{
 for(const mode of ['zero','pending','uncertain','closed_market'])await t.test(mode,async t=>{
  const f=fixture(t);f.beginBuy();
  const cycle=reconciliationCycle(f,{filled:mode==='zero'?0:2,pending:['pending','uncertain'].includes(mode),marketOpen:mode!=='closed_market'});
  if(mode==='uncertain'){f.controller.state=structuredClone(f.controller.state);f.controller.state.intents[f.submitted[0].intentId].status='uncertain';}
  await cycle.before();assert.deepEqual(f.controller.selected,[]);await f.controller.onBook(f.book({bid:.43,ask:.435}));assert.equal(f.submitted.length,1);
 });
});
test('terminal BUY risk selection permits profit exits but no replacement entry when inventory is absent',async t=>{
 const f=fixture(t);f.beginBuy();const cycle=reconciliationCycle(f,{filled:2});await cycle.before();
 await f.controller.onBook(f.book({bid:.515,ask:.520}));assert.equal(f.submitted[1].action,'sell');assert.equal(f.submitted[1].attribution.reason,'take_profit');
 const g=fixture(t);g.beginBuy();await reconciliationCycle(g,{filled:2}).before();g.controller.state=structuredClone(g.controller.state);delete g.controller.state.owned['700'];
 await g.controller.onBook(g.book());assert.equal(g.submitted.length,1,'reduce-only fence must not turn a disappeared position into a buy');
});
test('terminal partial SELL reconciliation enables only its latched residual on the same-cycle fresh book',async t=>{
 const f=fixture(t,{owned:true});f.advance(2000);await f.controller.onBook(f.book({bid:.465,ask:.470}));f.acknowledge(f.submitted[0]);
 const cycle=reconciliationCycle(f);await cycle.before();
 assert.deepEqual(f.controller.selected,['377']);assert.deepEqual(cycle.metadata,['377']);assert.equal(f.controller.state.owned['700'].quantity,15);
 await f.controller.onBook({...f.book({exchangeId:'701'}),endpoint:'/markets/378/orderbook?tournamentId='+TID+'&depth=20'});assert.equal(f.submitted.length,1,'candidate BUY must remain excluded from this reconciliation cycle');
 await f.controller.onBook(f.book({bid:.490,ask:.495}));assert.equal(f.submitted.length,2,'fresh owned book after terminal reconciliation must not lose a collector cycle');
 assert.equal(f.submitted[1].action,'sell');assert.equal(f.submitted[1].quantity,15);assert.equal(f.submitted[1].attribution.reason,'latched_stop_loss');
 await f.controller.onBook(f.book({bid:.490,ask:.495}));assert.equal(f.submitted.length,2,'an active slice blocks duplicate book dispatch');
});
test('newly reconciled BUY exits retain fresh evidence and depth participation fences',async t=>{
 for(const mode of ['cash','positions','orders','book','depth'])await t.test(mode,async t=>{
  const f=fixture(t);f.beginBuy();await reconciliationCycle(f,{filled:2}).before();const book=f.book({bid:.43,ask:.435,bidQuantity:mode==='depth'?99:10000});
  if(mode==='book')book.receivedAt=iso(f.now-200001);
  else if(mode==='cash')f.controller.cash.at=iso(f.now-200001);
  else if(mode!=='depth')f.controller.account[mode].at=iso(f.now-200001);
  if(mode==='book')await assert.rejects(f.controller.onBook(book));else await f.controller.onBook(book);
  assert.equal(f.submitted.length,1,'stale evidence or zero admitted depth cannot submit a sale');
 });
});

test('reconciliation must close a latched SELL with an owned residual before any same-cycle selection',async t=>{
 for(const mode of ['pending','uncertain','cancel_pending','unlatched','flat'])await t.test(mode,async t=>{
  const f=fixture(t,{owned:true});f.advance(2000);await f.controller.onBook(f.book({bid:.465,ask:.470}));f.acknowledge(f.submitted[0]);
  f.controller.state=structuredClone(f.controller.state);
  if(mode==='unlatched')delete f.controller.state.control.exits['700'];
  if(mode==='uncertain')f.controller.state.intents[f.submitted[0].intentId].status='uncertain';
  if(mode==='cancel_pending')f.controller.state.control.cancellations[f.submitted[0].intentId]={phase:'uncertain'};
  const cycle=reconciliationCycle(f,{pending:['pending','uncertain','cancel_pending'].includes(mode),filled:mode==='flat'?20:5});await cycle.before();
  assert.deepEqual(f.controller.selected,[]);assert.deepEqual(cycle.metadata,[]);
  await f.controller.onBook(f.book({bid:.490,ask:.495}));assert.equal(f.submitted.length,1);
 });
});

test('closed or failed metadata cannot reuse cached openness or clear a residual exit',async t=>{
 for(const fails of [false,true])await t.test(fails?'request fails':'market closes',async t=>{
  const f=fixture(t,{owned:true});f.advance(2000);await f.controller.onBook(f.book({bid:.465,ask:.470}));f.acknowledge(f.submitted[0]);
  const cycle=reconciliationCycle(f,{marketOpen:false,afterMetadata(){if(fails)throw Error('METADATA_UNAVAILABLE');}});
  if(fails)await assert.rejects(cycle.before,/METADATA_UNAVAILABLE/);else await cycle.before();
  assert.deepEqual(f.controller.selected,[]);assert.equal(f.controller.markets['377'],undefined);assert.equal(f.controller.state.control.exits['700'].reason,'stop_loss');
  await f.controller.onBook(f.book({bid:.490,ask:.495}));assert.equal(f.submitted.length,1);
 });
});

test('same-cycle residual slices keep one-percent depth and the per-order notional cap',async t=>{
 const f=fixture(t,{owned:true});f.advance(2000);await f.controller.onBook(f.book({bid:.465,ask:.470}));f.acknowledge(f.submitted[0]);
 await reconciliationCycle(f).before();
 await f.controller.onBook(f.book({bid:.490,ask:.495,bidQuantity:99}));assert.equal(f.submitted.length,1,'less than one share at 1% must stay latched without dispatch');
 f.controller.activation.maxOrderCost='1.000000';
 await f.controller.onBook(f.book({bid:.490,ask:.495,bidQuantity:500}));assert.equal(f.submitted.length,2);assert.equal(f.submitted[1].quantity,2,'notional limit is tighter than 1% participation and residual inventory');
});

test('same-cycle residual slices still require fresh cash, positions, orders and books',async t=>{
 for(const stale of ['cash','positions','orders','book'])await t.test(stale,async t=>{
  const f=fixture(t,{owned:true});f.advance(2000);await f.controller.onBook(f.book({bid:.465,ask:.470}));f.acknowledge(f.submitted[0]);await reconciliationCycle(f).before();
  const book=f.book({bid:.490,ask:.495});
  if(stale==='book'){
   book.receivedAt=book.requestStartedAt=book.data.contexts[0].orderbook.exchanges[0].asOf.at=iso(f.now-21000);
   await f.controller.onBook(book);assert.equal(f.controller.reason,'STALE_BOOK_EVIDENCE');
  }else{
   (stale==='cash'?f.controller.cash:f.controller.account[stale]).at=iso(f.now-180001);
   await f.controller.onBook(book);assert.equal(f.controller.reason,'STALE_ACCOUNT_EVIDENCE');
  }
  assert.equal(f.submitted.length,1);assert.equal(f.controller.state.control.exits['700'].reason,'stop_loss');
 });
});

test('a scoped exchange associated with a different market cannot produce an official order',async t=>{
 const f=fixture(t);let error;try{await f.controller.onBook(f.book({exchangeId:'701'}));}catch(e){error=e;}
 assert.equal(f.submitted.length,0,'URL market377 must never submit to exchange701 belonging to market378');
 if(error)assert.match(error.message,/IDENTITY|SCOPE|MISMATCH/);
});

test('an unfilled stop exit stays latched when price rebounds before the holding timeout',async t=>{
 const f=fixture(t,{owned:true});f.advance(2000);await f.controller.onBook(f.book({bid:.465,ask:.47}));
 assert.equal(f.submitted.length,1);assert.equal(f.submitted[0].action,'sell');assert.equal(f.submitted[0].attribution.reason,'latched_stop_loss');assert.equal(f.controller.state.control.exits['700'].reason,'stop_loss');
 f.terminal(f.submitted[0],0);assert.equal(f.controller.state.owned['700'].quantity,20);assert.equal(f.controller.active(),undefined);
 f.advance(2000);await f.controller.onBook(f.book({bid:.49,ask:.495}));
 assert.equal(f.submitted.length,2,'risk liquidation must survive the first order closing unfilled');assert.equal(f.submitted[1].action,'sell');
});

test('official fill timestamps cannot precede dispatch or occur after reconciliation',t=>{
 const f=fixture(t),now=f.now,spec={intentId:'timing-buy',idempotencyKey:'timing-key',marketId:'377',exchangeId:'700',side:'yes',action:'buy',quantity:1,price:'0.500000',expirationDate:iso(now+30000),attribution:{runId:'sig-pilot-review',strategyId:'fixture'}};
 let r=prepareIntent(f.controller.state,spec,f.snapshot(),now);assert.equal(r.ok,true,r.code);
 r=beginDispatch(r.state,spec.intentId,{snapshot:f.snapshot(),now});assert.equal(r.ok,true,r.code);
 r=recordPlacement(r.state,spec.intentId,{status:200,response:{orderId:999,exchangeId:'700',side:'yes',action:'buy',quantity:1,price:.5,open:false,quantityTraded:1,remainingQuantity:0,totalCost:.5}},now);assert.equal(r.ok,true,r.code);
 const snapshot=f.snapshot();snapshot.positions.rows=[{exchangeId:'700',marketId:'377',quantity:1}];
 for(const offset of [-60000,60000]){
  const result=reconcileIntent(r.state,spec.intentId,{at:iso(now),snapshot,order:{id:999,exchangeId:'700',tournamentId:TID,side:'yes',action:'buy',priceLimit:.5,expirationDate:spec.expirationDate,open:false,quantity:1},fills:{complete:true,orderId:999,exchangeId:'700',tournamentId:TID,totalQuantityFilled:1,rows:[{id:1999,quantity:1,price:.5,side:'yes',filledAt:iso(now+offset)}]}},now);
  assert.equal(result.ok,false,'a '+offset+'ms fill clock must not become holding-time evidence');
 }
});
