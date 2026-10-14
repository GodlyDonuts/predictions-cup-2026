// Fixed, offline-only ridge model for spread-aware executable markouts.
// Prices and labels are per-share dollar values; this is not a fill simulator.
export const MARKET_EDGE_RIDGE_PROTOCOL = Object.freeze({
  featureNames: Object.freeze([
    'sidePrice', 'lagReturn1s', 'lagReturn5s', 'lagReturn15s', 'spread',
    'imbalanceTop1', 'imbalanceTop5', 'imbalanceTop20',
    'logDepthTop1', 'logDepthTop5', 'logDepthTop20',
  ]),
  lookbacksMs: Object.freeze([1_000, 5_000, 15_000]),
  lagPointToleranceMs: 2_000,
  maximumSourceAgeMs: 5_000,
  decisionLatencyMs: 3_000,
  holdingHorizonMs: 5_000,
  maximumEntryWaitMs: 10_000,
  maximumExitWaitMs: 10_000,
  maximumObservationGapMs: 10_000,
  feesPerShare: 0,
  officialTick: 0.005,
  ridgeAlpha: 0.01,
  positivePredictionThreshold: 0,
});

export function marketBatchUsableAfterRecoveryV1({ready, invalidated}) {
  return ready === true && invalidated !== true;
}

export function captureLifecycleCanBeAppliedV1({configRows, expectedMarkets}) {
  return Number.isSafeInteger(configRows) && Number.isSafeInteger(expectedMarkets)
    && expectedMarkets > 0 && configRows === expectedMarkets;
}

const requireFinite = (x, code) => {
  if (!Number.isFinite(x)) throw Error(`MARKET_EDGE_${code}`);
  return x;
};

