import fs from 'node:fs';
import assert from 'node:assert/strict';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';
import {atomicJson} from '../execution/ledger.mjs';

const read=file=>JSON.parse(fs.readFileSync(file));
const evidence=read('data/native-mechanisms-evidence-20261004.json');
const checks=fs.readFileSync('data/native-mechanisms-checks-20261004.tap','utf8');
assert(/tests 169\b/.test(checks));assert(/fail 0\b/.test(checks));assert(/skipped 0\b/.test(checks));
assert.equal(evidence.preservation.strictBoundaryAndEndpointCardsExact,648);
assert.equal(evidence.preservation.originalLiveWalletCardsExact,72);
for(const [file,hash] of Object.entries(evidence.protocol.sourceHashes))assert.equal(sha(fs.readFileSync(file)),hash,file);
const baseline=Object.fromEntries(['yes','no'].map(side=>[side,{
 strict:evidence.sides[side].strictCards.filter(c=>c.scenario.id==='baseline_3s').map(c=>({
  strategy:c.strategy,episodes:c.summary.episodes,observed:c.summary.observed,
  meanQuotedNet:c.summary.quotedNetMarkout.mean,
  quotedSpreadOnlyMean:c.summary.observed?c.summary.quotedNetMarkout.mean+c.summary.meanSlippageCost:null,
  statusCounts:c.summary.statusCounts
 })),
 endpoints:evidence.sides[side].endpointCards.filter(c=>c.scenario.id==='baseline_3s').map(c=>({
  strategy:c.strategy,...c.summary,
  quotedSpreadOnlyMean:c.summary.observed?c.summary.meanQuotedNetMarkout+c.summary.meanSlippageCost:null
 }))
}]));
assert(Object.values(baseline).every(side=>side.strict.every(c=>c.quotedSpreadOnlyMean<0)&&side.endpoints.every(c=>c.quotedSpreadOnlyMean<0)));
const futureFile='data/native-mechanisms-future-1791141201465.json',future=read(futureFile);
assert.equal(future.protocolHash,evidence.protocolHash);assert.equal(future.signalAfter,evidence.protocol.signalAfter);
const milestone={schemaVersion:1,at:new Date().toISOString(),claimComment:5983314037,
 frozenProtocolComment:5983420326,status:'native_fixed_mechanisms_compared_and_future_cutoff_verified',
 cases:36,timing:evidence.timing,sourceDays:evidence.sourceDays,quoteSpanDays:evidence.quoteSpanDays,
 quoteRows:evidence.quoteRows,markets:evidence.markets,protocolHash:evidence.protocolHash,
 engineHash:evidence.protocol.engineHash,binaryHash:evidence.protocol.binaryHash,
 fullDevelopmentOutputHash:evidence.fullOutputHash,
 historicalEvidenceHash:sha(fs.readFileSync('data/native-mechanisms-evidence-20261004.json')),
 protocol:'data/native-mechanisms-protocol-20261004.json',
 fullSummaries:'data/native-mechanisms-evidence-20261004.json',baseline,
 quotedSpreadOnlyColumn:'Arithmetic cost bound on identical selected quoted paths, not a refitted strategy, executable fill or separate zero-slippage forward cohort.',
 futureProbe:{evidence:futureFile,evidenceHash:sha(fs.readFileSync(futureFile)),fullOutputHash:future.fullOutputHash,
  signalAfter:future.signalAfter,uniqueSignalEventsPerSide:future.sides.yes.uniqueSignalEvents,
  observations:future.sides.yes.strictCards.reduce((n,c)=>n+c.summary.observed,0)+future.sides.no.strictCards.reduce((n,c)=>n+c.summary.observed,0),
  currentRightCensoring:true,liveWalletsAdded:0},
 preservation:evidence.preservation,tests:{node:169,rust:9,failures:0,skips:0},
 frozenBanksPreserved:true,originalWalletsReset:false,activeCohortReplaced:false,
 fees:0,networkRequests:0,competitionOrders:0,additionalPersistentProcesses:0,profitabilityEstablished:false,
 conclusion:'All fixed baseline continuation/reversal cases were negative over this captured period, including before the extra modeled slippage cushion. Reversal has small positive midpoint movement which falls short of observed spread. No new executable strategy is promoted.',
 next:'Use the existing native source and book machinery to test source-backed fair probabilities or verified market relationships; short-window price-only drift has not covered spread. Preserve this protocol for separate future probes, without forcing an entry or blocking useful development.'};
fs.writeFileSync('data/native-mechanisms-milestone-20261004.json',JSON.stringify(milestone)+'\n',{flag:'wx'});
const state=read('data/quant-development-state.json');
state.updatedAt=milestone.at;state.nativeMechanismsMilestone={evidence:'data/native-mechanisms-milestone-20261004.json',status:milestone.status,
 cases:36,wallMs:evidence.timing.wallMs,allBaselineSpreadOnlyMeansNegative:true,
 originalLiveWalletsExact:72,originalStrictEndpointCardsExact:648,tests:milestone.tests,
 frozenProtocolComment:5983420326,signalAfter:milestone.futureProbe.signalAfter,profitabilityEstablished:false};
state.previousGoalTurnClassification='progress';
state.goalTurn={at:milestone.at,classification:'progress',milestone:milestone.status,requiredWorkRemaining:true};
state.nextConcreteMilestone=milestone.next;
state.nextImplementationMilestones=[
 {priority:1,task:'Check fresh primary research and verified contract rules for a source-backed fair-probability edge, or a verified conditional settlement relationship, using the existing shared collector and native execution/risk machinery.'},
 {priority:2,task:'Finite replay of the frozen mechanism probe only on a new verified prefix; signal request/publication must be after the fixed cutoff. Treat sparse quote labels as diagnostic evidence, not readiness.'},
 {priority:3,task:'Keep held-risk observations and paper exits healthy. Four protected books currently fill all discovery slots; never drop required exits to force exploration.'}
];
atomicJson('data/quant-development-state.json',state);
const monitor=read('data/daenerys-monitor.json');
monitor.updatedAt=milestone.at;monitor.lastCheckAt=milestone.at;monitor.nativeMechanismsMilestone=state.nativeMechanismsMilestone;
monitor.lastEngineeringMilestone=milestone.status;monitor.nextConcreteMilestone=milestone.next;
atomicJson('data/daenerys-monitor.json',monitor);
console.log(JSON.stringify({status:milestone.status,cases:36,wallMs:evidence.timing.wallMs,
 allBaselineSpreadOnlyMeansNegative:true,tests:milestone.tests,originalLiveWalletsExact:72,
 originalCardsExact:648,futureCutoff:milestone.futureProbe.signalAfter,goalClassification:'progress',profitabilityEstablished:false}));
