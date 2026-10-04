import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {Journal} from './transport-runtime.mjs';import {atomicJson,payloadHash} from '../execution/ledger.mjs';
import {createBaalPairState,advanceBaalPair,baalPairReport} from './baal-pair-paper-v2.mjs';
import {evaluateCanonicalNoCapacity,validateCapacityReview} from './canonical-no-capacity-v2.mjs';
import {paperBookContinuationFrontier} from './paper-book-continuation-frontier.mjs';
const sha=b=>createHash('sha256').update(b).digest('hex'),need=(v,c)=>{if(!v)throw Error('BAAL_PAIR_HOST_'+c);};
export const baalPairCodeIdentity=()=>Object.fromEntries(['./baal-pair-host-v3.mjs','./baal-pair-paper-v2.mjs','./paper-book-continuation-frontier.mjs','./canonical-no-capacity-v2.mjs','./canonical-no-capacity.mjs','./probability-receipt.mjs',
 './transport-runtime.mjs','./journal-lines.mjs','./model.mjs','../execution/core.mjs','../execution/api-tick-price.mjs','../execution/ledger.mjs','../paper/fixed.mjs','../feed/contract.mjs'].map(f=>[f,sha(fs.readFileSync(new URL(f,import.meta.url)))]));
export function openBaalPairHost({runtimeDir,now=Date.now,canOperate=()=>true}){
 const activeFile=path.join(runtimeDir,'baal-pair-active.json');if(!fs.existsSync(activeFile))return null;
 const active=JSON.parse(fs.readFileSync(activeFile));need(/^[a-z][a-z0-9-]{1,63}$/.test(active.cohort),'COHORT');
 need(active.hostRevision===3,'HOST_REVISION');
 const dir=path.join(runtimeDir,'baal-pair-cohorts',active.cohort),file=path.join(dir,'bank.json'),bankBytes=fs.readFileSync(file),bank=JSON.parse(bankBytes);
 need(payloadHash(bank)===active.bankHash&&payloadHash(bank.codeIdentity)===payloadHash(baalPairCodeIdentity()),'FROZEN_IDENTITY');
 validateCapacityReview(bank.review);
 const source=fs.readFileSync(path.join(runtimeDir,'public-books.jsonl')).subarray(0,bank.sourcePrefixBytes);
 need(source.length===bank.sourcePrefixBytes&&source.at(-1)===10&&sha(source)===bank.sourcePrefixHash,'SOURCE_PREFIX');
 const journal=new Journal(path.join(dir,'journal.jsonl')),statsFile=path.join(dir,'stats.json');
 function opening(){
  const initial=createBaalPairState(bank),c=bank.accountingContinuation;if(!c)return initial;
  need(/^[a-z][a-z0-9-]{1,63}$/.test(c.cohort),'CONTINUATION_SCOPE');
  const prior=path.join(runtimeDir,'baal-pair-cohorts',c.cohort),priorBank=fs.readFileSync(path.join(prior,'bank.json'));
  need(sha(priorBank)===c.bankFileHash&&payloadHash(JSON.parse(priorBank))===c.bankHash,'CONTINUATION_BANK');
  const bytes=fs.readFileSync(path.join(prior,'journal.jsonl')).subarray(0,c.journalPrefixBytes);need(bytes.length===c.journalPrefixBytes&&bytes.at(-1)===10&&sha(bytes)===c.journalPrefixHash,'CONTINUATION_PREFIX');
  const rows=bytes.toString().trimEnd().split('\n').map(JSON.parse);let previous='';
  for(const [i,row]of rows.entries()){const{hash,...body}=row;need(row.sequence===i+1&&row.previous===previous&&sha(JSON.stringify(body))===hash,'CONTINUATION_CHAIN');previous=hash;}
  const frontier=paperBookContinuationFrontier({file:path.join(runtimeDir,'public-books.jsonl'),endByte:bank.sourcePrefixBytes,priorState:rows.at(-1).state,legs:bank.review.legs});
  need(payloadHash(frontier)===payloadHash(c.sourceFrontier)&&frontier.sealedSequence===bank.sourcePrefixSequence&&frontier.sealedHash===bank.sourcePrefixTailHash,'CONTINUATION_FRONTIER');
  const seedBytes=fs.readFileSync(path.join(dir,'opening-state.json')),seed=JSON.parse(seedBytes);
  need(sha(seedBytes)===c.openingStateFileHash&&payloadHash(seed)===payloadHash(rows.at(-1).state.paper)&&seed.bankHash===c.bankHash,'CONTINUATION_SEED');
  return advanceBaalPair({...seed,bankHash:active.bankHash},{kind:'boundary',availableAt:bank.cutoff},bank);
 }
 let state=journal.last?.state||{paper:opening(),sourceSequence:bank.sourcePrefixSequence,sourceHash:bank.sourcePrefixTailHash},failed=false;
 const status=()=>({...baalPairReport(state.paper,bank,new Date(now()).toISOString()),failed,journalSequence:journal.sequence,journalHash:journal.previous,
  riskMarketIds:[...new Set([...Object.values(state.paper.positions).map(x=>x.marketId),...(state.paper.pending?[state.paper.pending.marketId]:[])])],
  sourceSequence:state.sourceSequence,sourceHash:state.sourceHash,accountingContinuation:bank.accountingContinuation||null});
 const verify=(allowStopped=false)=>need(!failed&&(allowStopped||canOperate()&&!fs.existsSync(path.join(dir,'STOP')))&&sha(fs.readFileSync(file))===sha(bankBytes),'STOP_OR_BANK');
 const append=(next,event,allowStopped=false)=>{verify(allowStopped);try{journal.commit(next,event);state=next;atomicJson(statsFile,status());}catch{failed=true;throw Error('BAAL_PAIR_HOST_DURABILITY');}};
 if(fs.existsSync(statsFile)){const old=JSON.parse(fs.readFileSync(statsFile));need(old.bankHash===active.bankHash&&old.journalSequence<=journal.sequence,'PUBLISHED_ROLLBACK');if(old.journalSequence===journal.sequence)need(old.journalHash===journal.previous,'PUBLISHED_TAIL');}
 if(!journal.last)append(state,{kind:'genesis',cutoff:bank.cutoff});
 else{const at=new Date(Math.max(now(),Date.parse(state.paper.lastAt))).toISOString();append({...state,paper:advanceBaalPair(state.paper,{kind:'boundary',availableAt:at},bank)},{kind:'boundary',availableAt:at,reason:'collector_restart'});}
 return{bank,status,
  onBook(publication,marketId,accountEligibility){verify();const leg=bank.review.legs.find(l=>l.marketId===marketId);if(!leg)return;
   const book=publication.books[leg.exchangeId+':YES'];if(!book)return;
   need(publication.sequence>state.sourceSequence&&/^[a-f0-9]{64}$/.test(publication.hash),'SOURCE_FRONTIER');
   const at=new Date(now()).toISOString(),books=bank.review.legs.map(l=>publication.books[l.exchangeId+':YES']);let admission={reason:'pair_incomplete_or_closed',positiveConditionalCapacity:false};
   if(!publication.closed&&books.every(Boolean))try{admission=evaluateCanonicalNoCapacity({review:bank.review,policy:bank.capacityPolicy,books,account:accountEligibility,now:at});}catch{admission={reason:'pair_validation_failed',positiveConditionalCapacity:false};}
   const event={kind:'book',availableAt:at,book,pairBooks:books.filter(Boolean),accountEligibility,admission};
   append({paper:advanceBaalPair(state.paper,event,bank),sourceSequence:publication.sequence,sourceHash:publication.hash},{...event,sourceSequence:publication.sequence,sourceHash:publication.hash});
  },
  boundary(reason){verify(true);const at=new Date(now()).toISOString();append({...state,paper:advanceBaalPair(state.paper,{kind:'boundary',availableAt:at},bank)},{kind:'boundary',availableAt:at,reason},true);},
 };
}
