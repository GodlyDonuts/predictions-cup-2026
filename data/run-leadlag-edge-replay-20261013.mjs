import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {sealedNativePrefix, hashNativePrefix} from '../live-paper/native-dip-host-v1.mjs';
import {engineBookPointsV1} from '../paper/maker-engine-book-point-v1.mjs';
import {instantNs} from '../feed/contract.mjs';
import {leadLagMarketGraphV1, analyzeLeadLagV1} from '../research/leadlag-edge-v1.mjs';

const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup/paper-runtime');
const session=JSON.parse(fs.readFileSync(path.join(runtime,'realtime-session.json'),'utf8'));
if(!/^realtime-[A-Za-z0-9.-]+\.jsonl$/.test(session.file)||session.scope?.channelMode!=='tournament')throw Error('LEADLAG_CAPTURE_BINDING');
const captureFile=path.join(runtime,session.file),seal=sealedNativePrefix(captureFile);
const tailLength=Math.min(seal.bytes,1024*1024),tail=Buffer.alloc(tailLength),tailFd=fs.openSync(captureFile,'r');
try{let read=0;while(read<tailLength){const n=fs.readSync(tailFd,tail,read,tailLength-read,seal.bytes-tailLength+read);if(!n)throw Error('LEADLAG_CAPTURE_TAIL');read+=n;}}finally{fs.closeSync(tailFd);}
const lastRow=JSON.parse(tail.toString('utf8').trimEnd().split(/\r?\n/).at(-1));
if(lastRow.format!=='sig-feed-capture'||!Number.isSafeInteger(lastRow.captureIndex)||typeof lastRow.observedAt!=='string')throw Error('LEADLAG_CAPTURE_TAIL');
const sealedThrough=lastRow.observedAt,sealedThroughNs=instantNs(sealedThrough);
const metadataBytes=fs.readFileSync('data/structural-universe-latest.json'),metadata=JSON.parse(metadataBytes),metadataHash=createHash('sha256').update(metadataBytes).digest('hex');
if(metadata.tournamentId!==session.scope.tournamentId)throw Error('LEADLAG_TOURNAMENT_MISMATCH');
const causalPairs=metadata.candidatePairs.filter(pair=>pair.legs.every(leg=>{try{return instantNs(leg.rootObservedAt)<=sealedThroughNs;}catch{return false;}}));
const graph=leadLagMarketGraphV1({...metadata,candidatePairs:causalPairs}),scope={tournamentId:session.scope.tournamentId,exchanges:session.scope.markets};
const topicMarket=topic=>{const m=/^tournament:[0-9a-f-]+:market:(\d+)$/.exec(topic??'');return m?.[1]??null;};
const topicStates=new Map(),lastRevision=new Map(),lastBookVersion=new Map(),breaks=[],observations=[];
const diagnostics={captureRows:0,events:0,nonEventRows:0,duplicateRevisions:0,revisionGaps:0,resyncRequired:0,bookDirtyMarkers:0,dirtyWithoutBook:0,settlements:0,lifecycleInvalidations:0,validRecoveryCompletions:0,booksObservedBeforeFullRecovery:0,scopeRejectedBooks:0,metadataMissingBooks:0,duplicateBookVersions:0,rollbackBookVersions:0,acceptedBookPoints:0};
let expectedIndex=0,lastObserved=-Infinity,firstObserved=null,through=null,groupAt=null,group=[];

