export const paperCohorts=[['dip-responsive','Responsive dip'],['dip-scale','Scaled dip'],['dip','Conservative dip']];
export function paperWalletView(d,id,name){
 if(!d?.strategy)throw Error('Unavailable');const s=d.strategy;
 return {id,name,available:true,updatedAt:d.updatedAt,status:d.workerHealth?.status??d.status,collectorStatus:d.collectorStatus,observations:d.observations,markets:d.universeSize,histories:d.historiesAvailable,feeBps:d.feeBps,
  metrics:{cash:s.cash,realizedPnl:s.realizedPnl,referencePnl:s.referencePnl,referenceEquity:s.referenceEquity,liquidationPnl:s.liquidationPnl,equity:s.estimatedLiquidationEquity,fills:s.fills,fees:s.fees,drawdown:s.maxObservedDrawdown},
  positions:(s.positions??[]).map(x=>({instrument:x.instrument,label:x.label,quantity:x.quantity,cost:x.cost,enteredAt:x.enteredAt,observationAgeMs:x.observationAgeMs??null,quoteReceivedAt:x.quoteReceivedAt??null,quoteAgeMs:x.quoteAgeMs??x.observationAgeMs??null,quoteInvalidated:x.quoteInvalidated===true,unsoldShares:x.unsoldShares??null,estimatedProceeds:x.estimatedProceeds??null,liquidationValue:x.quoteInvalidated||x.observationAgeMs==null||x.observationAgeMs<0||x.unsoldShares!==0?null:x.estimatedProceeds})),
  trades:(s.recentFills??[]).map(x=>({instrument:x.instrument,label:x.label,action:x.action,quantity:x.quantity,price:x.price,fee:x.fee,at:x.at})),
  pending:(s.pending??[]).map(x=>({instrument:x.instrument,label:x.label,action:x.action,quantity:x.quantity,limit:x.limit,createdAt:x.createdAt})),performance:d.performance?{closedRoundTrips:d.performance.closedRoundTrips}:null};
}
export function walletConnection(wallet,now=Date.now()){
 if(!wallet?.available)return {healthy:false,label:wallet?.official?'Account unavailable':'Worker unavailable'};
 if(wallet.official){
  // Recheck all source clocks in the browser as a stalled response can age out.
  const keys=['positions','orders','fills','pnl'],ages=keys.map(k=>wallet.sourceTimes?.[k]==null?null:now-Date.parse(wallet.sourceTimes[k]));
  const status=ages.some(age=>age!==null&&(!Number.isFinite(age)||age<0))?'invalid_clock':ages.some(age=>age===null)?'partial':Object.keys(wallet.errors??{}).length||wallet.collectorStatus!=='running'?'degraded':ages.some(age=>age>300000)?'stale':wallet.health?.status??'unverified';
  return {healthy:status==='fresh',label:({fresh:'Account fresh',partial:'Account partially synced',degraded:'Account degraded',invalid_clock:'Account clock invalid',stale:'Account snapshot stale',unavailable:'Account unavailable',unverified:'Account freshness unverified'})[status]??'Account freshness unverified',status};
 }
 const age=now-Date.parse(wallet.updatedAt);return !Number.isFinite(age)||age<0?{healthy:false,label:'Worker clock invalid'}:age>60000?{healthy:false,label:'Worker stale'}:{healthy:wallet.status==='running',label:'Worker '+wallet.status};
}
