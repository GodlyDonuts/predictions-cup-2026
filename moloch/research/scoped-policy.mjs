import fs from 'node:fs';import path from 'node:path';
import {payloadHash} from '../../execution/ledger.mjs';
import {createMoloch} from '../engine.mjs';import {verifyJournal,replayVariant} from '../compare.mjs';
const [directory,exchangeId,output]=process.argv.slice(2);if(!output||!/^[1-9]\d*$/.test(exchangeId))throw Error('source, exchange and new output required');
const json=p=>JSON.parse(fs.readFileSync(p,'utf8'));
const snapshot=json(path.join(directory,'state.json')),{hash,...body}=snapshot,manifest=json(path.join(directory,'manifest.json'));
if(hash!==payloadHash(body)||!manifest.scopes.some(p=>p.exchangeId===exchangeId)||!manifest.continuation)throw Error('SCOPED_SOURCE');
const parent=json(path.join(manifest.continuation.parentDirectory,'state.json')),{hash:ph,...pb}=parent;
if(ph!==payloadHash(pb)||ph!==manifest.continuation.parentSnapshotHash)throw Error('PARENT');
const rows=fs.readFileSync(path.join(directory,'events.jsonl'),'utf8').split('\n').slice(0,snapshot.sequence).map(JSON.parse),events=verifyJournal(rows,snapshot);
const baseline=replayVariant(parent.state,events,manifest.policy),clean=({policy,policyHash,...state})=>state;
if(payloadHash(clean(baseline.state))!==payloadHash(clean(snapshot.state)))throw Error('BASELINE_FINANCIAL_PARITY');
const policy={...json('runtime/moloch/policy-competition-scale-20261014-1.json'),entryExchangeIds:[exchangeId],inventoryAgeClock:'execution'};
const cases=[['paired',{}],['microprice',{quoteMode:'microprice'}],['imbalance30',{quoteMode:'microprice',minimumDepthImbalanceBps:3000}],['pairedBackoff',{entryBackoffTicks:1}],['pairedQueue50',{fillModel:'conditionalFifo',touchAttributionBps:5000}],['micropriceQueue50',{quoteMode:'microprice',fillModel:'conditionalFifo',touchAttributionBps:5000}]];
const results=[];
for(const [name,overrides]of cases){const p={...policy,...overrides},r=replayVariant(createMoloch(p),events);const result={name,policy:p,report:r.report,newFills:r.newFills,lastFresh:r.lastFreshLiquidation,worstFresh:r.worstFreshLiquidation,remainingQueues:r.state.orders.filter(q=>!q.retired).map(q=>({side:q.side,action:q.action,price:q.price,quantity:q.quantity,filled:q.filled,queueAhead:q.queueAhead,participationCredit:q.participationCredit}))};results.push(result);console.log(JSON.stringify({name,fills:r.newFills.length,realized:r.report.realized,ownedCost:r.report.ownedCost,reserved:r.report.reservedCash,lastFresh:r.lastFreshLiquidation}));}
fs.writeFileSync(output,JSON.stringify({schemaVersion:1,sourceSnapshotHash:hash,sourceFinalHash:snapshot.previous,sourceEvents:snapshot.sequence,window:{from:manifest.cutoff,to:snapshot.state.lastAt},exchangeId,baselineFinancialParity:true,results,contestOrders:0,networkRequests:0,profitabilityEstablished:false,limitations:['Retrospective pinned-prefix diagnostic, not untouched forward performance.','Each policy has its own funded 100,000 paper bank; these are comparisons, not deposits or added profits.','Original financial clocks, source invalidations, full tape coverage, depth, latency and fees apply.','Conditional FIFO assumes 50% at-touch attribution after visible queue; actual venue fills are unverified.','Original live wallets and all existing accounting remain unchanged.']},null,2)+'\n',{flag:'wx'});
