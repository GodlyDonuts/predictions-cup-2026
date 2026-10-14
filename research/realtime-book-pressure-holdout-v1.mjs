import {createReadStream, existsSync, readFileSync, statSync, writeFileSync} from 'node:fs';
import {basename, resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {createInterface} from 'node:readline';
import {Transform} from 'node:stream';
import {pathToFileURL} from 'node:url';
import {instantNs} from '../feed/contract.mjs';

export const POLICY = Object.freeze({
  maxBookAgeMs: 20_000,
  sampleEveryMs: 5_000,
  trainingFraction: 0.70,
  splitPurgeMs: 30_000,
  horizonsMs: Object.freeze([5_000, 30_000]),
  maximumLabelDelayMs: Object.freeze({5000: 5_000, 30000: 10_000}),
  ridgeLambda: 0.10,
  executionLatencyMs: 1_000,
  maximumEntryDelayMs: 15_000,
  maximumExitDelayMs: 15_000,
  longOnly: true,
  shareSize: 1,
  fees: 0,
});

const FEATURES = Object.freeze([
  'micropricePressure', 'topImbalance', 'depth5Imbalance',
  'momentum5s', 'momentum30s', 'midOffset',
]);
const timestampMs = value => {
  const n = Date.parse(value);
  return Number.isFinite(n) ? n : null;
};
const finite = value => typeof value === 'number' && Number.isFinite(value);

export function validCaptureEnvelope(row) {
  return Boolean(row && (row.format === 'sig-feed-capture' || row.format === 'sig-feed-replay')
    && row.schemaVersion === 1 && typeof row.kind === 'string' && timestampMs(row.observedAt) !== null);
}

export function targetDirtyHasFreshBook(payload, exchangeId) {
  const dirties = (Array.isArray(payload?.bookDirty) ? payload.bookDirty : [])
    .filter(row => String(row?.exchangeId) === String(exchangeId));
  if (dirties.length === 0) return true;
  const book = (Array.isArray(payload?.books) ? payload.books : [])
    .find(row => String(row?.exchangeId) === String(exchangeId));
  if (!book?.asOf?.at) return false;
  try {
    const bookAt = instantNs(book.asOf.at);
    return dirties.every(row => typeof row.at === 'string' && instantNs(row.at) <= bookAt);
  } catch {
    return false;
  }
}

export function bookFeatures(book, observedAtMs, maxAgeMs = POLICY.maxBookAgeMs) {
  if (!book || !finite(observedAtMs) || !Array.isArray(book.bids) || !Array.isArray(book.asks)
      || book.bids.length === 0 || book.asks.length === 0 || !book.asOf || typeof book.asOf.at !== 'string') return null;
  const sourceAt = timestampMs(book.asOf.at);
  const ageMs = sourceAt === null ? null : observedAtMs - sourceAt;
  const bid = book.bids[0], ask = book.asks[0];
  if (ageMs === null || ageMs < -1_000 || ageMs > maxAgeMs
      || !finite(bid?.price) || !finite(ask?.price) || bid.price < 0 || ask.price > 1
      || bid.price >= ask.price || !finite(bid?.quantity) || !finite(ask?.quantity)
      || bid.quantity <= 0 || ask.quantity <= 0) return null;
  const mid = (bid.price + ask.price) / 2;
  const spread = ask.price - bid.price;
  const topTotal = bid.quantity + ask.quantity;
  const microprice = (ask.price * bid.quantity + bid.price * ask.quantity) / topTotal;
  const depth = (levels, limit) => levels.slice(0, limit).reduce((sum, level) => sum + level.quantity, 0);
  const bidDepth5 = depth(book.bids, 5), askDepth5 = depth(book.asks, 5);
  const depthTotal = bidDepth5 + askDepth5;
  if (!finite(depthTotal) || depthTotal <= 0) return null;
  return {
    at: observedAtMs,
    sourceAgeMs: ageMs,
    bidPrice: bid.price,
    askPrice: ask.price,
    bidQuantity: bid.quantity,
    askQuantity: ask.quantity,
    mid,
    spread,
    micropricePressure: (microprice - mid) / spread,
    topImbalance: (bid.quantity - ask.quantity) / topTotal,
    depth5Imbalance: (bidDepth5 - askDepth5) / depthTotal,
    midOffset: mid - 0.5,
  };
}

export function firstAtOrAfter(rows, targetAt, maximumDelayMs, continuity = null) {
  let low = 0, high = rows.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (rows[middle].at < targetAt) low = middle + 1;
    else high = middle;
  }
  const row = rows[low];
  if (!row || row.at - targetAt > maximumDelayMs || (continuity !== null && row.continuity !== continuity)) return null;
  return row;
}

