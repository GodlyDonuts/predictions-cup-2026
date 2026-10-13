import fs from 'node:fs';
import path from 'node:path';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {sealedNativePrefix, hashNativePrefix} from '../live-paper/native-dip-host-v1.mjs';
import {engineBookPointsV1} from '../paper/maker-engine-book-point-v1.mjs';
import {instantNs} from '../feed/contract.mjs';
import {screenComplementBookV1, analyzeSameMarketComplementV1} from '../research/same-market-complement-arb-v1.mjs';

const runtime = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime');
const session = JSON.parse(fs.readFileSync(path.join(runtime, 'realtime-session.json'), 'utf8'));
if (!/^realtime-[A-Za-z0-9.-]+\.jsonl$/.test(session.file) || session.scope?.channelMode !== 'tournament'
  || !Array.isArray(session.scope.markets) || !session.scope.markets.length
  || typeof session.scope.tournamentId !== 'string') throw Error('COMPLEMENT_CAPTURE_BINDING');
const scope = {tournamentId:session.scope.tournamentId, exchanges:session.scope.markets};
const scopeByExchange = new Map(scope.exchanges.map(x => [x.exchangeId,x]));
if (scopeByExchange.size !== scope.exchanges.length) throw Error('COMPLEMENT_CAPTURE_SCOPE_DUPLICATE');
// Reuse the exact immutable prefix sealed by the preceding causal replay, not
// the live session pointer (which may have rolled to a short new capture).
const prior = JSON.parse(fs.readFileSync('data/leadlag-edge-replay-20261013.json','utf8'));
if(prior.mode!=='causal_cross_market_leadlag_quote_replay_v1'||!Number.isSafeInteger(prior.source?.bytes)
  ||!/^realtime-[A-Za-z0-9.-]+\.jsonl$/.test(prior.source.file)||!/^([a-f0-9]{64})$/.test(prior.source.sha256)
  ||prior.metadata?.captureScopeMarkets!==scope.exchanges.length)throw Error('COMPLEMENT_SEALED_MANIFEST');
const file = path.join(runtime, prior.source.file), seal = sealedNativePrefix(file), sealedBytes=prior.source.bytes;
if (!sealedBytes||seal.bytes<sealedBytes) throw Error('COMPLEMENT_CAPTURE_EMPTY');
const initialHash=(await hashNativePrefix(file,sealedBytes,0)).hash;
if(initialHash!==prior.source.sha256)throw Error('COMPLEMENT_CAPTURE_MANIFEST_HASH');
const tailLength = Math.min(sealedBytes, 65536), tail = Buffer.alloc(tailLength), tailFd = fs.openSync(file, 'r');
try {
  let read = 0;
  while (read < tailLength) { const n=fs.readSync(tailFd,tail,read,tailLength-read,sealedBytes-tailLength+read); if(!n)throw Error('COMPLEMENT_CAPTURE_TAIL'); read+=n; }
} finally { fs.closeSync(tailFd); }
const tailLines=tail.toString('utf8').trimEnd().split(/\r?\n/),lastRow=JSON.parse(tailLines.at(-1));
if(lastRow.format!=='sig-feed-capture'||!Number.isSafeInteger(lastRow.captureIndex)||typeof lastRow.observedAt!=='string')throw Error('COMPLEMENT_CAPTURE_TAIL');
const sealedThrough=lastRow.observedAt,sealedThroughNs=instantNs(sealedThrough),captureHash=initialHash;
if(sealedThrough!==prior.source.through||lastRow.captureIndex+1!==prior.source.captureRows)throw Error('COMPLEMENT_CAPTURE_MANIFEST_TAIL');

const topicMarket=topic=>{const m=/^tournament:[0-9a-f-]+:market:(\d+)$/.exec(topic??'');return m?.[1]??null;};
const topicStates=new Map(),lastRevision=new Map(),lastBookVersion=new Map(),breaks=[],observations=[];
const d={captureRows:0,events:0,nonEventRows:0,duplicateRevisions:0,revisionGaps:0,resyncRequired:0,bookDirtyMarkers:0,
  dirtyWithoutBook:0,settlements:0,lifecycleInvalidations:0,validRecoveryCompletions:0,booksBeforeRecovery:0,
  scopeRejectedBooks:0,duplicateBookVersions:0,rollbackBookVersions:0,acceptedBookPoints:0,invalidScreenedBooks:0};
let expectedIndex=0,lastObserved=-Infinity,firstObserved=null,through=null,groupAt=null,group=[];

