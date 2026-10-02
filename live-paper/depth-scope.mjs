export function validateDepthPlan(plan,c,universe){
 if(plan?.tournamentId!==c.tournamentId||!Array.isArray(plan.marketIds)||plan.marketIds.length>2||new Set(plan.marketIds).size!==plan.marketIds.length)throw Error('Invalid supplemental depth plan');
 for(const id of plan.marketIds){
  const exchanges=universe.exchanges.filter(x=>x.marketId===id);
  if(typeof id!=='string'||!/^([1-9]\d*)$/.test(id)||c.marketIds.includes(id)||exchanges.length!==1||exchanges[0].option!=='YES')throw Error('Unsupported supplemental depth market');
 }
 return plan;
}
export function scopedSnapshots(event,c){
 const core=event.sourceSnapshots||[],extra=event.supplementalDepth||[];
 if(!Array.isArray(core)||!Array.isArray(extra))throw Error('Invalid depth capture arrays');
 return [...core,...extra].filter(s=>{
  const id=s.skipped?s.marketId:/^\/markets\/(\d+)\/orderbook\?/.exec(s.endpoint||'')?.[1];
  return c.marketIds.includes(id);
 });
}