function quantile(sorted, p) {
  if (!sorted.length) return null;
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.floor(p * sorted.length)))];
}

function labelSamples(samples, booksByMarket) {
  for (const sample of samples) {
    const rows = booksByMarket.get(sample.marketId) ?? [];
    for (const horizon of POLICY.horizonsMs) {
      const future = firstAtOrAfter(rows, sample.at + horizon,
        POLICY.maximumLabelDelayMs[horizon], sample.continuity);
      if (!future) continue;
      sample.labels[horizon] = {
        midChange: future.mid - sample.mid,
        executableLongPnl: future.bidPrice - sample.askPrice,
        futureAt: future.at,
        effectiveHorizonMs: future.at - sample.at,
      };
    }
  }
  return samples;
}

function completeRows(samples, horizon, cutoff, side, purgeMs = POLICY.splitPurgeMs) {
  return samples.filter(sample => {
    const label = sample.labels[horizon];
    if (!label || FEATURES.some(name => !finite(sample[name]))) return false;
    return side === 'train'
      ? label.futureAt < cutoff - purgeMs
      : sample.at >= cutoff + purgeMs;
  });
}

function solve(matrix, vector) {
  const a = matrix.map((row, i) => [...row, vector[i]]);
  const n = vector.length;
  for (let col = 0; col < n; col++) {
    let pivot = col;
    for (let row = col + 1; row < n; row++) if (Math.abs(a[row][col]) > Math.abs(a[pivot][col])) pivot = row;
    if (Math.abs(a[pivot][col]) < 1e-12) throw Error('RIDGE_SINGULAR');
    [a[col], a[pivot]] = [a[pivot], a[col]];
    const divisor = a[col][col];
    for (let j = col; j <= n; j++) a[col][j] /= divisor;
    for (let row = 0; row < n; row++) if (row !== col) {
      const factor = a[row][col];
      for (let j = col; j <= n; j++) a[row][j] -= factor * a[col][j];
    }
  }
  return a.map(row => row[n]);
}

export function fitRidge(trainRows, lambda = POLICY.ridgeLambda) {
  if (!Array.isArray(trainRows) || trainRows.length < FEATURES.length * 5 || !finite(lambda) || lambda <= 0) throw Error('RIDGE_TRAINING_SAMPLE');
  const means = FEATURES.map(name => trainRows.reduce((sum, row) => sum + row[name], 0) / trainRows.length);
  const scales = FEATURES.map((name, i) => {
    const variance = trainRows.reduce((sum, row) => sum + (row[name] - means[i]) ** 2, 0) / trainRows.length;
    return Math.sqrt(variance) || 1;
  });
  const yMean = trainRows.reduce((sum, row) => sum + row.labels[30_000].midChange, 0) / trainRows.length;
  const gram = FEATURES.map(() => Array(FEATURES.length).fill(0));
  const cross = Array(FEATURES.length).fill(0);
  for (const row of trainRows) {
    const x = FEATURES.map((name, i) => (row[name] - means[i]) / scales[i]);
    const y = row.labels[30_000].midChange - yMean;
    for (let i = 0; i < x.length; i++) {
      cross[i] += x[i] * y;
      for (let j = 0; j < x.length; j++) gram[i][j] += x[i] * x[j];
    }
  }
  for (let i = 0; i < FEATURES.length; i++) {
    cross[i] /= trainRows.length;
    for (let j = 0; j < FEATURES.length; j++) gram[i][j] /= trainRows.length;
    gram[i][i] += lambda;
  }
  const coefficients = solve(gram, cross);
  return {
    features: [...FEATURES], means, scales, yMean, coefficients, lambda,
    predict(row) { return yMean + coefficients.reduce((sum, coefficient, i) =>
      sum + coefficient * (row[FEATURES[i]] - means[i]) / scales[i], 0); },
  };
}

