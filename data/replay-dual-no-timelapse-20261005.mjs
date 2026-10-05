import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';import {performance} from 'node:perf_hooks';import {pathToFileURL,fileURLToPath} from 'node:url';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {createBookPublicationState,applyBookPublication} from '../live-paper/book-publication-v2.mjs';
import {payloadHash} from '../execution/ledger.mjs';
import {evaluateCanonicalNoCapacity} from '../live-paper/canonical-no-capacity.mjs';
import * as paper from '../live-paper/dual-no-pair-paper-v1.mjs';
import {frozenPaperModule} from '../live-paper/frozen-paper-runtime.mjs';
import {timingDistribution} from '../live-paper/read-timing.mjs';
const sha=bytes=>createHash('sha256').update(bytes).digest('hex'),need=(ok,code)=>{if(!ok)throw Error('DUAL_TIMELAPSE_'+code);};
export function adaptPaperModule(module,mode){
 if(mode==='dual_no_sequential_paper_v1')return module;
 need(mode==='dual_no_sequential_paper_v2_exact_receipt'&&typeof module.createDualNoPairStateV2==='function'&&typeof module.advanceDualNoPairV2==='function'&&typeof module.dualNoPairReportV2==='function','MODE_ADAPTER');
 return{createDualNoPairState:module.createDualNoPairStateV2,advanceDualNoPair:module.advanceDualNoPairV2,dualNoPairReport:module.dualNoPairReportV2};
}
export function compareIndependentReports(reference,current){
 need(reference?.schemaVersion===1&&reference.strict?.report&&reference.source?.sha256&&reference.originalBankHash,'REFERENCE_REPORT');
 const summarize=r=>({cohort:r.cohort,bankHash:r.originalBankHash,cutoff:r.strict.report.cutoff,captureEnd:r.source.lastAt,fills:r.strict.report.fills,closedPackages:r.strict.report.closedPackages,realizedPnl:r.strict.report.realizedPnl,estimatedLiquidationPnl:r.strict.report.estimatedLiquidationPnl,conditionalSettlementSurplus:r.strict.report.ordinaryConditionalSettlementSurplus});
 return{reference:summarize(reference),current:summarize(current),identicalSealedCapture:reference.source.sha256===current.source.sha256,
  sameOriginalCutoff:reference.strict.report.cutoff===current.strict.report.cutoff,profitabilityProof:false,
  limitation:'Different frozen cohort cutoffs and sealed capture windows are independent experiments, not a paired causal cost-model comparison. Conditional surplus and open marks never prove realized or robust profit.'};
}
async function frozenStrategyModule(runtime,bank,modulePath){
 const archived=await frozenPaperModule({runtimeDir:runtime,bank,modulePath,fallback:null});if(archived)return archived;
 // A missing archive never permits a future edited dependency graph. The full
 // original graph must still match before any current checkout fallback loads.
 for(const [relative,hash]of Object.entries(bank.codeIdentity))need(sha(fs.readFileSync(new URL('../live-paper/'+relative,import.meta.url)))===hash,'UNARCHIVED_GRAPH_CHANGED');
 return import(new URL('../'+modulePath,import.meta.url).href);
}

export function sealedEnd(file){const size=fs.statSync(file).size,fd=fs.openSync(file,'r'),b=Buffer.alloc(Math.min(size,1048576));try{fs.readSync(fd,b,0,b.length,size-b.length);}finally{fs.closeSync(fd);}const n=b.lastIndexOf(10);need(n>=0,'EMPTY');return size-b.length+n+1;}

export function readEligibility(lines){let sequence=0,previous='',last=-Infinity;const snapshots=[];
 for(const line of lines){const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===++sequence&&row.previous===previous&&sha(JSON.stringify(body))===hash,'ACCOUNT_CHAIN');previous=hash;
  const s=row.event?.snapshot;need(row.state===null&&row.event?.type==='market_account_eligibility'&&s?.complete===true&&Array.isArray(s.excludedMarketIds)&&s.excludedMarketIds.every(x=>typeof x==='string'&&/^[1-9]\d*$/.test(x)),'ACCOUNT_SCHEMA');
  const knownAt=Math.max(Date.parse(row.at),Date.parse(s.positionsAt),Date.parse(s.ordersAt));need(Number.isFinite(knownAt)&&knownAt>=last&&Date.parse(s.ordersAt)>=Date.parse(s.positionsAt)&&Date.parse(s.expiresAt)===Math.min(Date.parse(s.positionsAt),Date.parse(s.ordersAt))+20000,'ACCOUNT_CLOCK');last=knownAt;snapshots.push({knownAt,snapshot:s,sequence:row.sequence,hash});
 }return{snapshots,sequence,hash:previous};
}

