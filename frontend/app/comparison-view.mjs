import {paperWalletView} from './wallet-view.mjs';
export const comparisonCohorts=[['comparison-dip','Matched dip','historical_dip'],['comparison-rebound','Matched rebound','shock_rebound']];
export const comparisonNameValid=value=>typeof value==='string'&&/^[a-z][a-z0-9-]{0,63}$/.test(value)&&!/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(value);
const count=value=>Number.isSafeInteger(value)&&value>=0?value:0;
export function ageComparisonWallet(view,now=Date.now()){
 if(!view?.comparison)return view;
 const v=structuredClone(view),age=now-Date.parse(v.updatedAt),validClock=Number.isFinite(age)&&age>=0;
 const fresh=validClock&&age<=60000&&v.status==='running',limit=v.comparison.maxMarkAgeMs;
 let invalidMark=!fresh;
 for(const p of v.positions){
  const quoteAge=now-Date.parse(p.quoteReceivedAt),baseAge=p.observationAgeMs;
  const valid=fresh&&Number.isFinite(limit)&&limit>0&&Number.isFinite(quoteAge)&&quoteAge>=0&&quoteAge<=limit
   &&Number.isFinite(baseAge)&&baseAge>=0&&baseAge+age<=limit&&!p.quoteInvalidated;
  p.quoteAgeMs=Number.isFinite(quoteAge)&&quoteAge>=0?quoteAge:null;
  if(!valid){invalidMark=true;p.liquidationValue=null;p.estimatedProceeds=null;p.quoteInvalidated=true;if(v.noMaker||v.baalPair)p.unsoldShares=p.quantity;}
 }
 if(invalidMark)for(const k of ['referencePnl','referenceEquity','liquidationPnl','equity'])v.metrics[k]=null;
 if(!fresh)v.status=validClock?'stale':'invalid_clock';
 return v;
}
export function comparisonWalletViews(report,manifest,now=Date.now()){
 if(manifest?.schemaVersion!==1||!comparisonNameValid(manifest.cohort)||!/^[a-f0-9]{64}$/.test(manifest.bankHash||'')
  ||report?.mode!=='prospective_common_feed_entry_comparison_paper'||report.bankHash!==manifest.bankHash||report.cohort!==manifest.cohort
  ||report.sharedExecution?.feeBps!==0||report.networkRequests!==0||report.competitionOrders!==0)throw Error('Comparison identity unavailable');
 const age=now-Date.parse(report.updatedAt),fresh=Number.isFinite(age)&&age>=0&&age<=60000&&report.status==='running';
 return comparisonCohorts.map(([id,name,key])=>{
  if(!report.strategies?.[key])throw Error('Comparison wallet unavailable');
  const v=paperWalletView({updatedAt:report.updatedAt,status:fresh?'running':Number.isFinite(age)&&age>=0?'stale':'invalid_clock',feeBps:0,
   observations:report.strategies[key].uniqueObservations,universeSize:report.marketIds?.length,strategy:report.strategies[key],
   performance:{closedRoundTrips:report.strategies[key].closedTrades}},id,name);
  v.comparison={cohort:manifest.cohort,cutoff:report.cutoff,maxMarkAgeMs:report.maxMarkAgeMs,
   acceptedSignals:count(report.strategies[key].acceptedSignals),contamination:Object.fromEntries(['account_exposure_or_orders','no_reported_account_exposure','unknown_account_context'].map(k=>[k,count(report.strategies[key].contaminationCounts?.[k])]))};
  return ageComparisonWallet(v,now);
 });
}
