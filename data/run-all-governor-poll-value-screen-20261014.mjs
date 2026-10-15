import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {parseCsv} from '../research/election-baseline-v1/model.mjs';
import {aggregateRacePolls} from '../research/poll-calibration-v1/model.mjs';
import {governorPollObservations} from '../research/governor-error-v1/model.mjs';
import {parseGovernorContracts} from '../research/onpoint-model-market-screen-v1.mjs';
import {createBookPublicationState, applyBookPublication} from '../live-paper/book-publication-v3.mjs';
import {sealedNativePrefix} from '../live-paper/native-dip-host-v1.mjs';
import {calibrateGovernorPollForecastsV1, screenGovernorPollForecastV1,
  summarizeGovernorPollValueScreenV1} from '../research/all-governor-poll-value-screen-v1.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const readJson = file => JSON.parse(fs.readFileSync(path.join(root, file), 'utf8'));
const bytesHash = bytes => createHash('sha256').update(bytes).digest('hex');
const fileHash = file => bytesHash(fs.readFileSync(file));
const iso = value => new Date(value).toISOString();
const sha256Prefix = async (file, endByte) => {
  const hash = createHash('sha256');
  let bytes = 0;
  for await (const chunk of fs.createReadStream(file, {start: 0, end: endByte - 1})) {
    hash.update(chunk);
    bytes += chunk.length;
  }
  assert.equal(bytes, endByte, 'PUBLIC_BOOK_PREFIX_SHORT_READ');
  return hash.digest('hex');
};

const localAppData = process.env.LOCALAPPDATA;
assert(localAppData, 'LOCALAPPDATA_REQUIRED_FOR_PINNED_POLL_AND_BOOK_ARCHIVES');
const previousModel = readJson('research/governor-error-v1/results-20261005-v1.json');
const definition = readJson('research/current-polls-v2/definition.json');
const forecastFile = path.join(root, 'data/onpoint-model-market-screen-20261014.json');
const forecastBytes = fs.readFileSync(forecastFile);
const forecastScreen = JSON.parse(forecastBytes.toString('utf8'));
assert.equal(forecastScreen.source.forecasts.length, 16, 'FORECAST_SET_CHANGED');
assert.equal(forecastScreen.accountOrRuntimeReads, false);
assert.equal(forecastScreen.contestOrders, 0);
const marketsFile = path.join(root, 'data/markets.json');
const marketBytes = fs.readFileSync(marketsFile);
const markets = JSON.parse(marketBytes.toString('utf8'));
assert.equal(markets.length, 237, 'MARKET_SNAPSHOT_CHANGED');
const parsedContracts = parseGovernorContracts({data: markets, pagination: {total: markets.length, hasMore: false}});
const democraticContracts = parsedContracts.contracts.filter(row => row.party === 'D');
const contractByState = new Map(democraticContracts.map(row => [row.state, row]));

const runtime = path.join(localAppData, 'SigTradingCup', 'paper-runtime');
const pollsFile = path.join(runtime, 'poll-calibration-sources-20261005', 'raw_polls.csv');
const outcomesFile = path.join(runtime, 'governor-error-sources-20261005', 'election_results_gubernatorial.csv');
const pollBytes = fs.readFileSync(pollsFile);
const outcomeBytes = fs.readFileSync(outcomesFile);
assert.equal(bytesHash(pollBytes), previousModel.sourceHashes.polls, 'POLL_ARCHIVE_HASH_MISMATCH');
assert.equal(bytesHash(outcomeBytes), previousModel.sourceHashes.outcomes, 'GOVERNOR_OUTCOME_ARCHIVE_HASH_MISMATCH');
const historical = governorPollObservations(parseCsv(pollBytes.toString('utf8')), parseCsv(outcomeBytes.toString('utf8')));
const fixedDefinition = {...definition, forecastHorizonDays: 21, maximumPollAgeDays: 14};
const raceRows = aggregateRacePolls(historical.rows, fixedDefinition).filter(row => row.year <= 2022);
const calibrated = calibrateGovernorPollForecastsV1({
  raceRows,
  forecasts: forecastScreen.source.forecasts,
  definition,
  cutoffYear: 2022,
  replicates: 4000,
  seed: 0x6ba42026,
});
const calibrationByState = new Map(calibrated.forecasts.map(row => [row.state, row]));

