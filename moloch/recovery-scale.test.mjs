import test from 'node:test';import assert from 'node:assert/strict';
import {createMoloch,advanceMoloch,reportMoloch,assertMoloch} from './engine.mjs';
import {captureScopeMaximum,projectRestReceipt} from './live.mjs';
import {StreamSource} from './stream-source.mjs';import {journalDecoder} from './run.mjs';import {payloadHash} from '../execution/ledger.mjs';
const at='2026-10-14T00:00:00.000Z';
const book=(bid=1000,ask=1000)=>({type:'book',receivedAt:at,book:{tournamentId:'cup',marketId:'1',exchangeId:'2',sequence:'1',requestStartedAt:at,receivedAt:at,sourceAt:at,bids:[{price:'0.450000',quantity:bid}],asks:[{price:'0.455000',quantity:ask}]}});
const policy={initialCash:'100000.000000',maximumCommitted:'40000.000000',maximumMarketCost:'10000.000000',quantity:2147483647};
test('depth imbalance is directional, uses full captured depth and leaves balanced books quiet',()=>{
 const p={quoteMode:'microprice',minimumDepthImbalanceBps:3000};
 for(const [bid,ask,side]of [[1000,100,'YES'],[100,1000,'NO']]){const s=createMoloch(p);advanceMoloch(s,book(bid,ask));assert.deepEqual(s.orders.map(q=>q.side),[side]);assertMoloch(s);}
 const s=createMoloch(p),b=book(1000,100);b.book.asks.push({price:'0.460000',quantity:900});advanceMoloch(s,b);assert.equal(s.orders.length,0);
 assert.throws(()=>createMoloch({minimumDepthImbalanceBps:3000}),/DEPTH_IMBALANCE_POLICY/);
});
test('maximum official quantity is sized exactly without per-share iteration',()=>{
 const s=createMoloch(policy);advanceMoloch(s,book());assert.deepEqual(s.orders.map(q=>q.quantity),[10050,10050]);assert.equal(reportMoloch(s).reservedCash,'9999.750000');assertMoloch(s);
 assert.throws(()=>createMoloch({...policy,quantity:2147483648}),/POLICY_LIMITS/);
});
test('both paired legs respect smaller observed queue and fractional depth cap',()=>{
 for(const [entryDepthBps,n,cost]of [[10000,200,'199.000000'],[5000,100,'99.500000']]){const s=createMoloch({...policy,entryDepthBps});advanceMoloch(s,book(1000,200));assert.deepEqual(s.orders.map(q=>q.quantity),[n,n]);assert.equal(reportMoloch(s).reservedCash,cost);assertMoloch(s);}
 for(const entryDepthBps of [0,10001,1.5])assert.throws(()=>createMoloch({...policy,entryDepthBps}),/ENTRY_DEPTH_LIMIT/);
});
test('REST recovery permits four scopes explicitly and cannot share realtime mode',()=>{
 assert.equal(captureScopeMaximum(false,false),2);assert.equal(captureScopeMaximum(false,true),4);assert.equal(captureScopeMaximum(true,false),4);assert.throws(()=>captureScopeMaximum(true,true),/REST_RECOVERY_MODE/);
});
test('REST recovery preserves first verified precise execution clock for duplicate IDs',()=>{
 const identity=new StreamSource([{marketId:'1',exchangeId:'2'}],'cup',at);
 identity.trade({type:'trade',marketId:'1',exchangeId:'2',tournamentId:'cup',id:'7',idNamespace:'sig_engine_match',yesPrice:'0.450000',quantity:10,executedAt:'2026-10-14T00:00:00.123456Z',receivedAt:'2026-10-14T00:00:01.000Z'});
 const r={source:'official_sig_api_v1',status:200,tournamentId:'cup',marketId:'1',exchangeId:'2',kind:'trades',requestStartedAt:at,receivedAt:'2026-10-14T00:00:02.000Z',endpoint:'/exchanges/2/trades?tournamentId=cup',data:{marketId:'1',exchangeId:'2',data:[{id:'7',price:.45,size:10,createdAt:'2026-10-14T00:00:00.123Z'}]}};
 const [e]=projectRestReceipt(r,identity);assert.equal(e.executedAt,'2026-10-14T00:00:00.123456Z');assert.equal(e.originalExecutedAt,'2026-10-14T00:00:00.123Z');r.data.data[0].price=.44;assert.throws(()=>projectRestReceipt(r,identity),/TRADE_FACT_CONFLICT/);
});
test('shared journal reader checks every hash, sequence and predecessor',()=>{
 const event={type:'tick',receivedAt:at},body={sequence:1,previous:'',event},row={...body,hash:payloadHash(body)},decode=journalDecoder();assert.deepEqual(decode(row),event);assert.throws(()=>decode(row),/SOURCE_JOURNAL_CHAIN/);assert.throws(()=>journalDecoder()({...row,event:{...event,type:'gap'}}),/SOURCE_JOURNAL_CHAIN/);
});
