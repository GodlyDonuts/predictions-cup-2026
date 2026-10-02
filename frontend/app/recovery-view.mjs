import {ageComparisonWallet,comparisonNameValid} from './comparison-view.mjs';
export const recoveryWalletKeys=['baseline','confirmed','pricecap'].flatMap(entry=>['legacy','balanced','reward'].flatMap(payoff=>[1000,3000].map(latency=>`${entry}_${payoff}_${latency}`)));
const money=v=>typeof v==='string'&&/^-?\d+\.\d{6}$/.test(v),count=v=>Number.isSafeInteger(v)&&v>=0;
const units=v=>{if(!money(v))throw Error('Recovery amount unavailable');return BigInt(v.replace('.',''));};
const format=n=>`${n<0n?'-':''}${(n<0n?-n:n)/1000000n}.${String((n<0n?-n:n)%1000000n).padStart(6,'0')}`;
export function recoveryManifestValid(m){return m?.schemaVersion===1&&comparisonNameValid(m.cohort)&&/^[a-f0-9]{64}$/.test(m.bankHash||'');}
export function recoveryWalletViews(report,manifest,bank,now=Date.now()){
 if(!recoveryManifestValid(manifest)||report?.mode!=='public_rest_recovery_paper'||report.bankHash!==manifest.bankHash||report.cohort!==manifest.cohort
  ||report.cutoff!==bank?.cutoff||bank.cohort!==manifest.cohort||bank.mode!==report.mode||report.feeBps!==0||bank.policy?.feeBps!==0
  ||bank.policy.maximumQuoteAgeMs!==20000||bank.policy.entryLifetimeMs!==120000||report.networkRequests!==0||report.competitionOrders!==0
  ||Object.keys(report.wallets??{}).length!==18||!count(report.observations)||!Number.isFinite(Date.parse(report.updatedAt)))throw Error('Recovery identity unavailable');
 const labels=new Map(bank.markets.map(m=>[m.marketId,m.label||m.title||`Market ${m.marketId}`]));
 return recoveryWalletKeys.map(key=>{
  const w=report.wallets[key],[entry,payoff,latency]=key.split('_');
  if(!w||w.variant?.id!==`${entry}_${payoff}`||w.variant.entry!==entry||w.variant.payoff?.name!==payoff||w.latencyMs!==Number(latency)
   ||!money(w.cash)||!money(w.realizedPnl)||!(w.totalPnl===null||money(w.totalPnl))||!count(w.orders)||!count(w.fills)||!Array.isArray(w.positions)||!Array.isArray(w.pending))throw Error('Recovery wallet unavailable');
  const label=id=>labels.get(id)||`Market ${id}`,positions=w.positions.map(p=>({instrument:p.marketId,label:label(p.marketId),quantity:p.quantity,cost:p.cost,enteredAt:p.openedAt,
   liquidationValue:p.markReady===true&&!p.quoteInvalidated?p.liquidationValue:null,estimatedProceeds:p.markReady===true&&!p.quoteInvalidated?p.liquidationValue:null,
   unsoldShares:p.markReady===true?0:null,observationAgeMs:p.observationAgeMs,quoteReceivedAt:p.quoteReceivedAt,quoteInvalidated:p.quoteInvalidated===true}));
  const equity=w.totalPnl===null?null:format(units(bank.policy.initialCash)+units(w.totalPnl));
  const v={id:`recovery-${key}`,name:`${{baseline:'Dip',confirmed:'Recovery',pricecap:'Recovery limit'}[entry]} · ${payoff} · ${Number(latency)/1000}s`,available:true,updatedAt:report.updatedAt,status:report.status,collectorStatus:report.status,
   observations:report.observations,markets:report.configuredMarkets,histories:report.historyCoverage?.usableHistoricalReferences,feeBps:0,
   metrics:{cash:w.cash,realizedPnl:w.realizedPnl,referencePnl:null,referenceEquity:null,liquidationPnl:w.totalPnl,equity,fills:w.fills,fees:'0.000000',drawdown:null},positions,
   trades:(w.recentFills??[]).map(f=>({instrument:f.marketId,label:label(f.marketId),action:f.side,quantity:f.quantity,price:f.price,fee:f.fee,at:f.confirmedAt})),
   pending:w.pending.map(o=>({instrument:o.marketId,label:label(o.marketId),action:o.side,quantity:o.remaining,limit:o.limit,createdAt:o.at})),performance:{closedRoundTrips:w.closedRoundTrips?.count},
   comparison:{kind:'public_recovery',cohort:manifest.cohort,cutoff:report.cutoff,maxMarkAgeMs:bank.policy.maximumQuoteAgeMs,clockExpiredPending:w.clockExpiredAwaitingSource},
   recovery:{takeProfit:w.variant.payoff.takeProfit,stopLoss:w.variant.payoff.stopLoss,idealizedBreakEvenWinRate:w.idealizedBreakEvenWinRate,
    roundTrips:w.closedRoundTrips,coverage:report.historyCoverage,gates:Object.entries(w.gateCounts??{}).filter(([k,v])=>/^[a-z_]+$/.test(k)&&count(v)).sort((a,b)=>b[1]-a[1]),
    outcomes:(w.outcomes??[]).map(o=>({id:o.id,marketId:o.marketId,side:o.side,status:o.status,filledQuantity:o.filledQuantity,reason:o.reason}))}};
  return ageComparisonWallet(v,now);
 });
}
