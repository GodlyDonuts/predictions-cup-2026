import {createHash} from 'node:crypto';
import {paperWalletView} from './wallet-view.mjs';
import {comparisonNameValid,ageComparisonWallet} from './comparison-view.mjs';

export function bulkDipWalletViews(report,manifest,bank,{workerAlive=false,now=Date.now()}={}){
 const hash=createHash('sha256').update(JSON.stringify(bank)).digest('hex');
 if(!comparisonNameValid(manifest?.cohort)||!/^[a-f0-9]{64}$/.test(manifest?.bankHash??'')
  ||hash!==manifest.bankHash||bank.schemaVersion!==2||bank.hostVersion!==2||bank.name!==manifest.cohort
  ||bank.mode!=='market_wide_bulk_reference_paper'||bank.config?.feeBps!==0
  ||report?.cohort!==manifest.cohort||report.bankHash!==hash||report.cutoff!==bank.cutoff
  ||report.hostVersion!==2||report.mode!==bank.mode||report.networkRequests!==0||report.competitionOrders!==0
  ||typeof report.failed!=='boolean'||typeof report.sourceClosed!=='boolean'
  ||!Number.isFinite(Date.parse(bank.cutoff))||!Number.isFinite(bank.config.maxDataAgeMs)||bank.config.maxDataAgeMs<=0)
  throw Error('Bulk paper identity unavailable');
 const universe=bank.universe?.exchanges;
 if(!Array.isArray(universe)||!universe.length||universe.some(x=>!/^[0-9]+$/.test(x.marketId)||!/^[0-9]+$/.test(x.exchangeId)||x.option!=='YES'))throw Error('Bulk paper scope unavailable');
 const scope=new Map(universe.map(x=>[`${x.marketId}:YES`,x]));
 if(scope.size!==universe.length||new Set(universe.map(x=>x.exchangeId)).size!==universe.length)throw Error('Bulk paper scope ambiguous');
 const latencies=bank.policy?.latenciesMs;
 if(!Array.isArray(latencies)||latencies.length!==2||latencies[0]!==1000||latencies[1]!==3000)throw Error('Bulk paper wallets unavailable');
 const count=x=>Number.isSafeInteger(x)&&x>=0?x:0;
 const label=x=>{
  const instrument=scope.get(x.instrument);
  if(!instrument)throw Error('Bulk paper instrument outside scope');
  return {...x,label:`Market ${instrument.marketId} · YES`,side:'yes'};
 };
 const status=report.failed?'failed':!workerAlive?'stopped':report.sourceClosed?'awaiting_source':'running';
 return latencies.map(latency=>{
  const wallet=report.wallets?.[latency];
  if(!wallet||wallet.fees!=='0.000000'||!Array.isArray(wallet.positions)||!Array.isArray(wallet.pending)||!Array.isArray(wallet.recentFills))throw Error('Bulk paper accounting unavailable');
  const strategy={...wallet,positions:wallet.positions.map(label),pending:wallet.pending.map(label),recentFills:wallet.recentFills.map(label)};
  const v=paperWalletView({updatedAt:report.updatedAt,status,feeBps:0,observations:report.bookSequence,
   universeSize:scope.size,strategy,performance:{closedRoundTrips:wallet.closedTrades}},`bulk-dip-${latency}`,`Market-wide dip · ${latency/1000}s`);
  v.comparison={kind:'bulk_dip',cohort:manifest.cohort,cutoff:bank.cutoff,maxMarkAgeMs:bank.config.maxDataAgeMs};
  v.bulkDip={latencyMs:latency,workerState:status,trackedMarkets:count(report.bulkReference?.marketCount),
   configuredMarkets:scope.size,candidateCount:count(report.bulkReference?.candidates?.length),
   gates:report.counts??{},referenceCounts:report.bulkReference?.counts??{},
   priceSequence:count(report.priceSequence),bookSequence:count(report.bookSequence),bankHash:hash};
  return ageComparisonWallet(v,now);
 });
}