const publicBooksFile = path.join(runtime, 'public-books.jsonl');
const statBefore = fs.statSync(publicBooksFile);
const sealed = sealedNativePrefix(publicBooksFile);
assert.equal(sealed.dev, statBefore.dev, 'PUBLIC_BOOK_DEVICE_CHANGED');
assert.equal(sealed.ino, statBefore.ino, 'PUBLIC_BOOK_INODE_CHANGED');
assert(sealed.bytes > 0, 'PUBLIC_BOOK_EMPTY');
const targetContracts = forecastScreen.source.forecasts.map(forecast => contractByState.get(forecast.state)).filter(Boolean);
const targetExchangeIds = new Set(targetContracts.map(row => row.exchangeId));
const booksByExchange = new Map([...targetExchangeIds].map(exchangeId => [exchangeId, []]));
let publication = null;
let chainRecords = 0;
let bookRecords = 0;
let capturedFirstAt = null;
let capturedLastAt = null;
let sourceBookCount = 0;
const stream = fs.createReadStream(publicBooksFile, {start: 0, end: sealed.bytes - 1});
for await (const line of readline.createInterface({input: stream, crlfDelay: Infinity})) {
  const row = JSON.parse(line);
  if (!publication) {
    assert.equal(row.kind, 'session_start', 'PUBLIC_BOOK_PREFIX_NOT_SESSION_ALIGNED');
    publication = createBookPublicationState(row.payload.scope);
    assert.equal(publication.scope.exchanges.length, 237, 'PUBLIC_BOOK_SCOPE_CHANGED');
    for (const contract of targetContracts) {
      assert(publication.scope.exchanges.some(entry => entry.marketId === contract.marketId
        && entry.exchangeId === contract.exchangeId), `TARGET_CONTRACT_OUTSIDE_CAPTURE:${contract.state}`);
    }
    capturedFirstAt = row.at;
  }
  publication = applyBookPublication(publication, row);
  chainRecords += 1;
  capturedLastAt = row.at;
  if (row.kind !== 'book') continue;
  bookRecords += 1;
  const raw = row.payload.book;
  if (!targetExchangeIds.has(raw.exchangeId)) continue;
  const accepted = publication.books[raw.instrument];
  if (accepted?.publicationSequence === row.sequence && accepted.publicationHash === row.hash) {
    booksByExchange.get(raw.exchangeId).push(accepted);
    sourceBookCount += 1;
  }
}
assert(publication && publication.sequence === chainRecords, 'PUBLIC_BOOK_CHAIN_INCOMPLETE');
const prefixHash = await sha256Prefix(publicBooksFile, sealed.bytes);
const statAfter = fs.statSync(publicBooksFile);
assert.equal(statAfter.dev, sealed.dev, 'PUBLIC_BOOK_DEVICE_CHANGED_DURING_READ');
assert.equal(statAfter.ino, sealed.ino, 'PUBLIC_BOOK_INODE_CHANGED_DURING_READ');
assert(statAfter.size >= sealed.bytes, 'PUBLIC_BOOK_PREFIX_TRUNCATED_DURING_READ');
assert.equal(await sha256Prefix(publicBooksFile, sealed.bytes), prefixHash, 'PUBLIC_BOOK_PREFIX_CHANGED_DURING_READ');