export function causalAccount(snapshots,requestAt,scope){
 let lo=0,hi=snapshots.length-1,index=-1;while(lo<=hi){const mid=(lo+hi)>>1;if(snapshots[mid].knownAt<=requestAt){index=mid;lo=mid+1;}else hi=mid-1;}
 if(index<0)return null;const s=snapshots[index].snapshot;
 need(s.tournamentId===scope.tournamentId,'ACCOUNT_TOURNAMENT');const members=new Map(scope.exchanges.map(x=>[x.marketId,x.exchangeId]));
 if(s.excludedMarketIds.some(id=>!members.has(id)))return null;
 return{schemaVersion:1,positionsAt:s.positionsAt,ordersAt:s.ordersAt,excludedExchangeIds:s.excludedMarketIds.map(id=>members.get(id)).sort((a,b)=>Number(a)-Number(b))};
}

export function verifyLiveJournal(lines,bank,module=paper){let seq=0,previous='',state={paper:module.createDualNoPairState(bank),sourceSequence:bank.sourcePrefixSequence,sourceHash:bank.sourcePrefixTailHash};
 for(const line of lines){const row=JSON.parse(line),{hash,...body}=row;need(row.sequence===++seq&&row.previous===previous&&sha(JSON.stringify(body))===hash,'LIVE_CHAIN');previous=hash;const e=row.event;
  if(seq===1)need(e.kind==='genesis'&&e.bankHash===payloadHash(bank)&&e.cutoff===bank.cutoff,'LIVE_GENESIS');
  else{need(['book','boundary'].includes(e.kind),'LIVE_EVENT');const p=module.advanceDualNoPair(state.paper,e,bank);if(e.kind==='book'){need(e.sourceSequence>state.sourceSequence,'LIVE_FRONTIER');state={paper:p,sourceSequence:e.sourceSequence,sourceHash:e.sourceHash};}else state={...state,paper:p};}
  need(payloadHash(state)===payloadHash(row.state),'LIVE_STATE_PARITY');
 }return{records:seq,journalHash:previous,stateHash:payloadHash(state),paperStateHash:payloadHash(state.paper),fills:state.paper.fills.length,realizedPnl:state.paper.realizedPnl,sourceSequence:state.sourceSequence,sourceHash:state.sourceHash,exactAsRecordedIdentityParity:true};
}

