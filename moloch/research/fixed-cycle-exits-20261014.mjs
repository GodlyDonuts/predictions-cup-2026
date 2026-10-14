import fs from 'node:fs';import path from 'node:path';
import {payloadHash} from '../../execution/ledger.mjs';import {createMoloch} from '../engine.mjs';
import {verifyJournal,replayVariant} from '../compare.mjs';import {scopeCoverage} from './scope-coverage.mjs';
const output=process.argv[2];if(!output)throw Error('new output required');
const json=p=>JSON.parse(fs.readFileSync(p,'utf8')),episodes=[];
for(let i=42;i<=46;i++){
 const d=`runtime/moloch/live-sig-20261014-${i}`,m=json(d+'/manifest.json'),snap=json(d+'/state.json'),{hash,...body}=snap;
 if(hash!==payloadHash(body)||!m.scopes.some(p=>p.exchangeId==='1027'))throw Error('SOURCE');
 const p=json(path.join(m.continuation.parentDirectory,'state.json')),{hash:ph,...pb}=p;
 if(ph!==payloadHash(pb)||ph!==m.continuation.parentSnapshotHash||(episodes.length&&ph!==episodes.at(-1).snap.hash))throw Error('PARENT_CHAIN');
 const rows=fs.readFileSync(d+'/events.jsonl','utf8').split('\n').slice(0,snap.sequence).map(JSON.parse),events=verifyJournal(rows,snap),r=replayVariant(p.state,events,m.policy),clean=({policy,policyHash,...s})=>s;
 if(payloadHash(clean(r.state))!==payloadHash(clean(snap.state)))throw Error('BASELINE_PARITY');episodes.push({i,m,snap,events});
}
const base={...json('runtime/moloch/policy-competition-scale-20261014-1.json'),entryExchangeIds:['1027'],inventoryAgeClock:'execution'},results=[];
for(const [name,overrides]of [['tenMinuteControl',{}],['immediateBreakEven',{inventoryTakerAfterMs:0}],['oneTickGain',{inventoryTakerAfterMs:0,inventoryMinimumGain:'0.005000'}],['oneTickThreeMinuteRisk',{inventoryTakerAfterMs:0,inventoryMinimumGain:'0.005000',maximumInventoryAgeMs:180000}]]){
 let state=createMoloch({...base,...overrides});const stages=[];
 for(const e of episodes){const r=replayVariant(state,e.events,{...overrides,entriesEnabled:e.i===42});state=r.state;stages.push({phase:e.i,realized:r.report.realized,ownedCost:r.report.ownedCost,unresolved:r.report.unresolvedOrders,lastFresh:r.lastFreshLiquidation});}
 const r=replayVariant(state,[]);results.push({name,overrides,report:r.report,coverageAudit:scopeCoverage(r.report,episodes.at(-1).m.scopes),fills:state.fills,stages});console.log(JSON.stringify({name,realized:r.report.realized,cash:r.report.cash,owned:r.report.ownedCost,unresolved:r.report.unresolvedOrders}));
}
fs.writeFileSync(output,JSON.stringify({schemaVersion:1,baselineFinancialParity:true,sourceSnapshots:episodes.map(e=>({phase:e.i,hash:e.snap.hash,events:e.events.length})),results,contestOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:['Retrospective development on an already observed single entry print; not untouched validation.','Each case preserves its own continuous funded state through all outages and recovery modes.','New entries drain after phase42; all original clocks, gaps, depth, fees and quote-window reconciliation remain enforced.','AON fills are conditional later-book simulations, not exchange execution receipts.','Original live bank balances and conservative completed-cycle result remain unchanged.']},null,2)+'\n',{flag:'wx'});
