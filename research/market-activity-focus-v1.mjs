// Research attention uses actual anonymous trade clocks, not cumulative counts
// or changing engine sequence numbers. No fill probability is inferred here.
export function marketActivityFocusV1({markets,universe,now,slots=3,maximumReceiptAgeMs=600000,recentTradeWindowMs=1800000,protectedMarketIds=[]}) {
 const clock=Date.parse(now);
 if(!Number.isFinite(clock)||!Array.isArray(markets)||!Array.isArray(universe?.exchanges)||!Number.isSafeInteger(slots)||slots<0||slots>3||![maximumReceiptAgeMs,recentTradeWindowMs].every(x=>Number.isSafeInteger(x)&&x>0))throw Error('ACTIVITY_FOCUS_INPUT');
 const scope=new Map(universe.exchanges.map(x=>[x.marketId,x.exchangeId])),seen=new Set(),protectedIds=new Set(protectedMarketIds),rows=[];
 for(const m of markets){
  if(scope.get(m.marketId)!==m.exchangeId||seen.has(m.marketId))throw Error('ACTIVITY_FOCUS_SCOPE');
  seen.add(m.marketId);
  const receipt=Date.parse(m.lastReceiptAt),trade=Date.parse(m.latestTradeAt),through=Date.parse(m.tailCompletedTo);
  const malformed=(m.lastReceiptAt!=null&&!Number.isFinite(receipt))||(m.latestTradeAt!=null&&!Number.isFinite(trade));
  const fresh=!malformed&&Number.isFinite(receipt)&&receipt<=clock&&clock-receipt<=maximumReceiptAgeMs;
  if(Number.isFinite(trade)&&trade>clock||Number.isFinite(receipt)&&receipt>clock)throw Error('ACTIVITY_FOCUS_FUTURE');
  const recent=Number.isFinite(trade)&&clock-trade<=recentTradeWindowMs;
  const status=!fresh?'observation_stale_or_missing':recent?'recent_trade_seen':'no_recent_trade_seen';
  rows.push({marketId:m.marketId,exchangeId:m.exchangeId,status,lastReceiptAt:m.lastReceiptAt??null,latestTradeAt:m.latestTradeAt??null,tradeAgeMs:Number.isFinite(trade)?clock-trade:null,reportedCoverageComplete:m.reportedCoverageComplete===true,tailGapCount:m.tailGapCount??null,tailCompletedTo:Number.isFinite(through)?m.tailCompletedTo:null,eligibleForResearch:status==='recent_trade_seen'&&!protectedIds.has(m.marketId)});
 }
 const selected=rows.filter(r=>r.eligibleForResearch).sort((a,b)=>a.tradeAgeMs-b.tradeAgeMs||a.marketId.localeCompare(b.marketId)).slice(0,slots);
 return {at:now,rows,selected,slots,executionAuthority:false,additionalApiClients:0,actualOrders:0,observedTradeActivityIsNotProfitability:true,missingRecentTradeIsNotProofOfNoTrading:true,queuePriorityKnown:false,fillProbabilityEstimated:false};
}
