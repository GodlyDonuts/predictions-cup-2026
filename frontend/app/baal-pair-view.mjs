import {createHash} from 'node:crypto';import {ageComparisonWallet,comparisonNameValid} from './comparison-view.mjs';
const canonical=v=>v&&typeof v==='object'?Array.isArray(v)?v.map(canonical):Object.fromEntries(Object.keys(v).sort().map(k=>[k,canonical(v[k])])):v;
const hash=v=>createHash('sha256').update(JSON.stringify(canonical(v))).digest('hex');
export function baalPairWalletView(report,manifest,bank,{collectorAlive=false,now=Date.now()}={}){
 if(!comparisonNameValid(manifest?.cohort)||hash(bank)!==manifest.bankHash||bank.name!==manifest.cohort||bank.mode!=='baal_sequential_no_pair_paper'
  ||report?.bankHash!==manifest.bankHash||report.cohort!==bank.name||report.mode!==bank.mode||report.cutoff!==bank.cutoff||report.fees!=='0.000000'
  ||report.executionAuthority!==false||report.competitionOrders!==0||report.networkRequests!==0||report.profitabilityEstablished!==false||typeof report.failed!=='boolean')throw Error('Baal pair identity unavailable');
 const scope=new Map(bank.review.legs.map(l=>[l.exchangeId,l.marketId]));
 const instrument=p=>{if(scope.get(p.exchangeId)!==p.marketId||p.quantity!==1)throw Error('Baal pair scope unavailable');return{instrument:p.exchangeId+':NO',label:`Market ${p.marketId} · NO`,side:'no'};};
 const view={id:'baal-paired-inventory',name:'Baal · paired inventory',available:true,updatedAt:report.updatedAt,status:report.failed?'failed':collectorAlive?'running':'stopped',
  feeBps:0,markets:2,observations:report.journalSequence,metrics:{cash:report.cash,realizedPnl:report.realizedPnl,liquidationPnl:report.estimatedLiquidationPnl,fees:report.fees,fills:report.fills},
  positions:report.positions.map(p=>({...instrument(p),quantity:p.quantity,cost:p.cost,enteredAt:p.enteredAt,quoteReceivedAt:p.quoteReceivedAt,
   observationAgeMs:p.quoteAgeMs,quoteAgeMs:p.quoteAgeMs,quoteInvalidated:p.estimatedLiquidationProceeds===null,unsoldShares:p.unsoldShares,liquidationValue:p.estimatedLiquidationProceeds,estimatedProceeds:p.estimatedLiquidationProceeds})),
  trades:report.recentFills.map(f=>({...instrument(f),action:f.action,quantity:1,price:f.price,at:f.at,fee:'0.000000',hypothetical:true})),
  pending:report.pending?[{...instrument(report.pending),action:report.pending.action,quantity:1,limit:report.pending.limit,createdAt:report.pending.createdAt}]:[],
  performance:{closedRoundTrips:report.closedPackages},comparison:{cohort:bank.name,cutoff:bank.cutoff,maxMarkAgeMs:20000},
  baalPair:{reserved:report.reserved,stage:report.package?.stage||'flat',counts:report.counts,conditionalSurplus:report.ordinaryConditionalSettlementSurplus,
   exceptionLoss:report.zeroPayoutExceptionLoss,reviewCurrent:report.reviewCurrent,profitabilityEstablished:false}};
 return ageComparisonWallet(view,now);
}
