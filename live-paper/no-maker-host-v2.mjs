import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {Journal} from './transport-runtime.mjs';import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {digest} from './model.mjs';import {evaluateCanonicalNoCapacity} from './canonical-no-capacity.mjs';
import {createNoMakerState,advanceNoMaker,noMakerReport} from './no-maker-paper.mjs';
const need=(ok,c)=>{if(!ok)throw Error('NO_MAKER_HOST_'+c);},sha=b=>createHash('sha256').update(b).digest('hex');
export const noMakerCodeIdentityV2=()=>Object.fromEntries(['./no-maker-host-v2.mjs','./no-maker-paper.mjs','./canonical-entry-exit-study.mjs','./canonical-no-capacity.mjs',
 './probability-receipt.mjs','./model.mjs','./transport-runtime.mjs','./journal-lines.mjs','../execution/api-tick-price.mjs','../execution/core.mjs',
 '../execution/ledger.mjs','../paper/fixed.mjs','../feed/contract.mjs'].map(f=>[f,sha(fs.readFileSync(new URL(f,import.meta.url)))]));
export function openNoMakerHostV2({runtimeDir,now=Date.now,canOperate=()=>true}){
 const activeFile=path.join(runtimeDir,'no-maker-active.json');if(!fs.existsSync(activeFile))return null;
 const active=JSON.parse(fs.readFileSync(activeFile));need(/^[a-z][a-z0-9-]{1,63}$/.test(active.cohort),'COHORT');
 const dir=path.join(runtimeDir,'no-maker-cohorts',active.cohort),bankFile=path.join(dir,'bank.json'),bankBytes=fs.readFileSync(bankFile),bank=JSON.parse(bankBytes);
 need(active.hostRevision===2&&bank.hostRevision===2,'HOST_REVISION');
 need(active.bankHash===payloadHash(bank)&&payloadHash(noMakerCodeIdentityV2())===payloadHash(bank.codeIdentity),'FROZEN_IDENTITY');
 const tapeFile=path.join(runtimeDir,'anonymous-trades.jsonl'),prefix=fs.readFileSync(tapeFile).subarray(0,bank.tapePrefixBytes);
 need(prefix.length===bank.tapePrefixBytes&&(!prefix.length||prefix.at(-1)===10)&&sha(prefix)===bank.tapePrefixHash,'TAPE_PREFIX');
 const journal=new Journal(path.join(dir,'journal.jsonl')),statsFile=path.join(dir,'stats.json'),stopped=()=>!canOperate()||fs.existsSync(path.join(dir,'STOP'));
 let state=journal.last?.state||{paper:createNoMakerState(bank),tapeSequence:bank.tapePrefixSequence,tapeHash:bank.tapePrefixTailHash},failed=false;
 need(state.paper.bankHash===active.bankHash,'STATE_BANK');
 if(fs.existsSync(statsFile)){const old=JSON.parse(fs.readFileSync(statsFile));need(old.bankHash===active.bankHash&&old.journalSequence<=journal.sequence,'PUBLISHED_JOURNAL_ROLLBACK');if(old.journalSequence===journal.sequence)need(old.journalHash===journal.previous,'PUBLISHED_JOURNAL_TAIL');}
 const status=()=>({...noMakerReport(state.paper,bank,new Date(now()).toISOString()),failed,hostRevision:2,lastAdmission:state.lastAdmission||null,journalSequence:journal.sequence,journalHash:journal.previous,tapeSequence:state.tapeSequence,
  riskMarketIds:[...new Set(Object.values(state.paper.wallets).flatMap(w=>[...Object.values(w.positions).map(p=>p.marketId),...Object.values(w.quotes).filter(q=>!q.resolvedAt).map(q=>q.marketId)]))]});
 const verify=(allowStopped=false)=>{need(!failed&&(allowStopped||!stopped()),'STOPPED');need(sha(fs.readFileSync(bankFile))===sha(bankBytes),'BANK_CHANGED');};
 function append(next,event,allowStopped=false){verify(allowStopped);try{journal.commit(next,event);state=next;atomicJson(statsFile,status());}catch{failed=true;throw Error('NO_MAKER_HOST_DURABILITY');}}
 function page(row,availableAt){
  need(row.sequence===state.tapeSequence+1&&row.previous===state.tapeHash,'TAPE_FRONTIER');const {hash,...body}=row;need(sha(JSON.stringify(body))===hash,'TAPE_HASH');
  const e=row.event,relevant=bank.legs.some(l=>l.marketId===e.marketId&&l.exchangeId===e.exchangeId);
  const next={paper:relevant?advanceNoMaker(state.paper,{kind:'trades',availableAt,page:e},bank):state.paper,tapeSequence:row.sequence,tapeHash:hash};
  append(next,{kind:relevant?'trade_page':'other_trade_page',availableAt,tapeSequence:row.sequence,tapeHash:hash,event:relevant?e:null});
 }
 // If the source page committed just before a crash, resume it before creating
 // another quote. Original receipt time is retained in the source binding.
 const raw=fs.readFileSync(tapeFile);need(!raw.length||raw.at(-1)===10,'PARTIAL_SOURCE');let tapePrevious='',sequence=0;
 for(const line of raw.toString().trimEnd().split('\n').filter(Boolean)){const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===tapePrevious&&sha(JSON.stringify(body))===hash,'SOURCE_CHAIN');tapePrevious=hash;
  if(row.sequence===state.tapeSequence)need(row.hash===state.tapeHash,'SOURCE_FRONTIER_CHANGED');
  if(row.sequence>state.tapeSequence)page(row,new Date(Math.max(now(),Date.parse(state.paper.lastAt))).toISOString());
 }
 need(sequence>=state.tapeSequence,'SOURCE_TRUNCATED');
 if(!journal.last)append(state,{kind:'genesis',cutoff:bank.cutoff});
 return {bank,dir,status,
  onPage(row){verify();if(row.sequence<=state.tapeSequence){need(row.sequence===state.tapeSequence&&row.hash===state.tapeHash,'SOURCE_DUPLICATE');return;}page(row,new Date(now()).toISOString());},
  onBook(publication,marketId,accountEligibility){verify();const leg=bank.legs.find(l=>l.marketId===marketId);if(!leg)return;
   const b=publication.books[leg.exchangeId+':YES'];if(!b)return;
   const at=new Date(now()).toISOString(),books=bank.legs.map(l=>publication.books[l.exchangeId+':YES']);let entryAllowed=false,reason='pair_incomplete_or_closed';
   if(!publication.closed&&books.every(Boolean))try{const frame=evaluateCanonicalNoCapacity({review:bank.review,policy:bank.capacityPolicy,books,account:accountEligibility,now:at});entryAllowed=frame.positiveConditionalCapacity;reason=frame.reason;}catch{reason='pair_validation_failed';}
   // A coherent pair becomes knowable at this consumer clock. Reconsider both
   // actual books, in canonical leg order, without changing their request,
   // receipt, version or publication clocks. A stale/incomplete pair only
   // advances the incoming leg for any already-owned risk bookkeeping.
   const evaluated=entryAllowed?books:[b];let paper=state.paper;
   for(const book of evaluated)paper=advanceNoMaker(paper,{kind:'book',availableAt:at,book,accountEligibility,entryAllowed},bank);
   const lastAdmission={at,reason,entryAllowed,marketIds:evaluated.map(x=>x.marketId),incomingMarketId:marketId};
   append({...state,paper,lastAdmission},{kind:entryAllowed?'paired_book':'book',availableAt:at,books:evaluated,accountEligibility,entryAllowed,
    sourceSequence:publication.sequence,sourceHash:publication.hash,incomingMarketId:marketId,reason});
  },
  boundary(reason){verify(true);const at=new Date(now()).toISOString();append({...state,paper:advanceNoMaker(state.paper,{kind:'boundary',availableAt:at},bank)},{kind:'boundary',availableAt:at,reason},true);},
 };
}