function score(rows, model, horizon) {
  if (!rows.length) return {n: 0, markets: 0, correlation: null, mse: null, trainingMeanMse: null, meanChange: null};
  const pairs = rows.map(row => ({prediction: model.predict(row), actual: row.labels[horizon].midChange, marketId: row.marketId}));
  const meanY = rows.reduce((sum, row) => sum + row.labels[horizon].midChange, 0) / rows.length;
  const mse = pairs.reduce((sum, row) => sum + (row.prediction - row.actual) ** 2, 0) / pairs.length;
  const baseline = rows.reduce((sum, row) => sum + (model.yMean - row.labels[horizon].midChange) ** 2, 0) / rows.length;
  const meanPred = pairs.reduce((sum, row) => sum + row.prediction, 0) / pairs.length;
  const covariance = pairs.reduce((sum, row) => sum + (row.prediction - meanPred) * (row.actual - meanY), 0);
  const vx = pairs.reduce((sum, row) => sum + (row.prediction - meanPred) ** 2, 0);
  const vy = pairs.reduce((sum, row) => sum + (row.actual - meanY) ** 2, 0);
  return {n: pairs.length, markets: new Set(pairs.map(row => row.marketId)).size,
    correlation: vx > 0 && vy > 0 ? covariance / Math.sqrt(vx * vy) : null,
    mse, trainingMeanMse: baseline, mseImprovementVsTrainingMean: baseline > 0 ? 1 - mse / baseline : null,
    meanChange: meanY, meanPrediction: meanPred};
}

function quantileBins(trainRows, testRows, feature, horizon) {
  const sorted = trainRows.map(row => row[feature]).sort((a, b) => a - b);
  if (!sorted.length) return [];
  const edges = [0.2, 0.4, 0.6, 0.8].map(p => quantile(sorted, p));
  const bins = Array.from({length: 5}, (_, i) => ({bin: i + 1, n: 0, markets: new Set(), meanChange: 0, meanExecutableLongPnl: 0}));
  for (const row of testRows) {
    let bin = 0;
    while (bin < edges.length && row[feature] > edges[bin]) bin++;
    const target = bins[bin], label = row.labels[horizon];
    target.n++; target.markets.add(row.marketId); target.meanChange += label.midChange;
    target.meanExecutableLongPnl += label.executableLongPnl;
  }
  return bins.map(row => ({bin: row.bin, trainUpperEdges: edges, n: row.n, markets: row.markets.size,
    meanChange: row.n ? row.meanChange / row.n : null,
    meanExecutableLongPnl: row.n ? row.meanExecutableLongPnl / row.n : null}));
}

function percentile(sorted, p) { return sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))]; }
function marketClusterInterval(trades, seed = 0xbaa1) {
  const groups = new Map();
  for (const trade of trades) {
    const rows = groups.get(trade.marketId) ?? [];
    rows.push(trade.pnl); groups.set(trade.marketId, rows);
  }
  const marketMeans = [...groups.values()].map(rows => rows.reduce((sum, x) => sum + x, 0) / rows.length);
  if (marketMeans.length < 2) return {clusters: marketMeans.length, lower95: null, upper95: null};
  let state = seed >>> 0;
  const random = () => { state = (1664525 * state + 1013904223) >>> 0; return state / 0x1_0000_0000; };
  const estimates = [];
  for (let b = 0; b < 2_000; b++) {
    let sum = 0;
    for (let i = 0; i < marketMeans.length; i++) sum += marketMeans[Math.floor(random() * marketMeans.length)];
    estimates.push(sum / marketMeans.length);
  }
  estimates.sort((a, b) => a - b);
  return {clusters: marketMeans.length, lower95: percentile(estimates, 0.025), upper95: percentile(estimates, 0.975)};
}

