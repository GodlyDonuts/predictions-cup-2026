import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
import * as scheduler from './book-scheduler.mjs';
import {collectionInputs,shockCollectionRisk} from './collection-inputs.mjs';
import {networkInterval,validateNetworkBudget} from './network-budget.mjs';
import {connectTournament} from './connection.mjs';
import {validateTiming,timingSummary} from './read-timing.mjs';
import * as model from './model.mjs';
import * as breadth from './breadth.mjs';
import * as depth from './depth-scope.mjs';
import {exclusionSet} from './runtime.mjs';
import {replayJournal} from './replay.mjs';
import {accountPositions,accountOrders,accountFills,accountPnl,accountView} from './account.mjs';
import {historyEndpoint,normalizeHistory} from './history.mjs';import {validateDipPolicy} from './dip-shadow.mjs';import {workerHealth,baselineView} from './worker-health.mjs';

const root=fileURLToPath(new URL('.',import.meta.url)).replace(/[/\\]$/,'');
const config=JSON.parse(fs.readFileSync(path.join(root,'config.json'),'utf8'));
const universe=JSON.parse(fs.readFileSync(path.join(root,'price-universe.json'),'utf8'));
const source=fs.readFileSync(path.join(root,'runner.mjs'),'utf8');

// Only module wiring is replaced. The real runner body executes with an in-memory
// journal/filesystem, synthetic API/key, fake clock/server/process and no network.
async function run(stopAt,{dipEnabled=false,officialSync=false}={}){
  let startupFailures=0;
  let now=Date.parse('2026-10-02T02:00:00Z'),cycle=0,stopped=false,triggered=false;
  const commits=[],memory=new Map(),listeners={},requests=[];
  if(dipEnabled)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','dip-bank.json'),JSON.stringify({policy:JSON.parse(fs.readFileSync(path.join(root,'dip-policy.json'))),config:{...config,feeBps:0},universe:breadth.validateUniverse(universe,config.tournamentId)}));
  if(dipEnabled){const dir=path.join('/synthetic','SigTradingCup','paper-runtime'),bank=JSON.parse(memory.get(path.join(dir,'dip-bank.json')));memory.set(path.join(dir,'dip-state.json'),JSON.stringify({bankHash:model.digest(bank),wallet:model.initialState(config,'fixture',new Date(now).toISOString()),histories:{},bookHistory:{}}));}
  if(officialSync)memory.set(path.join('/synthetic','SigTradingCup','paper-runtime','official-sync.json'),JSON.stringify({enabled:true}));
  const iso=()=>new Date(now).toISOString();
  const stop=()=>{triggered=true;stopped=true;listeners.SIGTERM();};
  const interrupt=stage=>{if(cycle===2&&stage===stopAt){stop();throw Error('Stopped');}};
  class Clock extends Date{constructor(...args){super(...(args.length?args:[now]));}static now(){return now;}}
  class Journal{
    constructor(){this.last=null;this.previous='';this.sequence=0;}
    commit(state,event){
      if(cycle===1&&event.type==='market_history'&&stopAt==='history-recording-failure'){stop();throw Error('Injected journal failure');}
      if(cycle===2&&event.type==='cycle'&&stopAt==='recording-failure'){stop();throw Error('Injected journal failure');}
      if(cycle===2&&event.type==='cycle'&&stopAt==='cycle-commit')stop();
      const body={sequence:++this.sequence,previous:this.previous,at:iso(),event:structuredClone(event),state:structuredClone(state)};
      const hash=model.digest(body);commits.push({...body,hash});this.last=body;this.previous=hash;
    }
  }
  class ReadClient{
    constructor(key,{onAdmission=()=>{}}={}){this.requests=0;this.cooldown=0;this.next=0;this.onAdmission=onAdmission;}
    close(){}
    async pause(delay){
      if(cycle===0&&stopAt==='startup-stop'){stop();throw Error('Stopped');}
      if(cycle===0)now+=delay;
      if((cycle===1&&stopAt==='cycle-wait')||(cycle===2&&stopAt==='outage-wait')){stop();throw Error('Stopped');}
    }
    async orders(){if(stopped)throw Error('Read after stop');interrupt('orders');this.requests++;now+=6000;return [];}
    async request(endpoint){
      if(stopped)throw Error('Read after stop');
      if(cycle===1&&endpoint.includes('/price-history?')&&stopAt==='history-stop'){stop();throw Error('Stopped');}
      if(endpoint.endsWith('/portfolio/positions'))cycle++;
      const market=/^\/markets\/(\d+)\/orderbook/.exec(endpoint)?.[1];
      if(endpoint.includes('/portfolio/fills?')){this.requests++;requests.push(endpoint);now+=6000;return {receivedAt:iso(),data:{data:[],pagination:{hasMore:false},coverage:{complete:true},privateField:'do-not-export'}};}
      if(endpoint.includes('/portfolio/pnl?')){this.requests++;requests.push(endpoint);now+=6000;return {receivedAt:iso(),data:{period:'day',totalAccountValue:100000,totalHoldingsValue:0,totalCostBasis:0,unrealizedPnl:0,periodPnl:0,privateField:'do-not-export'}};}
      const stage=endpoint.includes('/price-history?')?'history':endpoint.endsWith('/portfolio/positions')?'positions':market?'book-'+market:endpoint.startsWith('/exchanges/prices?')?'bulk-admission':'tournament';
      interrupt(stage);this.requests++;requests.push(endpoint);now+=6000;
      if(stage==='tournament'&&['startup-recover','startup-stop','startup-auth'].includes(stopAt)&&startupFailures++<2){const e=Error('Synthetic startup outage');e.recoverable=stopAt!=='startup-auth';throw e;}
      if(stage==='tournament')return {data:{id:config.tournamentId,currencyName:'SUSQies',status:'active',isPendingEnrolment:false}};
      if(stage==='history'){const id=/\/exchanges\/(\d+)\//.exec(endpoint)[1],to=new URLSearchParams(endpoint.split('?')[1]).get('to');return {endpoint,requestStartedAt:iso(),receivedAt:iso(),data:{exchangeId:id,marketId:universe.exchanges.find(x=>x.exchangeId===id).marketId,resolution:'1m',from:new Date(Date.parse(to)-3600000).toISOString(),to,candles:[],coverage:{complete:true,projectedThroughSequence:1}}};}
      if(stage==='positions'){
        if(cycle===2&&stopAt==='outage-wait'){const e=Error('Synthetic connection failure');e.recoverable=true;throw e;}
        return {receivedAt:iso(),data:{positions:[]}};
      }
      if(stage==='bulk-admission'){
        interrupt('bulk-response');
        const ids=new URLSearchParams(endpoint.split('?')[1]).get('ids').split(',');
        return {endpoint,requestStartedAt:iso(),receivedAt:iso(),data:{data:ids.map(id=>({...universe.exchanges.find(x=>x.exchangeId===id),latestPrice:.405,bestBid:.4,bestAsk:.41,spread:.01})),missingIds:[]}};
      }
      const spec=universe.exchanges.find(x=>x.marketId===market);
      return {endpoint,requestStartedAt:iso(),receivedAt:iso(),data:{contexts:[{type:'tournament',tournament:{id:config.tournamentId},orderbook:{exchanges:[{exchangeId:spec.exchangeId,option:'YES',asOf:{at:iso(),sequence:this.requests},bids:[{price:.4,quantity:10000}],asks:[{price:.41,quantity:10000}]}]}}]}};
    }
  }
  const mockfs={existsSync:p=>memory.has(p),mkdirSync(){},readFileSync(p){
    if(memory.has(p))return memory.get(p);
    if(['config.json','price-universe.json','depth-plan.json','dashboard.html'].some(n=>p===path.join(root,n)))return fs.readFileSync(p,'utf8');
    throw Error('Unexpected file access '+p);
  }};
  const process={argv:['node',path.join(root,'runner.mjs'),'--cycles','2'],env:{LOCALAPPDATA:'/synthetic'},pid:12345,on:(name,f)=>listeners[name]=f,exit(){throw Error('Unexpected exit');},exitCode:0};
  const atomicJson=(p,v)=>{
    if(stopAt==='status-publication'&&cycle===2&&!triggered&&p.endsWith('stats.json')&&v.runtime?.cycles===2){stop();throw Error('Synthetic status failure');}
    memory.set(p,JSON.stringify(v));
  };
  const bindings={...model,...breadth,...depth,...scheduler,collectionInputs,shockCollectionRisk,networkInterval,loadNetworkBudget:file=>mockfs.existsSync(file)?validateNetworkBudget(JSON.parse(mockfs.readFileSync(file))):null,connectTournament,validateTiming,timingSummary,historyEndpoint,normalizeHistory,validateDipPolicy,workerHealth,baselineView,accountPositions,accountOrders,accountFills,accountPnl,accountView,fs:mockfs,path,http:{createServer:()=>({once(){},listen(p,h,f){f();},close(){}})},randomUUID,fileURLToPath,
    loadKey:()=> 'SYNTHETIC',Journal,ReadClient,lock:()=>()=>{},atomicJson,exclusionSet,
    sampleEquity(s,c,at){if(stopAt==='sampling-failure'&&cycle===2){stop();throw Error('Synthetic sample failure');}return model.sampleEquity(s,c,at);},
    process,console:{log(){},error(){}},AbortController,structuredClone,Date:Clock,setInterval:()=>1,clearInterval(){},moduleUrl:new URL('./runner.mjs',import.meta.url).href};
  const body=source.replace(/^import .*;\r?\n/gm,'').replaceAll('import.meta.url','moduleUrl');
  await vm.runInNewContext('(async()=>{'+body+'\n})()',bindings);
  return {commits,process,memory,requests,triggered};
}

for(const stage of ['positions','orders',...config.marketIds.map(id=>'book-'+id),'book-342','book-376','bulk-admission','bulk-response','cycle-wait','outage-wait','cycle-commit','sampling-failure','status-publication']){
  test('STOP at '+stage+' preserves replayable committed accounting',async()=>{
    const result=await run(stage),last=result.commits.at(-1);
    assert.equal(result.triggered,true);assert.equal(result.process.exitCode,0);assert.equal(last.event.type,'stopped');
    const replay=replayJournal(result.commits.map(r=>JSON.stringify(r)).join('\n')+'\n',config);
    assert.equal(last.state.observations,replay.observations);
    for(const name of Object.keys(last.state.strategies)){
      const durable=last.state.strategies[name],reconstructed=replay.strategies[name];
      assert.equal(durable.cash,reconstructed.cash);assert.equal(durable.fees,reconstructed.fees);
      assert.equal(durable.realized,reconstructed.realizedPnl);assert.equal(durable.fills.length,reconstructed.fills);
    }
    assert.equal(replay.reportedAt,last.at);
  });
}
test('a journal failure concurrent with STOP fails closed without a later state-only append',async()=>{
  const r=await run('recording-failure');assert.equal(r.triggered,true);assert.equal(r.process.exitCode,1);
  assert.deepEqual(r.commits.map(x=>x.event.type),['started','cycle']);
  const stats=JSON.parse([...r.memory.entries()].find(([p])=>p.endsWith('stats.json'))[1]);
  assert.equal(stats.runtime.status,'failed');assert.match(stats.runtime.errors.at(-1).reason,/recording failed/);
});
test('ordinary bounded completion remains source/accounting equivalent',async()=>{
  const r=await run('none'),last=r.commits.at(-1),replay=replayJournal(r.commits.map(x=>JSON.stringify(x)).join('\n')+'\n',config);
  assert.equal(r.process.exitCode,0);assert.equal(last.state.observations,8);assert.equal(replay.observations,8);assert.equal(replay.strategies.buy_hold_benchmark.fills,4);
});

test('runner persists startup backoff, recovers into ordinary collection and stops during retry',async()=>{
 const recovered=await run('startup-recover');assert.equal(recovered.process.exitCode,0);assert.equal(recovered.commits.at(-1).state.observations,8);const rate=JSON.parse([...recovered.memory.entries()].find(([p])=>p.endsWith('rate.json'))[1]);assert.ok(Number.isFinite(rate.next));assert.equal(rate.manualReview,false);
 const stopped=await run('startup-stop');assert.equal(stopped.process.exitCode,0);assert.equal(stopped.requests.length,1);assert.deepEqual(stopped.commits.map(x=>x.event.type),['stopped']);
 const auth=await run('startup-auth');assert.equal(auth.process.exitCode,1);assert.equal(auth.requests.length,1);assert.equal(auth.commits.length,0);
});
test('dip collection rotates comparisons, records sparse historical reads, preserves replay and original slot count',async()=>{
 const r=await run('none',{dipEnabled:true}),raw=r.commits.map(r=>JSON.stringify(r)).join('\n')+'\n',last=r.commits.at(-1),replay=replayJournal(raw,config);
 assert.equal(r.process.exitCode,0);assert.equal(r.commits.filter(r=>r.event.type==='market_history').length,4);assert.equal(r.requests.filter(p=>p.startsWith('/markets/')).length,8);assert.equal(last.state.observations,4);assert.equal(replay.observations,4);assert.equal(last.state.strategies.mean_reversion.cash,replay.strategies.mean_reversion.cash);
});
test('STOP or durable recording failure during historical backfill cannot continue to an account/book read',async()=>{
 const stop=await run('history-stop',{dipEnabled:true});assert.equal(stop.process.exitCode,0);assert.equal(stop.triggered,true);assert.equal(stop.commits.at(-1).event.type,'stopped');
 const fail=await run('history-recording-failure',{dipEnabled:true});assert.equal(fail.process.exitCode,1);assert.equal(fail.commits.at(-1).event.type,'cycle');assert.equal(fail.requests.filter(p=>p.endsWith('/portfolio/positions')).length,1);
});

test('official sync replaces historical slots and stores only curated account projections outside the paper journal',async()=>{
 const r=await run('none',{dipEnabled:true,officialSync:true});assert.equal(r.process.exitCode,0);assert.equal(r.requests.filter(x=>x.includes('/price-history?')).length,2);assert.equal(r.requests.filter(x=>x.includes('/portfolio/fills?')||x.includes('/portfolio/pnl?')).length,2);const cache=JSON.parse(r.memory.get(path.join('/synthetic','SigTradingCup','paper-runtime','official-account.json')));assert.equal(cache.pnl.accountValue,100000);assert.equal(cache.fills.rows.length,0);assert.ok(!JSON.stringify(cache).includes('do-not-export'));assert.ok(!JSON.stringify(r.commits).includes('do-not-export'));assert.equal(replayJournal(r.commits.map(x=>JSON.stringify(x)).join('\n')+'\n',config).observations,4);
});
