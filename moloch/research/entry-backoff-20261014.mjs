import fs from 'node:fs';import path from 'node:path';
import {payloadHash} from '../../execution/ledger.mjs';
import {verifyJournal,replayVariant} from '../compare.mjs';
const json=p=>JSON.parse(fs.readFileSync(p,'utf8'));
const episodes=[];
for(let i=18;i<=25;i++){
 const dir=`runtime/moloch/live-sig-20261014-${i}`,manifest=json(dir+'/manifest.json'),stats=json(dir+'/stats.json'),snapshot=json(dir+'/state.json'),{hash,...body}=snapshot;
 if(stats.status!=='completed'||stats.captureExit!==0||hash!==payloadHash(body))throw Error('TERMINAL_SOURCE');
 const parent=json(path.join(manifest.continuation.parentDirectory,'state.json')),{hash:ph,...pb}=parent;
 if(ph!==payloadHash(pb)||ph!==manifest.continuation.parentSnapshotHash||(episodes.length&&ph!==episodes.at(-1).snapshot.hash))throw Error('PARENT_CHAIN');
 const rows=fs.readFileSync(dir+'/events.jsonl','utf8').trim().split(/\r?\n/).map(JSON.parse);
 episodes.push({i,manifest,snapshot,parent,events:verifyJournal(rows,snapshot)});
}
const results=[];
for(const backoff of [null,1,2,4]){
 let state=structuredClone(episodes[0].parent.state);const marks=[];
 for(const e of episodes){
  const r=replayVariant(state,e.events,{...e.manifest.policy,...(backoff===null?{}:{insideTicks:0,entryBackoffTicks:backoff})});
  if(backoff===null){const clean=({policy,policyHash,...rest})=>rest;if(payloadHash(clean(r.state))!==payloadHash(clean(e.snapshot.state)))throw Error('BASELINE_PARITY');}
  state=r.state;marks.push({episode:e.i,newFills:r.newFills.length,realized:r.report.realized,cash:r.report.cash,ownedCost:r.report.ownedCost,unresolved:r.report.unresolvedOrders,lastFresh:r.lastFreshLiquidation,worstFresh:r.worstFreshLiquidation});
 }
 results.push({entryBackoffTicks:backoff,marks});console.log(JSON.stringify({entryBackoffTicks:backoff,totalNewFills:marks.reduce((n,m)=>n+m.newFills,0),final:marks.at(-1)}));
}
const output=process.argv[2];if(!output)throw Error('NEW_OUTPUT_REQUIRED');
fs.writeFileSync(output,JSON.stringify({schemaVersion:1,baselineFinancialParity:true,sourceSnapshots:episodes.map(e=>({episode:e.i,hash:e.snapshot.hash})),results,newPaperCapital:'0.000000',contestOrders:0,profitabilityEstablished:false,limitations:['Previously inspected historical episodes; diagnostic, not untouched holdout.','Inherited orders keep their original limits; new entry protection changes only future purchases.','Continuous wallets retain all fills, losses, inventory and unresolved quote liabilities.','Improved marked performance without completed trades does not establish profitable turnover.']},null,2)+'\n',{flag:'wx'});