const tick = forecastScreen.screen.tick;
assert.equal(tick, 0.005, 'FIXED_TICK_CHANGED');
const scored = forecastScreen.source.forecasts.map(forecast => {
  const calibration = calibrationByState.get(forecast.state);
  const contract = contractByState.get(forecast.state);
  if (!contract) return {state: forecast.state, status: 'NO_EXACT_DEMOCRATIC_GOVERNOR_CONTRACT', ordersGenerated: 0};
  if (!calibration || calibration.reason === 'NO_POLL_MARGIN') {
    return {state: forecast.state, status: 'NO_HISTORICAL_POLL_CALIBRATION_INPUT', pollingMarginForDPoints: null,
      contract: {marketId: contract.marketId, exchangeId: contract.exchangeId, title: contract.marketTitle}, ordersGenerated: 0};
  }
  return screenGovernorPollForecastV1({
    forecast,
    calibration,
    contract,
    books: booksByExchange.get(contract.exchangeId) ?? [],
    tick,
    entryLatencyMs: 3000,
    maxEntryWaitMs: 3600000,
    exitLatencyMs: 3000,
    maxMarkoutLatenessMs: 60000,
    markoutHorizonsMs: [60000, 300000, 1800000, 3600000],
    maxSourceAgeMs: 5000,
    maxFutureSourceLeadMs: 2000,
    maxRequestMs: 20000,
    maxPublishMs: 20000,
  });
});
const summary = summarizeGovernorPollValueScreenV1(scored);
const generatedAt = new Date().toISOString();
const report = {
  schemaVersion: 1,
  id: 'all-governor-poll-value-screen-20261014-v2',
  generatedAt,
  analysisMode: 'offline_historical_poll_value_and_later_executable_quote_screen',
  design: {
    forecastSet: 'all 16 saved OnPoint Governor poll margins fetched October 14, 2026',
    marginUsed: 'pollingMarginForDPoints; the OnPoint model margin and displayed win probability are not substituted',
    pollModel: 'equal-race-weighted individual-poll errors; fixed Student-t4; no SIG market calibration',
    historicalCutoffYear: 2022,
    forecastHorizonDays: 21,
    maximumPollAgeDays: 14,
    sharedCycleBootstrapReplicates: calibrated.bootstrap.replicates,
    sharedCycleBootstrapValidReplicates: calibrated.bootstrap.validReplicates,
    independentHistoricalCalendarYearClusters: calibrated.bootstrap.independentCalendarYearClusters,
    bootstrapInterpretation: calibrated.bootstrap.interpretation,
    marketMapping: 'exact saved state, party D, and current Governor market title; no fuzzy contract match',
    entryBookRule: 'first valid depth-20 book with requestStartedAt at least 3000ms after forecast fetchedAt and no more than 3600000ms after that threshold; quote lag is explicitly reported',
    bookRequirements: {fullDepthLevelsPerSide: 20, sourceClockLeadAtReceiptMaximumMs: 2000, maximumSourceAgeAtReceiptMs: 5000,
      maximumRequestMs: 20000, maximumPublicationLagMs: 20000, oneShareTopDepth: true},
    feesPerShare: 0,
    entryAdverseTicks: 1,
    exitAdverseTicks: 1,
    exitDecisionLatencyMs: 3000,
    exitHorizonsMs: [60000, 300000, 1800000, 3600000],
    maximumExitLatenessMs: 60000,
    parameterTuning: false,
    productionOrPaperWalletChanges: false,
  },
  inputs: {
    forecastSnapshot: {path: 'data/onpoint-model-market-screen-20261014.json', sha256: bytesHash(forecastBytes), forecastCount: forecastScreen.source.forecasts.length},
    marketMetadata: {path: 'data/markets.json', sha256: bytesHash(marketBytes), rows: markets.length},
    historicalPollArchive: {sha256: bytesHash(pollBytes), url: previousModel.sourceUrls.polls},
    gubernatorialOutcomeArchive: {sha256: bytesHash(outcomeBytes), url: previousModel.sourceUrls.outcomes},
    priorGovernorModelReport: {path: 'research/governor-error-v1/results-20261005-v1.json', sha256: fileHash(path.join(root, 'research/governor-error-v1/results-20261005-v1.json'))},
    marketCapture: {
      file: 'public-books.jsonl',
      sealedPrefixBytes: sealed.bytes,
      sha256: prefixHash,
      captureStartAt: capturedFirstAt,
      captureEndAt: capturedLastAt,
      chainRecords,
      bookRecords,
      mappedExchangeBookRecords: sourceBookCount,
      finalSequence: publication.sequence,
      finalRecordHash: publication.hash,
      stablePrefixVerifiedAfterReplay: true,
      fileBytesAtReadStart: statBefore.size,
      fileBytesAtReadEnd: statAfter.size,
    },
  },
  calibration: {
    historicalVerifiedOutcomeRaces: historical.verifiedOutcomeRaces,
    acceptedHistoricalPollQuestions: historical.rows.length,
    historicalExclusions: historical.excluded,
    raceRowsAtForecastHorizon: raceRows.length,
    cyclesAtForecastHorizon: [...new Set(raceRows.map(row => row.year))].sort((a, b) => a - b),
    model: calibrated.model,
    sharedCycleBootstrap: calibrated.bootstrap,
    marketCalibrationEstablished: false,
  },
  summary,
  forecasts: scored,
  limitations: [
    'The historical outcome archive ends in 2022 and does not identify the current publisher aggregation, likely-voter population, or source publication timestamps.',
    'The bootstrap resamples election-year clusters, but its range is historical parameter sensitivity, not a calibrated 2026 confidence interval; the 16 races also share election-cycle shocks.',
    'A positive polling-value point estimate is settlement-value sensitivity, not a forecast of short-horizon price reversion.',
    'Top-depth markouts are counterfactual quotes, not attributable fills. They do not model own queue position, order acceptance, or capital competition.',
    'No settlement outcomes for these 2026 contracts are present in the input set; this cannot establish realized P&L or profitability.',
  ],
  profitabilityEstablished: false,
  executionAuthority: false,
  accountApiRequests: 0,
  contestOrders: 0,
  actualFills: 0,
};
const output = path.join(root, 'data/all-governor-poll-value-screen-20261014-v2.json');
const outputExists = fs.existsSync(output);
assert(!outputExists || fs.statSync(output).size === 0, 'OUTPUT_ALREADY_EXISTS_PRESERVE_PRIOR_REPORT');
fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, {flag: outputExists ? 'w' : 'wx'});
console.log(JSON.stringify({output: path.relative(root, output), generatedAt, ...summary,
  raceRows: raceRows.length, historicalYearClusters: calibrated.bootstrap.independentCalendarYearClusters,
  validBootstrapReplicates: calibrated.bootstrap.validReplicates, sourcePrefixBytes: sealed.bytes,
  sourcePrefixHash: prefixHash, executionAuthority: false, contestOrders: 0}));