export function simulateLongOnly(testRows, booksByMarket, model, {threshold = row => row.spread, horizonMs = 30_000} = {}) {
  const test = [...testRows].sort((a, b) => a.at - b.at || Number(a.marketId) - Number(b.marketId));
  const cooldown = new Map(), trades = [];
  let eligibleSignals = 0, noEntryBook = 0, noExitBook = 0, insufficientTopDepth = 0;
  for (const signal of test) {
    if (model && model.predict(signal) <= threshold(signal)) continue;
    if (signal.at < (cooldown.get(signal.marketId) ?? -Infinity)) continue;
    eligibleSignals++;
    const rows = booksByMarket.get(signal.marketId) ?? [];
    const entry = firstAtOrAfter(rows, signal.at + POLICY.executionLatencyMs, POLICY.maximumEntryDelayMs, signal.continuity);
    if (!entry) { noEntryBook++; continue; }
    const exit = firstAtOrAfter(rows, signal.at + horizonMs, POLICY.maximumExitDelayMs, signal.continuity);
    if (!exit || exit.at <= entry.at) { noExitBook++; continue; }
    cooldown.set(signal.marketId, exit.at);
    if (entry.askQuantity < POLICY.shareSize || exit.bidQuantity < POLICY.shareSize) { insufficientTopDepth++; continue; }
    trades.push({marketId: signal.marketId, signalAt: signal.at, entryAt: entry.at, exitAt: exit.at,
      entryDelayMs: entry.at - signal.at, exitHorizonMs: exit.at - signal.at,
      ask: entry.askPrice, bid: exit.bidPrice, pnl: exit.bidPrice - entry.askPrice});
  }
  const sorted = trades.map(row => row.pnl).sort((a, b) => a - b);
  return {
    mode: 'one-share-counterfactual-taker-markout', actualFills: false, fees: 0,
    threshold: model ? 'trained expected 30s mid move > decision-time full spread' : 'all non-overlapping test observations',
    eligibleSignals, trades: trades.length, markets: new Set(trades.map(row => row.marketId)).size,
    totalNetPnl: trades.reduce((sum, row) => sum + row.pnl, 0),
    meanNetPnlPerShare: trades.length ? trades.reduce((sum, row) => sum + row.pnl, 0) / trades.length : null,
    medianNetPnlPerShare: sorted.length ? percentile(sorted, 0.5) : null,
    positiveShareFraction: trades.length ? trades.filter(row => row.pnl > 0).length / trades.length : null,
    meanEntryDelayMs: trades.length ? trades.reduce((sum, row) => sum + row.entryDelayMs, 0) / trades.length : null,
    meanExitHorizonMs: trades.length ? trades.reduce((sum, row) => sum + row.exitHorizonMs, 0) / trades.length : null,
    clusterMean95: marketClusterInterval(trades),
    exclusions: {noEntryBook, noExitBook, insufficientTopDepth},
  };
}

function freshState(scope, staleMs) {
  return {marketId: String(scope.marketId), exchangeId: String(scope.exchangeIds[0]), staleMs,
    epoch: null, revision: null, ready: false, pendingSnapshot: null, continuity: 0,
    lastSampleAt: -Infinity, recent: [], books: [], samples: []};
}

function latestAtOrBefore(rows, targetAt) {
  for (let i = rows.length - 1; i >= 0; i--) if (rows[i].at <= targetAt) return rows[i];
  return null;
}

function appendBook(state, book, at, counters) {
  const core = bookFeatures(book, at, state.staleMs);
  if (!core) { counters.rejectedBooks++; return; }
  const history = state.recent;
  while (history.length && history[0].at < at - 35_000) history.shift();
  const prior5 = latestAtOrBefore(history, at - 5_000), prior30 = latestAtOrBefore(history, at - 30_000);
  const row = {...core, marketId: state.marketId, continuity: state.continuity,
    momentum5s: prior5 ? core.mid - prior5.mid : null,
    momentum30s: prior30 ? core.mid - prior30.mid : null};
  state.recent.push(row);
  state.books.push({...row});
  if (at - state.lastSampleAt >= POLICY.sampleEveryMs) {
    state.samples.push({...row, labels: {}}); state.lastSampleAt = at;
  }
  counters.acceptedBooks++;
}

