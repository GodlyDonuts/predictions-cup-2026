import test from 'node:test';import assert from 'node:assert/strict';
import {selectPairs,screenRows} from './screen.mjs';
import {liveScopes,captureScopes,retiredPaperScopes} from './live.mjs';
const catalog=(ids,cursor=null)=>({source:'official_sig_api_v1',status:200,kind:'catalog',endpoint:'/tournaments/midterm-elections/markets?status=open&limit=100',data:{data:ids.map(id=>({id:String(id),status:'open',title:'test',exchanges:[{id:String(id+100)}]})),pagination:{total:ids.length,hasMore:!!cursor,nextCursor:cursor}}});
test('catalog requires complete cursor chain and deterministic exploration',()=>{
  assert.deepEqual(selectPairs([catalog([1,2,3,4])],2).pairs.map(p=>p.marketId),['1','4']);
  assert.throws(()=>selectPairs([catalog([1,2,3],'next')],2),/INCOMPLETE/);
  assert.throws(()=>selectPairs([catalog([1,1])],2),/DUPLICATE/);
});
const scope={source:'official_sig_api_v1',status:200,tournamentId:'t',marketId:'1',exchangeId:'2',requestStartedAt:'2026-10-13T10:00:00Z',receivedAt:'2026-10-13T10:00:01Z'};
const book={...scope,kind:'book',endpoint:'/markets/1/orderbook?tournamentId=t',data:{contexts:[{type:'tournament',tournament:{id:'t'},orderbook:{exchanges:[{exchangeId:'2',asOf:{at:scope.receivedAt,sequence:1},bids:[{price:.4,quantity:100}],asks:[{price:.42,quantity:200}]}]}}]}};
const tape=complete=>({...scope,kind:'trades',endpoint:'/exchanges/2/trades?tournamentId=t&from=2026-10-13T09:45:00Z&to=2026-10-13T10:00:00Z',data:{marketId:'1',exchangeId:'2',from:'2026-10-13T09:45:00Z',to:'2026-10-13T10:00:00Z',pagination:{hasMore:!complete,nextCursor:complete?null:'next'},coverage:{complete,projectedThroughSequence:1},data:[{id:'engine:1',createdAt:'2026-10-13T09:59:00Z',price:.43,size:300}]}});
test('incomplete tape cannot rank; current queue turnover never creates fills',()=>{
  const r=screenRows([book,tape(true)])[0];assert.equal(r.spreadTicks,4);assert.equal(r.queueTurnover,3);assert.equal(r.askThroughVolume,300);assert.equal(r.eligible,true);assert.equal(r.fills,undefined);
  const partial=screenRows([book,tape(false)])[0];assert.equal(partial.explorationScore,null);assert.equal(partial.eligible,false);
});
test('screen refuses wrong endpoint exchange scope',()=>{assert.throws(()=>screenRows([book,{...tape(true),exchangeId:'3'}]),/TAPE_SCOPE/);});
test('live expansion retains original source and rejects duplicate exchange or excessive polling scope',()=>{
  assert.equal(liveScopes('1','2',[{marketId:'1',exchangeId:'2'},{marketId:'3',exchangeId:'4'}]).length,2);
  assert.throws(()=>liveScopes('1','2',[{marketId:'3',exchangeId:'4'}]),/MULTI_SCOPE/);
  assert.throws(()=>liveScopes('1','2',[{marketId:'1',exchangeId:'2'},{marketId:'3',exchangeId:'2'}]),/MULTI_SCOPE/);
  assert.throws(()=>liveScopes('1','2',[{marketId:'1',exchangeId:'2'},{marketId:'3',exchangeId:'4'},{marketId:'5',exchangeId:'6'}]),/MULTI_SCOPE/);
});
test('realtime execution can follow four distinct markets without duplicate market routing',()=>{
  const scope=[{marketId:'1',exchangeId:'2'},{marketId:'3',exchangeId:'4'},{marketId:'5',exchangeId:'6'},{marketId:'7',exchangeId:'8'}];assert.equal(liveScopes('1','2',scope,4).length,4);assert.throws(()=>liveScopes('1','2',[...scope,{marketId:'9',exchangeId:'10'}],4),/MULTI_SCOPE/);assert.throws(()=>liveScopes('1','2',[scope[0],{marketId:'1',exchangeId:'4'}],4),/MULTI_SCOPE/);
});
test('paper scope rotation requires zero unresolved orders and zero owned shares',()=>{
  const old=[{marketId:'1',exchangeId:'2'}],next=[{marketId:'3',exchangeId:'4'}];assert.equal(retiredPaperScopes(old,next,{orders:[{exchangeId:'2',retired:true}],positions:{}}).length,1);
  assert.throws(()=>retiredPaperScopes(old,next,{orders:[{exchangeId:'2',retired:false}],positions:{}}),/LIABILITIES/);
  assert.throws(()=>retiredPaperScopes(old,next,{orders:[],positions:{'2':{YES:[{quantity:1}],NO:[]}}}),/LIABILITIES/);
});
test('continuation requests original expired quote windows rather than releasing their cash on restart',()=>{
  const orders=[{exchangeId:'2',activationAt:1000,expiresAt:5000,retired:false},{exchangeId:'2',activationAt:2000,expiresAt:9000,cancelEffectiveAt:6000,retired:false},{exchangeId:'2',activationAt:0,expiresAt:99999,retired:false},{exchangeId:'2',activationAt:0,expiresAt:10000,retired:true}];
  const [r,untouched]=captureScopes([{marketId:'1',exchangeId:'2'},{marketId:'3',exchangeId:'4'}],{orders},20000);
  assert.equal(Date.parse(r.recoveryFrom),1000);assert.equal(Date.parse(r.recoveryTo),6001);assert.equal(untouched.recoveryFrom,undefined);assert.equal(orders[0].retired,false);
});