function flushGroup(){
  if(!group.length)return;
  const atMs=groupAt,atIso=new Date(atMs).toISOString(),invalidated=new Set(),updates=new Map(),events=[];
  const markInvalid=(topic,kind)=>{
    const marketId=topicMarket(topic);invalidated.add(marketId);
    breaks.push({marketId,at:atIso});
    const prior=updates.get(topic)??topicStates.get(topic)??{epoch:null,ready:false};
    updates.set(topic,{...prior,ready:false}); if(kind==='lifecycle')d.lifecycleInvalidations++;
  };
  for(const row of group){
    const topic=row.topic??null,marketId=topicMarket(topic);
    if(row.kind==='subscribed'){
      invalidated.add(marketId); if(marketId!==null)breaks.push({marketId,at:atIso});
      updates.set(topic,{epoch:row.epoch,ready:false}); d.lifecycleInvalidations++; continue;
    }
    if(['invalidation','disconnected','recovery_started'].includes(row.kind)){
      if(topic===null){invalidated.add(null);breaks.push({marketId:null,at:atIso});
        for(const [known,state] of topicStates)updates.set(known,{...state,ready:false});}
      else markInvalid(topic,'lifecycle');
      continue;
    }
    if(row.kind==='recovery_finished'){
      const ready=row.booksReady===true,prior=updates.get(topic)??topicStates.get(topic)??{epoch:null,ready:false};
      updates.set(topic,{...prior,epoch:row.epoch,ready}); if(ready)d.validRecoveryCompletions++; continue;
    }
    if(row.kind!=='event'||row.event!=='market_batch')continue;
    d.events++;
    const state=topicStates.get(topic),payload=row.payload??{},delivery=payload.delivery,revision=delivery?.revision,previous=delivery?.previousRevision;
    if(!state||state.epoch!==row.epoch){markInvalid(topic,'revision');continue;}
    const key=`${topic}#${row.epoch}`;
    if(!Number.isSafeInteger(revision)||revision<0||!Number.isSafeInteger(previous)||previous<0){markInvalid(topic,'revision');continue;}
    const old=lastRevision.get(key);
    if(old!==undefined&&revision<=old){d.duplicateRevisions++;continue;}
    const gap=old!==undefined&&previous>old; lastRevision.set(key,revision);
    if(gap){d.revisionGaps++;markInvalid(topic,'revision');continue;}
    if(payload.resyncRequired!==false){d.resyncRequired++;markInvalid(topic,'revision');continue;}
    if(payload.marketSettled?.length){d.settlements++;markInvalid(topic,'revision');continue;}
    d.bookDirtyMarkers+=payload.bookDirty?.length??0;
    if((payload.bookDirty??[]).some(x=>!(payload.books??[]).some(b=>b.exchangeId===x.exchangeId))){d.dirtyWithoutBook++;markInvalid(topic,'revision');continue;}
    if(!state.ready)d.booksBeforeRecovery++;
    events.push(row);
  }
  const candidates=new Map();
  for(const row of events){
    const marketId=topicMarket(row.topic);
    if(invalidated.has(null)||invalidated.has(marketId))continue;
    const result=engineBookPointsV1(row,scope,row.observedAt); d.scopeRejectedBooks+=result.rejected.length;
    for(const book of result.books){
      const member=scopeByExchange.get(book.exchangeId);
      if(!member||member.marketId!==book.marketId){d.scopeRejectedBooks++;continue;}
      const sequence=BigInt(book.sequence),sourceNs=instantNs(book.sourceAt),old=lastBookVersion.get(book.exchangeId);
      if(old&&(sequence<old.sequence||sequence===old.sequence&&sourceNs<=old.sourceNs)){
        d.duplicateBookVersions++; if(sequence<old.sequence)d.rollbackBookVersions++; continue;
      }
      lastBookVersion.set(book.exchangeId,{sequence,sourceNs});
      try {
        const point=screenComplementBookV1({marketId:book.marketId,exchangeId:book.exchangeId,sequence:String(book.sequence),
          availableAt:row.observedAt,sourceAt:book.sourceAt,nextExpiryAt:book.sourceBinding.nextExpiryAt,bids:book.bids,asks:book.asks});
        const prior=candidates.get(book.exchangeId);
        if(!prior||sequence>BigInt(prior.sequence)||sequence===BigInt(prior.sequence)&&point.sourceAt>prior.sourceAt)candidates.set(book.exchangeId,point);
      } catch { d.invalidScreenedBooks++; }
    }
  }
  for(const point of [...candidates.values()].sort((a,b)=>a.exchangeId.localeCompare(b.exchangeId))){observations.push(point);d.acceptedBookPoints++;}
  for(const [topic,state] of updates)topicStates.set(topic,state);
  if(invalidated.has(null))for(const [topic,state] of topicStates)topicStates.set(topic,{...state,ready:false});
  group=[];
}