function processCaptureRecord(row, states, counters) {
  const marketId = String(row.scope?.marketId ?? row.topic?.split(':').at(-1) ?? '');
  if (row.kind === 'config') {
    if (!marketId || !Array.isArray(row.scope?.exchangeIds) || row.scope.exchangeIds.length !== 1) throw Error('CAPTURE_SCOPE_INVALID');
    if (states.has(marketId)) throw Error('DUPLICATE_MARKET_CONFIG');
    states.set(marketId, freshState(row.scope, row.staleMs ?? POLICY.maxBookAgeMs));
    counters.configuredMarkets++;
    return;
  }
  const state = states.get(marketId);
  if (!state) throw Error('CAPTURE_CONFIG_REQUIRED');
  if (row.epoch !== undefined && state.epoch !== null && row.epoch !== state.epoch) {
    state.ready = false; state.revision = null; state.pendingSnapshot = null;
    state.continuity++; state.recent = []; state.lastSampleAt = -Infinity;
  }
  if (row.epoch !== undefined) state.epoch = row.epoch;
  counters.recordsByKind[row.kind] = (counters.recordsByKind[row.kind] ?? 0) + 1;
  if (row.kind === 'subscribed' || row.kind === 'recovery_started' || row.kind === 'disconnect') {
    state.ready = false; state.pendingSnapshot = null; state.revision = null;
    state.continuity++; state.recent = []; state.lastSampleAt = -Infinity;
    return;
  }
  if (row.kind === 'invalidation') {
    state.ready = false; state.pendingSnapshot = null; state.revision = null;
    state.continuity++; state.recent = []; state.lastSampleAt = -Infinity;
    counters.invalidationsByReason[row.reason ?? 'UNKNOWN'] = (counters.invalidationsByReason[row.reason ?? 'UNKNOWN'] ?? 0) + 1;
    return;
  }
  if (row.kind === 'snapshot') {
    const book = row.books?.find(item => String(item.exchangeId) === state.exchangeId);
    if (!book || row.books.length !== 1 || !timestampMs(row.receivedAt)) { state.pendingSnapshot = null; return; }
    state.pendingSnapshot = {book, at: timestampMs(row.observedAt ?? row.receivedAt)};
    return;
  }
  if (row.kind === 'recovery_finished') {
    if (row.booksReady !== true || !state.pendingSnapshot) { state.ready = false; return; }
    state.ready = true; state.revision = null;
    appendBook(state, state.pendingSnapshot.book, state.pendingSnapshot.at, counters);
    state.pendingSnapshot = null;
    return;
  }
  if (row.kind !== 'event') return;
  const targetDirtyRows = (Array.isArray(row.payload?.bookDirty) ? row.payload.bookDirty : [])
    .filter(item => String(item?.exchangeId) === state.exchangeId);
  const dirtyHasFreshBook = targetDirtyHasFreshBook(row.payload, state.exchangeId);
  if (targetDirtyRows.length) {
    counters.targetDirtyBatchesSeen++;
    if (dirtyHasFreshBook) counters.targetDirtyBatchesReconciled++;
    else counters.targetDirtyBatchesUnreconciled++;
  }
  if (!state.ready || row.event !== 'market_batch' || !row.payload || row.payload.resyncRequired === true) {
    if (row.payload?.resyncRequired === true) { state.ready = false; state.continuity++; state.recent = []; state.lastSampleAt = -Infinity; }
    return;
  }
  const delivery = row.payload.delivery;
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)
      || delivery.revision <= delivery.previousRevision
      || state.revision !== null && delivery.previousRevision !== state.revision
      || !dirtyHasFreshBook
      || (row.payload.marketSettled?.length ?? 0) > 0) {
    state.ready = false; state.revision = null; state.continuity++;
    state.recent = []; state.lastSampleAt = -Infinity; counters.rejectedEvents++;
    return;
  }
  state.revision = delivery.revision;
  const at = timestampMs(row.receivedAt ?? row.observedAt);
  if (at === null) { counters.rejectedEvents++; return; }
  for (const book of row.payload.books ?? []) if (String(book.exchangeId) === state.exchangeId) appendBook(state, book, at, counters);
}

function attachTargets(states, counters) {
  const samples = [];
  const booksByMarket = new Map();
  for (const [marketId, state] of states) {
    booksByMarket.set(marketId, state.books);
    samples.push(...state.samples);
  }
  samples.sort((a, b) => a.at - b.at || Number(a.marketId) - Number(b.marketId));
  labelSamples(samples, booksByMarket);
  counters.sampledObservations = samples.length;
  counters.with5sLabel = samples.filter(x => x.labels[5_000]).length;
  counters.with30sLabel = samples.filter(x => x.labels[30_000]).length;
  return {samples, booksByMarket};
}

function scoreFeature(train, test, feature, horizon) {
  const edges = [0.2, 0.4, 0.6, 0.8].map(p => quantile(train.map(x => x[feature]).sort((a, b) => a - b), p));
  return quantileBins(train, test, feature, horizon).map(row => ({...row, thresholdEdges: edges}));
}

