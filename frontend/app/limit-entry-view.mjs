import {paperWalletView} from './wallet-view.mjs';
import {ageComparisonWallet,comparisonNameValid} from './comparison-view.mjs';

export const limitEntryCohorts=[['limit-immediate','Immediate dip','immediate_dip'],['limit-hybrid','Price-capped dip','price_protected_hybrid']];
const count=value=>Number.isSafeInteger(value)&&value>=0?value:0;

export function limitEntryWalletViews(report,manifest,now=Date.now()){
 if(manifest?.schemaVersion!==1||!comparisonNameValid(manifest.cohort)||!/^[a-f0-9]{64}$/.test(manifest.bankHash||'')
  ||report?.mode!=='prospective_price_protected_limit_entry_paper'||report.cohort!==manifest.cohort||report.bankHash!==manifest.bankHash
  ||report.execution?.feeBps!==0||report.execution?.entryLifetimeMs!==30000||report.execution?.maximumQuoteAgeMs!==20000
  ||report.networkRequests!==0||report.competitionOrders!==0)throw Error('Limit-entry identity unavailable');
 return limitEntryCohorts.map(([id,name,key])=>{
  const strategy=report.strategies?.[key];if(!strategy)throw Error('Limit-entry wallet unavailable');
  const v=paperWalletView({updatedAt:report.updatedAt,status:report.status,feeBps:0,
   observations:strategy.uniqueObservations,universeSize:report.marketIds?.length,strategy,
   performance:{closedRoundTrips:strategy.closedTrades}},id,name);
  v.comparison={kind:'limit_entry',cohort:manifest.cohort,cutoff:report.cutoff,maxMarkAgeMs:report.execution.maximumQuoteAgeMs,
   clockExpiredPending:(strategy.pending??[]).filter(p=>p.clockExpiredAwaitingSource===true).length,
   entryOutcomes:Object.fromEntries(['orders','immediate','conditionalPark','expiredUnobserved','expiredObserved','unresolved'].map(k=>[k,count(strategy.entryOutcomes?.[k])]))};
  return ageComparisonWallet(v,now);
 });
}
