import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {digest} from '../live-paper/model.mjs';import {createBookPublicationState,applyBookPublication} from '../live-paper/book-publication.mjs';import {entryPathQuality} from './entry-path-quality.mjs';
export function auditEntryPaths(runtimeDir,universe,referenceReport){
 const lines=file=>{const bytes=fs.readFileSync(file),end=bytes.lastIndexOf(10)+1;if(!end)throw Error('No complete evidence');return bytes.subarray(0,end).toString().trim().split('\n').map(JSON.parse);};
 let pub=createBookPublicationState(universe);const books=[],invalidations=[];
 for(const row of lines(path.join(runtimeDir,'public-books.jsonl'))){pub=applyBookPublication(pub,row);if(row.kind==='book')books.push(row.payload.book);if(row.kind==='invalidation')invalidations.push({marketId:row.payload.marketId,at:row.at});if(row.kind==='session_start'||row.kind==='session_stop')invalidations.push({marketId:null,at:row.at});}
 const core=lines(path.join(runtimeDir,'competition/core-journal.jsonl'));let seq=0,previous='';for(const row of core){const {hash,...body}=row;if(row.sequence!==++seq||row.previous!==previous||hash!==digest(body))throw Error('Core journal chain failed');previous=hash;}
 const entries=referenceReport.entries.map(entry=>{const proof=core.find(r=>r.hash===entry.coreHash),intent=proof?.state.intents[proof.event.intentId];if(!intent||intent.action!=='buy'||intent.marketId!==entry.marketId||intent.exchangeId!==entry.exchangeId||intent.preparedAt!==entry.preparedAt||intent.attribution.signalId!==entry.bookVersion||digest(proof.event.referenceProof)!==digest(entry.referenceProof))throw Error('Exact entry proof mismatch');
  return {marketId:entry.marketId,exchangeId:entry.exchangeId,preparedAt:entry.preparedAt,referenceLastAgeMs:entry.quality.lastObservationAgeMs,referenceSamples:entry.signal.samples,referenceCenter:entry.signal.center,
   path:entryPathQuality({books,invalidations,entry,at:entry.preparedAt}),economicOutcomeSource:'actual_attributed_ledger_only'};});
 return {schemaVersion:1,at:new Date().toISOString(),coreSequence:seq,coreHash:previous,publicationSequence:pub.sequence,publicationHash:pub.hash,entries,networkRequests:0,competitionOrdersChanged:0,profitabilityEstablished:false,limitations:['Only pre-entry verified receipts; older entries can precede public publication coverage.','Session/invalidation gaps reset observed context.','Four actual entries do not establish a predictive admission rule.']};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const runtimeDir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),universe=JSON.parse(fs.readFileSync(new URL('../live-paper/price-universe.json',import.meta.url))),references=JSON.parse(fs.readFileSync('data/owned-reference-quality.json'));
 const report=auditEntryPaths(runtimeDir,universe,references);fs.writeFileSync('data/owned-entry-path-quality.json',JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report));
}
