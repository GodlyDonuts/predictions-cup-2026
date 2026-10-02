// Risk monitoring skips only caller idle time. The single API client still
// enforces its durable admission reservation and any Retry-After cooldown.
export function collectionCycleWait({cycleStartedAt,now,cycleIntervalMs,managedMarketIds=[]}){
 if(!Number.isSafeInteger(cycleStartedAt)||!Number.isSafeInteger(now)||now<cycleStartedAt
  ||!Number.isSafeInteger(cycleIntervalMs)||cycleIntervalMs<1000||!Array.isArray(managedMarketIds)
  ||managedMarketIds.some(x=>typeof x!=='string'||!/^([1-9]\d*)$/.test(x)||!Number.isSafeInteger(Number(x))))throw Error('INVALID_COLLECTION_CYCLE_CLOCK_OR_RISK');
 return {mode:managedMarketIds.length?'owned_risk':'normal',delayMs:managedMarketIds.length?0:Math.max(1000,cycleIntervalMs-(now-cycleStartedAt))};
}
