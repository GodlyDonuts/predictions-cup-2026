import {price, SCALE} from '../paper/fixed.mjs';

export const EXECUTABLE_TREND_REVERSION_V1 = Object.freeze({
  decisionLatencyMs: 3_000,
  lookbackMs: Object.freeze([300_000, 900_000]),
  holdingMs: Object.freeze([300_000, 900_000, 3_600_000]),
  decisionSpacingMs: 60_000,
  maxLookbackDriftMs: 30_000,
  maxBookAgeMs: 5_000,
  maxEntryDelayMs: 30_000,
  maxExitDelayMs: 30_000,
  fees: '0.000000',
  sharesPerTrade: 1,
});

const instant = value => {
  if (typeof value !== 'string' || !Number.isFinite(Date.parse(value))) throw Error('TREND_CLOCK');
  return Date.parse(value);
};
const priceUnits = value => price(value);
const positiveInts = (values, code) => {
  if (!Array.isArray(values) || values.length === 0
    || values.some(value => !Number.isSafeInteger(value) || value <= 0)) throw Error(code);
  return [...new Set(values)].sort((a, b) => a - b);
};
const lowerBound = (rows, target, start = 0) => {
  let lo = start, hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (rows[mid].atMs < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
};
const upperBound = (rows, target) => {
  let lo = 0, hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (rows[mid].atMs <= target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
};
const median = values => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  const i = Math.floor(sorted.length / 2);
  const units = sorted.length % 2 ? sorted[i] : (sorted[i - 1] + sorted[i]) / 2n;
  return Number(units) / Number(SCALE);
};
const quantile = (values, q) => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  return Number(sorted[Math.floor((sorted.length - 1) * q)]) / Number(SCALE);
};
const numericSummary = values => {
  if (!values.length) return {count: 0, p50: null, p95: null, max: null};
  const sorted = [...values].sort((a, b) => a - b);
  return {count: sorted.length, p50: sorted[Math.floor((sorted.length - 1) * 0.5)],
    p95: sorted[Math.floor((sorted.length - 1) * 0.95)], min: sorted[0], max: sorted.at(-1)};
};
function newAggregate() {
  return {decisions: 0, signals: 0, entered: 0, completed: 0, positive: 0, negative: 0, flat: 0,
    noLookback: 0, flatSignal: 0, entryNoBook: 0, entryDelayExceeded: 0, entryDepthInsufficient: 0,
    exitNoBook: 0, exitDelayExceeded: 0, exitDepthInsufficient: 0, invalidationCensored: 0,
    pnlMicros: 0n, entryCostMicros: 0n, markoutsMicros: [], byMarket: new Map(), byUtcDay: new Map()};
}
function addBreakdown(map, key, pnlMicros, entryCostMicros) {
  let item = map.get(key);
  if (!item) { item = {trades: 0, wins: 0, losses: 0, pnlMicros: 0n, costMicros: 0n}; map.set(key, item); }
  item.trades++;
  if (pnlMicros > 0n) item.wins++;
  else if (pnlMicros < 0n) item.losses++;
  item.pnlMicros += pnlMicros;
  item.costMicros += entryCostMicros;
}
function finishAggregate(row) {
  const dollars = value => Number(value) / Number(SCALE);
  return {
    decisions: row.decisions, signals: row.signals, entered: row.entered, completed: row.completed,
    wins: row.positive, losses: row.negative, flats: row.flat,
    winRate: row.completed ? row.positive / row.completed : null,
    totalNetPnlOneShare: dollars(row.pnlMicros), totalEntryCost: dollars(row.entryCostMicros),
    meanNetPnlPerShare: row.completed ? dollars(row.pnlMicros) / row.completed : null,
    medianNetPnlPerShare: median(row.markoutsMicros),
    p10NetPnlPerShare: quantile(row.markoutsMicros, 0.1), p90NetPnlPerShare: quantile(row.markoutsMicros, 0.9),
    censoring: {noLookback: row.noLookback, flatSignal: row.flatSignal, entryNoBook: row.entryNoBook,
      entryDelayExceeded: row.entryDelayExceeded, entryDepthInsufficient: row.entryDepthInsufficient,
      exitNoBook: row.exitNoBook, exitDelayExceeded: row.exitDelayExceeded,
      exitDepthInsufficient: row.exitDepthInsufficient, invalidation: row.invalidationCensored},
    byMarket: [...row.byMarket.entries()].map(([marketId, item]) => ({marketId, trades: item.trades,
      wins: item.wins, losses: item.losses, netPnlOneShare: dollars(item.pnlMicros),
      entryCost: dollars(item.costMicros)})).sort((a, b) => b.netPnlOneShare - a.netPnlOneShare || a.marketId.localeCompare(b.marketId)),
    byUtcDay: [...row.byUtcDay.entries()].map(([utcDay, item]) => ({utcDay, trades: item.trades,
      wins: item.wins, losses: item.losses, netPnlOneShare: dollars(item.pnlMicros),
      entryCost: dollars(item.costMicros)})).sort((a, b) => a.utcDay.localeCompare(b.utcDay)),
  };
}