function newExperiment(bank,module,label,evaluateCapacity=evaluateCanonicalNoCapacity){return{bank,state:module.createDualNoPairState(bank),label,evaluateCapacity,scopeBooks:0,eligibleAccountBooks:0,noEligibleAccountBooks:0,conditionalCapacityBooks:0,eligiblePairBookEvents:0,accountAgesMs:[],accountMissingBooks:0,currentPairRunStart:null,lastEligiblePairAt:null,longestContinuousEligiblePairMs:0,first:null,last:null,days:{},boundaries:0};}
function consume(x,row,pub,eligibility,module){const at=row.at;if(Date.parse(at)<Date.parse(x.bank.cutoff))return;
 if(row.kind!=='book'){x.state=module.advanceDualNoPair(x.state,{kind:'boundary',availableAt:at},x.bank);x.boundaries++;x.currentPairRunStart=null;x.lastEligiblePairAt=null;return;}
 const leg=x.bank.review.legs.find(l=>l.marketId===row.payload.book.marketId);if(!leg)return;
 const book=pub.books[leg.exchangeId+':YES'];if(!book)return;
 const books=x.bank.review.legs.map(l=>pub.books[l.exchangeId+':YES']),account=causalAccount(eligibility.snapshots,Date.parse(book.requestStartedAt),pub.scope);let admission={positiveConditionalCapacity:false};
 if(!pub.closed&&books.every(Boolean))try{admission=x.evaluateCapacity({review:x.bank.review,policy:x.bank.capacityPolicy,books,account,now:at});}catch{admission={positiveConditionalCapacity:false,reason:'invalid_pair'};}
 x.scopeBooks++;x.first??=at;x.last=at;const day=x.days[at.slice(0,10)]??={marketCounts:{},accountEligibleBooks:0,accountMissingOrStaleBooks:0};day.marketCounts[leg.marketId]=(day.marketCounts[leg.marketId]||0)+1;
 const accountFresh=account&&[account.positionsAt,account.ordersAt].every(t=>Date.parse(t)<=Date.parse(book.requestStartedAt)&&Date.parse(at)-Date.parse(t)<=20000);
 if(account)x.accountAgesMs.push(Date.parse(at)-Math.min(Date.parse(account.positionsAt),Date.parse(account.ordersAt)));else x.accountMissingBooks++;
 if(accountFresh){x.eligibleAccountBooks++;day.accountEligibleBooks++;}else{x.noEligibleAccountBooks++;day.accountMissingOrStaleBooks++;}
 if(admission.positiveConditionalCapacity)x.conditionalCapacityBooks++;
 const pairStarts=books.filter(Boolean).map(b=>Date.parse(b.requestStartedAt)),pairReceipts=books.filter(Boolean).map(b=>Date.parse(b.receivedAt));
 const eligiblePair=books.every(Boolean)&&pairStarts.every(t=>Date.parse(at)-t<=x.bank.policy.maxAgeMs)&&Math.max(...pairReceipts)-Math.min(...pairStarts)<=x.bank.policy.maxSkewMs
  &&account&&[account.positionsAt,account.ordersAt].every(t=>Date.parse(t)<=Math.min(...pairStarts)&&Date.parse(at)-Date.parse(t)<=20000)&&!x.bank.review.legs.some(l=>account.excludedExchangeIds.includes(l.exchangeId));
 if(eligiblePair){x.eligiblePairBookEvents++;day.eligiblePairBookEvents=(day.eligiblePairBookEvents||0)+1;const now=Date.parse(at);if(x.lastEligiblePairAt===null||now-x.lastEligiblePairAt>x.bank.policy.maxAgeMs)x.currentPairRunStart=now;x.lastEligiblePairAt=now;x.longestContinuousEligiblePairMs=Math.max(x.longestContinuousEligiblePairMs,now-x.currentPairRunStart);}
 else{x.currentPairRunStart=null;x.lastEligiblePairAt=null;}
 x.state=module.advanceDualNoPair(x.state,{kind:'book',book,pairBooks:pub.closed?[]:books.filter(Boolean),availableAt:at,accountEligibility:pub.closed?null:account,admission},x.bank);
}

