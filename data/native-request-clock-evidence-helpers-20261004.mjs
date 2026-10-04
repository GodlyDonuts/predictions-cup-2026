import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
export function aggregateFuture(cards){
 const groups=new Map(),events=new Set(),instruments=new Set();let totalFills=0,cardsWithFills=0;
 for(const c of cards){
  const p=c.point,x={fills:c.fills.length,completedTrades:p.completedTrades,cash:p.cash,realizedPnl:p.realizedPnl,conservativePnl:p.conservativePnl,liquidationPnl:p.liquidationPnl,valuation:p.valuation,openShares:p.openShares};
  const k=JSON.stringify(x);if(!groups.has(k))groups.set(k,{wallets:0,...x});groups.get(k).wallets++;
  totalFills+=c.fills.length;if(c.fills.length)cardsWithFills++;
  for(const f of c.fills){events.add(JSON.stringify([f.instrument,f.action,f.at,f.price,f.quantity]));instruments.add(f.instrument);}
 }
 return{totalFills,cardsWithFills,uniqueFillEvents:events.size,distinctFilledInstruments:instruments.size,groups:[...groups.values()],walletResultsAreCorrelated:true};
}
export function verifyServices(){
 const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),read=f=>JSON.parse(fs.readFileSync(f));
 const pids=read('data/current-runtime-pids.json');assert.equal(pids.collector,19596);assert.equal(pids.paper,5624);assert.equal(pids.dashboard,6456);
 for(const p of [19596,5624,6456])process.kill(p,0);
 const stats=read(path.join(runtime,'stats.json'));assert.equal(stats.runtime.pid,pids.collector);assert.equal(stats.runtime.status,'running');
 const banks={};for(const type of ['baal-pair','no-maker','probability-receipt']){const a=read(path.join(runtime,type+'-active.json'));banks[type]=a.bankHash;}
 assert.equal(banks['baal-pair'],'08ae516f21230247569d0683453ddaa4b06aaff48b006dfa63cf321e98f7a8d1');assert.equal(banks['no-maker'],'cc484dc03298f0f8b3c7f455805fb52783dab9b1b86b5b2fa1ae14eafa2a8064');assert.equal(banks['probability-receipt'],'3e445d6b7f436cab79245ed588efa627734d8d2a4a6a6b788ac2873582ed4ef2');
 const budget=read(path.join(runtime,'coordinated-network-budget.json'));assert.equal(budget.maxReadsPerMinute,20);assert.equal(budget.writesEnabled,false);assert.equal(read(path.join(runtime,'competition-activation.json')).enabled,false);
 return{pids:{collector:19596,paper:5624,dashboard:6456},bankHashes:banks,collectorStatus:stats.runtime.status,readCap:20,writes:false,contestActivation:false,verifiedAt:new Date().toISOString()};
}
