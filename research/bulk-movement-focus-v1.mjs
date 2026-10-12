import {bulkHistoryFeatureV1} from './bulk-history-feature-v1.mjs';
import {amount,price,money,SCALE,TICK} from '../paper/fixed.mjs';

export function bulkMovementFocusV1({histories,universe,now,minimumGap='0.010000'}) {
 const clock=Date.parse(now),gap=amount(minimumGap),scope=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId]));
 if(!Number.isFinite(clock)||gap<TICK||!Array.isArray(histories))throw Error('MOVEMENT_FOCUS_INPUT');
 const policy={lookbackMs:3600000,minimumPriorBooks:12,minimumHistorySpanMs:1800000,maximumBulkAgeMs:120000,maximumBulkGapMs:120000},rows=[],seen=new Set();
 for(const h of histories){
  if(scope.get(h.marketId)!==h.exchangeId||seen.has(h.marketId))throw Error('MOVEMENT_FOCUS_SCOPE');seen.add(h.marketId);
  if(h.points.some((p,i)=>!Number.isFinite(p.received)||p.received>clock||i>0&&p.received<h.points[i-1].received))throw Error('MOVEMENT_FOCUS_CLOCK');
  for(const p of h.points)if(price(p.bid)<=0n||price(p.ask)>=SCALE||price(p.bid)>=price(p.ask))throw Error('MOVEMENT_FOCUS_QUOTE');
  const feature=bulkHistoryFeatureV1(h.points,clock,policy);
  if(!feature)continue;
  const latest=h.points.at(-1),median=amount(feature.medianYesMid),bid=amount(latest.bid),ask=amount(latest.ask);
  if(bid<=0n||ask>=SCALE||bid>=ask)continue;
  const yesGap=median-ask-TICK,noGap=bid-median-TICK;
  const side=yesGap>=noGap?'YES':'NO',best=yesGap>=noGap?yesGap:noGap;
  rows.push({marketId:h.marketId,exchangeId:h.exchangeId,side,medianYesMid:feature.medianYesMid,lastBulkAt:feature.lastBulkAt,firstBulkAt:feature.firstBulkAt,observations:feature.observations,protectedEntry:money(side==='YES'?ask+TICK:SCALE-bid+TICK),historyGapAfterEntryStress:money(best),qualifiesForDepthResearch:best>=gap,spread:money(ask-bid)});
 }
 rows.sort((a,b)=>amount(a.historyGapAfterEntryStress)>amount(b.historyGapAfterEntryStress)?-1:amount(a.historyGapAfterEntryStress)<amount(b.historyGapAfterEntryStress)?1:a.marketId.localeCompare(b.marketId));
 return{at:now,policy,minimumGap,rows,candidates:rows.filter(r=>r.qualifiesForDepthResearch),historicalMedianIsNotFairValue:true,executableDepthAvailable:false,profitableTradesProven:false,actualOrders:0};
}
