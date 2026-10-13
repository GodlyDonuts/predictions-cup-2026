import fs from 'node:fs';import path from 'node:path';import {pathToFileURL,fileURLToPath} from 'node:url';import {createHash} from 'node:crypto';
import {createMoloch,advanceMoloch,reportMoloch,assertMoloch} from './engine.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {collectFrozenPaperFiles} from '../live-paper/frozen-paper-graph.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const check=(ok,code)=>{if(!ok)throw Error('COMPARE_'+code);};
const json=file=>JSON.parse(fs.readFileSync(file,'utf8').replace(/^\uFEFF/,''));
const financial=state=>{const {policy,policyHash,...rest}=state;return rest;};
export function verifyJournal(rows,parent){
  let previous='',sequence=0;
  for(const row of rows){const {hash,...body}=row;check(hash===payloadHash(body)&&body.sequence===sequence+1&&body.previous===previous,'JOURNAL_CHAIN');previous=hash;sequence++;}
  check(parent.sequence===sequence&&parent.previous===previous,'TERMINAL_JOURNAL');return rows.map(row=>row.event);
}
export function replayVariant(initial,events,overrides={}){
  const s=structuredClone(initial);s.policy=createMoloch({...s.policy,...overrides}).policy;s.policyHash=payloadHash(s.policy);assertMoloch(s);
  const firstFills=s.fills.length;let lastFreshLiquidation=null,worstFreshLiquidation=null;
  for(const e of events){advanceMoloch(s,e);const r=reportMoloch(s);if(r.netLiquidationPnl!==null){lastFreshLiquidation={at:r.at,pnl:r.netLiquidationPnl};if(worstFreshLiquidation===null||Number(r.netLiquidationPnl)<Number(worstFreshLiquidation.pnl))worstFreshLiquidation=lastFreshLiquidation;}}
  return{state:s,report:reportMoloch(s),newFills:s.fills.slice(firstFills),lastFreshLiquidation,worstFreshLiquidation};
}
export function compareEpisode(directory,windows=[0,30000,45000,60000]){
  const dir=path.resolve(directory),manifest=json(path.join(dir,'manifest.json')),stats=json(path.join(dir,'stats.json')),snapshot=json(path.join(dir,'state.json'));
  check(stats.status==='completed'&&stats.captureExit===0&&manifest.continuation,'TERMINAL_EPISODE');
  const {hash,...body}=snapshot;check(hash===payloadHash(body),'SNAPSHOT_HASH');
  const parent=json(path.join(manifest.continuation.parentDirectory,'state.json'));const {hash:parentHash,...parentBody}=parent;
  check(parentHash===payloadHash(parentBody)&&parentHash===manifest.continuation.parentSnapshotHash,'PARENT_HASH');
  const file=path.join(dir,'events.jsonl'),rows=fs.readFileSync(file,'utf8').trim().split(/\r?\n/).filter(Boolean).map(JSON.parse),events=verifyJournal(rows,snapshot);
  const variants=windows.map(renewBeforeMs=>{const result=replayVariant(parent.state,events,{...manifest.policy,renewBeforeMs});
    if(renewBeforeMs===0)check(payloadHash(financial(result.state))===payloadHash(financial(snapshot.state)),'BASELINE_FINANCIAL_PARITY');
    const {state,...report}=result;return{renewBeforeMs,...report};});
  return{schemaVersion:1,bot:'Moloch',sourceEventSha256:createHash('sha256').update(fs.readFileSync(file)).digest('hex'),parentSnapshotHash:parentHash,eventCount:events.length,
    sourceWindow:{from:manifest.cutoff,to:stats.at},codeIdentity:collectFrozenPaperFiles(root,['moloch/compare.mjs']),baselineFinancialParity:true,variants,
    contestOrders:0,newPaperCapital:'0.000000',profitabilityEstablished:false,usage:'Development diagnostic only. Recorded processing and execution clocks remain unchanged; faster renewal is a hypothetical policy intervention, not an observed fill or untouched holdout.'};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){const result=compareEpisode(process.argv[2]);fs.writeFileSync(process.argv[3],JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(result.variants.map(v=>({renewBeforeMs:v.renewBeforeMs,newFills:v.newFills.length,realized:v.report.realized,finalLiquidation:v.report.netLiquidationPnl,lastFresh:v.lastFreshLiquidation,worstFresh:v.worstFreshLiquidation,renewals:v.report.counts.earlyRenewals??0})),null,2));}
