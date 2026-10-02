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
 function terminal(intent,filled){
  const price=Number(intent.price),bid=intent.action==='sell'?price:Number((price-.005).toFixed(3)),ask=intent.action==='buy'?price:Number((price+.005).toFixed(3));
  let r=beginDispatch(c.state,intent.intentId,{snapshot:snapshot(intent.exchangeId,bid,ask),now});assert.equal(r.ok,true,r.code);c.state=r.state;
  r=recordPlacement(c.state,intent.intentId,{status:200,response:{orderId:100+Object.keys(c.state.intents).length,exchangeId:intent.exchangeId,side:'yes',action:intent.action,quantity:intent.quantity,price:Number(intent.price),open:false,quantityTraded:filled,remainingQuantity:intent.quantity-filled,totalCost:filled*Number(intent.price)}},now);assert.equal(r.ok,true,r.code);c.state=r.state;
  const i=r.intent,quantity=(c.state.owned[i.exchangeId]?.quantity||0)+(i.action==='buy'?filled:-filled),account=snapshot(i.exchangeId);
  account.positions.rows=quantity?[{exchangeId:i.exchangeId,marketId:i.marketId,quantity}]:[];
  r=reconcileIntent(c.state,i.intentId,{at:iso(now),snapshot:account,order:{id:i.orderId,exchangeId:i.exchangeId,tournamentId:TID,side:'yes',action:i.action,priceLimit:Number(i.price),expirationDate:i.expirationDate,open:false,quantity:i.quantity},fills:{complete:true,orderId:i.orderId,exchangeId:i.exchangeId,tournamentId:TID,totalQuantityFilled:filled,rows:filled?[{id:i.orderId+1000,quantity:filled,price:Number(i.price),side:'yes',filledAt:iso(now)}]:[]}},now);assert.equal(r.ok,true,r.code);c.state=r.state;refresh();
 }
 if(owned){
  const spec={intentId:'fixture-buy',idempotencyKey:'fixture-buy-key',marketId:'377',exchangeId:'700',side:'yes',action:'buy',quantity:20,price:'0.500000',expirationDate:iso(now+90000),attribution:{runId:'sig-pilot-review',strategyId:'fixture'}};
  const r=prepareIntent(c.state,spec,snapshot(),now);assert.equal(r.ok,true,r.code);c.state=r.state;terminal(r.intent,20);
 }
 refresh();
 function book({exchangeId='700',bid=.495,ask=.5}={}){return {endpoint:'/markets/377/orderbook?tournamentId='+TID+'&depth=20',receivedAt:iso(now),requestStartedAt:iso(now),data:{contexts:[{type:'tournament',tournament:{id:TID},orderbook:{exchanges:[{exchangeId,option:'YES',asOf:{at:iso(now),sequence:now},bids:[{price:bid,quantity:10000}],asks:[{price:ask,quantity:10000}]}]}}]}};}
 return {controller:c,submitted,book,terminal,snapshot,get now(){return now;},advance(ms){now+=ms;refresh();historyRefresh();}};
}

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
