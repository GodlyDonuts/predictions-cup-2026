import test from 'node:test';import assert from 'node:assert/strict';
import {projectCanonicalFifo} from './canonical-fifo.mjs';
const at='2026-10-02T23:00:00.000Z';
function fixture({side='no',action='buy',quantity=6,limit='0.200000',lots=[],filled=quantity}={}){
 const held=lots.reduce((n,l)=>n+(l.side==='yes'?1:-1)*l.quantity,0),backed=action==='sell'&&held*(side==='yes'?1:-1)>=quantity;
 const complement=action==='sell'&&!backed,cs=complement?(side==='yes'?'no':'yes'):side,ca=backed?'sell':'buy',cp=complement?(1-Number(limit)).toFixed(6):limit;
 return {wallet:{ownerId:'Codex',tournamentId:'scope',marketId:'381',exchangeId:'1070',cash:'100.000000',realizedPnl:'0.000000',accountSignedQuantity:held,lots},
  binding:{ownerId:'Codex',orderId:'1001',tournamentId:'scope',marketId:'381',exchangeId:'1070',side,action,quantity,limit,createdAt:at},
  ack:{orderId:1001,exchangeId:'1070',side:cs,action:ca,quantity,price:cp,all:null,quantityTraded:filled,remainingQuantity:quantity-filled,open:filled<quantity,totalCost:(filled*Number(cp)).toFixed(6)},
  rows:filled?[{id:2001,orderId:1001,exchangeId:'1070',marketId:'381',tournamentId:'scope',side:cs,quantity:(cs==='yes'?1:-1)*filled,price:cs==='yes'?Number(cp):1-Number(cp),filledAt:'2026-10-02T23:00:01.000Z'}]:[],source:'realtime_account_batch',readAt:'2026-10-02T23:00:02.000Z'};
}
const lot=(side,quantity,entryPrice,lotId='old')=>({side,quantity,entryPrice,lotId,openedAt:'2026-10-02T22:00:00.000Z'});
test('NO buy then backed NO sale conserves exact cash and realizes actual acquired basis',()=>{
 const buy=projectCanonicalFifo(fixture());assert.equal(buy.cashDelta,'-1.200000');assert.equal(buy.wallet.accountSignedQuantity,-6);
 const sell=fixture({side:'no',action:'sell',limit:'0.250000',lots:buy.wallet.lots});sell.wallet={...buy.wallet};sell.binding.createdAt='2026-10-02T23:00:01.000Z';sell.binding.orderId='1002';sell.ack.orderId=1002;sell.rows[0].orderId=1002;
 const result=projectCanonicalFifo(sell);assert.equal(result.realizedDelta,'0.300000');assert.equal(result.wallet.cash,'100.300000');assert.deepEqual(result.wallet.lots,[]);assert.equal(result.executionAuthority,false);
});
test('opposite BUY nets through zero with FIFO proceeds, loss and residual NO cost',()=>{
 const x=fixture({lots:[lot('yes',2,'0.850000','a'),lot('yes',2,'0.700000','b')]});const p=projectCanonicalFifo(x);
 assert.equal(p.closedQuantity,4);assert.equal(p.openedQuantity,2);assert.equal(p.cashDelta,'2.800000');assert.equal(p.realizedDelta,'0.100000');assert.equal(p.wallet.accountSignedQuantity,-2);assert.equal(p.wallet.lots[0].entryPrice,'0.200000');
 const loss=projectCanonicalFifo(fixture({lots:[lot('yes',4,'0.900000')]}));assert.equal(loss.realizedDelta,'-0.400000');assert.equal(loss.wallet.realizedPnl,'-0.400000');
});
test('flat and oversized SELL acknowledgement must match actual complement BUY',()=>{
 const p=projectCanonicalFifo(fixture({side:'yes',action:'sell',limit:'0.800000'}));assert.equal(p.complement,true);assert.equal(p.canonicalSide,'no');assert.equal(p.openedQuantity,6);
 const x=fixture({side:'yes',action:'sell',limit:'0.800000',lots:[lot('yes',2,'0.700000')]});assert.equal(projectCanonicalFifo(x).wallet.accountSignedQuantity,-4);x.ack.action='sell';assert.throws(()=>projectCanonicalFifo(x),/CANONICAL_ACK/);
});
test('split fills and duplicate delivery preserve FIFO quantities without double counting',()=>{
 const x=fixture({side:'yes',action:'sell',quantity:5,limit:'0.750000',lots:[lot('yes',2,'0.800000','a'),lot('yes',4,'0.600000','b')]});const row=x.rows[0];x.rows=[{...row,quantity:2},{...row,id:2002,quantity:3},{...row,quantity:2}];const p=projectCanonicalFifo(x);assert.equal(p.filledQuantity,5);assert.equal(p.realizedDelta,'0.350000');assert.equal(p.wallet.lots[0].quantity,1);
 x.rows[2].price=.755;assert.throws(()=>projectCanonicalFifo(x),/CONFLICTING/);
});
test('partial canonical BUY retains hard reservation; known terminal cancel releases it',()=>{
 const x=fixture({filled:2});const p=projectCanonicalFifo(x);assert.equal(p.remainingReservation,'0.800000');assert.equal(p.wallet.accountSignedQuantity,-2);
 x.ack.open=false;assert.throws(()=>projectCanonicalFifo(x),/FILL_TOTAL/);x.ack.terminalReasonCode='SelfTradePrevented';assert.equal(projectCanonicalFifo(x).remainingReservation,'0.000000');
});
test('unknown REST NO basis, collateral adjustment, unrelated holdings and mismatched ack reject',()=>{
 for(const mutate of [x=>x.source='rest_order_fills',x=>x.ack.all={},x=>x.wallet.accountSignedQuantity=1,x=>x.ack.price='0.205000',x=>x.ack.totalCost='1.199999',x=>x.wallet.ownerId='other',x=>x.rows[0].exchangeId='1071',x=>x.rows[0].quantity=6]){const x=fixture();mutate(x);assert.throws(()=>projectCanonicalFifo(x));}
});
test('losses remain signed on subsequent order and cash cannot underflow',()=>{
 const x=fixture();x.wallet.realizedPnl='-0.400000';assert.equal(projectCanonicalFifo(x).wallet.realizedPnl,'-0.400000');x.wallet.cash='0.100000';assert.throws(()=>projectCanonicalFifo(x),/UNDERFLOW/);
});
test('applied order cannot net or consume the same FIFO lots twice',()=>{
 const x=fixture({quantity:2,lots:[lot('yes',6,'0.900000')]});const p=projectCanonicalFifo(x);x.wallet=p.wallet;assert.throws(()=>projectCanonicalFifo(x),/ORDER_ALREADY_PROJECTED/);
});
test('SELL and complementary BUY have equal cash, FIFO realized result and residual exposure across partial and crossing fills',()=>{
 for(const heldSide of ['yes','no'])for(const held of [0,2,8])for(const quantity of [2,6,10])for(const filled of [0,1,quantity]){
  const lots=held?[lot(heldSide,held,'0.700000')]:[],opposite=heldSide==='yes'?'no':'yes';
  const sale=projectCanonicalFifo(fixture({side:heldSide,action:'sell',quantity,filled,limit:'0.750000',lots}));
  const buy=projectCanonicalFifo(fixture({side:opposite,action:'buy',quantity,filled,limit:'0.250000',lots}));
  assert.equal(sale.cashDelta,buy.cashDelta);assert.equal(sale.realizedDelta,buy.realizedDelta);assert.deepEqual(sale.wallet,buy.wallet);
 }
});
