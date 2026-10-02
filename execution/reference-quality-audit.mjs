import fs from 'node:fs';import path from 'node:path';import readline from 'node:readline';import {createHash} from 'node:crypto';
import {digest,normalizeBook} from '../live-paper/model.mjs';import {amount} from '../paper/fixed.mjs';
import {historicalReferenceQuality} from '../live-paper/reference-quality.mjs';import {dipSignal} from '../live-paper/dip-shadow.mjs';
const dir=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime'),read=p=>JSON.parse(fs.readFileSync(p)),bank=read(path.join(dir,'dip-responsive-bank.json'));
const requireThat=(x,m)=>{if(!x)throw Error(m);},targets=[];
async function* verified(file){
 let sequence=0,previous='',clock=-Infinity;const prefix=createHash('sha256');
 const input=fs.createReadStream(file),lines=readline.createInterface({input,crlfDelay:Infinity});
 try {for await(const line of lines){if(!line)throw Error('EMPTY_SOURCE_LINE');const row=JSON.parse(line),{hash,...body}=row;
  requireThat(row.sequence===++sequence&&row.previous===previous&&hash===digest(body),'SOURCE_CHAIN_FAILED');
  const t=Date.parse(row.event?.clockAt||row.at);requireThat(Number.isFinite(t)&&t>=clock,'SOURCE_CLOCK_FAILED');clock=t;previous=hash;
  prefix.update(line+'\n');yield {row,prefixHash:prefix.copy().digest('hex')};
 }}finally{lines.close();input.destroy();}
}
for await(const {row} of verified(path.join(dir,'competition/core-journal.jsonl'))){
 if(row.event.type!=='intent_prepared'||row.event.reason!=='historical_discount')continue;
 const intent=row.state.intents[row.event.intentId];requireThat(intent?.action==='buy'&&row.event.referenceProof?.bankHash===digest(bank),'ENTRY_PROOF_FAILED');
 targets.push({intent,proof:row.event.referenceProof,coreHash:row.hash});
}
requireThat(targets.length>0,'NO_ATTRIBUTED_ENTRIES');
let histories={},bookHistory={},sourceSequence=0,sourceHash=null;const wanted=new Set(targets.map(x=>x.proof.sourceSequence)),scope=new Map(bank.universe.exchanges.map(x=>[x.marketId,x.exchangeId]));
for await(const {row,prefixHash} of verified(path.join(dir,'journal.jsonl'))){
 const e=row.event,at=e.clockAt||row.at;sourceSequence=row.sequence;sourceHash=row.hash;
 if(row.sequence===bank.sourceRecords)requireThat(prefixHash===bank.sourceHash,'BANK_PREFIX_FAILED');
 if(e.type==='market_history')histories[e.history.exchangeId]=e.history;
 if(e.type==='cycle'){
  const rows=[...e.sourceSnapshots||[],...e.supplementalDepth||[]].filter(x=>!x.skipped&&Number.isFinite(Date.parse(x.receivedAt))&&Date.parse(x.receivedAt)<=Date.parse(at)).sort((a,b)=>Date.parse(a.receivedAt)-Date.parse(b.receivedAt));
  for(const snap of rows){const marketId=/^\/markets\/(\d+)\/orderbook\?/.exec(snap.endpoint||'')?.[1];if(!scope.has(marketId))continue;
   let b;try{b=normalizeBook(snap.data,marketId,bank.config.tournamentId,snap.receivedAt,snap.requestStartedAt,bank.config.maxDataAgeMs,new Set(e.accountExclusions),'authoritative_receipt_v1');if(b.exchangeId!==scope.get(marketId))continue;}catch{continue;}
   for(const target of targets)if(b.versionKey===target.intent.attribution.signalId&&b.exchangeId===target.intent.exchangeId&&Date.parse(b.receivedAt)<=Date.parse(target.intent.preparedAt))target.entryBook=b;
   const list=bookHistory[b.exchangeId]||[];if(list.at(-1)?.versionKey===b.versionKey)continue;
   const mid=(Number(amount(b.bids[0].price))+Number(amount(b.asks[0].price)))/2e6;
   bookHistory[b.exchangeId]=[...list,{at:b.sourceAt,receivedAt:b.receivedAt,mid,versionKey:b.versionKey}].filter(x=>Date.parse(x.at)>=Date.parse(b.receivedAt)-bank.policy.lookbackMs).slice(-512);
  }
 }
 if(wanted.has(row.sequence))for(const target of targets.filter(x=>x.proof.sourceSequence===row.sequence)){
  const exchangeId=target.intent.exchangeId,history=histories[exchangeId]??null,books=bookHistory[exchangeId]??[];
  requireThat(row.hash===target.proof.sourceHash&&digest({exchangeId,history,bookHistory:books})===target.proof.referenceHash,'REFERENCE_RECONSTRUCTION_FAILED');
  target.referenceInputs=structuredClone({history,books});
 }
}
const entries=targets.map(t=>{
 requireThat(t.entryBook&&t.referenceInputs,'ENTRY_BOOK_OR_REFERENCE_MISSING');const {history,books}=t.referenceInputs,b=t.entryBook;
 const diagnostic=historicalReferenceQuality(history,books,b.requestStartedAt,bank.policy),signal=dipSignal(diagnostic.reference,Number(amount(b.asks[0].price)+5000n)/1e6,Number(amount(b.bids[0].price)-5000n)/1e6,bank.policy);
 requireThat(signal.admitted,'ORIGINAL_SIGNAL_DID_NOT_REPRODUCE');
 return {marketId:t.intent.marketId,exchangeId:t.intent.exchangeId,preparedAt:t.intent.preparedAt,price:t.intent.price,quantity:t.intent.quantity,referenceProof:t.proof,coreHash:t.coreHash,bookVersion:b.versionKey,bookReceivedAt:b.receivedAt,decisionInputAt:b.requestStartedAt,signal,quality:diagnostic.quality};
});
const report={schemaVersion:1,at:new Date().toISOString(),status:'exact_reference_proofs_reproduced',sourceSequence,sourceHash,entries,
 networkRequests:0,competitionOrdersChanged:0,profitabilityEstablished:false,limitations:['All recorded admitted entries retained, including losses.','Descriptive pre-entry history is not a forecast or strategy validation.','Minute-balanced benchmark is a comparison, not a retroactive replacement of executed decisions.']};
fs.writeFileSync('data/owned-reference-quality.json',JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report));
