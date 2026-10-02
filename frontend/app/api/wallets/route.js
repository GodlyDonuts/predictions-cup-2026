export const dynamic='force-dynamic';
const cohorts=[['dip-scale','Scaled dip'],['dip','Conservative dip']];
export async function GET(){
 let official;try{const r=await fetch('http://127.0.0.1:8787/api/account',{cache:'no-store',signal:AbortSignal.timeout(4000)});if(!r.ok)throw Error();const d=await r.json();official={...d,id:'official',name:'Official account',official:true};}catch{official={id:'official',name:'Official account',official:true,available:false};}
 const wallets=await Promise.all(cohorts.map(async([id,name])=>{
  try{
   const response=await fetch(`http://127.0.0.1:8787/api/${id}`,{cache:'no-store',signal:AbortSignal.timeout(4000)});
   if(!response.ok)throw Error('Unavailable');const d=await response.json();if(!d.strategy)throw Error('Unavailable');
   const s=d.strategy;
   return {id,name,available:true,updatedAt:d.updatedAt,status:d.workerHealth?.status??d.status,collectorStatus:d.collectorStatus,observations:d.observations,markets:d.universeSize,histories:d.historiesAvailable,feeBps:d.feeBps,metrics:{cash:s.cash,realizedPnl:s.realizedPnl,liquidationPnl:s.liquidationPnl,equity:s.estimatedLiquidationEquity,fills:s.fills,fees:s.fees,drawdown:s.maxObservedDrawdown},positions:(s.positions??[]).map(x=>({instrument:x.instrument,label:x.label,quantity:x.quantity,cost:x.cost,enteredAt:x.enteredAt,liquidationValue:x.observationAgeMs==null||x.unsoldShares>0?null:x.estimatedProceeds})),trades:(s.recentFills??[]).map(x=>({instrument:x.instrument,label:x.label,action:x.action,quantity:x.quantity,price:x.price,fee:x.fee,at:x.at})),pending:(s.pending??[]).map(x=>({instrument:x.instrument,label:x.label,action:x.action,quantity:x.quantity,limit:x.limit,createdAt:x.createdAt})),performance:d.performance?{closedRoundTrips:d.performance.closedRoundTrips}:null};
  }catch{return {id,name,available:false,status:'unavailable'};}
 }));
 return Response.json({updatedAt:new Date().toISOString(),wallets:[official,...wallets]},{headers:{'Cache-Control':'no-store'}});
}