function normalizeObservation(raw, cfg) {
  try {
    if (!raw || typeof raw.marketId !== 'string' || !raw.marketId
      || typeof raw.exchangeId !== 'string' || !raw.exchangeId
      || typeof raw.sequence !== 'string' || !/^(0|[1-9]\d*)$/.test(raw.sequence)
      || typeof raw.sessionId !== 'string' || typeof raw.segment !== 'string'
      || !Array.isArray(raw.bids) || !Array.isArray(raw.asks)
      || raw.bids.length < 1 || raw.asks.length < 1) return null;
    const atMs = instant(raw.receivedAt), sourceMs = instant(raw.sourceAt);
    const sourceAgeMs = atMs - sourceMs;
    if (sourceAgeMs < -cfg.maxFutureSourceSkewMs || sourceAgeMs > cfg.maxBookAgeMs)
      return {stale: true, staleReason: sourceAgeMs < -cfg.maxFutureSourceSkewMs ? 'sourceTooFarFuture' : 'sourceTooOld'};
    const bid = priceUnits(raw.bids[0].price), ask = priceUnits(raw.asks[0].price);
    const bidQty = raw.bids[0].quantity, askQty = raw.asks[0].quantity;
    if (bid <= 0n || ask <= bid || ask >= SCALE || !Number.isSafeInteger(bidQty) || bidQty < 1
      || !Number.isSafeInteger(askQty) || askQty < 1) return null;
    return {marketId: raw.marketId, exchangeId: raw.exchangeId, sessionId: raw.sessionId, segment: raw.segment,
      sequence: raw.sequence, atMs, sourceMs, sourceAgeMs, bid, ask, mid: (bid + ask) / 2n, bidQty, askQty};
  } catch { return null; }
}

function getPeriod(atMs, splitAtMs, purgeMs) {
  if (atMs >= splitAtMs) return 'heldout';
  if (atMs < splitAtMs - purgeMs) return 'development';
  return 'purged';
}

