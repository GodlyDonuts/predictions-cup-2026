import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {parseCsv} from '../research/election-baseline-v1/model.mjs';
import {governorPollObservations} from '../research/governor-error-v1/model.mjs';
import {buildGovernorPollAverageMarketV1, compareGovernorPollProbabilityToBookV1} from '../research/governor-poll-average-market-v1.mjs';

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const readJson = file => JSON.parse(fs.readFileSync(file, 'utf8'));
const localAppData = process.env.LOCALAPPDATA;
assert(localAppData, 'LOCALAPPDATA_REQUIRED_FOR_PINNED_POLL_ARCHIVE');

const previousModel = readJson('research/governor-error-v1/results-20261005-v1.json');
const screenBytes = fs.readFileSync('data/onpoint-model-market-screen-20261014.json');
const screen = JSON.parse(screenBytes.toString('utf8'));
const runtime = path.join(localAppData, 'SigTradingCup', 'paper-runtime');
const pollBytes = fs.readFileSync(path.join(runtime, 'poll-calibration-sources-20261005', 'raw_polls.csv'));
const outcomeBytes = fs.readFileSync(path.join(runtime, 'governor-error-sources-20261005', 'election_results_gubernatorial.csv'));
assert.equal(sha256(pollBytes), previousModel.sourceHashes.polls, 'POLL_ARCHIVE_HASH_MISMATCH');
assert.equal(sha256(outcomeBytes), previousModel.sourceHashes.outcomes, 'GOVERNOR_OUTCOME_HASH_MISMATCH');

const historical = governorPollObservations(parseCsv(pollBytes.toString('utf8')), parseCsv(outcomeBytes.toString('utf8')));
const definition = readJson('research/current-polls-v2/definition.json');
const callMap = screen.independentModelCrossCheck.forecast;
const onPoint = screen.source.forecasts.find(row => row.state === callMap.state);
assert.equal(callMap.publisher, 'Call the Map');
assert.equal(callMap.state, 'MI');
assert.equal(callMap.pollAverageForDPoints, 7.5, 'CALL_MAP_POLL_MARGIN_CHANGED');
assert.equal(callMap.pollCount, 41, 'CALL_MAP_POLL_COUNT_CHANGED');
assert.equal(onPoint?.modelFavoriteParty, 'D');

const calibration = buildGovernorPollAverageMarketV1({
  observations: historical.rows,
  definition,
  horizonDays: 21,
  maximumPollAgeDays: 14,
  cutoffYear: 2022,
  currentPollMargin: callMap.pollAverageForDPoints,
  bootstrapReplicates: 4000,
  bootstrapSeed: 0x6ba4_2026,
});
const yes = screen.screen.screens.find(row => row.state === 'MI' && row.party === 'D' && row.side === 'YES');
const no = screen.screen.screens.find(row => row.state === 'MI' && row.party === 'D' && row.side === 'NO');
assert(yes && no, 'MICHIGAN_TWO_SIDED_SCREEN_REQUIRED');
assert.equal(yes.marketId, no.marketId);
assert.equal(yes.exchangeId, no.exchangeId);
const tick = screen.screen.tick;
const pDem = calibration.currentSensitivity.democraticWinProbability;
const pNo = 1 - pDem;
const pLow = calibration.currentSensitivity.bootstrap.probabilityPercentiles.p025;
const pHigh = calibration.currentSensitivity.bootstrap.probabilityPercentiles.p975;
const edgeComparison = compareGovernorPollProbabilityToBookV1({
  democraticWinProbability: pDem,
  probabilityLow: pLow,
  probabilityHigh: pHigh,
  yesAsk: yes.topAsk,
  noAsk: no.topAsk,
  tick,
});
const generatedAt = new Date().toISOString();
const screenAgeAtAnalysisMs = Date.parse(generatedAt) - Date.parse(screen.screen.asOf);
assert(screenAgeAtAnalysisMs >= 0, 'SCREEN_AFTER_ANALYSIS_CLOCK');

