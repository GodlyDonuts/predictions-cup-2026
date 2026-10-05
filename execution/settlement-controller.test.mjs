import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {SettlementCompetitionController} from './settlement-controller.mjs';
import {validateSettlementTransition,applyStoppedSettlementTransition} from './settlement-bank-transition.mjs';
import {validateProductionActivation,validateProductionHandoff} from './production-activation.mjs';
import {digest} from '../live-paper/model.mjs';
import {payloadHash} from './core.mjs';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';
import {nativeExecutable} from '../live-paper/rust-backend.mjs';

const TID='bda92870-621e-47b0-bc3c-3602c5c26f55',iso=n=>new Date(n).toISOString();
function fixture(t,options={}){
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'baal-production-'));let now=Date.parse('2026-10-04T21:00:00.000Z'),controller;
 const snapshot=JSON.parse(fs.readFileSync(new URL('../data/settlement-value-snapshot-1791144422978.json',import.meta.url))),nativeBackend=snapshot.nativeBackend??snapshot.native?.nativeBackend;
 // Use the independently verified executable graph, never a mock decision.
 const binding=nativeBackend??JSON.parse(fs.readFileSync(new URL('../data/settlement-value-milestone-20261004.json',import.meta.url))).nativeBackend;
 assert.equal(sha(fs.readFileSync(nativeExecutable)),binding.binaryHash);
 const build=path.join(dir,'native-builds',binding.binaryHash);fs.mkdirSync(build,{recursive:true});fs.copyFileSync(nativeExecutable,path.join(build,'baal-backend.exe'));fs.copyFileSync(path.join(path.dirname(nativeExecutable),'libunwind.dll'),path.join(build,'libunwind.dll'));
 const universe={exchanges:[{marketId:'377',exchangeId:'700',option:'YES'}]},handoff={schemaVersion:1,handoffId:'daenerys-to-baal-fixture',releaseCommentId:123,ackCommentId:124,formerWriter:'Daenerys',executionOwner:'Codex',writerStopped:true,inFlightResolved:true,formerReadsPerMinute:0,formerWritesPerMinute:0,releasedAt:iso(now-10000),reconciledAt:iso(now-9000),acknowledgedAt:iso(now-8000)};
 const policy={minimumEdge:'0.020000',maximumSpread:'0.050000',maximumBookAgeMs:20000,maximumResearchAgeMs:21600000,fractionalKellyBps:2500,slippageTicks:1,depthFractionBps:100,maxOrderCost:'1.000000',maxExposure:'10.000000',cashReserve:'50000.000000',cumulativeBuyBudget:'25.000000',groupCaps:{national:'5.000000',race1:'2.000000'}};
 const research={id:'fixture_forecast',predicateHash:'a'.repeat(64),side:'YES',lower:'0.800000',upper:'0.900000',recordedAtMs:now-7000,sourceAtMs:now-7500,expiresAtMs:now+3600000,method:'subjective_range',settlementReviewed:true,flags:[],sources:[{url:'https://example.org/fixture',contentHash:'b'.repeat(64)}]};
 if(options.expired)research.expiresAtMs=now-1000;if(options.conflict)research.flags=['source_conflict'];
 const root={node_type:'contract',contract_type:'Election Outcome',contract_details:{resolutionType:'Party Winner',winnerName:'Republican Party'},settled_with:null};
 const bank={schemaVersion:1,mode:'baal_settlement_production_v1',cutoff:iso(now-6000),nativeBackend:binding,policy,contracts:{'377':{rootHash:payloadHash(root),exchangeId:'700',predicateHash:research.predicateHash}},candidates:[{id:'377:YES',marketId:'377',exchangeId:'700',side:'YES',predicateHash:'a'.repeat(64),riskGroups:['national','race1'],settlesAtMs:now+30*86400000,research}]};
 const activation={schemaVersion:2,enabled:true,ownerId:'Codex',tournamentId:TID,runId:'sig-pilot-native-fixture',strategyId:'settlement-value-native-v1',handoffId:handoff.handoffId,handoffHash:payloadHash(handoff),releaseCommentId:123,ackCommentId:124,strategyBankHash:digest(bank),feeBps:0,depthFractionBps:100,maxManagedMarkets:3,maxOrderCost:policy.maxOrderCost,maxAggregateCost:policy.maxExposure,maxCumulativeBuyCost:policy.cumulativeBuyBudget,cashReserve:policy.cashReserve,sizingCapital:'100000.000000',startedAt:iso(now-5000),entryExpiresAt:iso(now+3600000)};
 fs.mkdirSync(path.join(dir,'production-banks'));fs.writeFileSync(path.join(dir,'production-banks',digest(bank)+'.json'),JSON.stringify(bank));fs.writeFileSync(path.join(dir,'production-handoff.json'),JSON.stringify(handoff));fs.writeFileSync(path.join(dir,'competition-activation.json'),JSON.stringify(activation));
 const server={cash:56266.08,quantity:options.external?10:0,orders:new Map(),operations:new Map(),posts:[],fraction:options.partial?.5:1,ambiguous:!!options.ambiguous,beforeFence:null,afterFence:null,root};
 const response=(endpoint,data)=>({status:200,endpoint,requestStartedAt:iso(now-1),receivedAt:iso(now),data});
 const page=rows=>({data:rows,pagination:{limit:200,hasMore:false,nextCursor:null}});
 const order=o=>({id:o.id,exchangeId:'700',tournamentId:TID,side:'yes',action:o.body.action,quantity:o.open?o.body.quantity-o.filled:o.body.quantity,priceLimit:o.body.price,open:o.open,createdAt:o.createdAt,expirationDate:o.body.expirationDate});
 const open=()=>{for(const o of server.orders.values())if(now>=Date.parse(o.body.expirationDate))o.open=false;return[...server.orders.values()].filter(o=>o.open).map(order);};
 const positions=()=>response('/tournaments/midterm-elections/portfolio/positions',{positions:server.quantity?[{exchangeId:'700',marketId:'377',quantity:server.quantity,lots:[]}]:[],summary:{totalMarketValue:0,totalCostBasis:0,totalUnrealizedPnl:0}});
 const book=({bid=.395,ask=.400,depth=1000}={})=>{now+=20;return response('/markets/377/orderbook?tournamentId='+TID+'&depth=20',{contexts:[{type:'tournament',tournament:{id:TID},orderbook:{exchanges:[{exchangeId:'700',option:'YES',asOf:{sequence:now,at:iso(now-500)},bids:[{price:bid,quantity:depth}],asks:[{price:ask,quantity:depth}]}]}}]});};
 const api={async dispatch(spec){now+=20;
  if(spec.method==='POST'){
   server.beforeFence?.();await spec.beforeDispatch();server.afterFence?.();await spec.beforeNetwork();server.posts.push(structuredClone(spec.body));
   const prior=server.operations.get(spec.body.idempotencyKey);if(prior){assert.equal(prior.bodyJson,spec.bodyJson);return response(spec.endpoint,prior.result);}
   const body=spec.body,filled=Math.floor(body.quantity*server.fraction),id=1001+server.orders.size,createdAt=iso(now),o={id,body:structuredClone(body),filled,open:filled<body.quantity,createdAt,fills:filled?[{id:2001+server.orders.size,price:body.price,quantity:filled,side:'yes',filledAt:createdAt}]:[]};
   server.orders.set(id,o);server.quantity+=(body.action==='buy'?1:-1)*filled;server.cash+=(body.action==='buy'?-1:1)*filled*body.price;
   const result={orderId:id,exchangeId:'700',open:o.open,remainingQuantity:body.quantity-filled,action:body.action,side:'yes',price:body.price,quantity:body.quantity,terminalReasonCode:null,quantityTraded:filled,totalCost:filled*body.price,fillPrice:body.price,all:null};server.operations.set(body.idempotencyKey,{bodyJson:spec.bodyJson,result});
   if(server.ambiguous){server.ambiguous=false;const e=Error('lost');e.status=502;e.apiCode='ORDER_STATUS_UNKNOWN';throw e;}return response(spec.endpoint,result);
  }
  assert.equal(spec.method,'GET');
  if(spec.endpoint==='/tournaments/midterm-elections')return response(spec.endpoint,{id:TID,slug:'midterm-elections',status:'active',myBalance:server.cash,isPendingEnrolment:false});
  if(spec.endpoint.endsWith('/portfolio/positions'))return positions();if(spec.endpoint.startsWith('/orders?'))return response(spec.endpoint,page(open()));
  if(spec.endpoint.startsWith('/markets/377/orderbook?'))return book();
  if(spec.endpoint.startsWith('/markets/377/nodes?'))return response(spec.endpoint,{market_id:'377',contexts:[{type:'tournament',tournament:{id:TID}}],root:server.root});
  if(spec.endpoint.startsWith('/markets/377?'))return response(spec.endpoint,{id:'377',title:'Synthetic implementation fixture',status:'open',isComposite:false,isMultiOutcome:false,exchanges:[{id:'700',option:'YES'}],contexts:[{type:'tournament',tournament:{id:TID},status:'open',settledWith:null}]});
  const match=/^\/orders\/(\d+)(\/fills)?/.exec(spec.endpoint);if(match){open();const o=server.orders.get(Number(match[1]));assert.ok(o);return response(spec.endpoint,match[2]?{...page(o.fills),orderId:o.id,exchangeId:'700',tournamentId:TID,totalQuantityFilled:o.filled,avgFillPrice:o.filled?o.body.price:null,coverage:{complete:true,projectedThroughSequence:2001+server.orders.size}}:order(o));}
  throw Error('Unexpected fixture request');
 }};
 const start=()=>controller=new SettlementCompetitionController({runtimeDir:dir,api,universe,config:{},isStopped:()=>false,activation,now:()=>now});
 const before=()=>controller.beforeBooks({positions:positions(),orders:open(),ordersAt:iso(now),candidateMarketIds:['377']});
 const cycle=async b=>{await before();await controller.onBook(book(b));};
 t.after(()=>{controller?.close();assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));fs.rmSync(dir,{recursive:true,force:true});});start();
 return{get c(){return controller;},dir,server,bank,activation,handoff,universe,before,cycle,book,advance:ms=>now+=ms,restart(){controller.close();start();},restartWith(nextActivation){controller.close();controller=new SettlementCompetitionController({runtimeDir:dir,api,universe,config:{},isStopped:()=>false,activation:nextActivation,now:()=>now});}};
}
test('current named handoff and configurable cash reserve permit a bounded native order below the old90000 balance',async t=>{
 const f=fixture(t);await f.cycle();assert.equal(f.server.posts.length,1,JSON.stringify(f.c.latestNative));assert.equal(f.server.posts[0].quantity,2);assert.equal(f.server.posts[0].price,.405);await f.before();assert.equal(f.c.state.owned['700'].cost,'0.810000');assert.equal(f.c.state.policy.cashReserve,'50000.000000');assert.ok(f.c.dir.includes('competition-runs'));assert.equal(f.c.latestNative.decision.orderEligible,false,'native proposal never grants authority');
});
test('native reduction and authoritative FIFO reconciliation survive restart without adopting external inventory',async t=>{
 const f=fixture(t);await f.cycle();await f.before();f.restart();f.advance(1000);await f.cycle({bid:.915,ask:.920});assert.equal(f.server.posts.length,2);assert.equal(f.server.posts[1].action,'sell');assert.equal(f.server.posts[1].quantity,1);await f.before();f.advance(1000);await f.cycle({bid:.915,ask:.920});await f.before();assert.deepEqual(f.c.state.owned,{});assert.equal(JSON.parse(fs.readFileSync(path.join(f.dir,'competition-stats.json'))).realizedPnl,'1.020000','fixture proves arithmetic, not profitability');
});
for(const mode of ['expired','conflict','external'])test('native entry rejects '+mode,async t=>{const f=fixture(t,{[mode]:true});await f.cycle();assert.equal(f.server.posts.length,0);assert.equal(f.c.latestNative.decision.action,'observe');});
test('ownership loss at either admission fence prevents network placement',async t=>{
 for(const phase of ['beforeFence','afterFence']){const f=fixture(t);f.server[phase]=()=>fs.unlinkSync(path.join(f.dir,'production-handoff.json'));await f.cycle();assert.equal(f.server.posts.length,0);if(phase==='beforeFence')assert.equal(f.c.active(),undefined);else assert.equal(f.c.active().status,'uncertain','once durable dispatch starts, retain uncertainty instead of erasing the intent');}
});
test('research expiry at the final fence prevents acceptance and preserves a conservatively uncertain intent',async t=>{
 const f=fixture(t);f.server.afterFence=()=>f.advance(3600000);await f.cycle();assert.equal(f.server.posts.length,0);assert.equal(f.c.active().status,'uncertain');assert.equal(f.c.active().attempts,1);
});
test('partial fill is adopted only once and unknown acceptance is recovered with the exact idempotency request',async t=>{
 const f=fixture(t,{partial:true,ambiguous:true});await f.cycle();assert.equal(f.server.posts.length,1);assert.equal(f.c.active().status,'uncertain');assert.deepEqual(f.c.state.owned,{});f.advance(31000);f.restart();await f.before();await f.before();assert.equal(f.server.posts.length,2);assert.deepEqual(f.server.posts[0],f.server.posts[1]);await f.before();assert.equal(f.c.state.owned['700'].quantity,1);assert.equal(f.c.state.owned['700'].cost,'0.405000');
});
test('changed bank or policy cannot silently resume the same ledger',async t=>{
 const f=fixture(t);await f.cycle();await f.before();const file=path.join(f.dir,'production-banks',f.activation.strategyBankHash+'.json');fs.writeFileSync(file,JSON.stringify({...f.bank,cutoff:'changed'}));f.advance(1000);await assert.rejects(f.c.onBook(f.book()),/BANK_CHANGED/);assert.equal(f.server.posts.length,1);
});
test('a changed current contract blocks orders even when the supplied forecast still has an edge',async t=>{
 const f=fixture(t);f.server.root={...f.server.root,contract_details:{...f.server.root.contract_details,winnerName:'Democratic Party'}};await f.before();await assert.rejects(f.c.onBook(f.book()),/CONTRACT_NOT_CURRENT/);assert.equal(f.server.posts.length,0);
});
test('activation and handoff validators reject old or fabricated coordination',t=>{
 const f=fixture(t);assert.equal(validateProductionActivation(f.activation),f.activation);assert.equal(validateProductionHandoff(f.handoff,f.activation),f.handoff);
 for(const change of [{schemaVersion:1},{releaseCommentId:0},{ackCommentId:0},{maxOrderCost:'25.000000'},{feeBps:1},{cashReserve:'100000.000000'}])assert.throws(()=>validateProductionActivation({...f.activation,...change}));
 for(const change of [{writerStopped:false},{inFlightResolved:false},{executionOwner:'Daenerys'},{ackCommentId:999}])assert.throws(()=>validateProductionHandoff({...f.handoff,...change},f.activation));
});
test('new research bank retains the same run, owned fills, FIFO cost and cumulative attempt budget across restart',async t=>{
 const f=fixture(t);await f.cycle();await f.before();const original=structuredClone(f.c.state),next=structuredClone(f.bank);next.cutoff=iso(f.c.now());next.candidates[0].research.id='new-reviewed-evidence';next.candidates[0].research.lower='0.700000';
 const updated=validateSettlementTransition({activation:f.activation,previousBank:f.bank,nextBank:next,universe:f.universe,intents:f.c.state.intents,owned:f.c.state.owned,nowMs:f.c.now()});
 assert.deepEqual({...updated,strategyBankHash:f.activation.strategyBankHash},f.activation);const run=f.c.dir;f.c.close();fs.writeFileSync(path.join(f.dir,'COLLECTOR_STOP'),JSON.stringify({source:'settlement_bank_transition',nextBankHash:digest(next)}));
 const applied=applyStoppedSettlementTransition({runtimeDir:f.dir,collectorLockFile:path.join(f.dir,'bot.lock'),nextBank:next,universe:f.universe,now:()=>f.c.now()});assert.equal(applied.nextActivation.runId,f.activation.runId);assert.equal(JSON.parse(fs.readFileSync(applied.recordFile)).accountingReset,false);fs.unlinkSync(path.join(f.dir,'COLLECTOR_STOP'));f.restartWith(applied.nextActivation);assert.equal(f.c.dir,run);assert.deepEqual(f.c.state.owned,original.owned);assert.deepEqual(f.c.state.intents,original.intents);await f.before();await f.cycle();assert.equal(f.server.posts.length,1,'owned inventory is not bought again');assert.equal(f.c.latestNative.decision.action,'hold');
});
test('source transition rejects open intents, widening limits and removal or rebinding of existing candidates',t=>{
 const f=fixture(t);const next=structuredClone(f.bank);next.cutoff='review2';const input={activation:f.activation,previousBank:f.bank,nextBank:next,universe:f.universe,nowMs:f.c.now()};assert.throws(()=>validateSettlementTransition({...input,intents:{x:{status:'uncertain'}}}),/UNRESOLVED_INTENT/);
 const policy=structuredClone(next);policy.policy.minimumEdge='0.019000';assert.throws(()=>validateSettlementTransition({...input,nextBank:policy}));const binding=structuredClone(next);binding.contracts['377'].rootHash='b'.repeat(64);assert.throws(()=>validateSettlementTransition({...input,nextBank:binding}),/EXISTING_CONTRACT_CHANGED/);assert.throws(()=>validateSettlementTransition({...input,owned:{'999':{marketId:'999'}}}),/OWNED_SCOPE_MISSING/);
 const lockFile=path.join(f.dir,'bot.lock');fs.writeFileSync(path.join(f.dir,'COLLECTOR_STOP'),JSON.stringify({source:'settlement_bank_transition',nextBankHash:digest(next)}));fs.writeFileSync(lockFile,'1');assert.throws(()=>applyStoppedSettlementTransition({runtimeDir:f.dir,collectorLockFile:lockFile,nextBank:next,universe:f.universe,now:()=>f.c.now()}),/WRITER_PRESENT/);assert.equal(JSON.parse(fs.readFileSync(path.join(f.dir,'competition-activation.json'))).strategyBankHash,f.activation.strategyBankHash);
});
test('stale account is refreshed before the next decision book without widening the20-second admission gate',async t=>{
 const f=fixture(t);await f.before();f.advance(25000);await f.c.beforeBook('377');const accountAt=Date.parse(f.c.account.positions.at);await f.c.onBook(f.book());assert.equal(f.server.posts.length,1);assert(accountAt<=f.c.now());assert.equal(f.c.state.policy.maxAccountAgeMs,20000);assert.equal(f.c.lastFreshnessRepair.marketId,'377');
});
test('fresh account and unrelated paper books add no freshness repair reads',async t=>{
 const f=fixture(t);await f.before();const at=f.c.account.positions.at;await f.c.beforeBook('377');assert.equal(f.c.account.positions.at,at);assert.equal(f.c.lastFreshnessRepair,undefined);f.advance(25000);await f.c.beforeBook('999');assert.equal(f.c.account.positions.at,at);
});
test('expired production pilot reports its actual window after bootstrap and cannot create a new purchase',async t=>{
 const f=fixture(t);f.advance(21600001);await f.before();await f.c.onBook(f.book());const s=JSON.parse(fs.readFileSync(path.join(f.dir,'competition-stats.json')));
 assert.equal(s.entryWindow.phase,'closed');assert.equal(s.status,'entry_window_closed');assert.equal(s.reason,'ENTRY_WINDOW_CLOSED');assert.equal(f.server.posts.length,0);assert.deepEqual(f.c.state.owned,{});assert.equal(s.entryAuthorityExtended,false);
});
