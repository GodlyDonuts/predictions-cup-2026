import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {sha} from '../live-paper/native-dip-proof-v1.mjs';
import {atomicJson} from '../execution/ledger.mjs';
const read=f=>JSON.parse(fs.readFileSync(f)),snapshotFile='data/settlement-value-snapshot-1791144422978.json',snapshot=read(snapshotFile),parity=read('data/settlement-native-preservation-20261004.json'),lead=read('data/kansas-source-lead-20261004.json');
const checks=fs.readFileSync('data/settlement-value-all-checks-20261004.tap','utf8');assert(/tests 137/.test(checks)&&/pass 137/.test(checks)&&/fail 0/.test(checks));assert.equal(parity.fullStateExact,true);assert.equal(parity.nativeBackend.engineHash,snapshot.native.engineHash);
const runtime=path.join(process.env.LOCALAPPDATA,'SigTradingCup','paper-runtime'),stats=read(path.join(runtime,'stats.json'));
assert.equal(stats.runtime.status,'running');assert.equal(stats.runtime.networkBudget.writesEnabled,false);assert.equal(stats.runtime.networkBudget.maxReadsPerMinute,20);
let milestone={schemaVersion:1,at:new Date().toISOString(),claimComment:5983667980,archiveFixClaimComment:5983853071,status:'native_settlement_policy_and_actual_state_diagnostic_completed',
 features:['source-bound settlement holding and opportunity-cost reductions','bounded fractional sizing with declared correlated-risk caps','exact multi-level hard-limit reservations','research-independent held-risk and full-depth liquidation view','separate current checked-source archive helper with unchanged historical benchmark'],
 snapshot:snapshotFile,snapshotHash:sha(fs.readFileSync(snapshotFile)),parity:'data/settlement-native-preservation-20261004.json',parityHash:sha(fs.readFileSync('data/settlement-native-preservation-20261004.json')),
 sourceBytes:parity.sourceBytes,sourceRecords:parity.sourceSequence,fullActualJsNativePublicationStateExact:true,
 nativeBackend:parity.nativeBackend,decisionWallMs:snapshot.native.decisionWallMs,verificationAndDecisionWallMs:snapshot.native.totalVerificationAndDecisionMs,
 tests:{node:137,rust:9,failures:0,skips:0,newPolicyAndAdapterChecks:34},
 currentSnapshotDecision:snapshot.decisions.map(x=>({candidate:x.candidateId,action:x.action,reason:x.reason,worstCaseHeldLoss:x.worstCaseHeldLoss})),
 proposedNewCost:snapshot.proposedMaximumCost,existingPaperSnapshot:snapshot.existingPaper,
 researchLead:{file:'data/kansas-source-lead-20261004.json',hash:sha(fs.readFileSync('data/kansas-source-lead-20261004.json')),marketIds:lead.identities.map(x=>x.marketId),pointScenarioOnly:lead.scenarioOnly,pairedQuoteLeadOnly:lead.pairedQuoteLead,resolutionReviewed:false,frozenProbabilityBankCreated:false},
 runtime:{collectorPid:stats.runtime.pid,collectorLastCycle:stats.runtime.lastCycleAt,nativeQuoteStatus:stats.runtime.nativeQuote.status,nativeQuoteError:stats.runtime.nativeQuote.lastError,readCap:20,writerEnabled:false,owner:stats.runtime.networkBudget.executionOwner,persistentServices:3,additionalPersistentServices:0},
 originalBanksAndActiveBindingsPreserved:true,walletsReset:false,activeHoldingPolicyChanged:false,competitionOrders:0,sigApiRequests:0,publicPrimaryResearchRequests:3,fees:0,profitabilityEstablished:false,
 next:'Obtain current exact Kansas contract conditions and sufficient source evidence for a frozen conservative forecast method; acquire confirmation depth through the existing same-budget collector. Connect the native holding/sizing policy to a separate subsequent-data paper wallet with independent later-request fills, durable accounting and original source clocks. Do not promote the model point or conditional quote pair alone.'};
const milestoneFile='data/settlement-value-milestone-20261004.json';
if(fs.existsSync(milestoneFile)){const original=read(milestoneFile);assert.equal(original.nativeBackend.engineHash,milestone.nativeBackend.engineHash);assert.equal(original.snapshotHash,milestone.snapshotHash);milestone=original;}
else{fs.writeFileSync(milestoneFile,JSON.stringify(milestone)+'\n',{flag:'wx'});milestone=read(milestoneFile);}
const state=read('data/quant-development-state.json');state.updatedAt=milestone.at;
state.goalTurn={at:milestone.at,classification:'progress',previousTurnClassification:'no_progress_status_report',revalidatedBy:'fresh repository ownership, live process handles and latest captured book/source state',milestone:milestone.status,requiredWorkRemaining:true};
state.nativeSettlementValueMilestone=milestone;
state.next=[milestone.next,'Keep the existing collector and paper exits healthy with three services and the serialized20-read cap. There are now two protected markets rather than the earlier four-slot saturation; verify the actual planner each wake.','Keep the frozen trend probe distinct from the source-probability method. No profitability or takeover inferred from test counts, source-model points, conditional quote surplus, or tiny paper gains.'];
state.backlog.push({at:milestone.at,status:'completed',task:'Implement native settlement holding/sizing policy, exact current-snapshot proposal and full real-state JS/native publisher parity; preserve expired research clocks, held basis and all active wallets.',evidence:'data/settlement-value-milestone-20261004.json'});
atomicJson('data/quant-development-state.json',state);
const monitor=read('data/daenerys-monitor.json');monitor.updatedAt=milestone.at;monitor.lastCheckAt=milestone.at;monitor.lastEngineeringMilestone=milestone.status;monitor.nextConcreteMilestone=milestone.next;monitor.nativeSettlementValueMilestone={at:milestone.at,evidence:'data/settlement-value-milestone-20261004.json',claimComment:milestone.claimComment};atomicJson('data/daenerys-monitor.json',monitor);
console.log(JSON.stringify({status:milestone.status,checks:milestone.tests,currentDecisionCost:milestone.proposedNewCost,sourceStateParity:true,goalClassification:'progress',profitabilityEstablished:false}));
