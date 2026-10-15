import {fitPollError, probability} from './poll-calibration-v1/model.mjs';

const finite = Number.isFinite;
const time = value => typeof value === 'string' ? Date.parse(value) : NaN;
const check = (ok, code) => { if (!ok) throw new Error(code); };

function quantile(values, p) {
  const sorted = values.slice().sort((a, b) => a - b);
  const position = (sorted.length - 1) * p;
  const low = Math.floor(position), high = Math.ceil(position);
  return sorted[low] + (sorted[high] - sorted[low]) * (position - low);
}

function random32(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Calibrate a saved set of poll margins using shared calendar-year resamples.
 * Each resample retains every race in each selected year, preserving the
 * cross-state cycle shock instead of bootstrapping forecasts independently.
 * The resulting interval is historical parameter sensitivity, not a 2026 CI.
 */
export function calibrateGovernorPollForecastsV1({
  raceRows,
  forecasts,
  definition,
  cutoffYear = 2022,
  replicates = 4000,
  seed = 0x6ba42026,
}) {
  check(Array.isArray(raceRows) && raceRows.length > 0, 'POLL_SCREEN_RACES');
  check(Array.isArray(forecasts) && forecasts.length > 0, 'POLL_SCREEN_FORECASTS');
  check(Number.isSafeInteger(cutoffYear) && Number.isSafeInteger(replicates) && replicates >= 100
    && Number.isSafeInteger(seed), 'POLL_SCREEN_BOOTSTRAP_OPTIONS');
  const fitOptions = {
    family: 'poll_centered',
    minimumRaces: definition.minimumTrainingRaces,
    minimumCycles: definition.minimumCalibrationCycles,
    minimumSd: definition.minimumMarginSdPoints,
  };
  const training = raceRows.filter(row => row.year <= cutoffYear);
  check(training.length === raceRows.length && training.every(row => Number.isSafeInteger(row.year)), 'POLL_SCREEN_CUTOFF');
  const model = fitPollError(training, fitOptions);
  check(model.trainedThroughYear <= cutoffYear, 'POLL_SCREEN_FUTURE_TRAINING');

  const states = new Set();
  for (const row of forecasts) {
    check(typeof row.state === 'string' && /^[A-Z]{2}$/.test(row.state) && !states.has(row.state), 'POLL_SCREEN_STATE');
    states.add(row.state);
    check(row.pollingMarginForDPoints === null || finite(row.pollingMarginForDPoints), 'POLL_SCREEN_MARGIN');
  }
  const eligible = forecasts.filter(row => finite(row.pollingMarginForDPoints));
  const probabilities = new Map(eligible.map(row => [row.state, []]));
  const byYear = new Map();
  for (const row of training) {
    const bucket = byYear.get(row.year) ?? [];
    bucket.push(row);
    byYear.set(row.year, bucket);
  }
  const years = [...byYear.keys()].sort((a, b) => a - b);
  check(years.length >= fitOptions.minimumCycles, 'POLL_SCREEN_TOO_FEW_CYCLES');

  const rng = random32(seed);
  let validReplicates = 0;
  for (let replicate = 0; replicate < replicates; replicate += 1) {
    const sample = [];
    const selectedYears = new Set();
    for (let draw = 0; draw < years.length; draw += 1) {
      const year = years[Math.floor(rng() * years.length)];
      selectedYears.add(year);
      sample.push(...byYear.get(year));
    }
    if (selectedYears.size < fitOptions.minimumCycles || sample.length < fitOptions.minimumRaces) continue;
    let bootstrapModel;
    try { bootstrapModel = fitPollError(sample, fitOptions); }
    catch { continue; }
    validReplicates += 1;
    for (const forecast of eligible) {
      probabilities.get(forecast.state).push(probability(bootstrapModel, forecast.pollingMarginForDPoints));
    }
  }
  check(validReplicates >= Math.floor(replicates * 0.9), 'POLL_SCREEN_BOOTSTRAP_COVERAGE');

  return {
    model,
    bootstrap: {
      method: 'shared_calendar_year_cluster_bootstrap_v1',
      replicates,
      validReplicates,
      independentCalendarYearClusters: years.length,
      years,
      seed,
      interpretation: 'historical_parameter_sensitivity_not_a_2026_confidence_interval',
    },
    forecasts: forecasts.map(row => {
      if (!finite(row.pollingMarginForDPoints)) {
        return {state: row.state, pollingMarginForDPoints: null, reason: 'NO_POLL_MARGIN'};
      }
      const draws = probabilities.get(row.state);
      return {
        state: row.state,
        pollingMarginForDPoints: row.pollingMarginForDPoints,
        pointProbability: probability(model, row.pollingMarginForDPoints),
        bootstrap: {p025: quantile(draws, 0.025), p50: quantile(draws, 0.5), p975: quantile(draws, 0.975)},
        bootstrapDraws: draws.length,
        marketCalibrationEstablished: false,
        classification: 'historical_poll_error_sensitivity_not_SIG_probability_or_trade',
      };
    }),
  };
}

function validDepth20Book(book, contract, {
  maxSourceAgeMs,
  maxFutureSourceLeadMs,
  maxRequestMs,
  maxPublishMs,
  tick,
}) {
  if (!book || book.marketId !== contract.marketId || book.exchangeId !== contract.exchangeId
      || book.instrument !== `${contract.exchangeId}:YES` || book.freshnessPolicy !== 'authoritative_receipt_v1'
      || !Array.isArray(book.bids) || !Array.isArray(book.asks) || book.bids.length !== 20 || book.asks.length !== 20) return null;
  const started = time(book.requestStartedAt), received = time(book.receivedAt), published = time(book.publishedAt);
  const source = time(book.sourceAt);
  if (![started, received, published, source].every(finite) || started > received || received > published
      || received - started > maxRequestMs || published - received > maxPublishMs
      || received - source > maxSourceAgeMs || source - received > maxFutureSourceLeadMs
      || !Number.isSafeInteger(book.sourceEventAgeMs) || book.sourceEventAgeMs !== received - source
      || !Number.isSafeInteger(book.publicationSequence) || !/^[a-f0-9]{64}$/.test(book.publicationHash ?? '')) return null;
  const levelsValid = (levels, ascending) => {
    let prior = null;
    for (const level of levels) {
      const price = Number(level?.price), quantity = level?.quantity;
      if (!finite(price) || price <= 0 || price >= 1 || !Number.isSafeInteger(quantity) || quantity < 1) return false;
      if (finite(tick) && Math.abs(price / tick - Math.round(price / tick)) > 1e-7) return false;
      if (prior !== null && (ascending ? price <= prior : price >= prior)) return false;
      prior = price;
    }
    return true;
  };
  if (!levelsValid(book.bids, false) || !levelsValid(book.asks, true)
      || Number(book.bids[0].price) >= Number(book.asks[0].price)) return null;
  return {started, received, published, source};
}

function firstBookAt(books, contract, lowerMs, upperMs, validity) {
  const ordered = books.slice().sort((a, b) => time(a.requestStartedAt) - time(b.requestStartedAt)
    || a.publicationSequence - b.publicationSequence);
  for (const book of ordered) {
    const requestAt = time(book.requestStartedAt);
    if (!finite(requestAt) || requestAt < lowerMs || requestAt > upperMs) continue;
    const clocks = validDepth20Book(book, contract, validity);
    if (clocks) return {book, clocks};
  }
  return null;
}

function sideTop(book, side) {
  const bid = book.bids[0], ask = book.asks[0];
  return side === 'YES'
    ? {ask: Number(ask.price), askQuantity: ask.quantity, bid: Number(bid.price), bidQuantity: bid.quantity}
    : {ask: 1 - Number(bid.price), askQuantity: bid.quantity, bid: 1 - Number(ask.price), bidQuantity: ask.quantity};
}

function markout(books, contract, side, entry, horizonMs, options) {
  const entryAvailable = time(entry.publishedAt);
  const lower = entryAvailable + horizonMs + options.exitLatencyMs;
  const found = firstBookAt(books, contract, lower, lower + options.maxMarkoutLatenessMs, options.validity);
  if (!found) return {horizonMs, status: 'NO_LATER_FRESH_FULL_DEPTH_BOOK'};
  const top = sideTop(found.book, side);
  if (top.bidQuantity < 1 || top.bid <= 0) return {horizonMs, status: 'NO_EXIT_BID_DEPTH'};
  const entryTop = sideTop(entry, side);
  const stressedEntryCost = entryTop.ask + options.tick;
  const stressedExitBid = Math.max(0, top.bid - options.tick);
  return {
    horizonMs,
    status: 'COUNTERFACTUAL_TOP_DEPTH_MARKOUT',
    exitRequestStartedAt: found.book.requestStartedAt,
    exitReceivedAt: found.book.receivedAt,
    exitPublishedAt: found.book.publishedAt,
    exitSequence: found.book.publicationSequence,
    exitRecordHash: found.book.publicationHash,
    rawEntryAsk: entryTop.ask,
    rawExitBid: top.bid,
    stressedEntryCost,
    stressedExitBid,
    shares: 1,
    fees: 0,
    netPerShare: stressedExitBid - stressedEntryCost,
    actualFill: false,
  };
}

/** Screen one saved poll forecast against a later verified depth-20 capture. */
export function screenGovernorPollForecastV1({
  forecast,
  calibration,
  contract,
  books,
  tick = 0.005,
  entryLatencyMs = 3000,
  maxEntryWaitMs = 60000,
  exitLatencyMs = 3000,
  maxMarkoutLatenessMs = 60000,
  markoutHorizonsMs = [60000, 300000, 1800000, 3600000],
  maxSourceAgeMs = 5000,
  maxFutureSourceLeadMs = 2000,
  maxRequestMs = 20000,
  maxPublishMs = 20000,
} = {}) {
  check(forecast && typeof forecast.state === 'string' && /^[A-Z]{2}$/.test(forecast.state), 'POLL_SCREEN_FORECAST');
  check(contract && contract.state === forecast.state && contract.party === 'D'
    && contract.marketTitle === `Will the Democratic Party win the ${contract.stateName} Governor?`
    && /^\d+$/.test(contract.marketId) && /^\d+$/.test(contract.exchangeId), 'POLL_SCREEN_CONTRACT_MAPPING');
  check(calibration?.state === forecast.state && finite(calibration.pointProbability)
    && calibration.pointProbability >= 0 && calibration.pointProbability <= 1
    && finite(calibration.bootstrap?.p025) && finite(calibration.bootstrap?.p975)
    && calibration.bootstrap.p025 >= 0 && calibration.bootstrap.p025 <= calibration.bootstrap.p975
    && calibration.bootstrap.p975 <= 1, 'POLL_SCREEN_CALIBRATION');
  check(finite(tick) && tick > 0 && finite(entryLatencyMs) && entryLatencyMs >= 0
    && finite(maxEntryWaitMs) && maxEntryWaitMs >= 0 && Array.isArray(books), 'POLL_SCREEN_OPTIONS');
  const fetchedAt = time(forecast.fetchedAt);
  check(finite(fetchedAt) && forecast.probabilityCalibratedForSIG === false, 'POLL_SCREEN_FORECAST_CLOCK');
  const validity = {maxSourceAgeMs, maxFutureSourceLeadMs, maxRequestMs, maxPublishMs, tick};
  const entry = firstBookAt(books, contract, fetchedAt + entryLatencyMs,
    fetchedAt + entryLatencyMs + maxEntryWaitMs, validity);
  const base = {
    state: forecast.state,
    forecastFetchedAt: forecast.fetchedAt,
    sourceForecastHash: forecast.contentHash,
    pollingMarginForDPoints: forecast.pollingMarginForDPoints,
    modelMarginForDPointsNotUsed: forecast.modelMarginForDPoints,
    contract: {marketId: contract.marketId, exchangeId: contract.exchangeId, title: contract.marketTitle},
    classification: 'offline_poll_value_sensitivity_not_SIG_fair_value_or_trade',
  };
  if (!entry) return {...base, status: 'NO_FRESH_FULL_DEPTH_ENTRY_BOOK', entryLatencyMs, sideValues: []};
  const book = entry.book;
  const bookClocks = entry.clocks;
  const snapshot = {
    requestStartedAt: book.requestStartedAt,
    receivedAt: book.receivedAt,
    publishedAt: book.publishedAt,
    sourceAt: book.sourceAt,
    sourceAgeAtReceiptMs: bookClocks.received - bookClocks.source,
    sourceClockLeadAtReceiptMs: bookClocks.source - bookClocks.received,
    sourceAgeAtPublicationMs: bookClocks.published - bookClocks.source,
    sequence: book.publicationSequence,
    recordHash: book.publicationHash,
    sourceVersion: book.versionKey,
    topYesBid: Number(book.bids[0].price),
    topYesBidQuantity: book.bids[0].quantity,
    topYesAsk: Number(book.asks[0].price),
    topYesAskQuantity: book.asks[0].quantity,
    fullDepthLevelsPerSide: 20,
  };
  const sides = [
    {side: 'YES', probability: calibration.pointProbability, lowerProbability: calibration.bootstrap.p025},
    {side: 'NO', probability: 1 - calibration.pointProbability, lowerProbability: 1 - calibration.bootstrap.p975},
  ];
  const markoutOptions = {tick, exitLatencyMs, maxMarkoutLatenessMs, validity};
  const sideValues = sides.map(candidate => {
    const top = sideTop(book, candidate.side);
    const stressedAsk = top.ask + tick;
    const eligible = top.askQuantity >= 1 && stressedAsk < 1;
    return {
      side: candidate.side,
      physicalProbabilityPoint: candidate.probability,
      historicalSensitivityLowerProbability: candidate.lowerProbability,
      rawAsk: top.ask,
      topAskQuantity: top.askQuantity,
      oneTickStressedAsk: stressedAsk,
      fees: 0,
      pointSettlementEdgePerShare: eligible ? candidate.probability - stressedAsk : null,
      historicalSensitivityLowerEdgePerShare: eligible ? candidate.lowerProbability - stressedAsk : null,
      eligibleForOneShareDepthScreen: eligible,
      markouts: eligible ? markoutHorizonsMs.map(horizon => markout(books, contract, candidate.side, book, horizon, markoutOptions)) : [],
      actualFill: false,
      ordersGenerated: 0,
    };
  });
  return {...base, status: 'SCREENED', entryLatencyMs, entryBook: snapshot, sideValues};
}

/** Small deterministic summary; correlated races are not summed as a portfolio. */
export function summarizeGovernorPollValueScreenV1(rows) {
  check(Array.isArray(rows), 'POLL_SCREEN_ROWS');
  const candidates = rows.flatMap(row => row.sideValues ?? []);
  const marks = candidates.flatMap(side => side.markouts ?? []);
  return {
    forecasts: rows.length,
    withEntryBook: rows.filter(row => row.status === 'SCREENED').length,
    withoutEntryBook: rows.filter(row => row.status !== 'SCREENED').length,
    sideValues: candidates.length,
    positivePointSettlementEdges: candidates.filter(row => row.pointSettlementEdgePerShare > 0).length,
    positiveHistoricalSensitivityLowerEdges: candidates.filter(row => row.historicalSensitivityLowerEdgePerShare > 0).length,
    counterfactualMarkouts: marks.filter(row => row.status === 'COUNTERFACTUAL_TOP_DEPTH_MARKOUT').length,
    positiveCounterfactualMarkouts: marks.filter(row => row.netPerShare > 0).length,
    realizedPnl: 0,
    actualFills: 0,
    contestOrders: 0,
    profitabilityEstablished: false,
  };
}

