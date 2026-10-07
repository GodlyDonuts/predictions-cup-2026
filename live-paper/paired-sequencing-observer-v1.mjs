import {validateUniverse} from './breadth.mjs';import fs from 'node:fs';import path from 'node:path';import {payloadHash} from '../execution/core.mjs';import {Journal} from './transport-runtime.mjs';import {atomicJson} from '../execution/ledger.mjs';import {comparePairedEntryLegRiskV1} from '../execution/paired-entry-leg-risk-v1.mjs';import {amount,money,SCALE,TICK} from '../paper/fixed.mjs';
// Prospective observation only: no timers, credentials, API, wallet fills or orders.
export function openPairedSequencingObserverV1({runtimeDir,universe,now=Date.now}){
 const file=path.join(runtimeDir,'paired-sequencing-observer-request.json');if(!fs.existsSync(file))return null;const requestBytes=fs.readFileSync(file),r=JSON.parse(requestBytes),{hash:declaredHash,...rawUniverse}=universe,universeHash=declaredHash??validateUniverse(rawUniverse,universe.tournamentId).hash;
 if(r.kind!=='paired_sequencing_prospective_observer_v1'||r.tournamentId!==universe.tournamentId||r.universeHash!==universeHash||!Number.isFinite(Date.parse(r.cutoff))||!Array.isArray(r.pairs))throw Error('SEQUENCING_OBSERVER_REQUEST');
 for(const pair of r.pairs){if(pair.length!==2||pair[0].exchangeId===pair[1].exchangeId||pair.some(l=>!universe.exchanges.some(e=>e.exchangeId===l.exchangeId&&e.marketId===l.marketId)))throw Error('SEQUENCING_OBSERVER_SCOPE');}
 const cohort=r.cohort??'original';if(!/^[a-z][a-z0-9-]{1,63}$/.test(cohort))throw Error('SEQUENCING_OBSERVER_COHORT');const dir=path.join(runtimeDir,'paired-sequencing-observer-cohorts',cohort);fs.mkdirSync(dir,{recursive:true,mode:0o700});const journalFile=path.join(dir,'journal.jsonl'),journal=new Journal(journalFile);let state=journal.last?.state??{requestHash:payloadHash(r),observations:0,conditionalPositive:0,sequence:0,pairs:{},seen:{}};
 if(state.requestHash!==payloadHash(r))throw Error('SEQUENCING_OBSERVER_IMMUTABLE_REQUEST');
 const statsFile=path.join(dir,'stats.json'),status=()=>({kind:r.kind,cohort,cutoff:r.cutoff,observations:state.observations,conditionalPositive:state.conditionalPositive,sourceSequence:state.sequence,latestPairs:state.pairs,orders:0,paperFills:0,probabilityEstimated:false,executionAuthority:false,additionalApiClients:0});
 if(!journal.last)journal.commit(state,{kind:'genesis',requestHash:state.requestHash});
 return{status,onBook(publication){
  if(!fs.readFileSync(file).equals(requestBytes))throw Error('SEQUENCING_OBSERVER_REQUEST_CHANGED');const at=now();if(publication.closed||publication.tournamentId!==r.tournamentId)return;
  for(const pair of r.pairs){const books=pair.map(l=>publication.books[l.exchangeId+':YES']);if(books.some(b=>!b||Date.parse(b.requestStartedAt)<=Date.parse(r.cutoff)||Date.parse(b.receivedAt)>at||at-Date.parse(b.requestStartedAt)>20000))continue;
   if(Math.max(...books.map(b=>Date.parse(b.receivedAt)))-Math.min(...books.map(b=>Date.parse(b.requestStartedAt)))>10000)continue;
   const key=pair.map(l=>l.exchangeId).join(':'),sourceHash=payloadHash(books.map(b=>({hash:b.publicationHash,version:b.versionKey})));if(state.seen[key]===sourceHash)continue;
   const legs=books.map(b=>({exchangeId:b.exchangeId,noBuyLimit:money(SCALE-amount(b.bids[0].price)+TICK),noSellLimit:money(SCALE-amount(b.asks[0].price))}));
   const depth=Math.min(...books.map(b=>Math.min(b.bids[0].quantity,b.asks[0].quantity)),100);let quantity=Math.min(depth,...legs.map(l=>Number(25000000n/amount(l.noBuyLimit))));if(quantity<1)continue;
   let result;try{result=comparePairedEntryLegRiskV1({legs,quantity});}catch{continue;}const next=structuredClone(state);next.observations++;next.sequence=publication.sequence;next.seen[key]=sourceHash;if(result.scenarios[0].conditionalOrdinarySurplusIfBothFill[0]!=='-'&&amount(result.scenarios[0].conditionalOrdinarySurplusIfBothFill)>0n)next.conditionalPositive++;next.pairs[key]={at:new Date(at).toISOString(),sourceHash,sourceSequence:publication.sequence,rootsReviewed:false,...result};journal.commit(next,{kind:'prospective_pair_depth',sourceHash,sourceSequence:publication.sequence,at:new Date(at).toISOString()});state=next;
  }
  atomicJson(statsFile,status());
 },close(){}};
}

