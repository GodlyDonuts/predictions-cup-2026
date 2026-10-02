import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import test from 'node:test';import assert from 'node:assert/strict';
import {createPairedPortfolio,reservePairedOrder,applyReconciledPairedOrder} from './paired-portfolio.mjs';
import {openCanonicalOrderJournal} from './canonical-order-journal.mjs';
const scope={ownerId:'Codex',runId:'pair-1',tournamentId:'scope',tournamentSlug:'midterm-elections',markets:[{marketId:'381',exchangeId:'1070'},{marketId:'382',exchangeId:'1071'}]},h='a'.repeat(64),beforeStart='2026-10-02T22:59:58.000Z',clock='2026-10-02T23:00:00.000Z';
const snapshot=(cash='1000.000000',positions=[],start=beforeStart,end='2026-10-02T22:59:59.000Z')=>({tournamentId:'scope',complete:true,positionsSourceHash:h,positions:[{marketId:'379',exchangeId:'1068',quantity:3},...positions],openOrders:[],startedAt:start,completedAt:end,cashReceipt:{kind:'tournament_cash',tournamentId:'scope',joined:true,method:'GET',endpoint:'/tournaments/midterm-elections',sourceBodyHash:h,receivedAt:end,fields:{myBalance:{available:true,decimal:cash}}}});
const request=(extra={})=>({intentId:'i1',idempotencyKey:'k1',marketId:'381',exchangeId:'1070',action:'buy',quantity:6,limit:'0.200000',createdAt:clock,expirationDate:'2026-10-02T23:00:30.000Z',...extra});
function fixture(t,{filled=6}={}){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'paired-core-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));const original=snapshot(),initial=createPairedPortfolio(scope,original,clock),reserved=reservePairedOrder(initial,request(),original,clock),w=openCanonicalOrderJournal({dir});
 w.prepare(reserved.active.preparation);w.markDispatch();w.bind('1001');
 const rows=[{id:2001,orderId:1001,exchangeId:'1070',marketId:'381',tournamentId:'scope',side:'no',quantity:-filled,price:.8,filledAt:'2026-10-02T23:00:01.000Z'}];
 w.observe({ack:{orderId:1001,exchangeId:'1070',side:'no',action:'buy',price:'0.200000',quantity:6,all:null,totalCost:(filled*.2).toFixed(6),quantityTraded:filled,remainingQuantity:6-filled,open:false,...(filled<6?{terminalReasonCode:'SelfTradePrevented'}:{})},rows,source:'realtime_account_batch',readAt:'2026-10-02T23:00:03.000Z'});
 const order=w.status();w.close();const after=snapshot((1000-filled*.2).toFixed(6),[{marketId:'381',exchangeId:'1070',quantity:-filled}],'2026-10-02T23:00:04.000Z','2026-10-02T23:00:05.000Z'),activity={tournamentId:'scope',complete:true,sourceHash:h,from:beforeStart,through:after.completedAt,events:[{kind:'fill',fillId:2001,orderId:'1001',marketId:'381',exchangeId:'1070',at:rows[0].filledAt}]},readAt='2026-10-02T23:00:06.000Z';return {original,initial,reserved,order,after,activity,readAt};
}
test('per-order terminal projection joins shared cash only after matching complete account evidence',t=>{
 const f=fixture(t),next=applyReconciledPairedOrder(f.reserved,f.order,f.after,f.activity,f.readAt);assert.equal(next.cash,'23.800000');assert.equal(next.cumulativeBuy,'1.200000');assert.equal(next.owned['1070'].lots[0].quantity,6);assert.equal(next.externalPositions['1068'].quantity,3);assert.equal(next.balanceMode,'entry');assert.equal(next.active,null);assert.equal(next.executionAuthority,false);assert.throws(()=>applyReconciledPairedOrder(next,f.order,f.after,f.activity,f.readAt),/NOT_TERMINAL/);
});
test('one unresolved order blocks the other leg and retains full reservation',t=>{
 const f=fixture(t,{filled:2});assert.equal(f.reserved.active.reservation,'1.200000');assert.throws(()=>reservePairedOrder(f.reserved,request({marketId:'382',exchangeId:'1071'}),f.original,clock),/ACTIVE_INTENT/);
 const next=applyReconciledPairedOrder(f.reserved,f.order,f.after,f.activity,f.readAt);assert.equal(next.cash,'24.600000');assert.throws(()=>reservePairedOrder(next,request({intentId:'i2',idempotencyKey:'k2',marketId:'382',exchangeId:'1071',createdAt:f.readAt,expirationDate:'2026-10-02T23:00:36.000Z'}),f.after,f.readAt),/BALANCE_LEG/);
 const second=reservePairedOrder(next,request({intentId:'i2',idempotencyKey:'k2',marketId:'382',exchangeId:'1071',quantity:2,createdAt:f.readAt,expirationDate:'2026-10-02T23:00:36.000Z'}),f.after,f.readAt);assert.equal(second.active.preparation.wallet.cash,'24.600000');assert.equal(second.active.reservation,'0.400000');
});
test('cash mismatch, external changes, incomplete activity, open orders and persistence uncertainty cannot release reservation',t=>{
 const f=fixture(t);for(const mutate of [x=>x.after.cashReceipt.fields.myBalance.decimal='998.799999',x=>x.after.positions[0].quantity=4,x=>x.after.openOrders=[{}],x=>x.activity.complete=false,x=>x.activity.events.push({kind:'collateral'}),x=>x.order.unconfirmedPersistence=true,x=>x.order.state.wallet.cash='26.000000',x=>x.after.startedAt=clock]){const x=structuredClone(f);mutate(x);assert.throws(()=>applyReconciledPairedOrder(x.reserved,x.order,x.after,x.activity,x.readAt));assert.equal(f.reserved.active.reservation,'1.200000');}
});
test('initial unrelated holdings are kept external, same-exchange holdings reject adoption',()=>{
 const s=snapshot();s.positions.push({marketId:'381',exchangeId:'1070',quantity:-1});assert.throws(()=>createPairedPortfolio(scope,s,clock),/EXISTING_HOLDING/);s.positions.pop();s.cashReceipt.joined=false;assert.throws(()=>createPairedPortfolio(scope,s,clock),/CASH_RECEIPT/);
});
test('hard cost, quantity, TTL and covered-sale fences block unintended exposure',()=>{
 const a=snapshot(),s=createPairedPortfolio(scope,a,clock);for(const change of [{limit:'0.900000'},{quantity:7},{action:'sell'},{expirationDate:'2026-10-02T23:01:00.000Z'},{limit:'0.201000'}])assert.throws(()=>reservePairedOrder(s,request(change),a,clock));
});
test('both legs and sequential closes share one cash ledger, preserve losses and never replenish cumulative purchase budget',t=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'paired-lifecycle-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
 for(const salePrice of [.25,.15]){
  let acct=snapshot(),s=createPairedPortfolio(scope,acct,clock),officialCash=1000,counter=0;
  for(const [exchangeId,marketId,action] of [['1070','381','buy'],['1071','382','buy'],['1070','381','sell'],['1071','382','sell']]){
   counter++;const now=new Date(Date.parse(clock)+(counter-1)*6000).toISOString(),price=action==='buy'?.2:salePrice,q=6;
   const reserved=reservePairedOrder(s,request({intentId:'intent-'+counter,idempotencyKey:'key-'+counter,exchangeId,marketId,action,limit:price.toFixed(6),createdAt:now,expirationDate:new Date(Date.parse(now)+30000).toISOString()}),acct,now);
   const w=openCanonicalOrderJournal({dir:path.join(dir,String(salePrice)+'-'+counter)});w.prepare(reserved.active.preparation);w.markDispatch();const orderId=String(1000+counter);w.bind(orderId);
   const fillAt=new Date(Date.parse(now)+1000).toISOString(),readAt=new Date(Date.parse(now)+3000).toISOString();
   w.observe({ack:{orderId:Number(orderId),exchangeId,side:'no',action,quantity:q,price:price.toFixed(6),all:null,totalCost:(q*price).toFixed(6),quantityTraded:q,remainingQuantity:0,open:false},rows:[{id:2000+counter,orderId:Number(orderId),marketId,exchangeId,tournamentId:'scope',side:'no',quantity:-q,price:1-price,filledAt:fillAt}],source:'realtime_account_batch',readAt});
   const positions=scope.markets.flatMap(m=>{const owned=s.owned[m.exchangeId].lots.reduce((n,l)=>n+l.quantity,0),quantity=owned+(m.exchangeId===exchangeId?(action==='buy'?q:-q):0);return quantity?[{marketId:m.marketId,exchangeId:m.exchangeId,quantity:-quantity}]:[];});
   officialCash+=(action==='buy'?-1:1)*q*price;const after=snapshot(officialCash.toFixed(6),positions,new Date(Date.parse(now)+4000).toISOString(),new Date(Date.parse(now)+5000).toISOString());
   const activity={tournamentId:'scope',complete:true,sourceHash:h,from:acct.startedAt,through:after.completedAt,events:[{kind:'fill',fillId:2000+counter,orderId,marketId,exchangeId,at:fillAt}]};
   s=applyReconciledPairedOrder(reserved,w.status(),after,activity,new Date(Date.parse(now)+6000).toISOString());w.close();acct=after;
   assert.equal(s.balanceMode,counter===1?'entry':counter===3?'exit':null);
   if(counter===3){const nextClock=new Date(Date.parse(now)+6000).toISOString();assert.throws(()=>reservePairedOrder(s,request({intentId:'reopen',idempotencyKey:'reopen-key',quantity:1,createdAt:nextClock,expirationDate:new Date(Date.parse(nextClock)+30000).toISOString()}),acct,nextClock),/BALANCE_LEG/);}
  }
  assert.equal(s.cash,salePrice===.25?'25.600000':'24.400000');assert.equal(s.realizedPnl,salePrice===.25?'0.600000':'-0.600000');assert.equal(s.cumulativeBuy,'2.400000');assert.equal(s.completed.length,4);assert(Object.values(s.owned).every(x=>x.lots.length===0));
 }
});
test('a covered close must bound the cost of every possible unmatched opposite holding',()=>{
 const original=snapshot(),s=createPairedPortfolio(scope,original,clock);for(const m of scope.markets)s.owned[m.exchangeId].lots=[{lotId:'old-'+m.exchangeId,side:'no',quantity:6,entryPrice:'0.900000',openedAt:beforeStart}];s.cash='14.200000';
 const observed=snapshot('989.200000',scope.markets.map(m=>({marketId:m.marketId,exchangeId:m.exchangeId,quantity:-6})));
 assert.throws(()=>reservePairedOrder(s,request({action:'sell',limit:'0.950000'}),observed,clock),/ORPHAN_LIMIT/);
 assert.equal(reservePairedOrder(s,request({action:'sell',quantity:5,limit:'0.950000'}),observed,clock).active.reservation,'0.000000');
});
