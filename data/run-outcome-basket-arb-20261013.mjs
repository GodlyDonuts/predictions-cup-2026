import fs from 'node:fs';
import crypto from 'node:crypto';
import {emptyOutcomeBasketScenarioV1} from '../research/outcome-basket-arb-v1.mjs';

const read = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const sha256 = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const marketsPath = 'data/markets.json';
const universePath = 'data/structural-universe-latest.json';
const semanticsPath = 'data/basket-semantics-evidence.json';
const complementPath = 'data/same-market-complement-arb-20261013.json';
const settlementPath = 'research/settlement-rules-20261009/source.json';
const affiliationPath = 'research/settlement-rules-20261009/owned-pairs-coverage.json';

const markets = read(marketsPath), universe = read(universePath), semantics = read(semanticsPath);
const complement = read(complementPath), settlement = read(settlementPath), affiliations = read(affiliationPath);
if (!Array.isArray(markets) || complement.mode !== 'same_market_exact_complement_sequential_depth_replay_v1'
  || complement.source?.kind !== 'existing_sealed_realtime_capture'
  || !/^[a-f0-9]{64}$/.test(complement.source?.sha256 ?? '')
  || !Number.isSafeInteger(complement.source?.captureRows)
  || complement.source.scopedMarketExchanges !== 237
  || universe.acceptedMarkets !== 220 || universe.candidateObservationsNotTrades !== true) {
  throw Error('OUTCOME_BASKET_EVIDENCE_BINDING');
}
const categories = new Map();
for (const market of markets) for (const category of market.categories ?? []) categories.set(category, (categories.get(category) ?? 0) + 1);
const multiOutcomeMarkets = markets.filter(m => m.isMultiOutcome === true);
const compositeMarkets = markets.filter(m => m.isComposite === true);
const structurallyCertified = universe.candidatePairs.filter(p => p.settlementProof?.status === 'settlement_verified'
  && p.settlementProof.mutuallyExclusive === true && p.settlementProof.exhaustive === true
  && Array.isArray(p.settlementProof.coveredOutcomeIds) && p.settlementProof.coveredOutcomeIds.length >= 2
  && p.administrativeOrRefundRulesUnresolved !== true && p.thirdPartyWinPaysBoth !== true);
const externallyVerifiedPayoutFloors = semantics.conclusion?.authoritativelyVerifiedPayoutFloors ?? null;
const settlementCoverageComplete = affiliations.fullFutureExclusionEstablished === true;
const eligibleBaskets = structurallyCertified.length;

const latenciesMs = [0, 250, 1000, 3000];
const emptyScenarios = latenciesMs.map(ms => ({...emptyOutcomeBasketScenarioV1(ms),
  replayAttempted: false, qualifiedBasketSets: 0}));
const report = {
  schemaVersion: 1,
  mode: 'settlement_verified_exhaustive_yes_basket_capture_screen_v1',
  checkedAt: new Date().toISOString(),
  input: {
    capturedBooks: {kind: 'existing_sealed_capture_replay_artifact', file: complement.source.file,
      sha256: complement.source.sha256, captureRows: complement.source.captureRows,
      firstAt: complement.source.from, lastAt: complement.source.through,
      acceptedBookPoints: complement.observations?.accepted, markets: complement.observations?.markets,
      orders: 0, networkRequests: 0},
    marketMetadata: {file: marketsPath, sha256: sha256(marketsPath), totalMarkets: markets.length,
      categories: Object.fromEntries([...categories.entries()].sort()),
      multiOutcomeMarkets: multiOutcomeMarkets.length, compositeMarkets: compositeMarkets.length,
      metadataIsHistoricalLocalCache: true},
    structuralScreen: {file: universePath, sha256: sha256(universePath), acceptedMarkets: universe.acceptedMarkets,
      pairCandidates: universe.candidatePairs.length,
      pairCandidatesWithExplicitMutualExclusion: universe.candidatePairs.filter(p => p.mutualExclusionEstablished === true).length,
      pairCandidatesWithVerifiedExhaustiveSettlementManifest: structurallyCertified.length,
      pairCandidatesWithPossibleThirdPartyDualYes: universe.candidatePairs.filter(p => p.thirdPartyWinPaysBoth === true).length,
      pairCandidatesWithUnresolvedAdministrativeRules: universe.candidatePairs.filter(p => p.administrativeOrRefundRulesUnresolved === true).length},
    settlementRules: {file: settlementPath, sha256: sha256(settlementPath), sourceUrl: settlement.url,
      winningSharePayout: settlement.facts?.ordinaryWinningSharePayout,
      marketSpecificCompletenessProven: false, documentedLimitations: settlement.limitations},
    priorProofAudits: {semanticsFile: semanticsPath, semanticsSha256: sha256(semanticsPath),
      authoritativelyVerifiedPayoutFloors: externallyVerifiedPayoutFloors,
      capturedRelationshipCount: semantics.cachedRelationshipEvidence?.dataCount ?? null,
      relationshipCaptureHasTournamentScope: semantics.cachedRelationshipEvidence?.tournamentScope != null,
      affiliationsFile: affiliationPath, affiliationsSha256: sha256(affiliationPath),
      listedCandidateAffiliationCoverageComplete: affiliations.listedAffiliationCoverageComplete,
      futureExhaustiveOutcomeCoverageProven: settlementCoverageComplete},
  },
  result: {settlementVerifiedCompleteBasketSets: eligibleBaskets,
    outcomeBasketReplayPerformed: false, reason: 'NO_SETTLEMENT_VERIFIED_EXHAUSTIVE_BASKETS',
    explanation: 'The cached 237-market universe contains 233 binary Election Outcome and 4 Freeform contracts; no market is multi-outcome or composite. The Oct 13 structural screen has 112 paired party-winner candidates, all explicitly unverified, with possible third-party dual-YES settlement and unresolved administration/refund rules. The settlement page confirms an ordinary winning-share payout, but not completeness of a basket. Prior relationship and candidate-affiliation audits also leave the future exhaustive outcome set unproven.',
    scenarios: emptyScenarios},
  economics: {fees: '0.000000', winningPayoutPerUnit: '1.000000', tick: '0.005000',
    oneTickStressWouldAdd: '0.005000 per YES leg per package unit',
    fullVisibleAskDepthSweep: 'not run: no eligible basket set',
    latencyReplay: 'not run on capture: no eligible basket set; the analyzer tests all specified latencies on isolated fixtures only'},
  interpretation: {observationsAreNotTrades: true, paperFills: 0, actualCashFlows: 0,
    marketProfitabilityEstablished: false, competitionOrders: 0, executionAuthority: false,
    nextPrerequisite: 'A settlement source and a complete exact list of all YES outcomes must prove both mutual exclusion and exhaustiveness for the same payout event before replaying prices.'},
};
fs.writeFileSync('data/outcome-basket-arb-20261013.json', JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({output:'data/outcome-basket-arb-20261013.json',eligibleBaskets:report.result.settlementVerifiedCompleteBasketSets,
  replayPerformed:report.result.outcomeBasketReplayPerformed,latencyScenarios:report.result.scenarios.length,
  markets:markets.length,structuralPairs:universe.candidatePairs.length,networkRequests:0,orders:0}));