export function replayPublicationLines({lines,eligibility,bank,module=paper,development=false,evaluateCapacity=evaluateCanonicalNoCapacity}){
 let pub=null,records=0,bytes=0,first=null,last=null,prefixVerified=false;const runningHash=createHash('sha256'),strict=newExperiment(bank,module,'subsequent_original_frozen_bank',evaluateCapacity);let dev=null;
 const coverage={firstAt:null,lastAt:null,rows:0,marketIds:bank.review.legs.map(x=>x.marketId),days:{},nanosecondSourceValidation:true,missingDataInterpolated:false};const legClocks={},legGaps={};
 for(const line of lines){const row=JSON.parse(line);if(!pub){need(row.kind==='session_start','FIRST_SESSION');pub=createBookPublicationState(row.payload.scope);if(development){const historical=structuredClone(bank);historical.cutoff=row.at;historical.review.recordedAt=new Date(Date.parse(row.at)-1).toISOString();historical.review.expiresAt='2030-01-01T00:00:00.000Z';historical.capacityPolicy.maximumReviewAgeMs=366*86400000;dev=newExperiment(historical,module,'historical_development_static_rule_transport_assumption',evaluateCapacity);}}
  pub=applyBookPublication(pub,row);records++;first??=row.at;last=row.at;bytes+=Buffer.byteLength(line+'\n');runningHash.update(line+'\n');
  if(bytes===bank.sourcePrefixBytes){need(runningHash.copy().digest('hex')===bank.sourcePrefixHash&&row.sequence===bank.sourcePrefixSequence&&row.hash===bank.sourcePrefixTailHash,'FROZEN_PREFIX');prefixVerified=true;}
  if(row.kind==='book'&&bank.review.legs.some(l=>l.marketId===row.payload.book.marketId)){coverage.rows++;coverage.firstAt??=row.at;coverage.lastAt=row.at;const id=row.payload.book.marketId,clock=Date.parse(row.payload.book.receivedAt);if(legClocks[id]!==undefined)(legGaps[id]??=[]).push(clock-legClocks[id]);legClocks[id]=clock;const day=coverage.days[row.at.slice(0,10)]??={};day[id]=(day[id]||0)+1;}
  if(row.sequence>bank.sourcePrefixSequence)consume(strict,row,pub,eligibility,module);if(dev)consume(dev,row,pub,eligibility,module);
 }
 need(prefixVerified,'PREFIX_NOT_IN_CAPTURE');const summarize=x=>{const at=last||x.bank.cutoff,report=module.dualNoPairReport(x.state,x.bank,at);return{label:x.label,bankHash:payloadHash(x.bank),scopeBooks:x.scopeBooks,eligibleAccountBooks:x.eligibleAccountBooks,noEligibleAccountBooks:x.noEligibleAccountBooks,accountMissingBooks:x.accountMissingBooks,causalAccountAgeMs:timingDistribution(x.accountAgesMs),eligiblePairBookEvents:x.eligiblePairBookEvents,longestContinuousEligiblePairMs:x.longestContinuousEligiblePairMs,conditionalCapacityBooks:x.conditionalCapacityBooks,firstBookAt:x.first,lastBookAt:x.last,days:x.days,boundaries:x.boundaries,stateHash:payloadHash(x.state),report,openOrPendingCensored:report.positions.length>0||!!report.pending,nearTermProfitabilityEstablished:false,ruleTransport:x===dev?{historicalAsOfRulesKnown:false,originalRuleReviewRecordedAt:bank.review.recordedAt,assumptions:['Today\'s exact party-winner predicates transported backward without historical rule verification','Review validity/age transported across captured history solely for development; not actual historical authorization','Original fill, depth, latency, cash, package limits and risk policy remain fixed','No current account snapshot backfill; only causally published eligibility records']}:null};};
 return{source:{records,bytes,sha256:runningHash.digest('hex'),firstAt:first,lastAt:last,hours:(Date.parse(last)-Date.parse(first))/3600000,sealedCompleteRecordsOnly:true,frozenPrefixVerified:true},coverage:{...coverage,spanHours:coverage.firstAt?(Date.parse(coverage.lastAt)-Date.parse(coverage.firstAt))/3600000:0,daysWithBothMarkets:Object.values(coverage.days).filter(d=>coverage.marketIds.every(id=>d[id]>0)).length,legReceiptGapsMs:Object.fromEntries(coverage.marketIds.map(id=>[id,timingDistribution(legGaps[id]||[])])),threeDaysOfContinuousEligibleCoverageEstablished:Math.max(strict.longestContinuousEligiblePairMs,dev?.longestContinuousEligiblePairMs||0)>=72*3600000},strict:summarize(strict),development:dev?summarize(dev):null};
}

