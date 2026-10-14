import fs from 'node:fs';import path from 'node:path';
import {payloadHash} from '../../execution/ledger.mjs';import {createMoloch} from '../engine.mjs';
import {verifyJournal,replayVariant} from '../compare.mjs';import {scopeCoverage} from './scope-coverage.mjs';
const output=process.argv[2];if(!output)throw Error('new output required');
const json=p=>JSON.parse(fs.readFileSync(p,'utf8')),episodes=[];
for(let i=21;i<=27;i++){
 const d=`runtime/moloch/live-sig-20261014-${i}`,m=json(d+'/manifest.json'),snap=json(d+'/state.json'),stats=json(d+'/stats.json'),{hash,...body}=snap;
 if(hash!==payloadHash(body)||stats.status!=='completed'||stats.captureExit!==0||!m.scopes.some(s=>s.exchangeId==='1051'))throw Error('FIXED_SCOPE_SOURCE');
 const parent=json(path.join(m.continuation.parentDirectory,'state.json')),{hash:ph,...pb}=parent;
 if(ph!==payloadHash(pb)||ph!==m.continuation.parentSnapshotHash||(episodes.length&&ph!==episodes.at(-1).snap.hash))throw Error('PARENT');
 const rows=fs.readFileSync(d+'/events.jsonl','utf8').trim().split(/\r?\n/).map(JSON.parse),events=verifyJournal(rows,snap),baseline=replayVariant(parent.state,events,m.policy),clean=({policy,policyHash,...s})=>s;
 if(payloadHash(clean(baseline.state))!==payloadHash(clean(snap.state)))throw Error('BASELINE');episodes.push({i,m,snap,events});
}
const results=[];
for(const latencyMs of [3000,1500,500])for(const participationBps of [100,1000,10000]){
 const policy={initialCash:'100000.000000',maximumCommitted:'40000.000000',maximumMarketCost:'10000.000000',quantity:500,maximumMarkets:1,entryExchangeIds:['1051'],entryDepthBps:10000,participationBps,latencyMs,retainProfitableHedge:true,maximumHedgeOvershoot:0,inventoryTakerAfterMs:0,maximumInventoryAgeMs:600000,inventoryAgeClock:'execution',lifetimeMs:600000,insideTicks:0};
 let state=createMoloch(policy);const marks=[];
 for(const e of episodes){const r=replayVariant(state,e.events,policy);state=r.state;marks.push({episode:e.i,newFills:r.newFills.length,realized:r.report.realized,ownedCost:r.report.ownedCost,lastFresh:r.lastFreshLiquidation,worstFresh:r.worstFreshLiquidation});}
 const last=replayVariant(state,[]),result={latencyMs,participationBps,policy,report:last.report,coverageAudit:scopeCoverage(last.report,episodes.at(-1).m.scopes),marks};results.push(result);console.log(JSON.stringify({latencyMs,participationBps,fills:last.report.fills,realized:last.report.realized,owned:last.report.ownedCost,unresolved:last.report.unresolvedOrders,lastFresh:marks.at(-1).lastFresh}));
}
fs.writeFileSync(output,JSON.stringify({schemaVersion:1,baselineFinancialParity:true,fixedExchangeId:'1051',sourceSnapshots:episodes.map(e=>({episode:e.i,hash:e.snap.hash})),results,contestOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:['Previously inspected archived prices; diagnostic, not untouched forward validation.','Selected scope remains in all seven source manifests; temporal outages, stale books and missing tape coverage are still enforced.','500/1500ms are unverified hypothetical exchange activation delays, not measured production latency.','Read and write budgets, minimum quote age, depth and fees remain unchanged.','Independent 100,000 paper comparisons are not account capital or additive profits.','Unfinished positions and quote liabilities remain in the ledger; missing final marks are not fabricated.']},null,2)+'\n',{flag:'wx'});
