import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {createBookPublicationState,applyBookPublication} from './book-publication.mjs';
import {amount,money,TICK,instant} from '../paper/fixed.mjs';import {digest} from './model.mjs';
const need=(ok,reason)=>{if(!ok)throw Error('EDGE_AUDIT_'+reason);};
export function attributeEdgeOutcome(label,entryRow,exitRow,bank){
 const entry=entryRow?.payload?.book,exit=exitRow?.payload?.book;
 need(entryRow?.kind==='book'&&exitRow?.kind==='book'&&entry.marketId===label.marketId&&exit.marketId===label.marketId&&entry.exchangeId===exit.exchangeId&&entry.tournamentId===bank.config.tournamentId&&exit.tournamentId===bank.config.tournamentId,'SCOPE');
 need(entry.versionKey===label.entryVersion&&exit.versionKey===label.exitVersion&&entry.receivedAt===label.filledAt&&exit.receivedAt===label.at&&exitRow.sequence===label.sourceSequence&&exitRow.hash===label.sourceHash,'LABEL_SOURCE');
 need(bank.config.feeBps===0&&bank.config.slippageTicks===1&&bank.config.depthFractionBps===100&&bank.config.orderShares===1,'COST_POLICY');
 need(instant(entry.requestStartedAt)>=instant(label.preparedAt)+bank.config.latencyMs&&instant(exit.requestStartedAt)>=instant(label.filledAt)+bank.policy.horizonMs&&instant(label.at)<=instant(label.filledAt)+bank.policy.horizonMs+bank.policy.labelLatenessMs&&entry.versionKey!==exit.versionKey,'CAUSAL_WINDOW');
 const entryBid=amount(entry.bids[0].price),entryAsk=amount(entry.asks[0].price),exitBid=amount(exit.bids[0].price);
 need(entryAsk>entryBid&&[entry.bids[0],entry.asks[0],exit.bids[0],exit.asks[0]].every(x=>Number.isSafeInteger(x.quantity)&&x.quantity>=100),'DEPTH');
 need(amount(label.entry)===entryAsk+TICK&&amount(label.exitBid)===exitBid-TICK&&Number.isSafeInteger(label.netMicros),'PRICE');
 const bidMove=exitBid-entryBid,spread=entryAsk-entryBid,rawCrossed=exitBid-entryAsk,modeled=rawCrossed-2n*TICK;
 need(modeled===BigInt(label.netMicros),'ACCOUNTING');
 return {marketId:label.marketId,cell:label.cell,preparedAt:label.preparedAt,filledAt:label.filledAt,exitAt:label.at,entrySequence:entryRow.sequence,exitSequence:exitRow.sequence,entryHash:entryRow.hash,exitHash:exitRow.hash,bidMovement:money(bidMove),entrySpread:money(spread),rawCrossedReturn:money(rawCrossed),modeledAdverseTicks:money(2n*TICK),modeledNet:money(modeled),explicitFees:'0.000000',prospective:instant(label.preparedAt)>instant(bank.cutoff)};
}
function summary(rows){
 const values=key=>rows.map(x=>amount(x[key])).sort((a,b)=>a<b?-1:a>b?1:0),median=key=>{const v=values(key);return v.length?money(v[Math.floor((v.length-1)/2)]):null;};
 return {labels:rows.length,markets:new Set(rows.map(x=>x.marketId)).size,positiveBidMovements:rows.filter(x=>amount(x.bidMovement)>0n).length,positiveRawCrossedReturns:rows.filter(x=>amount(x.rawCrossedReturn)>0n).length,positiveModeledReturns:rows.filter(x=>amount(x.modeledNet)>0n).length,medianBidMovement:median('bidMovement'),medianEntrySpread:median('entrySpread'),medianRawCrossedReturn:median('rawCrossedReturn'),medianModeledNet:median('modeledNet')};
}
export function auditRecentEdgeOutcomes({bank,stats,sourceBytes}){
 need(digest(bank)===stats.bankHash&&stats.mode==='execution_aware_online_paper'&&Array.isArray(stats.recentLabels),'BANK');
 const complete=sourceBytes.lastIndexOf(10)+1;need(complete>0,'EMPTY_SOURCE');const bytes=sourceBytes.subarray(0,complete),rows=bytes.toString('utf8').split('\n').slice(0,-1).map(JSON.parse);
 let publication=createBookPublicationState(bank.universe);const entries=new Map(),bySequence=new Map();
 for(const row of rows){publication=applyBookPublication(publication,row);bySequence.set(row.sequence,row);if(row.kind==='book'){const b=row.payload.book;entries.set(b.marketId+'|'+b.versionKey+'|'+b.receivedAt,row);}}
 need(publication.sequence>=stats.sourceSequence,'SOURCE_BEHIND_STATS');const seen=new Set();
 const outcomes=stats.recentLabels.map(label=>{const key=label.marketId+'|'+label.entryVersion+'|'+label.filledAt+'|'+label.exitVersion;need(!seen.has(key),'DUPLICATE_LABEL');seen.add(key);return attributeEdgeOutcome(label,entries.get(label.marketId+'|'+label.entryVersion+'|'+label.filledAt),bySequence.get(label.sourceSequence),bank);});
 return {schemaVersion:1,mode:'recent_executable_label_attribution',at:stats.updatedAt,cutoff:bank.cutoff,bankHash:stats.bankHash,statsHash:digest(stats),sourceBytes:complete,incompleteTailBytes:sourceBytes.length-complete,sourceBytesHash:createHash('sha256').update(bytes).digest('hex'),sourceSequence:publication.sequence,sourceHash:publication.hash,availableRecentLabels:outcomes.length,totalCompletedLabels:stats.counts.labels_completed,development:summary(outcomes.filter(x=>!x.prospective)),prospective:summary(outcomes.filter(x=>x.prospective)),outcomes,networkRequests:0,ordersSubmitted:0,profitabilityEstablished:false,limitations:['Only the recent label window retained in current stats is attributed; this is not an all-history summary.','Labels are hypothetical one-share probes, not wallet fills, independent samples, or portfolio P&L.','Raw crossed return still pays the observed entry spread; removing assumed ticks does not imply attainable fills.','Prospective labels may overlap, share event exposure, and be conditioned on fill and data availability.']};
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===path.resolve(process.argv[1])){
 const rt=process.env.SIG_PAPER_DIR||path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),manifest=JSON.parse(fs.readFileSync(path.join(rt,'executable-edge-active.json')));need(/^[a-z][a-z0-9-]{1,63}$/.test(manifest.cohort),'COHORT');const dir=path.join(rt,'executable-edge-cohorts',manifest.cohort),bank=JSON.parse(fs.readFileSync(path.join(dir,'bank.json'))),stats=JSON.parse(fs.readFileSync(path.join(dir,'stats.json')));need(digest(bank)===manifest.bankHash,'MANIFEST');console.log(JSON.stringify(auditRecentEdgeOutcomes({bank,stats,sourceBytes:fs.readFileSync(path.join(rt,'public-books.jsonl'))}),null,2));
}