function lowerBound(rows, at, field = 'observedMs') {
  let lo = 0, hi = rows.length;
  while (lo < hi) {
    const mid = lo + ((hi - lo) >> 1);
    if (rows[mid][field] < at) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function lastAtOrBefore(rows, at, beforeIndex) {
  let lo = 0, hi = beforeIndex + 1;
  while (lo < hi) {
    const mid = lo + ((hi - lo) >> 1);
    if (rows[mid].observedMs <= at) lo = mid + 1;
    else hi = mid;
  }
  return lo ? rows[lo - 1] : null;
}

function quoteAtOrAfter(rows, startIndex, targetMs, maxWaitMs, segment) {
  let i = Math.max(startIndex, lowerBound(rows, targetMs));
  while (i < rows.length) {
    const row = rows[i++];
    if (row.observedMs - targetMs > maxWaitMs) return null;
    if (row.segment !== segment) return null;
    if (row.sourceMs >= targetMs && row.sourceAgeMs <= MARKET_EDGE_RIDGE_PROTOCOL.maximumSourceAgeMs) return row;
  }
  return null;
}

// Preserve the arrival-time order from the capture. Unexplained observation
// gaps and source-clock rollbacks/gaps start a new segment; they are never
// silently sorted into a plausible price path.
export function segmentSeriesGapsV1(rows) {
  if (!Array.isArray(rows)) throw Error('MARKET_EDGE_ROWS');
  if (!rows.length) return { rows, gapBreaks: 0, chronological: true };
  let nextSegment = 1;
  for (const row of rows) if (Number.isSafeInteger(row.segment) && row.segment >= nextSegment) nextSegment = row.segment + 1;
  const declaredSegments = rows.map(row => row.segment);
  let activeSegment = declaredSegments[0];
  let previousDeclaredSegment = declaredSegments[0];
  let gapBreaks = 0, declaredBreaks = 0, observationGapBreaks = 0, sourceGapBreaks = 0, sourceRollbackBreaks = 0;
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    if (!Number.isSafeInteger(row.observedMs) || !Number.isSafeInteger(row.sourceMs) || row.sourceMs > row.observedMs
      || !Number.isSafeInteger(row.segment)) throw Error('MARKET_EDGE_SERIES_POINT');
    if (i === 0) continue;
    const prev = rows[i - 1];
    if (row.marketId !== prev.marketId) throw Error('MARKET_EDGE_SERIES_MARKET');
    if (row.observedMs < prev.observedMs) throw Error('MARKET_EDGE_SERIES_ORDER');
    const declaredSegment = declaredSegments[i];
    const declaredBoundary = declaredSegment !== previousDeclaredSegment;
    const sourceRollback = row.sourceMs < prev.sourceMs;
    const observedGap = row.observedMs - prev.observedMs > MARKET_EDGE_RIDGE_PROTOCOL.maximumObservationGapMs;
    const sourceGap = row.sourceMs - prev.sourceMs > MARKET_EDGE_RIDGE_PROTOCOL.maximumObservationGapMs;
    if (declaredBoundary) declaredBreaks++;
    if (sourceRollback) sourceRollbackBreaks++;
    if (observedGap) observationGapBreaks++;
    if (sourceGap) sourceGapBreaks++;
    if (declaredBoundary || sourceRollback || observedGap || sourceGap) {
      activeSegment = nextSegment++;
      gapBreaks++;
    }
    row.segment = activeSegment;
    previousDeclaredSegment = declaredSegment;
  }
  return { rows, gapBreaks, declaredBreaks, observationGapBreaks, sourceGapBreaks, sourceRollbackBreaks, chronological: true };
}

function sideFeatures(rows, index, side) {
  const current = rows[index], cfg = MARKET_EDGE_RIDGE_PROTOCOL;
  if (current.sourceAgeMs < 0 || current.sourceAgeMs > cfg.maximumSourceAgeMs) return null;
  const mid = (current.bid + current.ask) / 2;
  const sign = side === 'YES' ? 1 : -1;
  const values = [side === 'YES' ? mid : 1 - mid];
  for (const lag of cfg.lookbacksMs) {
    const past = lastAtOrBefore(rows, current.observedMs - lag, index - 1);
    if (!past || past.segment !== current.segment || current.observedMs - lag - past.observedMs > cfg.lagPointToleranceMs
      || past.sourceAgeMs < 0 || past.sourceAgeMs > cfg.maximumSourceAgeMs) return null;
    values.push(sign * (mid - (past.bid + past.ask) / 2));
  }
  values.push(current.ask - current.bid);
  for (const depth of [1, 5, 20]) {
    const bid = current[`bidDepth${depth}`], ask = current[`askDepth${depth}`];
    const total = bid + ask;
    values.push(sign * (bid - ask) / total);
  }
  for (const depth of [1, 5, 20]) values.push(Math.log1p(current[`bidDepth${depth}`] + current[`askDepth${depth}`]));
  if (values.length !== cfg.featureNames.length || values.some(x => !Number.isFinite(x))) return null;
  return values;
}

function executableLabel(rows, index, side) {
  const cfg = MARKET_EDGE_RIDGE_PROTOCOL, decision = rows[index];
  const entryTarget = decision.observedMs + cfg.decisionLatencyMs;
  const entry = quoteAtOrAfter(rows, index + 1, entryTarget, cfg.maximumEntryWaitMs, decision.segment);
  if (!entry) return null;
  // A one-share marketable entry must be present on the relevant displayed side.
  const entryDepth = side === 'YES' ? entry.askDepth1 : entry.bidDepth1;
  if (!(entryDepth >= 1)) return null;
  // The holding clock begins when the entry quote is actually available to
  // the strategy. Requiring both source and observation clocks after that
  // target prevents a source timestamp behind its receipt from shortening the
  // realized holding interval.
  const exitTarget = entry.observedMs + cfg.holdingHorizonMs;
  const exit = quoteAtOrAfter(rows, lowerBound(rows, exitTarget), exitTarget, cfg.maximumExitWaitMs, decision.segment);
  if (!exit) return null;
  const exitDepth = side === 'YES' ? exit.bidDepth1 : exit.askDepth1;
  if (!(exitDepth >= 1)) return null;
  const net = side === 'YES' ? exit.bid - entry.ask : entry.bid - exit.ask;
  const provenance = point => point.provenance ?? { marketId: point.marketId, segment: point.segment,
    observedMs: point.observedMs, sourceMs: point.sourceMs };
  return { net: requireFinite(net, 'LABEL'), entryObservedMs: entry.observedMs, exitObservedMs: exit.observedMs,
    entrySourceMs: entry.sourceMs, exitSourceMs: exit.sourceMs, decisionSegment: decision.segment,
    entrySegment: entry.segment, exitSegment: exit.segment, realizedObservedHoldingMs: exit.observedMs - entry.observedMs,
    decisionProvenance: provenance(decision), entryProvenance: provenance(entry), exitProvenance: provenance(exit) };
}

export function candidatesForSeriesV1(rows, { includeLabels = true } = {}) {
  if (!Array.isArray(rows)) throw Error('MARKET_EDGE_ROWS');
  for (let i = 1; i < rows.length; i++) {
    if (rows[i].marketId !== rows[i - 1].marketId) throw Error('MARKET_EDGE_SERIES_MARKET');
    if (rows[i].observedMs < rows[i - 1].observedMs) throw Error('MARKET_EDGE_SERIES_ORDER');
  }
  const out = [];
  for (let i = 0; i < rows.length; i++) {
    const point = rows[i];
    if (!Number.isSafeInteger(point.observedMs) || !Number.isSafeInteger(point.sourceMs)
      || point.sourceMs > point.observedMs || !Number.isSafeInteger(point.segment)
      || !Number.isFinite(point.bid) || !Number.isFinite(point.ask) || point.bid <= 0 || point.ask >= 1 || point.ask <= point.bid) continue;
    for (const side of ['YES', 'NO']) {
      const features = sideFeatures(rows, i, side);
      if (!features) continue;
      const label = includeLabels ? executableLabel(rows, i, side) : null;
      out.push({ marketId: point.marketId, side, decisionMs: point.observedMs,
        decisionProvenance: point.provenance ?? {marketId: point.marketId, segment: point.segment,
          observedMs: point.observedMs, sourceMs: point.sourceMs}, features, label });
    }
  }
  return out;
}

function solvePositiveDefinite(matrix, vector) {
  const n = vector.length;
  const a = matrix.map((row, i) => [...row, vector[i]]);
  for (let p = 0; p < n; p++) {
    let best = p;
    for (let r = p + 1; r < n; r++) if (Math.abs(a[r][p]) > Math.abs(a[best][p])) best = r;
    [a[p], a[best]] = [a[best], a[p]];
    const pivot = a[p][p];
    if (!Number.isFinite(pivot) || Math.abs(pivot) < 1e-12) throw Error('MARKET_EDGE_SINGULAR');
    for (let c = p; c <= n; c++) a[p][c] /= pivot;
    for (let r = 0; r < n; r++) if (r !== p) {
      const factor = a[r][p];
      for (let c = p; c <= n; c++) a[r][c] -= factor * a[p][c];
    }
  }
  return a.map(row => row[n]);
}

export function fitMarketEdgeRidgeV1(trainingCandidates) {
  if (!Array.isArray(trainingCandidates)) throw Error('MARKET_EDGE_TRAINING');
  const rows = trainingCandidates.filter(x => Array.isArray(x.features) && x.features.length === MARKET_EDGE_RIDGE_PROTOCOL.featureNames.length
    && x.features.every(Number.isFinite) && Number.isFinite(x.label?.net));
  if (rows.length < 20) throw Error('MARKET_EDGE_TRAINING_ROWS');
  const p = MARKET_EDGE_RIDGE_PROTOCOL.featureNames.length;
  const mean = Array(p).fill(0), m2 = Array(p).fill(0);
  let ySum = 0;
  rows.forEach((r, n) => {
    ySum += r.label.net;
    for (let j = 0; j < p; j++) {
      const delta = r.features[j] - mean[j];
      mean[j] += delta / (n + 1);
      m2[j] += delta * (r.features[j] - mean[j]);
    }
  });
  const scale = m2.map(x => Math.sqrt(Math.max(0, x / rows.length)) || 1);
  const dim = p + 1, gram = Array.from({ length: dim }, () => Array(dim).fill(0)), cross = Array(dim).fill(0);
  let yy = 0;
  for (const row of rows) {
    const x = [1, ...row.features.map((v, j) => (v - mean[j]) / scale[j])];
    for (let a = 0; a < dim; a++) {
      cross[a] += x[a] * row.label.net;
      for (let b = 0; b < dim; b++) gram[a][b] += x[a] * x[b];
    }
    yy += row.label.net * row.label.net;
  }
  for (let a = 0; a < dim; a++) {
    cross[a] /= rows.length;
    for (let b = 0; b < dim; b++) gram[a][b] /= rows.length;
  }
  // Objective is mean squared error + alpha * sum(slopes^2); intercept is unpenalized.
  for (let j = 1; j < dim; j++) gram[j][j] += MARKET_EDGE_RIDGE_PROTOCOL.ridgeAlpha;
  const coefficients = solvePositiveDefinite(gram, cross);
  let residualSse = 0;
  for (const row of rows) {
    let prediction = coefficients[0];
    for (let j = 0; j < p; j++) prediction += coefficients[j + 1] * ((row.features[j] - mean[j]) / scale[j]);
    residualSse += (prediction - row.label.net) ** 2;
  }
  return Object.freeze({
    featureNames: MARKET_EDGE_RIDGE_PROTOCOL.featureNames,
    mean: Object.freeze(mean), scale: Object.freeze(scale), coefficients: Object.freeze(coefficients),
    trainingRows: rows.length, trainingMean: ySum / rows.length,
    trainingRmse: Math.sqrt(residualSse / rows.length),
    alpha: MARKET_EDGE_RIDGE_PROTOCOL.ridgeAlpha,
  });
}

export function predictMarketEdgeRidgeV1(model, features) {
  if (!model || !Array.isArray(features) || features.length !== model.mean.length || features.some(x => !Number.isFinite(x))) throw Error('MARKET_EDGE_PREDICT_INPUT');
  let value = model.coefficients[0];
  for (let j = 0; j < features.length; j++) value += model.coefficients[j + 1] * ((features[j] - model.mean[j]) / model.scale[j]);
  return requireFinite(value, 'PREDICTION');
}

function aggregate(outcomes) {
  if (!outcomes.length) return { n: 0, meanPerShare: null, medianPerShare: null, positiveFraction: null,
    oneTickPerLegMeanPerShare: null, oneTickPerLegPositiveFraction: null, marketClusters: 0, equalWeightedMarketMean: null,
    marketClusterStandardError: null, positiveMarketClusterCount: 0, marketClusterMeans: [] };
  const sorted = outcomes.map(x => x.net).sort((a, b) => a - b), mean = outcomes.reduce((s, x) => s + x.net, 0) / outcomes.length;
  const clusters = new Map();
  for (const x of outcomes) { const c = clusters.get(x.marketId) ?? { n: 0, sum: 0 }; c.n++; c.sum += x.net; clusters.set(x.marketId, c); }
  const marketMeans = [...clusters.values()].map(c => c.sum / c.n), marketMean = marketMeans.reduce((a, b) => a + b, 0) / marketMeans.length;
  const variance = marketMeans.length > 1 ? marketMeans.reduce((s, x) => s + (x - marketMean) ** 2, 0) / (marketMeans.length - 1) : null;
  const stress = MARKET_EDGE_RIDGE_PROTOCOL.officialTick * 2;
  return {
    n: outcomes.length,
    meanPerShare: mean,
    medianPerShare: sorted.length % 2 ? sorted[(sorted.length - 1) / 2] : (sorted[sorted.length / 2 - 1] + sorted[sorted.length / 2]) / 2,
    sumOverlappingOneShareLabels_notPortfolioPnl: outcomes.reduce((s, x) => s + x.net, 0),
    positiveFraction: outcomes.filter(x => x.net > 0).length / outcomes.length,
    oneTickPerLegStressPerShare: stress,
    oneTickPerLegMeanPerShare: mean - stress,
    oneTickPerLegPositiveFraction: outcomes.filter(x => x.net - stress > 0).length / outcomes.length,
    marketClusters: marketMeans.length,
    equalWeightedMarketMean: marketMean,
    marketClusterStandardError: variance === null ? null : Math.sqrt(variance / marketMeans.length),
    positiveMarketClusterCount: marketMeans.filter(x => x > 0).length,
    marketClusterMeans: [...clusters.entries()].map(([marketId, c]) => ({marketId, n: c.n, meanPerShare: c.sum / c.n})),
  };
}

function quantile(sorted, q) {
  if (!sorted.length) return null;
  const x = (sorted.length - 1) * q, lo = Math.floor(x), hi = Math.ceil(x);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (x - lo);
}

export function scoreMarketEdgeRidgeV1(model, candidates) {
  if (!Array.isArray(candidates)) throw Error('MARKET_EDGE_HOLDOUT');
  const predictions = [], predictionRows = [], labeled = [], selected = [], selectedLabeled = [];
  let modelSse = 0, meanSse = 0;
  for (const row of candidates) {
    const predictedNet = predictMarketEdgeRidgeV1(model, row.features);
    predictions.push(predictedNet);
    const item = { marketId: row.marketId, side: row.side, decisionMs: row.decisionMs, predictedNet };
    const label = Number.isFinite(row.label?.net) ? row.label : null;
    const captureIndex = provenance => Number.isSafeInteger(provenance?.captureIndex) ? provenance.captureIndex : null;
    predictionRows.push({
      rowIndex: predictionRows.length,
      captureId: row.decisionProvenance?.captureId ?? label?.decisionProvenance?.captureId ?? null,
      decisionCaptureIndex: captureIndex(row.decisionProvenance ?? label?.decisionProvenance),
      marketId: row.marketId, side: row.side, decisionObservedMs: row.decisionMs, predictedNetPerShare: predictedNet,
      outcomeStatus: label ? 'labeled' : 'censored', outcomeNetPerShare: label?.net ?? null,
      entryCaptureIndex: captureIndex(label?.entryProvenance), exitCaptureIndex: captureIndex(label?.exitProvenance),
      entryObservedMs: label?.entryObservedMs ?? null, exitObservedMs: label?.exitObservedMs ?? null,
      entrySourceMs: label?.entrySourceMs ?? null, exitSourceMs: label?.exitSourceMs ?? null,
    });
    if (predictedNet > MARKET_EDGE_RIDGE_PROTOCOL.positivePredictionThreshold) selected.push(item);
    if (Number.isFinite(row.label?.net)) {
      const scored = { ...item, net: row.label.net };
      labeled.push(scored);
      modelSse += (predictedNet - row.label.net) ** 2;
      meanSse += (model.trainingMean - row.label.net) ** 2;
      if (predictedNet > MARKET_EDGE_RIDGE_PROTOCOL.positivePredictionThreshold) selectedLabeled.push(scored);
    }
  }
  const predictionsSorted = predictions.slice().sort((a, b) => a - b);
  const selectedSummary = aggregate(selectedLabeled), unconditional = aggregate(labeled);
  const bySide = {};
  for (const side of ['YES', 'NO']) {
    const all = labeled.filter(x => x.side === side), picked = selectedLabeled.filter(x => x.side === side);
    bySide[side] = { candidateCount: candidates.filter(x => x.side === side).length, labeled: aggregate(all), selected: aggregate(picked),
      selectedCandidateCount: selected.filter(x => x.side === side).length,
      selectedCensoredOutcomeCount: selected.filter(x => x.side === side).length - picked.length };
  }
  return {
    candidatePredictions: predictions.length,
    predictionRows,
    predictions: { positive: predictions.filter(x => x > 0).length, negativeOrZero: predictions.filter(x => x <= 0).length,
      mean: predictions.length ? predictions.reduce((s, x) => s + x, 0) / predictions.length : null,
      median: quantile(predictionsSorted, 0.5), p90: quantile(predictionsSorted, 0.9), p99: quantile(predictionsSorted, 0.99) },
    fixedPositiveThreshold: 0,
    selectedCandidateCount: selected.length,
    selectedWithOutcomeCount: selectedLabeled.length,
    selectedCensoredOutcomeCount: selected.length - selectedLabeled.length,
    selected: selectedSummary,
    unconditionalAllLabeledCandidateControl: unconditional,
    bySide,
    labeledCandidates: labeled.length,
    predictionRmse: labeled.length ? Math.sqrt(modelSse / labeled.length) : null,
    trainingMeanControlRmse: labeled.length ? Math.sqrt(meanSse / labeled.length) : null,
    actualFillsProven: false,
    realizedAccountPnl: null,
    profitabilityEstablished: false,
  };
}

