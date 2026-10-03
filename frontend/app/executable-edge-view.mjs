import {paperWalletView} from './wallet-view.mjs';import {comparisonNameValid,ageComparisonWallet} from './comparison-view.mjs';
export function executableEdgeWalletView(report,manifest,now=Date.now()){
 if(manifest?.schemaVersion!==1||!comparisonNameValid(manifest.cohort)||!/^[a-f0-9]{64}$/.test(manifest.bankHash||'')||report?.bankHash!==manifest.bankHash||report.cohort!==manifest.cohort||report.mode!=='execution_aware_online_paper'||report.feesBps!==0||report.networkRequests!==0||report.competitionOrders!==0||!report.wallet)throw Error('Executable edge identity unavailable');
 const v=paperWalletView({updatedAt:report.updatedAt,status:report.failed?'failed':report.sourceClosed?'awaiting_source':'running',feeBps:0,observations:report.sourceSequence,strategy:report.wallet,performance:{closedRoundTrips:report.wallet.closedTrades}},'executable-edge','Executable edge lab');
 v.comparison={kind:'executable_edge',cohort:manifest.cohort,cutoff:report.cutoff,maxMarkAgeMs:20000};v.edge={development:report.development,counts:report.counts,prospectiveCounts:report.prospectiveCounts,cells:report.cells};return ageComparisonWallet(v,now);
}