export function analyzeSamples(samples, booksByMarket, {firstAt, lastAt, sourceHash, sourceBytes, counters}) {
  if (!finite(firstAt) || !finite(lastAt) || lastAt <= firstAt) throw Error('CAPTURE_WINDOW_INVALID');
  const cutoff = firstAt + (lastAt - firstAt) * POLICY.trainingFraction;
  const train5 = completeRows(samples, 5_000, cutoff, 'train');
  const test5 = completeRows(samples, 5_000, cutoff, 'test');
  const train30 = completeRows(samples, 30_000, cutoff, 'train');
  const test30 = completeRows(samples, 30_000, cutoff, 'test');
  const result = {
    schemaVersion: 1,
    report: 'realtime-book-pressure-holdout-20261014',
    at: new Date().toISOString(),
    mode: 'offline_causal_book_pressure_evaluation',
    source: {basename: sourceHash.basename, bytes: sourceBytes, sha256: sourceHash.sha256,
      firstValidBookAt: new Date(firstAt).toISOString(), lastValidBookAt: new Date(lastAt).toISOString(), ...counters},
    protocol: {maxBookAgeMs: POLICY.maxBookAgeMs, sampleEveryMs: POLICY.sampleEveryMs,
      trainingFraction: POLICY.trainingFraction, splitAt: new Date(cutoff).toISOString(), purgeMs: POLICY.splitPurgeMs,
      horizonsMs: [...POLICY.horizonsMs], maximumLabelDelayMs: POLICY.maximumLabelDelayMs,
      bookDirtyRule: 'A target dirty marker is coherent only with a same-batch target book whose asOf is at or after dirty.at, compared at nanosecond precision.',
      fees: 0, executionLatencyMs: POLICY.executionLatencyMs, oneShareDepthRequired: true,
      ridgeLambda: POLICY.ridgeLambda, features: [...FEATURES]},
    samples: {all: samples.length, training5s: train5.length, test5s: test5.length,
      training30s: train30.length, test30s: test30.length,
      trainMarkets30s: new Set(train30.map(x => x.marketId)).size, testMarkets30s: new Set(test30.map(x => x.marketId)).size},
    fiveSecondFeatureBins: Object.fromEntries(['micropricePressure','topImbalance','depth5Imbalance','momentum5s','momentum30s']
      .map(name => [name, scoreFeature(train5.filter(x => finite(x[name])), test5.filter(x => finite(x[name])), name, 5_000)])),
    thirtySecondFeatureBins: Object.fromEntries(['micropricePressure','topImbalance','depth5Imbalance','momentum5s','momentum30s']
      .map(name => [name, scoreFeature(train30.filter(x => finite(x[name])), test30.filter(x => finite(x[name])), name, 30_000)])),
    model30s: null,
    counterfactualExecution: null,
    limitations: [
      'One capture window only; markets are correlated and the market-cluster interval is not an independent-day confidence bound.',
      'The chronological split is purged, but this exploratory feature study was formulated after capture completion.',
      'Public books do not prove Baal queue position or actual fills; one-share top-of-book outcomes are counterfactual taker marks.',
      'Feed batches are asynchronous; reported effective horizons and decision-to-entry delays expose timing granularity.',
      'A positive markout is not a production profitability result. No actual fills or contest orders are represented.',
    ],
    reads: {network: 0, accountApi: 0}, actualFills: false, contestOrders: 0,
    executionAuthority: false, profitabilityEstablished: false,
  };
  if (train30.length >= FEATURES.length * 5 && test30.length) {
    const model = fitRidge(train30);
    result.model30s = {training: score(train30, {predict: row => model.predict(row), yMean: model.yMean}, 30_000),
      test: score(test30, model, 30_000), coefficients: Object.fromEntries(FEATURES.map((name, i) => [name, model.coefficients[i]])),
      means: Object.fromEntries(FEATURES.map((name, i) => [name, model.means[i]])),
      scales: Object.fromEntries(FEATURES.map((name, i) => [name, model.scales[i]])), yMean: model.yMean, lambda: model.lambda};
    const alwaysLong = simulateLongOnly(test30, booksByMarket, null);
    const modelLong = simulateLongOnly(test30, booksByMarket, model);
    result.counterfactualExecution = {alwaysLong, modelGatedLong: modelLong,
      actualFills: false, executionClaims: false};
  }
  return result;
}

