import test from 'node:test';import assert from 'node:assert/strict';
import {allocateFundedMakerQuotesV1} from './maker-funded-allocation-v1.mjs';
import {createMakerSchedulerV1,planMakerQuoteV1} from './maker-quote-scheduler-v1.mjs';
const at='2026-10-12T22:30:00.000Z';
const quote=(extra={})=>({exchangeId:'101',side:'yes',action:'buy',quantity:2,price:'0.450000',netEdgePerShare:'0.010000',
  sourceAt:at,validUntil:'2026-10-12T22:30:10.000Z',reduceOnly:false,...extra});
const order=(extra={})=>({...quote(),orderId:'1',remainingQuantity:2,status:'open',createdAt:'2026-10-12T22:29:55.000Z',...extra});
const owned=(extra={})=>({exchangeId:'101',side:'yes',quantity:4,cost:'1.800000',...extra});
const allocate=(extra={})=>allocateFundedMakerQuotesV1({at,cashAvailable:'1.000000',maximumCommitted:'5.000000',maximumOrderCost:'5.000000',
  maximumShares:100,ownedPositions:[],openOrders:[],proposals:[quote()],...extra});

test('cash is allocated once across ranked quotes, using exact fractional prices',()=>{
  const r=allocate({proposals:[quote(),quote({exchangeId:'102',price:'0.500000'})]});
  assert.equal(r.quotes.length,1);assert.equal(r.quotes[0].quantity,2);assert.equal(r.additionalBuyReservation,'0.900000');
  assert.equal(r.cashAvailableAfter,'0.100000');assert.equal(r.profitabilityEstablished,false);
});
test('same-price resting quote retains funding and queue position with zero incremental cash',()=>{
  const r=allocate({cashAvailable:'0.000000',openOrders:[order()]});
  assert.equal(r.quotes[0].quantity,2);assert.equal(r.additionalBuyReservation,'0.000000');
  assert.equal(planMakerQuoteV1(createMakerSchedulerV1(),{at,account:{complete:true,observedAt:at,orders:[order()]},desired:r.quotes,
    budget:{availableWrites:1,nextWriteAt:at},entriesEnabled:true,exitsEnabled:true,cancelEnabled:true}).kind,'hold');
});
test('reprice cannot spend cash reserved by prior order until cancellation confirmation',()=>{
  const before=allocate({cashAvailable:'0.000000',openOrders:[order()],proposals:[quote({price:'0.455000'})]});
  assert.equal(before.quotes.length,0);assert.equal(before.existingBuyReservation,'0.900000');
  const after=allocate({cashAvailable:'0.900000',openOrders:[],proposals:[quote({price:'0.455000'})]});
  assert.equal(after.quotes[0].quantity,1);assert.equal(after.additionalBuyReservation,'0.455000');
});
test('shared risk cap includes owned basis and all still-open purchases',()=>{
  const r=allocate({cashAvailable:'10.000000',maximumCommitted:'3.000000',ownedPositions:[owned()],openOrders:[order({exchangeId:'102'})],
    proposals:[quote({exchangeId:'103',quantity:100})]});
  assert.equal(r.riskCapacityBefore,'0.300000');assert.equal(r.quotes.length,0);
});
test('SELL and complement BUY cannot consume the same owned shares',()=>{
  const r=allocate({cashAvailable:'10.000000',ownedPositions:[owned()],proposals:[quote({action:'sell',reduceOnly:true,price:'0.460000',quantity:3}),
    quote({side:'no',price:'0.535000',quantity:4})]});
  assert.equal(r.quotes[0].action,'sell');assert.equal(r.quotes[0].quantity,3);assert.equal(r.quotes[1].quantity,1);
});
test('existing hedges reserve inventory, and matching old quotes do not double-count reservations',()=>{
  const oldSell=order({action:'sell',reduceOnly:true,price:'0.460000'}),oldHedge=order({orderId:'2',side:'no',price:'0.535000'});
  const r=allocate({cashAvailable:'0.000000',ownedPositions:[owned()],openOrders:[oldSell,oldHedge],proposals:[
    quote({action:'sell',reduceOnly:true,price:'0.460000'}),quote({side:'no',price:'0.535000',quantity:3})]});
  assert.equal(r.quotes.length,2);assert.deepEqual(r.quotes.map(q=>q.quantity),[2,2]);assert.equal(r.additionalBuyReservation,'0.000000');
});
test('foreign holdings are never adopted to support a sale; unresolved orders cannot free cash',()=>{
  assert.equal(allocate({proposals:[quote({action:'sell',reduceOnly:true})]}).quotes.length,0);
  assert.throws(()=>allocate({openOrders:[order({status:'cancel_pending'})]}),/UNRESOLVED_ORDER/);
  assert.throws(()=>allocate({ownedPositions:[owned({quantity:2,cost:'0.900000'})],openOrders:[order({action:'sell'}),order({orderId:'2',side:'no'})]}),/EXISTING_DOUBLE_PLEDGE/);
});
test('partial fills preserve only remaining reservation; per-order and share caps apply',()=>{
  const r=allocate({cashAvailable:'0.450000',maximumOrderCost:'0.900000',maximumShares:2,openOrders:[order({status:'partial',remainingQuantity:1})],
    proposals:[quote({quantity:100})]});
  assert.equal(r.existingBuyReservation,'0.450000');assert.equal(r.additionalBuyReservation,'0.450000');assert.equal(r.quotes[0].quantity,2);
});
test('stale high-edge proposals do not consume capacity needed by fresh quotes',()=>{
  const r=allocate({proposals:[quote({netEdgePerShare:'0.100000',sourceAt:'2026-10-12T22:29:00.000Z'}),quote({exchangeId:'102'})]});
  assert.equal(r.quotes[0].exchangeId,'102');assert.equal(r.additionalBuyReservation,'0.900000');
  assert.equal(r.declined[0].reason,'stale_expired_or_insufficient_net_edge');
  assert.equal(allocate({proposals:[quote({validUntil:at})]}).additionalBuyReservation,'0.000000');
});