for(const line of journalLines(file,{endByte:sealedBytes})){
  const row=JSON.parse(line);
  if(row.format!=='sig-feed-capture'||row.schemaVersion!==1||row.captureIndex!==expectedIndex++)throw Error('COMPLEMENT_CAPTURE_CHAIN');
  const observed=Date.parse(row.observedAt); if(!Number.isFinite(observed)||observed<lastObserved)throw Error('COMPLEMENT_CAPTURE_CLOCK');
  lastObserved=observed; firstObserved??=observed; through=row.observedAt; d.captureRows++;
  if(groupAt!==null&&observed!==groupAt)flushGroup(); groupAt=observed; group.push(row);
  if(row.kind!=='event')d.nonEventRows++;
}
flushGroup();
const current=fs.statSync(file);
if(current.dev!==seal.dev||current.ino!==seal.ino||current.size<sealedBytes)throw Error('COMPLEMENT_CAPTURE_REPLACED');
if((await hashNativePrefix(file,sealedBytes,0)).hash!==captureHash)throw Error('COMPLEMENT_CAPTURE_PREFIX_CHANGED');
if(!firstObserved||instantNs(through)>sealedThroughNs)throw Error('COMPLEMENT_CAPTURE_BOUND');

const startMs=Date.parse(prior.source.from),endMs=Date.parse(through);
if(!Number.isFinite(startMs)||endMs<=startMs)throw Error('COMPLEMENT_CAPTURE_TIME_RANGE');
const splitAt=new Date(startMs+.7*(endMs-startMs)).toISOString();
const replay=analyzeSameMarketComplementV1({observations,breaks,options:{splitAt}});
const report={schemaVersion:1,at:new Date().toISOString(),source:{kind:'existing_sealed_realtime_capture',file:prior.source.file,bytes:sealedBytes,
  sha256:captureHash,captureRows:d.captureRows,from:new Date(firstObserved).toISOString(),through,
  tournamentId:session.scope.tournamentId,scopedMarketExchanges:session.scope.markets.length},captureDiagnostics:d,...replay,
  contractSemantics:{
    identity:'Each YES/NO pair is grouped by exact exchangeId and marketId; unrelated markets or contracts never offset each other.',
    marketBinary:'YES and NO are opposite sides of one binary question; a winning share pays 1 SUSQie.',
    oppositePositionNetting:'Buying one opposite-side share cancels one held share and credits 1 SUSQie immediately; an uncompleted second leg receives no pair credit.',
    selfTrade:'An order cannot fill against the same account\'s opposite resting order; another compatible order is required.',
    modeledFees:'0.000000 SUSQies per share, consistent with this challenge’s fee-free test assumption. The public product guide does not state a separate per-fill fee schedule.',
    officialSources:['https://sig.thesuper.market/docs/markets-and-trading','https://sig.thesuper.market/docs/settlement-and-payouts','https://sig.thesuper.market/'],
    localRestContractEvidence:'research/paired-rest-contract-20261006.json',
  },
  externalResearchSources:[
    'https://sig.thesuper.market/docs/markets-and-trading',
    'https://sig.thesuper.market/docs/settlement-and-payouts',
    'research/paired-rest-contract-20261006.json',
  ],noAccountStateRead:true,networkRequests:0,contestOrders:0,profitabilityEstablished:false};
fs.writeFileSync('data/same-market-complement-arb-20261013.json',JSON.stringify(report,null,2)+'\n',{flag:'w'});
const heldout=replay.diagnostics.filter(x=>x.period==='heldout'),summary={source:report.source,splitAt:report.splitAt,
  acceptedBookPoints:replay.observations.accepted,markets:replay.observations.markets,contracts:replay.observations.contracts,
  atomic:replay.atomic,heldout:heldout.map(x=>({firstSide:x.firstSide,firstLegLatencyMs:x.firstLegLatencyMs,secondLegLatencyMs:x.secondLegLatencyMs,
    decisions:x.decisionCount,completedPairs:x.completedPairs,meanPairNet:x.pairNetPnlPerShare.mean,
    meanUnhedgedAtHedge:x.firstLegUnhedgedLiquidationPnlAtHedge.mean,meanWorstUnhedged:x.firstLegWorstObservedLiquidationPnlBeforeHedge.mean,
    hedgeBookDelayMs:x.secondLegObservedDelayMs.mean,depthCensored:x.firstLegDepthCensored+x.secondLegDepthCensored,
    delayCensored:x.firstLegBookDelayCensored+x.secondLegBookDelayCensored,breakCensored:x.breakCensored})),
  note:'These are one-share displayed-quote counterfactuals, not actual orders/fills or profitability evidence.',networkRequests:0,contestOrders:0};
console.log(JSON.stringify(summary));
