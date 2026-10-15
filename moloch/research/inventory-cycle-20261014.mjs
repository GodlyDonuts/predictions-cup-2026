import fs from 'node:fs';import path from 'node:path';
import {payloadHash} from '../../execution/ledger.mjs';
import {createMoloch,reportMoloch} from '../engine.mjs';import {verifyJournal,replayVariant} from '../compare.mjs';
import {amount,money} from '../../paper/fixed.mjs';
import {scopeCoverage} from './scope-coverage.mjs';
const output=process.argv[2];if(!output)throw Error('new output required');
const json=p=>JSON.parse(fs.readFileSync(p,'utf8')),episodes=[];
for(let i=18;i<=28;i++){
 const dir=`runtime/moloch/live-sig-20261014-${i}`,manifest=json(dir+'/manifest.json'),stats=json(dir+'/stats.json'),snapshot=json(dir+'/state.json'),{hash,...body}=snapshot;
 if(stats.status!=='completed'||stats.captureExit!==0||hash!==payloadHash(body))throw Error('TERMINAL_SOURCE');
 const parent=json(path.join(manifest.continuation.parentDirectory,'state.json')),{hash:ph,...pb}=parent;
 if(ph!==payloadHash(pb)||ph!==manifest.continuation.parentSnapshotHash||(episodes.length&&ph!==episodes.at(-1).snapshot.hash))throw Error('PARENT_CHAIN');
 const rows=fs.readFileSync(dir+'/events.jsonl','utf8').trim().split(/\r?\n/).map(JSON.parse),events=verifyJournal(rows,snapshot),baseline=replayVariant(parent.state,events,manifest.policy),clean=({policy,policyHash,...s})=>s;
 if(payloadHash(clean(baseline.state))!==payloadHash(clean(snapshot.state)))throw Error('BASELINE_PARITY');
 episodes.push({i,manifest,snapshot,events});
}
const results=[],packetMode=process.argv.includes('--packets'),cases=[];
if(packetMode){for(const quantity of [5,50,500])for(const participationBps of [100,10000])for(const retainProfitableHedge of [false,true])cases.push({quantity,participationBps,retainProfitableHedge,maximumInventoryAgeMs:600000});}
else{for(const participationBps of [100,1000,10000])for(const maximumInventoryAgeMs of [60000,180000,600000,1800000])cases.push({quantity:10000,participationBps,retainProfitableHedge:false,maximumInventoryAgeMs});}
for(const {quantity,participationBps,retainProfitableHedge,maximumInventoryAgeMs} of cases){
 const policy={initialCash:'100000.000000',maximumCommitted:'40000.000000',maximumMarketCost:'10000.000000',quantity,entryDepthBps:10000,participationBps,insideTicks:0,retainProfitableHedge,maximumHedgeOvershoot:0,inventoryTakerAfterMs:0,maximumInventoryAgeMs,inventoryAgeClock:'execution',lifetimeMs:600000};
 let state=createMoloch({...episodes[0].manifest.policy,...policy}),buyNotional=0n,sellNotional=0n,worstFresh=null;const marks=[];
 for(const e of episodes){
  const r=replayVariant(state,e.events,{...e.manifest.policy,...policy});state=r.state;
  for(const f of r.newFills){const n=amount(f.price)*BigInt(f.quantity);if(f.action==='buy')buyNotional+=n;else sellNotional+=n;}
  if(r.worstFreshLiquidation&&(!worstFresh||amount(r.worstFreshLiquidation.pnl)<amount(worstFresh.pnl)))worstFresh=r.worstFreshLiquidation;
  marks.push({episode:e.i,realized:r.report.realized,ownedCost:r.report.ownedCost,reservedCash:r.report.reservedCash,unresolved:r.report.unresolvedOrders,newFills:r.newFills.length,lastFresh:r.lastFreshLiquidation});
 }
 const report=reportMoloch(state),result={participationBps,maximumInventoryAgeMs,policy,report,buyNotional:money(buyNotional),sellNotional:money(sellNotional),aonExitFills:state.fills.filter(f=>f.executionStyle==='inventoryAON').length,worstFresh,marks,coverageAudit:scopeCoverage(report,episodes.at(-1).manifest.scopes)};results.push(result);
 console.log(JSON.stringify({quantity,retainProfitableHedge,participationBps,maximumInventoryAgeMs,realized:report.realized,ownedCost:report.ownedCost,buyNotional:result.buyNotional,sellNotional:result.sellNotional,lastFresh:marks.at(-1).lastFresh,worstFresh,rankingEligible:result.coverageAudit.policyRankingEligible}));
}
fs.writeFileSync(output,JSON.stringify({schemaVersion:1,baselineFinancialParity:true,sourceSnapshots:episodes.map(e=>({episode:e.i,hash:e.snapshot.hash})),results,contestOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:['Previously inspected original captures; this is retrospective sensitivity, not untouched validation.','Twelve independent funded 100,000 paper comparisons; original wallets are unchanged and profits cannot be added.','All cases use original acquisition execution time and allow a later depth-dependent break-even exit without waiting ten minutes.','Forced exits include losses; maximum age does not guarantee liquidation when fresh executable depth or liability release is missing.','Original capture scopes, processing clocks, source gaps, full quote-window reconciliation, fee rules and write budget remain enforced.','Full participation is a hypothetical fill assumption, not demonstrated queue priority or market impact.']},null,2)+'\n',{flag:'wx'});
