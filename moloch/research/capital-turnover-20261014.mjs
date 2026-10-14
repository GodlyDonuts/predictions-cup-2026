import fs from 'node:fs';import path from 'node:path';import {payloadHash} from '../../execution/ledger.mjs';
import {createMoloch,reportMoloch} from '../engine.mjs';import {verifyJournal,replayVariant} from '../compare.mjs';
import {amount,money} from '../../paper/fixed.mjs';
const json=p=>JSON.parse(fs.readFileSync(p,'utf8').replace(/^\uFEFF/,''));const episodes=[];
for(let i=18;i<=28;i++){
 const dir=`runtime/moloch/live-sig-20261014-${i}`,manifest=json(dir+'/manifest.json'),stats=json(dir+'/stats.json'),snapshot=json(dir+'/state.json'),{hash,...body}=snapshot;
 if(stats.status!=='completed'||stats.captureExit!==0||hash!==payloadHash(body))throw Error('terminal evidence '+i);
 const parent=json(path.join(manifest.continuation.parentDirectory,'state.json')),{hash:ph,...pb}=parent;
 if(ph!==payloadHash(pb)||ph!==manifest.continuation.parentSnapshotHash||episodes.length&&ph!==episodes.at(-1).snapshot.hash)throw Error('continuation '+i);
 const rows=fs.readFileSync(dir+'/events.jsonl','utf8').trim().split(/\r?\n/).filter(Boolean).map(JSON.parse),events=verifyJournal(rows,snapshot);
 const baseline=replayVariant(parent.state,events,manifest.policy),clean=s=>{const{policy,policyHash,...rest}=s;return rest;};
 if(payloadHash(clean(baseline.state))!==payloadHash(clean(snapshot.state)))throw Error('baseline financial parity '+i);
 episodes.push({i,manifest,snapshot,events});
}
const results=[],repriceProfitableHedge=process.argv.includes('--reprice-hedge'),queue=process.argv.includes('--queue');
for(const participationBps of [100,1000,10000])for(const insideTicks of [0,1]){
 const overrides={initialCash:'100000.000000',maximumCommitted:'40000.000000',maximumMarketCost:'10000.000000',quantity:10000,entryDepthBps:10000,participationBps,insideTicks,retainProfitableHedge:repriceProfitableHedge,repriceProfitableHedge,maximumHedgeOvershoot:0,inventoryTakerAfterMs:600000,maximumInventoryAgeMs:1800000,lifetimeMs:600000};
 if(queue)Object.assign(overrides,{fillModel:'conditionalFifo',touchAttributionBps:5000});
 let state=createMoloch({...episodes[0].manifest.policy,...overrides}),buyNotional=0n,sellNotional=0n,shares=0,peakCommitted=0n;const marks=[];
 for(const e of episodes){const r=replayVariant(state,e.events,{...e.manifest.policy,...overrides});state=r.state;
  for(const fill of r.newFills){const cost=amount(fill.price)*BigInt(fill.quantity);if(fill.action==='buy')buyNotional+=cost;else sellNotional+=cost;shares+=fill.quantity;}
  const committed=amount(r.report.reservedCash)+amount(r.report.ownedCost);if(committed>peakCommitted)peakCommitted=committed;
  marks.push({episode:e.i,newFills:r.newFills.length,cash:r.report.cash,realized:r.report.realized,ownedCost:r.report.ownedCost,reservedCash:r.report.reservedCash,unresolvedOrders:r.report.unresolvedOrders,lastFresh:r.lastFreshLiquidation,worstFresh:r.worstFreshLiquidation});
 }
 const result={overrides,report:reportMoloch(state),buyNotional:money(buyNotional),sellNotional:money(sellNotional),filledShares:shares,buyTurnoverEstimate:Number(buyNotional)/Number(amount('100000.000000')),peakEpisodeEndCommitted:money(peakCommitted),marks};results.push(result);console.log(JSON.stringify({participationBps,insideTicks,realized:result.report.realized,ownedCost:result.report.ownedCost,fills:result.report.fills,buyNotional:result.buyNotional,lastFresh:marks.at(-1).lastFresh}));
}
const out={schemaVersion:1,bot:'Moloch',source:'verified original SIG episode journals',baselineFinancialParity:true,independentInitialPaperCash:'100000.000000',episodes:episodes.map(e=>({episode:e.i,snapshotHash:e.snapshot.hash,eventCount:e.events.length})),results,contestOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:['Retrospective sensitivity analysis, not untouched holdout or observed exchange fills.','Six independent paper banks; gains are not summed and original bank is unchanged.','Participation scenarios are assumptions; full participation cannot certify queue priority or market impact.','All original receipt/processing/execution clocks, coverage gaps, observed depth, write pacing and exact funded accounting are preserved.','Same captured entry scopes as original episodes; no hindsight universe selection.','Peak committed metric samples episode ends, not every intermediate event.','Fresh liquidation marks must be read with their timestamps; missing terminal depth remains null.']};
fs.writeFileSync(`runtime/moloch/capital-turnover${repriceProfitableHedge?'-hedge':''}${queue?'-queue50':''}-20261014.json`,JSON.stringify(out,null,2)+'\n');
