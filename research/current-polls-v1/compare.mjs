import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {priceDecision} from '../current-election/assessment.mjs';import {payloadHash} from '../../execution/ledger.mjs';import {decimalMicros,formatMicros} from '../../execution/core.mjs';
const base='research/current-polls-v1',runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime');
const record=JSON.parse(fs.readFileSync(base+'/assessments-20261005-v1.json','utf8'));
if(createHash('sha256').update(fs.readFileSync(base+'/assessment.mjs')).digest('hex')!==record.assessmentCodeHash)throw Error('ASSESSMENT_CODE_CHANGED');
const activation=JSON.parse(fs.readFileSync(path.join(runtime,'competition-activation.json'),'utf8')),bank=JSON.parse(fs.readFileSync(path.join(runtime,'production-banks',activation.strategyBankHash+'.json'),'utf8'));
const metadata=path.join(runtime,'production-metadata','current-poll-post-assessment-20261005-v1'),now=new Date().toISOString(),rows=[];
for(const a of record.assessments.filter(a=>a.marketId)){
 const node=JSON.parse(fs.readFileSync(path.join(metadata,a.marketId+'.json'),'utf8')),receipt=JSON.parse(fs.readFileSync(path.join(metadata,a.marketId+'.book.json'),'utf8')),book=receipt.book;
 const details=node.root.contract_details,expected=a.party==='republican'?'Republican Party':'Democratic Party';
 if(node.rootHash!==payloadHash(node.root)||node.marketId!==a.marketId||details.usState!==a.state||details.winnerName!==expected||details.raceStage!=='General'||details.electionDate!=='2026-11-03'||details.resolutionType!=='Party Winner')throw Error('CURRENT_CONTRACT_BINDING_CHANGED');
 // Historical comparison at an actual subsequent receipt, not a relabeled live
 // quote. A model unavailable when a request began is rejected by priceDecision.
 const decision=priceDecision({assessment:a.assessment,book,asOf:book.receivedAt,policy:bank.policy});
 const raw=decision.maximumAsk?decimalMicros(decision.maximumAsk):null;
 const maximumExecutableAsk=raw===null?null:raw<0n?null:formatMicros(raw/5000n*5000n);
 const mean=a.assessment.ordinary.variants.reduce((sum,v)=>sum+v.discountedValuation,0)/a.assessment.ordinary.variants.length;
 rows.push({assessmentId:a.id,state:a.state,party:a.party,marketId:a.marketId,exchangeId:book.exchangeId,rootHash:node.rootHash,bookResponseHash:receipt.responseHash,
  assessmentAt:a.assessment.assessedAt,bookRequestStartedAt:book.requestStartedAt,bookReceivedAt:book.receivedAt,quoteAgeAtReportMs:Date.parse(now)-Date.parse(book.receivedAt),bookAsk:book.asks[0].price,bookBid:book.bids[0].price,
  ordinary:[a.assessment.ordinary.lower,a.assessment.ordinary.upper],stress:[a.assessment.stress.lower,a.assessment.stress.upper],uniformScenarioMean:mean,
  meanInterpretation:'Descriptive average over equally enumerated judgment scenarios, not a calibrated forecast or admission rule.',maximumExecutableAsk,decision,liveQuoteAtReport:false,competitionOrders:0,executionAuthority:false});
}
const out={schemaVersion:1,id:'subsequent-primary-poll-price-comparison-20261005-v1',computedAt:now,sourceRecordId:record.id,originalProductionRunId:activation.runId,originalEntryExpiresAt:activation.entryExpiresAt,activationChanged:false,productionBankChanged:false,
 rows,eligiblePriceReceipts:rows.filter(r=>r.decision.eligible).length,additionalAccountClients:0,metadataUsesExistingAuxiliarySlots:true,fees:'0.000000',profitabilityEstablished:false,
 interpretation:'These are exact, dated model/contract/book comparisons. No hypothetical fill or actual net profit is inferred. Execution remains under the original expired entry window.'};
fs.writeFileSync(base+'/price-comparison-20261005-v1.json',JSON.stringify(out,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({computedAt:now,eligibleReceipts:out.eligiblePriceReceipts,rows:rows.map(r=>({state:r.state,party:r.party,ask:r.bookAsk,maximumExecutableAsk:r.maximumExecutableAsk,reason:r.decision.reason,sourceAfterModel:Date.parse(r.bookRequestStartedAt)>=Date.parse(r.assessmentAt)})),ordersSubmitted:0}));