export async function analyzeCapture(inputPath) {
  const absolute = resolve(inputPath);
  const before = statSync(absolute);
  const hash = createHash('sha256');
  const input = createReadStream(absolute);
  const hashed = input.pipe(new Transform({transform(chunk, _encoding, callback) { hash.update(chunk); callback(null, chunk); }}));
  const lines = createInterface({input: hashed, crlfDelay: Infinity});
  const states = new Map();
  const counters = {records: 0, recordsByKind: {}, configuredMarkets: 0, acceptedBooks: 0,
    rejectedBooks: 0, rejectedEvents: 0, invalidationsByReason: {}, sampledObservations: 0,
    with5sLabel: 0, with30sLabel: 0, targetDirtyBatchesSeen: 0,
    targetDirtyBatchesReconciled: 0, targetDirtyBatchesUnreconciled: 0};
  let firstRecordAt = null, lastRecordAt = null, firstValidBookAt = null, lastValidBookAt = null;
  try {
    for await (const line of lines) {
      if (!line) continue;
      counters.records++;
      let row;
      try { row = JSON.parse(line); } catch { throw Error(`CAPTURE_JSON_INVALID_AT_${counters.records}`); }
      if (!validCaptureEnvelope(row)) throw Error('CAPTURE_SCHEMA_INVALID');
      if (row.format === 'sig-feed-capture'
          && (!Number.isSafeInteger(row.captureIndex) || row.captureIndex !== counters.records - 1)) {
        throw Error('CAPTURE_INDEX_GAP');
      }
      const at = timestampMs(row.observedAt);
      if (at === null || lastRecordAt !== null && at < lastRecordAt) throw Error('CAPTURE_TIME_ORDER_INVALID');
      firstRecordAt ??= at; lastRecordAt = at;
      const beforeAccepted = counters.acceptedBooks;
      processCaptureRecord(row, states, counters);
      if (counters.acceptedBooks > beforeAccepted) {
        firstValidBookAt ??= at; lastValidBookAt = at;
      }
    }
  } catch (error) {
    input.destroy();
    throw error;
  }
  const after = statSync(absolute);
  if (before.size !== after.size || before.mtimeMs !== after.mtimeMs || before.ino !== after.ino) throw Error('CAPTURE_CHANGED_DURING_READ');
  if (firstValidBookAt === null || lastValidBookAt === null) throw Error('CAPTURE_HAS_NO_VALID_BOOKS');
  const sourceHash = {basename: basename(absolute), sha256: hash.digest('hex')};
  const {samples, booksByMarket} = attachTargets(states, counters);
  const result = analyzeSamples(samples, booksByMarket, {firstAt: firstValidBookAt, lastAt: lastValidBookAt,
    sourceHash, sourceBytes: before.size, counters: {...counters, firstRecordAt: new Date(firstRecordAt).toISOString(),
      lastRecordAt: new Date(lastRecordAt).toISOString(), markets: states.size}});
  return result;
}

function cliValue(name, args = process.argv.slice(2)) {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : null;
}

async function main(args) {
  if (args.length === 1 && args[0] === '--help') {
    process.stdout.write('Offline only: node research/realtime-book-pressure-holdout-v1.mjs --input <stopped-capture.jsonl> --out <new-report.json>\n');
    return 0;
  }
  const input = cliValue('--input', args), output = cliValue('--out', args);
  if (!input || !output || args.length !== 4 || existsSync(output)) throw Error('INPUT_OUTPUT_REQUIRED_OR_EXISTS');
  const report = await analyzeCapture(input);
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, {flag: 'wx', mode: 0o600});
  process.stdout.write(JSON.stringify({output: basename(output), sourceBytes: report.source.bytes, sourceSha256: report.source.sha256,
    records: report.source.records, acceptedBooks: report.source.acceptedBooks, samples: report.samples,
    model30s: report.model30s?.test ?? null, counterfactualExecution: report.counterfactualExecution,
    profitabilityEstablished: false}) + '\n');
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { process.exitCode = await main(process.argv.slice(2)); }
  catch (error) { process.stderr.write(`Book-pressure replay failed: ${error.message}\n`); process.exitCode = 1; }
}
