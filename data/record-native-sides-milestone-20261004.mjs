import fs from 'node:fs';
import assert from 'node:assert/strict';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';
import {atomicJson} from '../execution/ledger.mjs';

const read=file=>JSON.parse(fs.readFileSync(file));
const evidence=read('data/native-quote-endpoints-20261004.json');
const initial=read('data/native-quote-sides-20261004.json');
const preserved=read('data/native-sides-preservation-20261004.json');
const checks=fs.readFileSync('data/native-sides-checks-20261004.tap','utf8');
assert(/tests 163\b/.test(checks));assert(/fail 0\b/.test(checks));assert(/skipped 0\b/.test(checks));
assert.equal(evidence.originalSideCardsExact,432);assert.equal(preserved.exactOriginalCards,72);
assert.equal(preserved.newEngineHash,evidence.engineHash);
for(const [file,hash] of Object.entries(evidence.sourceHashes))assert.equal(sha(fs.readFileSync(file)),hash,file);
assert.equal(preserved.collector.currentRecoveryVerified,true);
const sides=Object.fromEntries(['yes','no'].map(side=>[side,{
  uniqueSignalEvents:evidence[side].uniqueSignalEvents,
  uniqueObservedIntervals:evidence[side].uniqueObservedIntervals,
  strictCards:evidence[side].cards,
  endpointCards:evidence[side].endpointSensitivityCards,
  endpointInterpretation:evidence[side].endpointInterpretation,
  bookBoundarySensitivity:'All original sensitivity cards retained in the immutable full output; not replaced by endpoint labels.'
}]));
const result={schemaVersion:1,at:new Date().toISOString(),claimComment:5982841708,
  status:'canonical_two_side_rust_replay_and_endpoint_labels_completed',
  dataUse:'reused_exploratory_development_data_no_fitting_or_promotion',
  cohort:evidence.cohort,declarationHash:evidence.declarationHash,
  engineHash:evidence.engineHash,binaryHash:evidence.binaryHash,runtimeLibraryHash:evidence.runtimeLibraryHash,
  sourceHashes:evidence.sourceHashes,sources:evidence.sources,sourceBytes:evidence.sourceBytes,
  combinedSourceSpanDays:evidence.sourceDays,quoteSpanDays:evidence.quoteSpanDays,
  quoteRows:evidence.quoteRows,markets:evidence.markets,
  cases:216,strictAndBookBoundaryPaths:432,endpointSensitivityCases:216,
  workers:8,sideWorkers:4,timing:evidence.timing,initialSideTiming:initial.timing,
  endpointSensitivityMs:evidence.endpointSensitivityMs,projection:evidence.projection,
  canonicalDepthChecks:evidence.canonicalDepthChecks,sides,
  sourceGuidance:{reviewedAt:'2026-10-04T18:12:00Z',
    markets:'https://sig.thesuper.market/docs/markets-and-trading',
    settlement:'https://sig.thesuper.market/docs/settlement-and-payouts',
    conclusion:'Canonical contract quotes and public ladders complement across YES/NO. This does not establish a universal REST NO fill-price convention or executable capacity.'},
  immutableFullEvidence:{initialOutputHash:initial.outputHash,initialEvidenceHash:sha(fs.readFileSync('data/native-quote-sides-20261004.json')),
    endpointOutputHash:evidence.outputHash,endpointEvidenceHash:sha(fs.readFileSync('data/native-quote-endpoints-20261004.json')),
    originalYesOutputHash:evidence.originalYesOutputHash,strictSideAndSensitivityCardsExact:432},
  tests:{node:163,rust:9,failures:0,skips:0},
  originalWallets:{cards:72,allOriginalFillsExact:true,activeCohortReplaced:false,originalWalletsReset:false,
    evidence:'data/native-sides-preservation-20261004.json'},
  live:{paperThrough:preserved.through,paperGroups:preserved.paperGroups,collector:preserved.collector,
    pids:preserved.pids,persistentProcesses:3,additionalPersistentProcesses:0},
  fees:0,networkRequests:0,competitionOrders:0,profitabilityEstablished:false,
  conclusion:'Adding NO breadth and fixed-horizon endpoint coverage did not establish an executable edge. Broad NO dip observations remain negative. Positive YES two-hour endpoints are sparse and mostly have incomplete intermediate paths.',
  next:'Compare economic mechanisms beyond historical dip levels using the native chronological engine; retain exact book/quote separation, frozen policies and prospective validation. Do not add more copied dip wallets merely to increase model count.'};
fs.writeFileSync('data/native-sides-milestone-20261004.json',JSON.stringify(result)+'\n',{flag:'wx'});
const state=read('data/quant-development-state.json');
state.updatedAt=result.at;
state.nativeSidesMilestone={evidence:'data/native-sides-milestone-20261004.json',status:result.status,
  cases:216,paths:432,wallMs:result.timing.wallMs,tests:result.tests,
  originalWalletsExact:72,profitabilityEstablished:false};
state.goalTurn={at:result.at,classification:'progress',milestone:result.status,requiredWorkRemaining:true};
state.nextConcreteMilestone=result.next;
state.nextImplementationMilestones=[
  {priority:1,task:'Native chronological comparison of economic signals beyond historical dips, with realistic spread and independent source-clock labels. No refitting or promotion from sparse reused paths.'},
  {priority:2,task:'Only test new prospective wallets when there is a distinct mechanism and verified executable book support; preserve original wallet histories and ownership.'},
  {priority:3,task:'Keep the single serialized collector and existing paper exits healthy; retain observable outages, persistent backoff and source identities.'}
];
atomicJson('data/quant-development-state.json',state);
const monitor=read('data/daenerys-monitor.json');
monitor.updatedAt=result.at;monitor.lastCheckAt=result.at;
monitor.nativeSidesMilestone=state.nativeSidesMilestone;
monitor.lastEngineeringMilestone=result.status;monitor.nextConcreteMilestone=result.next;
atomicJson('data/daenerys-monitor.json',monitor);
console.log(JSON.stringify({status:result.status,paths:432,wallMs:result.timing.wallMs,
  tests:result.tests,originalWalletsExact:72,collectorRecovered:true,profitabilityEstablished:false}));
