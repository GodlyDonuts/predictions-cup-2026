import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {digest} from '../live-paper/model.mjs';import {createBookPublicationState,applyBookPublication} from '../live-paper/book-publication.mjs';
const need=(v,m)=>{if(!v)throw Error(m);};
export function ownedRiskTimingAudit(runtimeDir,universe){
 const complete=file=>{const b=fs.readFileSync(file),end=b.lastIndexOf(10)+1;need(end>0,'TIMING_NO_COMPLETE_ROWS');return b.subarray(0,end).toString().trim().split('\n').map(JSON.parse);};
 const core=complete(path.join(runtimeDir,'competition/core-journal.jsonl'));let sequence=0,previous='',clock=-Infinity;
 for(const r of core){const {hash,...body}=r,t=Date.parse(r.at);need(r.sequence===++sequence&&r.previous===previous&&hash===digest(body)&&Number.isFinite(t)&&t>=clock,'TIMING_CORE_CHAIN');previous=hash;clock=t;}
 let pub=createBookPublicationState(universe);const books=[];
 for(const r of complete(path.join(runtimeDir,'public-books.jsonl'))){pub=applyBookPublication(pub,r);if(r.kind==='book')books.push(r.payload.book);}
 const final=core.at(-1).state,entries=Object.values(final.intents).filter(i=>i.action==='buy'&&i.filledQuantity>0&&i.status==='closed');
 const rows=entries.map(i=>{const reconciled=core.find(r=>r.event.type==='official_reconciliation'&&r.event.intentId===i.intentId&&r.state.intents[i.intentId]?.status==='closed'&&r.state.owned[i.exchangeId]);
  const exit=core.find(r=>r.event.type==='exit_obligation_latched'&&r.event.exchangeId===i.exchangeId&&Date.parse(r.at)>Date.parse(reconciled?.at));
  const first=books.find(b=>b.exchangeId===i.exchangeId&&Date.parse(b.receivedAt)>=Date.parse(reconciled?.at)&&(!exit||Date.parse(b.receivedAt)<=Date.parse(exit.at)));
  const sell=exit&&core.find(r=>r.event.type==='intent_prepared'&&r.state.intents[r.event.intentId]?.action==='sell'&&r.state.intents[r.event.intentId]?.exchangeId===i.exchangeId&&Date.parse(r.at)>=Date.parse(exit.at)),sale=sell&&final.intents[sell.event.intentId];
  return {marketId:i.marketId,exchangeId:i.exchangeId,buyIntentId:i.intentId,entryFilledAt:i.reconciledFills?.[0]?.filledAt??null,buyClosedReconciledAt:reconciled?.at??null,
   firstBookAfterClosedReconciliation:first?{receivedAt:first.receivedAt,requestStartedAt:first.requestStartedAt,versionKey:first.versionKey,bestBid:first.bids[0]?.price}:null,
   exitLatchedAt:exit?.at??null,exitTriggerBid:exit?exit.state.control.exits[i.exchangeId]?.triggerBid:null,
   firstBookToExitLatchMs:exit&&first?Date.parse(exit.at)-Date.parse(first.receivedAt):null,exitPreparedAt:sell?.at??null,
   exitFirstFillAt:sale?.reconciledFills?.[0]?.filledAt??null,evidence:'verified_actual_ledger_and_public_receipts_not_counterfactual_fill'};
 });
 return {schemaVersion:1,at:new Date().toISOString(),coreSequence:sequence,coreHash:previous,publicationSequence:pub.sequence,publicationHash:pub.hash,entries:rows,networkRequests:0,competitionOrdersChanged:0,profitabilityEstablished:false,
  limitations:['Only observed quotes; gaps do not reveal intermediate prices.','Earlier eligibility is not proof an earlier order would fill or improve profit.','A take-profit exit need not have a risk latch.']};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),universe=JSON.parse(fs.readFileSync(new URL('../live-paper/price-universe.json',import.meta.url)));
 const report=ownedRiskTimingAudit(runtime,universe);fs.writeFileSync('data/owned-risk-timing.json',JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report));
}
