// Frozen, offline-only short-horizon model for executable one-share returns.
// Labels use the delayed arrival ask and later executable bid; they do not
// condition on the path between decision and arrival being favorable.
export const EXECUTABLE_RETURN_POLICY_V1 = Object.freeze({
  horizonMs: 300_000,
  latencyMs: 3_000,
  maximumArrivalWaitMs: 10_000,
  maximumExitWaitMs: 30_000,
  ridgeAlpha: 0.01,
  adverseTicksPerLeg: 1,
  tick: 0.005,
  feesPerShare: 0,
  oneShareCapacity: 1,
});

const need = (value, code) => { if (!value) throw Error(`EXEC_RETURN_${code}`); };

function lowerBoundRequestAt(rows, target, start = 0) {
  let lo = start, hi = rows.length;
  while (lo < hi) {
    const mid = lo + ((hi - lo) >> 1);
    if (rows[mid].requestAt < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function normalizedPrice(row, side) {
  return side === 'YES'
    ? {bid: row.bid, ask: row.ask, mid: row.mid, entryCapacity: row.buyCapacity, exitCapacity: row.sellCapacity}
    : {bid: 1 - row.ask, ask: 1 - row.bid, mid: 1 - row.mid, entryCapacity: row.sellCapacity, exitCapacity: row.buyCapacity};
}

export function executableReturnFeaturesV1(row, side) {
  need(side === 'YES' || side === 'NO', 'SIDE');
  need(Array.isArray(row?.features) && row.features.length >= 5, 'FEATURES');
  const sign = side === 'YES' ? 1 : -1;
  const spread = row.ask - row.bid;
  const mid = side === 'YES' ? row.mid : 1 - row.mid;
  const imbalance = sign * row.features[1];
  const pastChange = sign * row.features[3];
  const values = [mid, spread, imbalance, imbalance * spread / 2, pastChange, mid * (1 - mid)];
  need(values.every(Number.isFinite), 'FEATURE_VALUE');
  return values;
}

function validPrice(row) {
  return row && Number.isFinite(row.at) && Number.isFinite(row.requestAt)
    && Number.isFinite(row.bid) && Number.isFinite(row.ask) && row.bid >= 0.005
    && row.ask <= 0.995 && row.bid <= row.ask && Number.isFinite(row.mid)
    && Number.isSafeInteger(row.sequence);
}

export function executableReturnLabelsV1(streams, policy = EXECUTABLE_RETURN_POLICY_V1) {
  need(Array.isArray(streams), 'STREAMS');
  const labels = [];
  const excluded = {invalidDecision: 0, noArrival: 0, lateArrival: 0, noEntryDepth: 0,
    noExit: 0, lateExit: 0, noExitDepth: 0, invalidNet: 0};

  for (const rows of streams) {
    need(Array.isArray(rows), 'SERIES');
    for (let i = 1; i < rows.length; i++) {
      need(rows[i].marketId === rows[i - 1].marketId, 'MARKET_ORDER');
      need(rows[i].at >= rows[i - 1].at && rows[i].requestAt >= rows[i - 1].requestAt
        && rows[i].sequence > rows[i - 1].sequence, 'TIME_ORDER');
    }
    for (let i = 0; i < rows.length; i++) {
      const decision = rows[i];
      if (!validPrice(decision) || !Array.isArray(decision.features) || decision.features.length < 5) {
        excluded.invalidDecision++;
        continue;
      }
      const arrivalTarget = decision.at + policy.latencyMs;
      const arrivalIndex = lowerBoundRequestAt(rows, arrivalTarget, i + 1);
      if (arrivalIndex >= rows.length) { excluded.noArrival++; continue; }
      const arrival = rows[arrivalIndex];
      if (arrival.sequence <= decision.sequence || arrival.requestAt <= decision.at) { excluded.noArrival++; continue; }
      if (arrival.requestAt - arrivalTarget > policy.maximumArrivalWaitMs) { excluded.lateArrival++; continue; }
      if (!validPrice(arrival)) { excluded.noArrival++; continue; }

      const exitTarget = arrival.at + policy.horizonMs;
      const exitIndex = lowerBoundRequestAt(rows, exitTarget, arrivalIndex + 1);
      if (exitIndex >= rows.length || rows[exitIndex].sequence <= arrival.sequence) { excluded.noExit++; continue; }
      const exit = rows[exitIndex];
      if (exit.requestAt - exitTarget > policy.maximumExitWaitMs) { excluded.lateExit++; continue; }
      if (!validPrice(exit)) { excluded.noExit++; continue; }

      for (const side of ['YES', 'NO']) {
        const entry = normalizedPrice(arrival, side), liquidation = normalizedPrice(exit, side);
        if (!(entry.entryCapacity >= policy.oneShareCapacity)) { excluded.noEntryDepth++; continue; }
        if (!(liquidation.exitCapacity >= policy.oneShareCapacity)) { excluded.noExitDepth++; continue; }
        const net = liquidation.bid - entry.ask - policy.feesPerShare;
        if (!Number.isFinite(net)) { excluded.invalidNet++; continue; }
        labels.push({
          marketId: decision.marketId,
          side,
          sequence: decision.sequence,
          decisionAt: decision.at,
          entryAt: arrival.at,
          exitAt: exit.at,
          entryAsk: entry.ask,
          exitBid: liquidation.bid,
          net,
          stressedNet: net - 2 * policy.adverseTicksPerLeg * policy.tick,
          features: executableReturnFeaturesV1(decision, side),
        });
      }
    }
  }
  labels.sort((a, b) => a.decisionAt - b.decisionAt || a.marketId.localeCompare(b.marketId)
    || a.sequence - b.sequence || a.side.localeCompare(b.side));
  return {labels, excluded};
}

function solve(matrix, vector) {
  const n = vector.length, a = matrix.map((row, i) => [...row, vector[i]]);
  for (let p = 0; p < n; p++) {
    let best = p;
    for (let r = p + 1; r < n; r++) if (Math.abs(a[r][p]) > Math.abs(a[best][p])) best = r;
    [a[p], a[best]] = [a[best], a[p]];
    const pivot = a[p][p];
    need(Number.isFinite(pivot) && Math.abs(pivot) > 1e-12, 'SINGULAR');
    for (let c = p; c <= n; c++) a[p][c] /= pivot;
    for (let r = 0; r < n; r++) if (r !== p) {
      const factor = a[r][p];
      for (let c = p; c <= n; c++) a[r][c] -= factor * a[p][c];
    }
  }
  return a.map(row => row[n]);
}

export function fitExecutableReturnModelV1(labels, cutoff, policy = EXECUTABLE_RETURN_POLICY_V1) {
  need(Number.isFinite(cutoff), 'CUTOFF');
  const train = labels.filter(row => row.decisionAt < cutoff && row.exitAt <= cutoff
    && Array.isArray(row.features) && row.features.length === 6 && row.features.every(Number.isFinite)
    && Number.isFinite(row.net));
  need(train.length >= 40, 'TRAINING_ROWS');
  const dimensions = 6, n = train.length;
  const mean = Array(dimensions).fill(0), m2 = Array(dimensions).fill(0);
  let targetMean = 0;
  for (let i = 0; i < n; i++) {
    const row = train[i];
    targetMean += row.net / n;
    for (let j = 0; j < dimensions; j++) {
      const delta = row.features[j] - mean[j];
      mean[j] += delta / (i + 1);
      m2[j] += delta * (row.features[j] - mean[j]);
    }
  }
  const scale = m2.map(value => Math.sqrt(Math.max(0, value / n)) || 1);
  const size = dimensions + 1, gram = Array.from({length: size}, () => Array(size).fill(0));
  const cross = Array(size).fill(0);
  for (const row of train) {
    const x = [1, ...row.features.map((value, j) => (value - mean[j]) / scale[j])];
    for (let i = 0; i < size; i++) {
      cross[i] += x[i] * row.net / n;
      for (let j = 0; j < size; j++) gram[i][j] += x[i] * x[j] / n;
    }
  }
  for (let j = 1; j < size; j++) gram[j][j] += policy.ridgeAlpha;
  const coefficients = solve(gram, cross);
  let squaredError = 0;
  for (const row of train) {
    const prediction = predictExecutableReturnV1({mean, scale, coefficients}, row.features);
    squaredError += (prediction - row.net) ** 2;
  }
  return Object.freeze({mean: Object.freeze(mean), scale: Object.freeze(scale),
    coefficients: Object.freeze(coefficients), trainingRows: n, trainingMean: targetMean,
    trainingRmse: Math.sqrt(squaredError / n), cutoff, ridgeAlpha: policy.ridgeAlpha});
}

export function predictExecutableReturnV1(model, features) {
  need(model && Array.isArray(features) && features.length === model.mean.length
    && features.every(Number.isFinite), 'PREDICT_INPUT');
  let value = model.coefficients[0];
  for (let j = 0; j < features.length; j++) value += model.coefficients[j + 1]
    * ((features[j] - model.mean[j]) / model.scale[j]);
  need(Number.isFinite(value), 'PREDICT_VALUE');
  return value;
}

function aggregate(rows) {
  if (!rows.length) return {n: 0, meanNet: null, medianNet: null, positiveFraction: null,
    stressedMeanNet: null, positiveMarketClusters: 0, marketClusters: 0,
    equalWeightedMarketMean: null, marketClusterStandardError: null};
  const values = rows.map(row => row.net).sort((a, b) => a - b);
  const clusters = new Map();
  for (const row of rows) {
    const entry = clusters.get(row.marketId) ?? {sum: 0, n: 0};
    entry.sum += row.net; entry.n++;
    clusters.set(row.marketId, entry);
  }
  const marketMeans = [...clusters.values()].map(row => row.sum / row.n);
  const equalWeightedMarketMean = marketMeans.reduce((sum, value) => sum + value, 0) / marketMeans.length;
  const variance = marketMeans.length > 1
    ? marketMeans.reduce((sum, value) => sum + (value - equalWeightedMarketMean) ** 2, 0) / (marketMeans.length - 1)
    : null;
  return {n: rows.length, meanNet: rows.reduce((sum, row) => sum + row.net, 0) / rows.length,
    medianNet: values.length % 2 ? values[(values.length - 1) / 2]
      : (values[values.length / 2 - 1] + values[values.length / 2]) / 2,
    positiveFraction: rows.filter(row => row.net > 0).length / rows.length,
    stressedMeanNet: rows.reduce((sum, row) => sum + row.stressedNet, 0) / rows.length,
    positiveMarketClusters: marketMeans.filter(value => value > 0).length,
    marketClusters: marketMeans.length, equalWeightedMarketMean,
    marketClusterStandardError: variance === null ? null : Math.sqrt(variance / marketMeans.length)};
}

export function scoreExecutableReturnModelV1(model, decisions, labels, cutoff, policy = EXECUTABLE_RETURN_POLICY_V1) {
  const outcomeByDecision = new Map(labels.filter(row => row.decisionAt >= cutoff)
    .map(row => [`${row.marketId}:${row.sequence}:${row.side}`, row]));
  const sorted = decisions.filter(row => row.at >= cutoff)
    .sort((a, b) => a.at - b.at || a.marketId.localeCompare(b.marketId) || a.sequence - b.sequence);
  const blockedUntil = new Map(), selected = [], censored = [];
  for (const decision of sorted) {
    if ((blockedUntil.get(decision.marketId) ?? -Infinity) > decision.at) continue;
    const sides = ['YES', 'NO'].map(side => {
      const features = executableReturnFeaturesV1(decision, side);
      const predictedNet = predictExecutableReturnV1(model, features);
      const stressedPrediction = predictedNet - 2 * policy.adverseTicksPerLeg * policy.tick;
      return {side, predictedNet, stressedPrediction};
    }).sort((a, b) => b.stressedPrediction - a.stressedPrediction
      || (a.side === b.side ? 0 : a.side === 'YES' ? -1 : 1));
    const best = sides[0];
    if (!(best.stressedPrediction > 0)) continue;
    // Hold the market lock through the latest permissible arrival, holding
    // interval, and exit observation so delayed receipts cannot overlap.
    blockedUntil.set(decision.marketId, decision.at + policy.latencyMs
      + policy.maximumArrivalWaitMs + policy.horizonMs + policy.maximumExitWaitMs);
    const key = `${decision.marketId}:${decision.sequence}:${best.side}`;
    const outcome = outcomeByDecision.get(key);
    const record = {marketId: decision.marketId, side: best.side, decisionAt: decision.at,
      predictedNet: best.predictedNet, stressedPrediction: best.stressedPrediction,
      actualNet: outcome?.net ?? null, actualStressedNet: outcome?.stressedNet ?? null};
    if (outcome) selected.push({...record, net: outcome.net, stressedNet: outcome.stressedNet});
    else censored.push(record);
  }
  return {decisionRows: sorted.length, selectedDecisions: selected.length + censored.length,
    selectedWithOutcome: selected.length, selectedCensored: censored.length,
    selected: aggregate(selected), censored: censored.length,
    censoredRate: selected.length + censored.length ? censored.length / (selected.length + censored.length) : null,
    selectedRecords: selected, censoredRecords: censored,
    actualFillsProven: false, profitabilityEstablished: false};
}
