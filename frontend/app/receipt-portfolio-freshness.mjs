const validClock=(at,now,maxAge)=>typeof at==='string'&&Number.isFinite(Date.parse(at))&&now>=Date.parse(at)&&now-Date.parse(at)<=maxAge;
export function receiptObservationHealth(wallet,now=Date.now()){
 if(!wallet?.available)return{healthy:false,label:'Paper observation unavailable',status:'unavailable'};
 if(wallet.status==='failed')return{healthy:false,label:'Paper batch failed',status:'failed'};
 if(!Number.isFinite(now)||!Number.isFinite(Date.parse(wallet.updatedAt))||now<Date.parse(wallet.updatedAt))return{healthy:false,label:'Paper observation clock invalid',status:'invalid_clock'};
 if(!validClock(wallet.updatedAt,now,wallet.receiptPortfolio?.maxObservationAgeMs??60000))return{healthy:false,label:'Paper observation stale · one-shot batch',status:'stale'};
 return{healthy:true,label:'Paper observation fresh · batch completed',status:'completed_batch'};
}
export function ageReceiptPortfolioWallet(wallet,now=Date.now()){
 if(!wallet?.receiptPortfolio)return wallet;
 const maxAge=wallet.receiptPortfolio.maxQuoteAgeMs??20000,positions=(wallet.positions??[]).map(p=>{
  const known=validClock(p.quoteReceivedAt,now,maxAge)&&validClock(p.quoteRequestStartedAt,now,maxAge)&&validClock(p.quoteAvailableAt,now,maxAge)&&Date.parse(p.quoteRequestStartedAt)<=Date.parse(p.quoteReceivedAt)&&Date.parse(p.quoteReceivedAt)<=Date.parse(p.quoteAvailableAt);
  const current=known&&p.snapshotLiquidationValue!==null&&wallet.status!=='failed';
  return{...p,quoteAgeMs:typeof p.quoteReceivedAt==='string'&&Number.isFinite(Date.parse(p.quoteReceivedAt))?now-Date.parse(p.quoteReceivedAt):null,quoteInvalidated:!current,liquidationValue:current?p.snapshotLiquidationValue:null,unsoldShares:current?0:p.quantity,liquidationStatus:current?'estimated_from_observed_depth':known?'unavailable_depth':'stale_or_unverified_quote'};
 });
 const allFresh=positions.every(p=>!p.quoteInvalidated),metrics={...wallet.metrics,liquidationPnl:allFresh&&wallet.status!=='failed'?wallet.receiptPortfolio.snapshotLiquidationPnl:null},sensitivities=Object.fromEntries(Object.entries(wallet.receiptPortfolio.snapshotSensitivities??{}).map(([key,c])=>[key,{...c,estimatedLiquidationPnl:allFresh&&wallet.status!=='failed'?c.estimatedLiquidationPnl:null,estimatedProceeds:allFresh&&wallet.status!=='failed'?c.estimatedProceeds:null,realized:false}]));
 return{...wallet,positions,metrics,receiptPortfolio:{...wallet.receiptPortfolio,liquidationSensitivities:sensitivities,quoteMarksFresh:allFresh}};
}
