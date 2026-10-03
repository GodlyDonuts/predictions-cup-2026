import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {fileURLToPath} from 'node:url';
import {createBookPublicationState,applyBookPublication} from './book-publication.mjs';
import {pairedPackageCodeIdentity,pairedPackageEvent} from './paired-package-watch-v4.mjs';
import {createPairedPaper,advancePairedPaper,pairedPaperHash,pairedPaperReport} from './paired-package-paper-v3.mjs';
import {amount,money,price,SCALE,TICK,instant} from '../paper/fixed.mjs';
const legs=['381','382'],need=(x,c)=>{if(!x)throw Error(c);},sha=x=>createHash('sha256').update(x).digest('hex');
export function measurePairedExit(state,bank,clock){
 const books=legs.map(id=>state.books[id]),now=instant(clock);
 let reason='ready';if(books.some(x=>!x))reason='missing_book';
 else if(books.some(x=>!x.bids.length||!x.asks.length))reason='missing_side';
 else if(books.some(x=>instant(x.receivedAt)>now))reason='future_quote';
 else if(books.some(x=>now-instant(x.receivedAt)>bank.policy.maxQuoteAgeMs))reason='stale_quote';
 else if(Math.abs(instant(books[0].receivedAt)-instant(books[1].receivedAt))>bank.policy.maxPairSkewMs)reason='quote_skew';
 const lots=[...state.packages,...(state.active&&state.active.fills[legs[0]]===state.active.fills[legs[1]]&&state.active.fills[legs[0]]>0?[state.active]:[])];
 const results=lots.map(p=>{
  const q=Math.min(...legs.map(id=>p.fills[id])),lotCost=legs.reduce((n,id)=>n+amount(p.costs[id]),0n);
  const basic={packageId:p.id,quantity:q,lotCost:money(lotCost),openedAt:p.completedAt||p.createdAt,reason};
  if(reason!=='ready')return basic;
  const n=Math.min(q,...books.map(x=>Number(BigInt(x.asks[0].quantity)*BigInt(bank.policy.depthFractionBps)/10000n)));
  if(!n)return {...basic,reason:'insufficient_top_depth'};
  const rawUnits=books.map(x=>SCALE-price(x.asks[0].price)),units=rawUnits.map(x=>x-TICK);
  if(units.some(x=>x<TICK||x>SCALE-TICK))return {...basic,reason:'invalid_sale_limit'};
  const basisCeiling=(lotCost*BigInt(n)+BigInt(q)-1n)/BigInt(q),rawMargin=BigInt(n)*(rawUnits[0]+rawUnits[1])-basisCeiling,paddedMargin=rawMargin-2n*BigInt(n)*TICK,target=BigInt(n)*amount(bank.policy.pairedTakeProfitPerShare);
  return {...basic,reason:'ready',saleQuantity:n,rawMargin:money(rawMargin),paddedMargin:money(paddedMargin),target:money(target),targetSurplus:money(paddedMargin-target),targetEligible:paddedMargin>=target,versions:books.map(x=>x.versionKey)};
 });return {reason,results};
}
function rows(buffer,{partial=false}={}){
 const complete=buffer.lastIndexOf(10)+1;need(partial||complete===buffer.length,'EXIT_AUDIT_PARTIAL_JOURNAL');let start=0;const out=[];
 while(start<complete){const end=buffer.indexOf(10,start)+1;need(end-start>1&&end-start<=8*1024*1024,'EXIT_AUDIT_RECORD_SIZE');out.push({row:JSON.parse(buffer.subarray(start,end)),offset:end});start=end;}return out;
}
export function auditPairedExits({bank,sourceBytes,journalBytes}){
 need(pairedPaperHash(bank.codeIdentity)===pairedPaperHash(pairedPackageCodeIdentity()),'EXIT_AUDIT_FROZEN_CODE');
 const source=rows(sourceBytes,{partial:true}),saved=rows(journalBytes);let publication=createBookPublicationState(bank.universe),offset=0,index=0;
 while(index<source.length&&source[index].offset<=bank.sourcePrefixBytes){const x=source[index++];publication=applyBookPublication(publication,x.row);offset=x.offset;}
 need(offset===bank.sourcePrefixBytes&&sha(sourceBytes.subarray(0,offset))===bank.sourcePrefixHash&&publication.sequence===bank.sourcePrefixSequence&&publication.hash===bank.sourcePrefixJournalHash&&publication.scopeHash===bank.sourceScopeHash&&publication.sessionId===bank.sourceSessionId&&!publication.closed,'EXIT_AUDIT_PREFIX');
 const hostFile=path.join(path.dirname(fileURLToPath(import.meta.url)),'paired-package-watch-v4.mjs'),hostHash=sha(fs.readFileSync(hostFile));
 let state=createPairedPaper(bank),sequence=0,previous=pairedPaperHash(bank),ended=false;const checkpoints={},lots=new Map(),pairs=new Set();
 for(const savedRow of saved){const {hash,...body}=savedRow.row;
  need(body.sequence===++sequence&&body.previous===previous&&pairedPaperHash(body)===hash&&body.bankHash===pairedPaperHash(bank),'EXIT_AUDIT_JOURNAL_CHAIN');
  if(body.kind==='genesis')need(sequence===1&&body.stateHash===pairedPaperHash(state),'EXIT_AUDIT_GENESIS');
  else{
   need(body.kind==='batch'&&body.sourceSequence>publication.sequence,'EXIT_AUDIT_CHECKPOINT');
   const continuity=body.hostVersion!==undefined;need(!continuity||body.hostVersion==='session-aware-paired-v4'&&body.hostHash===hostHash,'EXIT_AUDIT_HOST');
   const events=[];let eventId=state.lastEventId;
   while(publication.sequence<body.sourceSequence){const x=source[index++];need(x,'EXIT_AUDIT_SOURCE_TRUNCATED');const r=x.row;
    need(continuity||r.sessionId===bank.sourceSessionId&&r.kind!=='session_start','EXIT_AUDIT_SESSION');publication=applyBookPublication(publication,r);offset=x.offset;
    if(r.kind==='book')events.push(pairedPackageEvent(r,++eventId,body.availableAt));
    else if(['invalidation','session_stop','session_start'].includes(r.kind)){const id=r.kind==='invalidation'?r.payload.marketId:null;for(const m of bank.markets.filter(m=>id===null||m.marketId===id))events.push({kind:'invalidate',eventId:++eventId,tournamentId:bank.tournamentId,marketId:m.marketId,exchangeId:m.exchangeId,confirmedAt:r.at,availableAt:body.availableAt,reason:r.kind==='session_start'?'SESSION_START':r.payload.reason});}
    if(r.kind==='session_stop')ended=true;if(continuity&&r.kind==='session_start')ended=false;
   }
   state=advancePairedPaper(state,events,bank,{availableAt:body.availableAt,allowDecisions:body.allowDecisions&&!ended});
   const expected={sourceBytes:offset,sourceSequence:publication.sequence,sourceHash:publication.hash,sourceStateHash:pairedPaperHash(publication),evidenceThrough:publication.lastAt,stateHash:pairedPaperHash(state)};
   need(Object.entries(expected).every(([k,v])=>pairedPaperHash(v)===pairedPaperHash(body[k])),'EXIT_AUDIT_REPLAY');
   const measured=measurePairedExit(state,bank,body.availableAt);
   if(measured.results.length)checkpoints[measured.reason]=(checkpoints[measured.reason]||0)+1;
   for(const x of measured.results){let l=lots.get(x.packageId);if(!l){l={packageId:x.packageId,initialQuantity:x.quantity,initialCost:x.lotCost,openedAt:x.openedAt,lastObservedAt:body.availableAt,validVersionPairs:new Set(),targetEligiblePairs:new Set(),reasons:{},bestRawMargin:null,bestPaddedMargin:null,bestTargetSurplus:null,bestAt:null};lots.set(x.packageId,l);}l.lastObservedAt=body.availableAt;l.reasons[x.reason]=(l.reasons[x.reason]||0)+1;
    if(x.reason!=='ready')continue;const key=x.versions.join('|');l.validVersionPairs.add(key);pairs.add(key);if(x.targetEligible)l.targetEligiblePairs.add(key);
    if(l.bestPaddedMargin===null||amount(x.paddedMargin)>amount(l.bestPaddedMargin)){l.bestPaddedMargin=x.paddedMargin;l.bestAt=body.availableAt;l.bestSaleQuantity=x.saleQuantity;l.bestQuoteVersions=x.versions;}
    for(const [k,v]of [['bestRawMargin',x.rawMargin],['bestTargetSurplus',x.targetSurplus]])if(l[k]===null||amount(v)>amount(l[k]))l[k]=v;
   }
  }previous=hash;
 }
 need(sequence>0,'EXIT_AUDIT_EMPTY');const report=pairedPaperReport(state,bank,state.lastAvailableAt||bank.cutoff);
 return {schemaVersion:1,bankHash:pairedPaperHash(bank),checkpointCount:sequence,journalHash:previous,stateHash:pairedPaperHash(state),sourceSequence:publication.sequence,sourceHash:publication.hash,evidenceThrough:publication.lastAt,checkpointsWithHolding:checkpoints,distinctFreshVersionPairs:pairs.size,
  lots:[...lots.values()].map(l=>{const closed=state.roundTrips.find(p=>p.packageId===l.packageId);return {...l,closedAt:closed?.closedAt??null,rightCensored:!closed,validVersionPairs:l.validVersionPairs.size,targetEligiblePairs:l.targetEligiblePairs.size,observedHoldingMinutes:(instant(closed?.closedAt||l.lastObservedAt)-instant(l.openedAt))/60000};}),
  finalWallet:report.wallets.sequential_dual_no,networkRequests:0,runtimeWrites:0,profitabilityEstablished:false,
  limitations:['This verifies original consumer checkpoints, not unobserved continuous prices or actual queue fills.','Distinct paired quote versions and repeated checkpoints are not independent trades.','Raw margin removes only the two sale ticks; acquired basis retains original conservative entry ticks. Diagnostic alternatives are not a newly validated strategy.','Holding durations are right-censored for open packages. Conditional settlement value is not realized profit.']};
}
