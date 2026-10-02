// A process heartbeat and market-data freshness describe different failures.
export function workerHealth(report, now=Date.now(), maxHeartbeatAgeMs=60000){
 const timestamp=report?.updatedAt||report?.reportedAt,at=Date.parse(timestamp),age=now-at;
 if(!Number.isFinite(age)||age<0)return {status:'invalid_heartbeat',heartbeatAgeMs:null};
 if(age>maxHeartbeatAgeMs)return {status:'stale_worker',heartbeatAgeMs:age};
 return {status:report.status||'unknown',heartbeatAgeMs:age};
}
function assessMarketClocks(marketClocks,requiredMarketIds,heldInstrumentIds,now,maxAgeMs){
 const books=marketClocks.map(b=>{const age=now-Date.parse(b.receivedAt);return {...b,ageMs:Number.isFinite(age)&&age>=0?age:null,status:!Number.isFinite(age)||age<0?'invalid_clock':b.invalidated?'invalidated':age>maxAgeMs?'stale':'fresh'};});
 const priority={invalid_clock:5,invalidated:4,stale:3,missing:2,fresh:1};
 const markets=requiredMarketIds.map(marketId=>{const matches=books.filter(b=>b.marketId===marketId).sort((a,b)=>priority[b.status]-priority[a.status]);return {marketId,status:matches[0]?.status??'missing'};});
 const held=heldInstrumentIds.map(instrument=>({instrument,status:books.find(b=>b.instrument===instrument)?.status??'missing'}));
 const ages=books.map(b=>b.ageMs).filter(x=>x!==null);
 return {marketClocks:marketClocks.map(b=>({...b})),requiredMarketIds:[...requiredMarketIds],heldInstrumentIds:[...heldInstrumentIds],asOf:new Date(now).toISOString(),maxDataAgeMs:maxAgeMs,markets,held,
  invalidated:books.filter(b=>b.status==='invalidated').length,invalidClocks:books.filter(b=>b.status==='invalid_clock').length,
  coveredMarkets:markets.filter(x=>x.status==='fresh').length,everObservedMarkets:new Set(books.map(b=>b.marketId).filter(Boolean)).size,expectedMarkets:requiredMarketIds.length,
  staleMarkets:markets.filter(x=>x.status==='stale').length,missingMarkets:markets.filter(x=>x.status==='missing').length,
  staleHeldInstruments:held.filter(x=>x.status==='stale').length,missingHeldInstruments:held.filter(x=>x.status==='missing').length,
  oldestBookAgeMs:ages.length?Math.max(...ages):null};
}
export function marketDataHealth(wallet,config,at){
 const now=typeof at==='number'?at:Date.parse(at),heldInstrumentIds=[...new Set(Object.values(wallet.strategies??{}).flatMap(s=>Object.keys(s.positions??{})))];
 const requiredMarketIds=[...new Set([...(config.marketIds??[]),...heldInstrumentIds.map(id=>wallet.books?.[id]?.marketId).filter(Boolean)])];
 const marketClocks=Object.entries(wallet.books??{}).filter(([id,b])=>requiredMarketIds.includes(b.marketId)||heldInstrumentIds.includes(id)).map(([instrument,b])=>({instrument,marketId:b.marketId,receivedAt:b.receivedAt,invalidated:!!wallet.invalidated?.[instrument]}));
 return assessMarketClocks(marketClocks,requiredMarketIds,heldInstrumentIds,now,config.maxDataAgeMs);
}
export function baselineView(baseline,legacy,runtime,now=Date.now()){
 if(baseline?.config?.feeBps!==0||baseline.mode!=='prospective_fee_free_paper_baseline')throw Error('Invalid fee-free baseline');
 const health=workerHealth(baseline,now),bookAge=now-Date.parse(baseline.lastObservationAt),verified=Array.isArray(baseline.dataHealth?.marketClocks);
 const dataHealth=verified?assessMarketClocks(baseline.dataHealth.marketClocks,baseline.dataHealth.requiredMarketIds,baseline.dataHealth.heldInstrumentIds,now,baseline.config.maxDataAgeMs):baseline.dataHealth;
 const dataStatus=!baseline.observations?'awaiting_first_accepted_book':!Number.isFinite(bookAge)||bookAge<0||dataHealth?.invalidClocks?'invalid_market_clock':dataHealth?.invalidated?'invalidated_market_data':bookAge>baseline.config.maxDataAgeMs||dataHealth?.staleMarkets||dataHealth?.staleHeldInstruments?'stale_market_data':dataHealth?.missingHeldInstruments||dataHealth&&dataHealth.coveredMarkets<dataHealth.expectedMarkets?'partial_market_coverage':verified?'fresh_market_data':'unverified_market_coverage';
 return {...baseline,dataHealth,baselineWorkerStatus:health.status,baselineHeartbeatAgeMs:health.heartbeatAgeMs,
  baselineBookAgeMs:baseline.observations?(verified?dataHealth.oldestBookAgeMs:Number.isFinite(bookAge)&&bookAge>=0?bookAge:null):null,
  baselineDataStatus:dataStatus,
  legacyFeeStress:{feeBps:legacy.config.feeBps,runId:legacy.runId,strategies:legacy.strategies},runtime};
}
