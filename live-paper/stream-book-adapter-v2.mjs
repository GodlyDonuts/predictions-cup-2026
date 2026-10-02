import {createHash} from 'node:crypto';
import {createPassiveCapture,consumePassiveCapture} from './passive-capture-v2.mjs';
import {instantNs} from '../feed/contract.mjs';
import {instant} from '../paper/fixed.mjs';

export const streamDigest=x=>createHash('sha256').update(JSON.stringify(x)).digest('hex');
const need=(x,m)=>{if(!x)throw Error(m);},ms=x=>Number(instantNs(x)/1000000n),iso=x=>new Date(x).toISOString();
// Accepted public market evidence only. This module never invents a REST
// request for a push, nor turns a quote/trade into an execution claim.
export function createStreamBookAdapter(bank){
 need(bank?.schemaVersion===1&&bank.mode==='verified_stream_book_input_v2'&&Number.isSafeInteger(bank.sourcePrefixRecords)&&bank.sourcePrefixRecords>=0,'INVALID_STREAM_BANK');
 instant(bank.cutoff);need(/^[a-f0-9]{64}$/.test(bank.sourceHash),'INVALID_STREAM_SOURCE');
 const capture=createPassiveCapture({scope:bank.scope,sourceHash:bank.sourceHash,cutoff:bank.cutoff,sessionId:bank.sessionId});
 return {schemaVersion:1,bankHash:streamDigest(bank),capture,lastAvailableAt:null,latest:{},
  counters:{sourceRows:0,prefixRows:0,acceptedBooks:0,streamBooks:0,restBooks:0,bufferedRecoveryBooks:0,invalidations:0,staleAtConsumption:0},lastEventId:0};
}
export function advanceStreamBookAdapter(input,row,bank,{availableAt}){
 need(input.bankHash===streamDigest(bank),'STREAM_BANK_CHANGED');const host=instant(availableAt),observed=ms(row.observedAt);
 need(host>=observed&&(!input.lastAvailableAt||host>=instant(input.lastAvailableAt)),'STREAM_HOST_CLOCK_INVALID');
 const marketId=/:market:([1-9]\d*)$/.exec(row.topic||'')?.[1],before=input.capture.contexts[marketId];
 const out=consumePassiveCapture(input.capture,row),state={...input,capture:out.state,lastAvailableAt:availableAt,
  latest:structuredClone(input.latest),counters:{...input.counters}},events=[];
 const prefix=row.captureIndex<bank.sourcePrefixRecords;need(!prefix||observed<=instant(bank.cutoff),'STREAM_PREFIX_AFTER_CUTOFF');state.counters.sourceRows++;if(prefix)state.counters.prefixRows++;
 const publish=e=>{if(!prefix)events.push({...e,eventId:++state.lastEventId,sourceCaptureIndex:row.captureIndex,sourceRowHash:streamDigest(row),
  captureObservedAt:row.observedAt,availableAt});};
 for(const record of out.records){
  const e=record.event,ctx=out.state.contexts[record.marketId],common={marketId:record.marketId,exchangeId:ctx.observer.bank.exchangeId,
   tournamentId:bank.scope.tournamentId,confirmedAt:e.receivedAt,continuity:structuredClone(e.continuity)};
  if(e.kind==='invalidate'){
   delete state.latest[record.marketId];if(!prefix)state.counters.invalidations++;
   publish({...common,kind:'invalidate',reason:e.reason});continue;
  }
  if(e.kind!=='book')continue;
  let origin,quoteReceivedAt,requestStartedAt=null;
  if(row.kind==='event'){
   origin='stream';quoteReceivedAt=row.receivedAt??row.observedAt;
  }else{
   need(row.kind==='recovery_finished'&&before?.snapshot,'RECOVERY_EVIDENCE_MISSING');
   const snap=before.snapshot.book,fromSnapshot=String(snap.asOf.sequence)===e.book.sequence&&instantNs(snap.asOf.at)===instantNs(e.book.sourceAt);
   if(fromSnapshot){origin='rest_recovery';quoteReceivedAt=before.snapshot.receivedAt;requestStartedAt=before.snapshot.originalRequestStartedAt;}
   else{need(before.latestBook?.origin==='stream'&&String(before.latestBook.book.asOf.sequence)===e.book.sequence
     &&instantNs(before.latestBook.book.asOf.at)===instantNs(e.book.sourceAt),'BUFFERED_RECOVERY_EVIDENCE_MISSING');
    origin='buffered_stream_recovery';quoteReceivedAt=before.latestBook.receivedAt;}
  }
  const receipt=ms(quoteReceivedAt),confirmed=instant(e.receivedAt),expiry=ctx.knownExpiry?.at??null;
  need(receipt<=host+1000&&confirmed<=host&&(!requestStartedAt||instantNs(requestStartedAt)<=instantNs(quoteReceivedAt)),'STREAM_RECEIPT_CLOCK_INVALID');
  // A recovery confirmation can be later than the book's original receipt.
  // Never refresh quote age to the confirmation or the local polling time.
  const age=host-receipt,eligible=!prefix&&observed>instant(bank.cutoff)&&ctx.ready&&e.continuity.ready&&e.continuity.continuous
   &&age>=0&&age<Math.min(20000,ctx.staleMs)&&(!expiry||ms(expiry)>host);
  const {requestStartedAt:unusedRequest,receivedAt:unusedReceipt,...book}=e.book;
  const event={...common,kind:'book',origin,quoteReceivedAt,requestStartedAt,nextExpiryAt:expiry,
   expiryKnown:ctx.knownExpiry!==null,expiryVersion:structuredClone(ctx.knownExpiry?.version??null),decisionEligible:eligible,
   book:structuredClone(book),evidence:'accepted_public_book_not_order_fill'};
  state.latest[record.marketId]={...event,availableAt,captureObservedAt:row.observedAt};
  if(!prefix){state.counters.acceptedBooks++;state.counters[origin==='stream'?'streamBooks':origin==='rest_recovery'?'restBooks':'bufferedRecoveryBooks']++;
   if(!eligible)state.counters.staleAtConsumption++;}
  publish(event);
 }
 return {state,events};
}
export function streamBookReport(state,bank,at){
 const now=instant(at);need(state.bankHash===streamDigest(bank)&&(!state.lastAvailableAt||now>=instant(state.lastAvailableAt)),'STREAM_REPORT_CLOCK_INVALID');
 return {schemaVersion:1,mode:bank.mode,bankHash:state.bankHash,cutoff:bank.cutoff,updatedAt:at,sourceCaptureIndex:state.capture.captureIndex,
  lastAvailableAt:state.lastAvailableAt,counters:structuredClone(state.counters),sourceEnded:state.capture.sourceEnded,
  markets:bank.scope.markets.map(({marketId,exchangeId})=>{const b=state.latest[marketId],age=b?now-ms(b.quoteReceivedAt):null;
   return {marketId,exchangeId,origin:b?.origin??null,expiryKnown:b?.expiryKnown??false,nextExpiryAt:b?.nextExpiryAt??null,quoteReceivedAt:b?.quoteReceivedAt??null,confirmedAt:b?.confirmedAt??null,
    availableAt:b?.availableAt??null,ageMs:age,ready:Boolean(b?.decisionEligible&&state.capture.contexts[marketId].ready&&age>=0&&age<20000&&(!b.nextExpiryAt||ms(b.nextExpiryAt)>now))};}),
  networkRequests:0,ordersSubmitted:0,simulatedFills:null,pnl:null};
}
