// A process heartbeat and market-data freshness describe different failures.
export function workerHealth(report, now=Date.now(), maxHeartbeatAgeMs=60000){
 const timestamp=report?.updatedAt||report?.reportedAt,at=Date.parse(timestamp),age=now-at;
 if(!Number.isFinite(age)||age<0)return {status:'invalid_heartbeat',heartbeatAgeMs:null};
 if(age>maxHeartbeatAgeMs)return {status:'stale_worker',heartbeatAgeMs:age};
 return {status:report.status||'unknown',heartbeatAgeMs:age};
}
export function baselineView(baseline,legacy,runtime,now=Date.now()){
 if(baseline?.config?.feeBps!==0||baseline.mode!=='prospective_fee_free_paper_baseline')throw Error('Invalid fee-free baseline');
 const health=workerHealth(baseline,now),bookAge=now-Date.parse(baseline.lastObservationAt);
 return {...baseline,baselineWorkerStatus:health.status,baselineHeartbeatAgeMs:health.heartbeatAgeMs,
  baselineBookAgeMs:baseline.observations&&Number.isFinite(bookAge)?bookAge:null,
  baselineDataStatus:!baseline.observations?'awaiting_first_accepted_book':baseline.dataHealth?.invalidated?'invalidated_market_data':!Number.isFinite(bookAge)||bookAge<0||baseline.dataHealth?.invalidClocks?'invalid_market_clock':bookAge>baseline.config.maxDataAgeMs?'stale_market_data':baseline.dataHealth&&baseline.dataHealth.coveredMarkets<baseline.dataHealth.expectedMarkets?'partial_market_coverage':'fresh_market_data',
  legacyFeeStress:{feeBps:legacy.config.feeBps,runId:legacy.runId,strategies:legacy.strategies},runtime};
}