const report = {
  schemaVersion: 1,
  id: 'governor-poll-average-market-20261014-v1',
  generatedAt,
  method: 'governor_poll_average_error_vs_captured_SIG_market_v1',
  sourceHashes: {
    historicalPollArchive: sha256(pollBytes),
    governorOutcomeArchive: sha256(outcomeBytes),
    priorGovernorModelReport: sha256(fs.readFileSync('research/governor-error-v1/results-20261005-v1.json')),
    priorOnPointScreen: sha256(screenBytes),
    onPointMichiganPage: onPoint.contentHash,
    callTheMapMichiganPage: callMap.contentHash,
    calibrationCode: sha256(fs.readFileSync('research/governor-poll-average-market-v1.mjs')),
  },
  historicalSource: {
    pollUrl: previousModel.sourceUrls.polls,
    outcomeUrl: previousModel.sourceUrls.outcomes,
    verifiedOutcomeRaces: historical.verifiedOutcomeRaces,
    acceptedPollQuestions: historical.rows.length,
    exclusions: historical.excluded,
    sourcePublisherIndependentlyCertified: false,
  },
  calibration: {
    fixedDesign: calibration.fixedDesign,
    sample: calibration.sample,
    model: calibration.model,
    chronological: calibration.chronological,
    currentPollingInput: {
      publisher: callMap.publisher,
      state: callMap.state,
      favoriteCandidate: callMap.favoriteCandidate,
      pollAverageForDemocraticMarginPoints: callMap.pollAverageForDPoints,
      displayedPollCount: callMap.pollCount,
      publisherMethodMatchesHistoricalAggregation: false,
      sourceDate: callMap.forecastAsOf,
      sourceModifiedAt: callMap.sourceModifiedAt,
      sourceUrl: callMap.url,
      displayedWinProbability: callMap.displayedWinPct / 100,
      probabilityCalibratedForSIG: false,
    },
    sensitivity: calibration.currentSensitivity,
  },
  marketComparison: {
    state: 'MI',
    marketId: yes.marketId,
    exchangeId: yes.exchangeId,
    screenSnapshotAsOf: screen.screen.asOf,
    screenSnapshotAgeAtAnalysisMs: screenAgeAtAnalysisMs,
    snapshotFreshAtAnalysis: screenAgeAtAnalysisMs <= 60_000,
    sourceQuoteReceivedAt: yes.quoteReceivedAt,
    sourceBookPublishedAt: yes.bookPublishedAt,
    sourceQuoteAgeMs: yes.quoteAgeMs,
    tick,
    fees: 0,
    oneTickAdverseStress: tick,
    democraticYes: {...edgeComparison.yes, externalPollModelProbability: pDem},
    democraticNo: {...edgeComparison.no, externalPollModelProbability: pNo},
    onPointDisplayedDemocraticProbability: onPoint.displayedFavoriteWinPct / 100,
    independentPublisherDisplayedDemocraticProbability: callMap.displayedWinPct / 100,
    pollAverageCalibratedDemocraticProbability: pDem,
    noRobustEdgeAcrossHistoricalCycleBootstrap: !edgeComparison.anyDirectionHasPositiveBootstrapLowerEdge,
    classification: 'historical_external_poll_sensitivity_not_SIG_fair_value_or_trade',
  },
  limitations: calibration.limitations,
  profitabilityEstablished: false,
  executionAuthority: false,
  ordersGenerated: 0,
};

const output = 'data/governor-poll-average-market-20261014.json';
const replaceExisting = process.argv.includes('--replace-existing');
assert(!fs.existsSync(output) || replaceExisting, 'OUTPUT_ALREADY_EXISTS_PRESERVE_PRIOR_REPORT');
fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, {flag: replaceExisting ? 'w' : 'wx'});
console.log(JSON.stringify({
  output,
  generatedAt: report.generatedAt,
  sourceRaceCount: calibration.sample.uniqueGovernorRaces,
  sourceCycleCount: calibration.sample.independentCalendarYearClusters,
  cutoffYear: calibration.sample.trainedThroughYear,
  aggregatePollSigmaPoints: calibration.model.standardDeviationPoints,
  walkForward: {
    races: calibration.chronological.predictionCount,
    brier: calibration.chronological.metrics.brier,
    midtermRaces: calibration.chronological.midtermCycleMetrics?.races,
    midtermBrier: calibration.chronological.midtermCycleMetrics?.brier,
  },
  calibratedPollProbability: pDem,
  bootstrap95: [pLow, pHigh],
  yesEdgeAfterOneTick: edgeComparison.yes.pointEdge,
  noEdgeAfterOneTick: edgeComparison.no.pointEdge,
  robustEdge: false,
  profitabilityEstablished: false,
  ordersGenerated: 0,
}));