export async function main(argv=process.argv.slice(2)){
 const option=(key,fallback)=>{const i=argv.indexOf(key);return i<0?fallback:argv[i+1];},runtime=path.resolve(option('--runtime',path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'))),scenario=option('--scenario','stress');need(['stress','exact','v2'].includes(scenario),'SCENARIO');
 const exact=scenario!=='stress',pointerFile=option('--pointer',exact?'dual-no-pair-v2-active.json':'dual-no-pair-active.json'),explicitCohort=option('--cohort',null),cohort=explicitCohort||JSON.parse(fs.readFileSync(path.join(runtime,pointerFile))).cohort;
 need(/^[a-z][a-z0-9-]{1,63}$/.test(cohort),'COHORT');const dir=path.join(runtime,exact?'dual-no-pair-v2-cohorts':'dual-no-pair-cohorts',cohort),bankBytes=fs.readFileSync(path.join(dir,'bank.json')),bank=JSON.parse(bankBytes),requested=option('--markets',bank.review.legs.map(l=>l.marketId).join(','));need(requested===bank.review.legs.map(l=>l.marketId).join(','),'MARKET_BANK_SCOPE');
 need(bank.mode===(exact?'dual_no_sequential_paper_v2_exact_receipt':'dual_no_sequential_paper_v1'),'SCENARIO_BANK_MODE');
 const modulePath='live-paper/dual-no-pair-paper-'+(exact?'v2':'v1')+'.mjs',module=adaptPaperModule(await frozenStrategyModule(runtime,bank,modulePath),bank.mode),capacityModule=await frozenStrategyModule(runtime,bank,'live-paper/canonical-no-capacity.mjs');
 const start=performance.now(),source=path.join(runtime,'public-books.jsonl'),account=path.join(runtime,'account-eligibility.jsonl'),live=path.join(dir,'journal.jsonl'),ends={books:sealedEnd(source),account:sealedEnd(account),live:sealedEnd(live)};
 const eligibility=readEligibility(journalLines(account,{endByte:ends.account})),identity=verifyLiveJournal(journalLines(live,{endByte:ends.live}),bank,module),result=replayPublicationLines({lines:journalLines(source,{endByte:ends.books}),eligibility,bank,module,evaluateCapacity:capacityModule.evaluateCanonicalNoCapacity,development:argv.includes('--development')});
 need(sha(fs.readFileSync(path.join(dir,'bank.json')))===sha(bankBytes),'BANK_CHANGED');const wallMs=performance.now()-start;
 const output={schemaVersion:1,createdAt:new Date().toISOString(),cohort,scenario:exact?'exact_later_receipt_v2':'one_adverse_tick_stress_v1',modulePath,frozenCapacityModule:true,originalBankHash:payloadHash(bank),originalBankBytesSha256:sha(bankBytes),frozenCodeIdentity:bank.codeIdentity,parserHash:sha(fs.readFileSync(new URL('../live-paper/book-publication-v2.mjs',import.meta.url))),eligibility:{records:eligibility.sequence,journalHash:eligibility.hash,firstKnownAt:new Date(eligibility.snapshots[0].knownAt).toISOString(),lastKnownAt:new Date(eligibility.snapshots.at(-1).knownAt).toISOString(),currentAccountBackfilled:false},identity,...result,performance:{wallMs,simulatedSourceHours:result.source.hours,speedupAgainstWallClock:result.source.hours*3600000/wallMs,recordsPerSecond:result.source.records/(wallMs/1000),intentSleeps:0},fees:'0.000000',networkRequests:0,contestOrders:0,profitabilityEstablished:false,limitations:['Accelerated replay changes wall time, never the chronological availability or latency gates.','As-recorded journal parity is exact; row-at replay is a separate virtual-consumption experiment, not necessarily bit-identical to live processing delays.','Coverage gaps and pre-eligibility hours are censored; three days of executable paired observations are not manufactured.','Historical static-rule transport is a development assumption, never held-out profitability evidence.','Conditional settlement surplus is never realized P&L; unsold and unmatched inventory remains in results.']};
 const reference=option('--reference-report',null);if(reference)output.independentComparison=compareIndependentReports(JSON.parse(fs.readFileSync(path.resolve(reference))),output);
 const out=path.resolve(option('--out',fileURLToPath(new URL(exact?'./dual-no-timelapse-v2-results-20261005.json':'./dual-no-timelapse-results-20261005.json',import.meta.url))));fs.writeFileSync(out,JSON.stringify(output,null,2)+'\n');console.log(JSON.stringify({out,cohort,wallMs,sourceHours:result.source.hours,strictFills:result.strict.report.fills,strictRealized:result.strict.report.realizedPnl,developmentFills:result.development?.report.fills,developmentRealized:result.development?.report.realizedPnl}));return output;
}
if(process.argv[1]&&pathToFileURL(path.resolve(process.argv[1])).href===import.meta.url)await main();