function flushGroup(){
  if(!group.length)return;
  const at=groupAt,invalidated=new Set(),updates=new Map(),events=[];
  const markInvalid=(topic,kind)=>{
    const marketId=topicMarket(topic);invalidated.add(marketId);
    if(marketId!==null)breaks.push({marketId,at:new Date(at).toISOString()});
    else breaks.push({marketId:null,at:new Date(at).toISOString()});
    const prior=updates.get(topic)??topicStates.get(topic)??{epoch:null,ready:false};
    updates.set(topic,{...prior,ready:false});
    if(kind==='lifecycle')diagnostics.lifecycleInvalidations++;
  };
  for(const row of group){
    const topic=row.topic??null,marketId=topicMarket(topic);
    if(row.kind==='subscribed'){
      invalidated.add(marketId);if(marketId!==null)breaks.push({marketId,at:new Date(at).toISOString()});
      updates.set(topic,{epoch:row.epoch,ready:false});diagnostics.lifecycleInvalidations++;continue;
    }
    if(['invalidation','disconnected','recovery_started'].includes(row.kind)){
      if(topic===null){invalidated.add(null);breaks.push({marketId:null,at:new Date(at).toISOString()});
        for(const [known,state] of topicStates)updates.set(known,{...state,ready:false});}
      else markInvalid(topic,'lifecycle');
      continue;
    }
    if(row.kind==='recovery_finished'){
      const ready=row.booksReady===true,prior=updates.get(topic)??topicStates.get(topic)??{epoch:null,ready:false};
      updates.set(topic,{...prior,epoch:row.epoch,ready});if(ready)diagnostics.validRecoveryCompletions++;
      continue;
    }
    if(row.kind!=='event'||row.event!=='market_batch')continue;
    diagnostics.events++;
    const state=topicStates.get(topic),payload=row.payload??{},delivery=payload.delivery,revision=delivery?.revision,previous=delivery?.previousRevision;
    if(!state||state.epoch!==row.epoch){markInvalid(topic,'revision');continue;}
    const key=`${topic}#${row.epoch}`;
    if(!Number.isSafeInteger(revision)||revision<0||!Number.isSafeInteger(previous)||previous<0){markInvalid(topic,'revision');continue;}
    const old=lastRevision.get(key);
    if(old!==undefined&&revision<=old){diagnostics.duplicateRevisions++;continue;}
    const gap=old!==undefined&&previous>old;
    lastRevision.set(key,revision);
    if(gap){diagnostics.revisionGaps++;markInvalid(topic,'revision');continue;}
    if(payload.resyncRequired!==false){diagnostics.resyncRequired++;markInvalid(topic,'revision');continue;}
    if(payload.marketSettled?.length){diagnostics.settlements++;markInvalid(topic,'revision');continue;}
    diagnostics.bookDirtyMarkers+=payload.bookDirty?.length??0;
    if((payload.bookDirty??[]).some(dirty=>!(payload.books??[]).some(book=>book.exchangeId===dirty.exchangeId))){diagnostics.dirtyWithoutBook++;markInvalid(topic,'revision');continue;}
    // Full fresh pushed books are usable offline observations even while the
    // consumer awaits its separate depth-200 recovery receipt. Breaks still
    // clear features and censor trades spanning them; execution readiness is
    // not a reason to discard a valid observed depth-20 quote.
    if(!state.ready)diagnostics.booksObservedBeforeFullRecovery++;
    events.push(row);
  }
  const candidates=new Map();
  for(const row of events){
    const marketId=topicMarket(row.topic);
    if(invalidated.has(null)||invalidated.has(marketId))continue;
    const result=engineBookPointsV1(row,scope,row.observedAt);
    diagnostics.scopeRejectedBooks+=result.rejected.length;
    for(const book of result.books){
      const meta=graph.exchanges.get(book.exchangeId);
      if(!meta||meta.marketId!==book.marketId){diagnostics.metadataMissingBooks++;continue;}
      const seq=BigInt(book.sequence),sourceNs=instantNs(book.sourceAt);
      const old=lastBookVersion.get(book.exchangeId);
      if(old&&(seq<old.seq||seq===old.seq&&sourceNs<=old.sourceNs)){diagnostics.duplicateBookVersions++;if(seq<old.seq)diagnostics.rollbackBookVersions++;continue;}
      lastBookVersion.set(book.exchangeId,{seq,sourceNs});
      const point={exchangeId:book.exchangeId,marketId:book.marketId,sequence:String(book.sequence),availableAt:row.observedAt,sourceAt:book.sourceAt,
        nextExpiryAt:book.sourceBinding.nextExpiryAt,bid:Number(book.bids[0]?.price),ask:Number(book.asks[0]?.price),
        bidSize:book.bids[0]?.quantity,askSize:book.asks[0]?.quantity};
      const prior=candidates.get(point.exchangeId);
      if(!prior||seq>BigInt(prior.sequence)||seq===BigInt(prior.sequence)&&point.sourceAt>prior.sourceAt)candidates.set(point.exchangeId,point);
    }
  }
  for(const point of [...candidates.values()].sort((a,b)=>a.exchangeId.localeCompare(b.exchangeId))){observations.push(point);diagnostics.acceptedBookPoints++;}
  for(const [topic,state] of updates)topicStates.set(topic,state);
  if(invalidated.has(null))for(const [topic,state] of topicStates)topicStates.set(topic,{...state,ready:false});
  group=[];
}