// Fixed sign-only comparison: follow a positive trailing YES-mid move for the
// trend rule, or buy YES after a negative move for the dip-reversion rule.
// Entries use the first later ask after the declared decision latency; exits
// use the first later bid after the holding horizon. These are counterfactual
// marketable markouts, not evidence that a live order would fill.
export function analyzeExecutableTrendReversionV1({observations, splitAt, options = {}} = {}) {
  if (!Array.isArray(observations)) throw Error('TREND_INPUT');
  const cfg = {
    decisionLatencyMs: options.decisionLatencyMs ?? EXECUTABLE_TREND_REVERSION_V1.decisionLatencyMs,
    lookbackMs: positiveInts(options.lookbackMs ?? [...EXECUTABLE_TREND_REVERSION_V1.lookbackMs], 'TREND_LOOKBACKS'),
    holdingMs: positiveInts(options.holdingMs ?? [...EXECUTABLE_TREND_REVERSION_V1.holdingMs], 'TREND_HORIZONS'),
    decisionSpacingMs: options.decisionSpacingMs ?? EXECUTABLE_TREND_REVERSION_V1.decisionSpacingMs,
    maxLookbackDriftMs: options.maxLookbackDriftMs ?? EXECUTABLE_TREND_REVERSION_V1.maxLookbackDriftMs,
    maxBookAgeMs: options.maxBookAgeMs ?? EXECUTABLE_TREND_REVERSION_V1.maxBookAgeMs,
    maxFutureSourceSkewMs: options.maxFutureSourceSkewMs ?? 2_000,
    maxEntryDelayMs: options.maxEntryDelayMs ?? EXECUTABLE_TREND_REVERSION_V1.maxEntryDelayMs,
    maxExitDelayMs: options.maxExitDelayMs ?? EXECUTABLE_TREND_REVERSION_V1.maxExitDelayMs,
  };
  for (const [key, value] of Object.entries(cfg)) if (key !== 'lookbackMs' && key !== 'holdingMs'
    && (!Number.isSafeInteger(value) || value < 0 || key === 'decisionSpacingMs' && value < 1)) throw Error(`TREND_${key.toUpperCase()}`);
  const splitAtMs = instant(splitAt);
  const purgeMs = Math.max(...cfg.lookbackMs) + cfg.decisionLatencyMs + cfg.maxEntryDelayMs
    + Math.max(...cfg.holdingMs) + cfg.maxExitDelayMs;
  const rejected = {invalid: 0, stale: 0, sourceTooOld: 0, sourceTooFarFuture: 0, duplicateSequence: 0};
  const acceptedSourceAgeMs = [], bidTopQuantity = [], askTopQuantity = [];
  const byMarket = new Map(), seenSequence = new Map();
  for (const raw of observations) {
    const row = normalizeObservation(raw, cfg);
    if (!row) { rejected.invalid++; continue; }
    if (row.stale) { rejected.stale++; rejected[row.staleReason]++; continue; }
    let seen = seenSequence.get(row.marketId);
    if (!seen) { seen = new Set(); seenSequence.set(row.marketId, seen); }
    if (seen.has(row.sequence)) { rejected.duplicateSequence++; continue; }
    seen.add(row.sequence);
    let rows = byMarket.get(row.marketId);
    if (!rows) { rows = []; byMarket.set(row.marketId, rows); }
    const prior = rows.at(-1);
    if (prior && (row.atMs < prior.atMs || row.sourceMs < prior.sourceMs
      || BigInt(row.sequence) <= BigInt(prior.sequence))) throw Error('TREND_INPUT_ORDER');
    rows.push(row);
    acceptedSourceAgeMs.push(row.sourceAgeMs);
    bidTopQuantity.push(row.bidQty);
    askTopQuantity.push(row.askQty);
  }
  const aggregates = new Map(), decisionsByPeriod = {development: 0, heldout: 0, purged: 0};
  const get = (period, lookbackMs, holdingMs, rule) => {
    const key = `${period}|${lookbackMs}|${holdingMs}|${rule}`;
    if (!aggregates.has(key)) aggregates.set(key, newAggregate());
    return aggregates.get(key);
  };
  const evaluate = (rows, index, decision, period, lookbackMs, holdingMs, rule) => {
    const out = get(period, lookbackMs, holdingMs, rule);
    if (rule !== 'unconditional') {
      if (decision.delta === 0n) { out.flatSignal++; return; }
      if (rule === 'trend' && decision.delta < 0n || rule === 'reversion' && decision.delta > 0n) return;
    }
    out.signals++;
    const entryTarget = rows[index].atMs + cfg.decisionLatencyMs;
    const entryIndex = lowerBound(rows, entryTarget, index + 1);
    if (entryIndex >= rows.length) { out.entryNoBook++; return; }
    const entry = rows[entryIndex];
    if (entry.segment !== rows[index].segment) { out.invalidationCensored++; return; }
    const entryDelay = entry.atMs - entryTarget;
    if (entryDelay > cfg.maxEntryDelayMs) { out.entryDelayExceeded++; return; }
    if (entry.askQty < EXECUTABLE_TREND_REVERSION_V1.sharesPerTrade) { out.entryDepthInsufficient++; return; }
    out.entered++;
    const exitTarget = entry.atMs + holdingMs;
    const exitIndex = lowerBound(rows, exitTarget, entryIndex + 1);
    if (exitIndex >= rows.length) { out.exitNoBook++; return; }
    const exit = rows[exitIndex];
    if (exit.segment !== entry.segment) { out.invalidationCensored++; return; }
    const exitDelay = exit.atMs - exitTarget;
    if (exitDelay > cfg.maxExitDelayMs) { out.exitDelayExceeded++; return; }
    if (exit.bidQty < EXECUTABLE_TREND_REVERSION_V1.sharesPerTrade) { out.exitDepthInsufficient++; return; }
    const pnl = exit.bid - entry.ask;
    out.completed++;
    if (pnl > 0n) out.positive++;
    else if (pnl < 0n) out.negative++;
    else out.flat++;
    out.pnlMicros += pnl;
    out.entryCostMicros += entry.ask;
    out.markoutsMicros.push(pnl);
    addBreakdown(out.byMarket, entry.marketId, pnl, entry.ask);
    addBreakdown(out.byUtcDay, new Date(rows[index].atMs).toISOString().slice(0, 10), pnl, entry.ask);
  };

  let accepted = 0, firstAtMs = Infinity, lastAtMs = -Infinity;
  for (const [marketId, rows] of byMarket) {
    rows.sort((a, b) => a.atMs - b.atMs || (BigInt(a.sequence) < BigInt(b.sequence) ? -1 : BigInt(a.sequence) > BigInt(b.sequence) ? 1 : 0));
    accepted += rows.length;
    if (rows.length) { firstAtMs = Math.min(firstAtMs, rows[0].atMs); lastAtMs = Math.max(lastAtMs, rows.at(-1).atMs); }
    let nextDecisionMs = -Infinity;
    for (let i = 0; i < rows.length; i++) {
      const current = rows[i];
      if (current.atMs < nextDecisionMs) continue;
      nextDecisionMs = current.atMs + cfg.decisionSpacingMs;
      const period = getPeriod(current.atMs, splitAtMs, purgeMs);
      decisionsByPeriod[period]++;
      if (period === 'purged') {
        for (const lookbackMs of cfg.lookbackMs) for (const holdingMs of cfg.holdingMs)
          for (const rule of ['unconditional', 'trend', 'reversion']) get(period, lookbackMs, holdingMs, rule).decisions++;
        continue;
      }
      for (const lookbackMs of cfg.lookbackMs) {
        const refIndex = upperBound(rows, current.atMs - lookbackMs) - 1;
        for (const holdingMs of cfg.holdingMs) for (const rule of ['unconditional', 'trend', 'reversion'])
          get(period, lookbackMs, holdingMs, rule).decisions++;
        if (refIndex < 0) {
          for (const holdingMs of cfg.holdingMs) for (const rule of ['unconditional', 'trend', 'reversion']) get(period, lookbackMs, holdingMs, rule).noLookback++;
          continue;
        }
        const ref = rows[refIndex];
        const driftMs = current.atMs - lookbackMs - ref.atMs;
        if (ref.segment !== current.segment || driftMs < 0 || driftMs > cfg.maxLookbackDriftMs) {
          for (const holdingMs of cfg.holdingMs) for (const rule of ['unconditional', 'trend', 'reversion']) get(period, lookbackMs, holdingMs, rule).noLookback++;
          continue;
        }
        const decision = {delta: current.mid - ref.mid};
        for (const holdingMs of cfg.holdingMs) {
          for (const rule of ['unconditional', 'trend', 'reversion']) evaluate(rows, i, decision, period, lookbackMs, holdingMs, rule);
        }
      }
    }
  }
  const results = [...aggregates.entries()].map(([key, value]) => {
    const [period, lookbackMs, holdingMs, rule] = key.split('|');
    return {period, lookbackMs: Number(lookbackMs), holdingMs: Number(holdingMs), rule, ...finishAggregate(value)};
  }).sort((a, b) => ['development', 'heldout', 'purged'].indexOf(a.period) - ['development', 'heldout', 'purged'].indexOf(b.period)
    || a.lookbackMs - b.lookbackMs || a.holdingMs - b.holdingMs || a.rule.localeCompare(b.rule));
  const sourceAgeSummary = {...numericSummary(acceptedSourceAgeMs),
    futureTimestampWithinTolerance: acceptedSourceAgeMs.filter(value => value < 0).length};
  const bidQuantitySummary = numericSummary(bidTopQuantity), askQuantitySummary = numericSummary(askTopQuantity);
  return {
    mode: 'fixed_sign_executable_yes_trend_vs_reversion_v1',
    methodology: {decisionLatencyMs: cfg.decisionLatencyMs, lookbackMs: cfg.lookbackMs, holdingMs: cfg.holdingMs,
      decisionSpacingMs: cfg.decisionSpacingMs, maxLookbackDriftMs: cfg.maxLookbackDriftMs,
      maxBookAgeMs: cfg.maxBookAgeMs, maxEntryDelayMs: cfg.maxEntryDelayMs, maxExitDelayMs: cfg.maxExitDelayMs,
      fees: '0.000000', sharesPerTrade: 1, entry: 'first subsequent executable YES ask after decision latency',
      exit: 'first executable YES bid at or after entry plus fixed hold', purgeMs,
      strategyDefinitions: {trend: 'enter YES only when trailing mid-price change is positive',
        reversion: 'enter YES only when trailing mid-price change is negative', unconditional: 'enter YES at every eligible decision'},
      counterfactualOnly: true, profitabilityEstablished: false},
    observations: {acceptedUniqueBooks: accepted, markets: byMarket.size, firstAt: Number.isFinite(firstAtMs) ? new Date(firstAtMs).toISOString() : null,
      lastAt: Number.isFinite(lastAtMs) ? new Date(lastAtMs).toISOString() : null, rejected,
      acceptedSourceAgeMs: sourceAgeSummary,
      topOfBookQuantity: {unit: 'shares at best price; replay trades one share only',
        bid: {...bidQuantitySummary,
          atLeast1: bidTopQuantity.length, atLeast5: bidTopQuantity.filter(value => value >= 5).length,
          atLeast10: bidTopQuantity.filter(value => value >= 10).length},
        ask: {...askQuantitySummary,
          atLeast1: askTopQuantity.length, atLeast5: askTopQuantity.filter(value => value >= 5).length,
          atLeast10: askTopQuantity.filter(value => value >= 10).length}},
      decisionsByPeriod},
    splitAt: new Date(splitAtMs).toISOString(), results,
  };
}
