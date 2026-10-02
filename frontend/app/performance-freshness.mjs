const marks=['observedLiquidationValue','observedLiquidationUnrealizedPnl','observedLiquidationTotalPnl','midpointValue','midpointUnrealizedPnl','midpointTotalPnl'];
const money=value=>typeof value==='string'&&/^-?\d+\.\d{6}$/.test(value);
const fresh=(value,now,maxAge)=>typeof value==='string'&&Number.isFinite(Date.parse(value))&&now>=Date.parse(value)&&now-Date.parse(value)<=maxAge;

// Reapply execution-view.mjs freshness bounds at render time. Repainting never
// changes source clocks or turns a server-rejected projection into fresh data.
export function ageExecutionPerformance(performance,now=Date.now()){
 const validClock=Number.isFinite(now),available=performance?.available===true&&validClock
  &&fresh(performance.asOf,now,10000)&&fresh(performance.updatedAt,now,10000);
 // A verified flat portfolio has no quote timestamp and needs no depth mark.
 const quotesFresh=available&&(fresh(performance.quoteAt,now,20000)
  ||performance.quoteAt===null&&performance.ownedCost==='0.000000');
 const view={...performance,available,quotesFresh,ownedCost:money(performance?.ownedCost)?performance.ownedCost:null};
 for(const key of marks)if(!quotesFresh||!money(view[key]))view[key]=null;
 return view;
}