for(const line of journalLines(captureFile,{endByte:seal.bytes})){
  const row=JSON.parse(line);
  if(row.format!=='sig-feed-capture'||row.schemaVersion!==1||row.captureIndex!==expectedIndex++)throw Error('LEADLAG_CAPTURE_CHAIN');
  const observed=Date.parse(row.observedAt);if(!Number.isFinite(observed)||observed<lastObserved)throw Error('LEADLAG_CAPTURE_CLOCK');
  lastObserved=observed;firstObserved??=observed;through=row.observedAt;diagnostics.captureRows++;
  if(groupAt!==null&&observed!==groupAt)flushGroup();groupAt=observed;group.push(row);
  if(row.kind!=='event')diagnostics.nonEventRows++;
}
flushGroup();
const current=fs.statSync(captureFile);
if(current.dev!==seal.dev||current.ino!==seal.ino||current.size<seal.bytes)throw Error('LEADLAG_CAPTURE_REPLACED');
const rawHash=await hashNativePrefix(captureFile,seal.bytes,0);
const startMs=Date.parse(session.startedAt),endMs=Date.parse(through),splitAt=new Date(startMs+.7*(endMs-startMs)).toISOString();
const report=analyzeLeadLagV1({observations,breaks,graph,options:{splitAt}});
const result={at:new Date().toISOString(),source:{file:session.file,bytes:seal.bytes,sha256:rawHash.hash,captureRows:diagnostics.captureRows,from:new Date(firstObserved).toISOString(),through},
  metadata:{file:'data/structural-universe-latest.json',sha256:metadataHash,candidatePairRows:metadata.candidatePairs.length,causalCandidatePairRows:causalPairs.length,futureOrInvalidRootPairRows:metadata.candidatePairs.length-causalPairs.length,marketCount:graph.markets.size,raceCount:graph.raceCount,stateGroupCount:graph.stateGroups.length,
    captureScopeMarkets:scope.exchanges.length,marketMetadataCoverage:scope.exchanges.filter(x=>graph.markets.has(String(x.marketId))).length,unmappedCaptureMarkets:scope.exchanges.filter(x=>!graph.markets.has(String(x.marketId))).length},
  captureDiagnostics:diagnostics,...report,results:report.results.map(x=>({...x,selectedMeanNet:x.selectedMeanNet===null?null:Number(x.selectedMeanNet.toFixed(6)),heldoutRawNetMean:x.heldoutRawNetMean===null?null:Number(x.heldoutRawNetMean.toFixed(6)),selectedNetOneShareSum:x.selectedNetOneShareSum===null?null:Number(x.selectedNetOneShareSum.toFixed(6)),developmentSlope:x.developmentSlope===null?null:Number(x.developmentSlope.toFixed(6))}))};
fs.writeFileSync('data/leadlag-edge-replay-20261013.json',JSON.stringify(result,null,2)+'\n');
const top=result.results.filter(x=>x.latencyMs===0&&x.horizonMs===5000&&x.feature.startsWith('raceOpponent:')&&x.selectedTrades>0).sort((a,b)=>b.selectedMeanNet-a.selectedMeanNet).slice(0,8);
console.log(JSON.stringify({source:result.source,metadata:result.metadata,splitAt:result.cutoff,diagnostics:result.diagnostics,captureDiagnostics:result.captureDiagnostics,heldout:result.holdout,rows:result.results.length,positiveRows:result.results.filter(x=>x.selectedNetOneShareSum>0).length,lossRows:result.results.filter(x=>x.selectedNetOneShareSum<0).length,topRaceOpponentFiveSecond:top.map(x=>({latencyMs:x.latencyMs,horizonMs:x.horizonMs,feature:x.feature,developmentLabels:x.developmentLabels,heldoutLabels:x.heldoutLabels,selectedTrades:x.selectedTrades,meanNet:x.selectedMeanNet,selectedSum:x.selectedNetOneShareSum,selectedGroups:x.selectedGroups,positiveSelectedGroups:x.positiveSelectedGroups})),networkRequests:0,contestOrders:0,profitabilityEstablished:false}));
