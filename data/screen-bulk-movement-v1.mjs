import fs from 'node:fs';import path from 'node:path';import readline from 'node:readline';import {createHash} from 'node:crypto';
import {sealedNativePrefix} from '../live-paper/native-dip-host-v1.mjs';import {bulkMovementFocusV1} from '../research/bulk-movement-focus-v1.mjs';import {atomicJson} from '../execution/ledger.mjs';
const begun=performance.now(),rt=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),file=path.join(rt,'market-prices.jsonl'),seal=sealedNativePrefix(file),universe=JSON.parse(fs.readFileSync('live-paper/price-universe.json')),scope=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId]));
const now=new Date().toISOString(),clock=Date.parse(now),histories=new Map(),hash=createHash('sha256'),stream=fs.createReadStream(file,{end:seal.bytes-1});stream.on('data',b=>hash.update(b));let sequence=0,previous='',lastReceived=-Infinity;
for await(const line of readline.createInterface({input:stream,crlfDelay:Infinity})){
 const row=JSON.parse(line),{hash:expected,...body}=row;
 if(row.sequence!==++sequence||row.previous!==previous||createHash('sha256').update(JSON.stringify(body)).digest('hex')!==expected)throw Error('MOVEMENT_SOURCE_CHAIN');previous=expected;
 if(row.event.type!=='market_price_observation')continue;
 const s=row.event.snapshot,at=Date.parse(s.receivedAt),start=Date.parse(s.requestStartedAt);
 if(s.tournamentId!==universe.tournamentId||!Number.isFinite(at)||!Number.isFinite(start)||at<lastReceived||start>at||at-start>20000||at>clock)throw Error('MOVEMENT_SOURCE_CLOCK_SCOPE');lastReceived=at;
 if(at<clock-3600000)continue;
 for(const q of s.quotes){
  if(scope.get(q.marketId)!==q.exchangeId)throw Error('MOVEMENT_SOURCE_INSTRUMENT');
  if(q.crossed||q.option!=='YES'||![q.bestBid,q.bestAsk].every(x=>typeof x==='number'&&Number.isFinite(x))||q.bestBid<=0||q.bestAsk>=1||q.bestBid>=q.bestAsk)continue;
  const h=histories.get(q.marketId)??{marketId:q.marketId,exchangeId:q.exchangeId,points:[]};h.points.push({received:at,bid:q.bestBid.toFixed(6),ask:q.bestAsk.toFixed(6),hash:row.hash});histories.set(q.marketId,h);
 }
}
const stat=fs.statSync(file);if(stat.ino!==seal.ino||stat.dev!==seal.dev||stat.size<seal.bytes)throw Error('MOVEMENT_SOURCE_REPLACED');
const result=bulkMovementFocusV1({histories:[...histories.values()],universe,now});
const report={...result,createdAt:new Date().toISOString(),durationMs:performance.now()-begun,source:{bytes:seal.bytes,sha256:hash.digest('hex'),records:sequence,lastHash:previous},apiRequests:0,originalWalletsModified:false};atomicJson('data/bulk-movement-focus-latest.json',report);
console.log(JSON.stringify({durationMs:report.durationMs,sourceRecords:sequence,eligibleHistoryMarkets:result.rows.length,candidates:result.candidates.slice(0,10),actualOrders:0}));
