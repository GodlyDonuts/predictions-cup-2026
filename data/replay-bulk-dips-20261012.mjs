import fs from 'node:fs';import path from 'node:path';import readline from 'node:readline';import {createHash} from 'node:crypto';
import {validateUniverse} from '../live-paper/breadth.mjs';
import {sealedNativePrefix} from '../live-paper/native-dip-host-v1.mjs';
import {bulkHistoryFeatureV1} from '../research/bulk-history-feature-v1.mjs';
import {price,amount,money,SCALE,TICK} from '../paper/fixed.mjs';import {atomicJson} from '../execution/ledger.mjs';
const began=performance.now(),rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),file=path.join(rt,'market-prices.jsonl'),seal=sealedNativePrefix(file);
const rawUniverse=JSON.parse(fs.readFileSync('live-paper/price-universe.json')),universe=validateUniverse(rawUniverse,rawUniverse.tournamentId),scope=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId]));
const config={start:'2026-10-12T09:03:00.000Z',minimumGap:'0.010000',horizonMs:1800000,decisionSpacingMs:300000,maximumExitLatenessMs:120000,entryStressTicks:1,exitStressTicks:1,fees:0,nonOverlappingPerMarket:true};
const policy={lookbackMs:3600000,minimumPriorBooks:12,minimumHistorySpanMs:1800000,maximumBulkAgeMs:120000,maximumBulkGapMs:120000};
const start=Date.parse(config.start),histories=new Map(),pending=new Map(),nextDecision=new Map(),outcomes=[];let sequence=0,previous='',lastReceived=-Infinity,eligible=0,invalidQuotes=0;
const hash=createHash('sha256');const stream=fs.createReadStream(file,{end:seal.bytes-1});stream.on('data',b=>hash.update(b));
atomicJson(path.join(rt,'dip-replay-20261012-manifest.json'),{createdAt:new Date().toISOString(),config,policy,sourceBytes:seal.bytes,codeHash:createHash('sha256').update(fs.readFileSync('data/replay-bulk-dips-20261012.mjs')).digest('hex'),actualOrders:0});
for await(const line of readline.createInterface({input:stream,crlfDelay:Infinity})){
 const row=JSON.parse(line),{hash:expected,...body}=row;
 if(row.sequence!==++sequence||row.previous!==previous||createHash('sha256').update(JSON.stringify(body)).digest('hex')!==expected)throw Error('DIP_REPLAY_SOURCE_CHAIN');previous=expected;
 if(row.event.type!=='market_price_observation')continue;
 const s=row.event.snapshot,at=Date.parse(s.receivedAt),request=Date.parse(s.requestStartedAt);
 if(s.tournamentId!==universe.tournamentId||s.universeHash!==universe.hash||!Number.isFinite(at)||!Number.isFinite(request)||at<lastReceived||request>at||at-request>20000)throw Error('DIP_REPLAY_SOURCE_SCOPE_CLOCK');lastReceived=at;
 if(at<start-policy.lookbackMs)continue;
 for(const q of s.quotes){
  if(scope.get(q.marketId)!==q.exchangeId)throw Error('DIP_REPLAY_INSTRUMENT');
  if(q.crossed||q.option!=='YES'||![q.bestBid,q.bestAsk].every(x=>typeof x==='number'&&Number.isFinite(x))||q.bestBid<=0||q.bestAsk>=1||q.bestBid>=q.bestAsk){invalidQuotes++;continue;}
  const bid=price(q.bestBid.toFixed(6)),ask=price(q.bestAsk.toFixed(6));
  const owned=pending.get(q.marketId);
  if(owned&&at>=owned.due){
   const exit=owned.side==='YES'?bid-TICK:SCALE-ask-TICK;
   outcomes.push({...owned,exitAt:new Date(at).toISOString(),exitSourceHash:row.hash,exitLatenessMs:at-owned.due,quotePnl:at-owned.due<=config.maximumExitLatenessMs&&exit>=TICK?money(exit-price(owned.entry)):null});pending.delete(q.marketId);
  }
  let history=histories.get(q.marketId)??[];history=history.filter(p=>p.received>=request-policy.lookbackMs);
  if(at>=start&&!pending.has(q.marketId)&&at>=(nextDecision.get(q.marketId)??0)){
   nextDecision.set(q.marketId,at+config.decisionSpacingMs);
   const feature=bulkHistoryFeatureV1(history,request-1,policy);
   if(feature){eligible++;const median=amount(feature.medianYesMid),yesGap=median-ask-TICK,noGap=bid-median-TICK,side=yesGap>=noGap?'YES':'NO',gap=yesGap>=noGap?yesGap:noGap;
    if(gap>=price(config.minimumGap))pending.set(q.marketId,{marketId:q.marketId,exchangeId:q.exchangeId,side,entry:money(side==='YES'?ask+TICK:SCALE-bid+TICK),signalAt:new Date(at).toISOString(),due:at+config.horizonMs,sourceHash:row.hash,priorLastAt:feature.lastBulkAt,median:feature.medianYesMid,gap:money(gap)});
   }
  }
  history.push({received:at,bid:money(bid),ask:money(ask),hash:row.hash});histories.set(q.marketId,history);
 }
}
const stat=fs.statSync(file);if(stat.ino!==seal.ino||stat.dev!==seal.dev||stat.size<seal.bytes)throw Error('DIP_REPLAY_SOURCE_REPLACED');
const labeled=outcomes.filter(x=>x.quotePnl!==null),pnl=labeled.reduce((n,x)=>n+amount(x.quotePnl),0n);
const report={createdAt:new Date().toISOString(),config,policy,durationMs:performance.now()-began,source:{bytes:seal.bytes,sha256:hash.digest('hex'),records:sequence,lastHash:previous,lastAt:new Date(lastReceived).toISOString()},eligibleDecisions:eligible,signals:outcomes.length+pending.size,labeled: labeled.length,positive:labeled.filter(x=>amount(x.quotePnl)>0n).length,negative:labeled.filter(x=>amount(x.quotePnl)<0n).length,totalOneShareQuotePnl:money(pnl),outcomes,pending:[...pending.values()],invalidQuotes,executableDepthAvailable:false,latencyAndFillNotModeled:true,capitalAndCorrelationNotModeled:true,indicativeQuotesAreNotTrades:true,profitabilityEstablished:false,actualOrders:0,apiRequests:0,originalWalletsModified:false};
atomicJson('data/dip-replay-20261012.json',report);console.log(JSON.stringify({durationMs:report.durationMs,sourceRecords:sequence,eligible,signals:report.signals,labeled:report.labeled,positive:report.positive,negative:report.negative,totalOneShareQuotePnl:report.totalOneShareQuotePnl}));

