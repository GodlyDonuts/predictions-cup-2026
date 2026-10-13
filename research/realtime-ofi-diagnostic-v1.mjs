import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { normalizeOfficialFillTick } from '../execution/api-tick-price.mjs';

export const REALTIME_OFI_HORIZONS_MS_V1 = Object.freeze([1_000, 3_000, 10_000]);
export const REALTIME_OFI_LABEL_GRACE_MS_V1 = 1_000;
export const REALTIME_OFI_THRESHOLD_QUANTILE_V1 = 0.8;

const FEATURE_NAMES = Object.freeze(['queueImbalanceTop', 'normalizedBboOfi']);
const HISTOGRAM_BINS = 20_001;
const CHUNK_BYTES = 1_048_576;
const MAX_RECORD_BYTES = 16_777_216;
const MAX_PREFIX_BYTES = 2_147_483_648;
const MAX_TOTAL_RECORDS = 500_000;
const MAX_MARKETS = 1_000;
const MAX_PENDING_OBSERVATIONS = 250_000;
const MAX_LABEL_GRACE_MS = 10_000;
const EXPECTED_TOURNAMENT_ID = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const PREFIX_BYTES = 187_899_071;
const EXPECTED_PREFIX_SHA256 = '49e9dc3f5ce96c95149c5b394c3ae640be339dcc53ac3b752233118bc29c62b8';
const EXPECTED_CAPTURE_RECORDS = 52_222;

const requireValue = (condition, code) => {
  if (!condition) throw new Error(code);
};

function parsedTimestamp(value, code) {
  const result = typeof value === 'string' ? Date.parse(value) : NaN;
  requireValue(Number.isSafeInteger(result), code);
  return result;
}

function parsedLevels(rows, side) {
  requireValue(Array.isArray(rows) && rows.length > 0, 'OFI_BOOK_LEVELS');
  const levels = [];
  let previousTick = null;
  for (const row of rows.slice(0, 20)) {
    const tick = normalizeOfficialFillTick(row?.price).tick;
    const quantity = row?.quantity;
    requireValue(tick >= 1 && tick <= 199, 'OFI_BOOK_PRICE');
    requireValue(Number.isSafeInteger(quantity) && quantity > 0, 'OFI_BOOK_QUANTITY');
    if (previousTick !== null) {
      requireValue(side === 'bid' ? tick < previousTick : tick > previousTick, 'OFI_BOOK_ORDER');
    }
    levels.push({ tick, quantity });
    previousTick = tick;
  }
  return levels;
}

export function topOfBookSignalsV1(book, previous = null) {
  const bidQuantity = book.bids[0].quantity;
  const askQuantity = book.asks[0].quantity;
  const queueImbalanceTop = (bidQuantity - askQuantity) / (bidQuantity + askQuantity);
  if (!previous) return { queueImbalanceTop, normalizedBboOfi: null };

  const bidFlow = book.bidTick > previous.bidTick
    ? bidQuantity
    : book.bidTick === previous.bidTick
      ? bidQuantity - previous.bids[0].quantity
      : -previous.bids[0].quantity;
  const askFlow = book.askTick < previous.askTick
    ? -askQuantity
    : book.askTick === previous.askTick
      ? -(askQuantity - previous.asks[0].quantity)
      : previous.asks[0].quantity;
  const denominator = bidQuantity + askQuantity + previous.bids[0].quantity + previous.asks[0].quantity;
  requireValue(denominator > 0, 'OFI_FLOW_DENOMINATOR');
  return { queueImbalanceTop, normalizedBboOfi: (bidFlow + askFlow) / denominator };
}

function parsePublicBook(record, book, marketId, nowMs) {
  const exchangeId = book?.exchangeId;
  requireValue(typeof exchangeId === 'string' && exchangeId.length > 0, 'OFI_EXCHANGE_ID');
  const receivedAtMs = parsedTimestamp(record.receivedAt, 'OFI_RECEIVED_AT');
  const sourceAtMs = parsedTimestamp(book?.asOf?.at, 'OFI_SOURCE_AT');
  requireValue(receivedAtMs <= nowMs && sourceAtMs <= nowMs, 'OFI_FUTURE_BOOK');
  requireValue(nowMs - receivedAtMs <= 10_000, 'OFI_STALE_RECEIPT');
  requireValue(book?.expiryKnown === true, 'OFI_EXPIRY_UNKNOWN');
  const expiryAtMs = book.nextExpiryAt === null
    ? null
    : parsedTimestamp(book.nextExpiryAt, 'OFI_EXPIRY_TIME');
  requireValue(expiryAtMs === null || expiryAtMs > nowMs, 'OFI_EXPIRED_BOOK');
  const sequenceValue = book?.asOf?.sequence;
  requireValue((typeof sequenceValue === 'number' && Number.isSafeInteger(sequenceValue) && sequenceValue >= 0)
    || (typeof sequenceValue === 'string' && /^(0|[1-9]\d*)$/.test(sequenceValue)), 'OFI_BOOK_SEQUENCE');
  const bids = parsedLevels(book.bids, 'bid');
  const asks = parsedLevels(book.asks, 'ask');
  requireValue(bids[0].tick < asks[0].tick, 'OFI_CROSSED_BOOK');
  const sumQuantity = levels => levels.reduce((sum, level) => sum + level.quantity, 0);
  const bidDepth20 = sumQuantity(bids);
  const askDepth20 = sumQuantity(asks);
  return {
    marketId,
    exchangeId,
    sequence: BigInt(sequenceValue),
    sourceAtMs,
    observedAtMs: nowMs,
    bidTick: bids[0].tick,
    askTick: asks[0].tick,
    midTicks: (bids[0].tick + asks[0].tick) / 2,
    spreadTicks: asks[0].tick - bids[0].tick,
    bidDepthTop: bids[0].quantity,
    askDepthTop: asks[0].quantity,
    topDepthShares: bids[0].quantity + asks[0].quantity,
    depth20Shares: bidDepth20 + askDepth20,
    bids,
    asks,
  };
}

function newHistogram() {
  return { bins: new Uint32Array(HISTOGRAM_BINS), count: 0, nonzero: 0 };
}

function addDiscoverySignal(histogram, value) {
  histogram.count++;
  const magnitude = Math.abs(value);
  if (magnitude === 0) return;
  histogram.nonzero++;
  const bin = Math.min(HISTOGRAM_BINS - 1, Math.floor(magnitude * HISTOGRAM_BINS));
  histogram.bins[bin]++;
}

function fittedThreshold(histogram) {
  if (!histogram.nonzero) return null;
  const target = Math.ceil(histogram.nonzero * REALTIME_OFI_THRESHOLD_QUANTILE_V1);
  let seen = 0;
  for (let i = 0; i < histogram.bins.length; i++) {
    seen += histogram.bins[i];
    if (seen >= target) return (i + 0.5) / HISTOGRAM_BINS;
  }
  throw new Error('OFI_THRESHOLD_FIT');
}

function emptyMoment() {
  return {
    n: 0,
    sumX: 0,
    sumY: 0,
    sumX2: 0,
    sumY2: 0,
    sumXY: 0,
    sumDirectional: 0,
    sumSpread: 0,
    sumTopDepth: 0,
    sumDepth20: 0,
  };
}

function addMoment(moment, x, y, spreadTicks, topDepthShares, depth20Shares) {
  const directional = Math.sign(x) * y;
  moment.n++;
  moment.sumX += x;
  moment.sumY += y;
  moment.sumX2 += x * x;
  moment.sumY2 += y * y;
  moment.sumXY += x * y;
  moment.sumDirectional += directional;
  moment.sumSpread += spreadTicks;
  moment.sumTopDepth += topDepthShares;
  moment.sumDepth20 += depth20Shares;
}

function emptyMarketCell() {
  return {
    all: emptyMoment(),
    threshold: emptyMoment(),
    opportunities: 0,
    labels: 0,
    thresholdOpportunities: 0,
    thresholdLabels: 0,
    noBookWithinGrace: 0,
    thresholdNoBookWithinGrace: 0,
    boundaryCensored: 0,
    thresholdBoundaryCensored: 0,
  };
}

function emptyCell(labelGraceMs) {
  return {
    opportunities: 0,
    labels: 0,
    thresholdOpportunities: 0,
    thresholdLabels: 0,
    noBookWithinGrace: 0,
    thresholdNoBookWithinGrace: 0,
    boundaryCensored: 0,
    thresholdBoundaryCensored: 0,
    labelDelaySumMs: 0,
    labelDelayBins: new Uint32Array(labelGraceMs + 1),
    thresholdLabelDelaySumMs: 0,
    thresholdLabelDelayBins: new Uint32Array(labelGraceMs + 1),
    markets: new Map(),
  };
}

function marketCell(cell, marketId) {
  let item = cell.markets.get(marketId);
  if (!item) {
    item = emptyMarketCell();
    cell.markets.set(marketId, item);
  }
  return item;
}

function clusterMean(values, countField, sumField) {
  let n = 0;
  let total = 0;
  const clusters = [];
  for (const value of values) {
    const count = value[countField];
    if (!count) continue;
    n += count;
    total += value[sumField];
    clusters.push({ count, sum: value[sumField] });
  }
  if (!n) return { n: 0, markets: 0, mean: null, clusterSe: null, ci95: null };
  const mean = total / n;
  const marketCount = clusters.length;
  if (marketCount < 2) return { n, markets: marketCount, mean, clusterSe: null, ci95: null };
  const scoreSquares = clusters.reduce((sum, cluster) => sum + (cluster.sum - mean * cluster.count) ** 2, 0);
  const se = Math.sqrt((marketCount / (marketCount - 1)) * scoreSquares / (n * n));
  return { n, markets: marketCount, mean, clusterSe: se, ci95: [mean - 1.96 * se, mean + 1.96 * se] };
}

function coverageSummary(cell, threshold = false) {
  const opportunityField = threshold ? 'thresholdOpportunities' : 'opportunities';
  const labelField = threshold ? 'thresholdLabels' : 'labels';
  const noBookField = threshold ? 'thresholdNoBookWithinGrace' : 'noBookWithinGrace';
  const boundaryField = threshold ? 'thresholdBoundaryCensored' : 'boundaryCensored';
  const delayBins = threshold ? cell.thresholdLabelDelayBins : cell.labelDelayBins;
  const delaySum = threshold ? cell.thresholdLabelDelaySumMs : cell.labelDelaySumMs;
  const opportunities = cell[opportunityField];
  const labels = cell[labelField];
  const rate = opportunities ? labels / opportunities : null;
  const clusters = [...cell.markets.values()].filter(market => market[opportunityField] > 0);
  let clusterSe = null;
  let ci95 = null;
  if (rate !== null && clusters.length >= 2) {
    const scoreSquares = clusters.reduce((sum, market) => sum + (market[labelField] - rate * market[opportunityField]) ** 2, 0);
    clusterSe = Math.sqrt((clusters.length / (clusters.length - 1)) * scoreSquares / (opportunities * opportunities));
    ci95 = [Math.max(0, rate - 1.96 * clusterSe), Math.min(1, rate + 1.96 * clusterSe)];
  }
  const delayQuantile = p => {
    if (!labels) return null;
    const target = Math.ceil(labels * p);
    let seen = 0;
    for (let i = 0; i < delayBins.length; i++) {
      seen += delayBins[i];
      if (seen >= target) return i;
    }
    return null;
  };
  return {
    opportunities,
    labeled: labels,
    rate,
    markets: clusters.length,
    clusterSe,
    ci95,
    labelDelayMs: labels ? {
      mean: delaySum / labels,
      p50: delayQuantile(0.5),
      p90: delayQuantile(0.9),
    } : null,
    noBookWithinGrace: cell[noBookField],
    boundaryCensored: cell[boundaryField],
  };
}

function outcomeSummary(cell, threshold = false) {
  const rows = [...cell.markets.values()];
  const all = threshold ? rows.map(row => row.threshold) : rows.map(row => row.all);
  const n = all.reduce((sum, row) => sum + row.n, 0);
  if (!n) return { observations: 0, markets: 0, meanMidMoveTicks: null, meanMidMoveClusterSe: null, meanMidMoveCi95: null, meanDirectionalMarkoutTicks: null, meanDirectionalMarkoutClusterSe: null, meanDirectionalMarkoutCi95: null, adverseMarkoutTicks: null, adverseMarkoutCi95: null, continuousSlopeTicksPerFeatureUnit: null, continuousSlopeCi95: null, continuousPearsonCorrelation: null, meanSignalSpreadTicks: null, meanTopDepthShares: null, meanDepth20Shares: null };

  const y = clusterMean(all, 'n', 'sumY');
  const directional = clusterMean(all, 'n', 'sumDirectional');
  const adverseCi = directional.ci95 ? [-directional.ci95[1], -directional.ci95[0]] : null;
  const sumX = all.reduce((sum, row) => sum + row.sumX, 0);
  const sumY = all.reduce((sum, row) => sum + row.sumY, 0);
  const sumX2 = all.reduce((sum, row) => sum + row.sumX2, 0);
  const sumY2 = all.reduce((sum, row) => sum + row.sumY2, 0);
  const sumXY = all.reduce((sum, row) => sum + row.sumXY, 0);
  const xBar = sumX / n;
  const yBar = sumY / n;
  const sxx = sumX2 - sumX * xBar;
  const syy = sumY2 - sumY * yBar;
  const sxy = sumXY - sumX * yBar;
  let slope = null;
  let slopeCi = null;
  if (sxx > 0 && n > 2) {
    slope = sxy / sxx;
    const active = rows.filter(row => (threshold ? row.threshold : row.all).n > 0);
    if (active.length > 1) {
      const alpha = yBar - slope * xBar;
      const scoreSquares = active.reduce((sum, row) => {
        const moment = threshold ? row.threshold : row.all;
        const score = moment.sumXY - xBar * moment.sumY
          - alpha * (moment.sumX - moment.n * xBar)
          - slope * (moment.sumX2 - xBar * moment.sumX);
        return sum + score * score;
      }, 0);
      const finiteCorrection = (active.length / (active.length - 1)) * ((n - 1) / Math.max(1, n - 2));
      const slopeSe = Math.sqrt(finiteCorrection * scoreSquares / (sxx * sxx));
      slopeCi = [slope - 1.96 * slopeSe, slope + 1.96 * slopeSe];
    }
  }
  const correlation = sxx > 0 && syy > 0 ? sxy / Math.sqrt(sxx * syy) : null;
  const selected = rows.map(row => threshold ? row.threshold : row.all);
  const signalN = selected.reduce((sum, row) => sum + row.n, 0);
  const spread = selected.reduce((sum, row) => sum + row.sumSpread, 0);
  const topDepth = selected.reduce((sum, row) => sum + row.sumTopDepth, 0);
  const depth20 = selected.reduce((sum, row) => sum + row.sumDepth20, 0);
  return {
    observations: n,
    markets: directional.markets,
    meanMidMoveTicks: y.mean,
    meanMidMoveClusterSe: y.clusterSe,
    meanMidMoveCi95: y.ci95,
    meanDirectionalMarkoutTicks: directional.mean,
    meanDirectionalMarkoutClusterSe: directional.clusterSe,
    meanDirectionalMarkoutCi95: directional.ci95,
    adverseMarkoutTicks: directional.mean === null ? null : -directional.mean,
    adverseMarkoutCi95: adverseCi,
    continuousSlopeTicksPerFeatureUnit: slope,
    continuousSlopeCi95: slopeCi,
    continuousPearsonCorrelation: correlation,
    meanSignalSpreadTicks: signalN ? spread / signalN : null,
    meanTopDepthShares: signalN ? topDepth / signalN : null,
    meanDepth20Shares: signalN ? depth20 / signalN : null,
  };
}

function rounded(value, digits = 6) {
  if (!Number.isFinite(value)) return null;
  const scale = 10 ** digits;
  return Math.round(value * scale) / scale;
}

function roundDeep(value) {
  if (Array.isArray(value)) return value.map(roundDeep);
  if (value && typeof value === 'object') {
    if (value instanceof Map) return [...value.entries()].map(([key, row]) => [key, roundDeep(row)]);
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, roundDeep(item)]));
  }
  return typeof value === 'number' ? rounded(value) : value;
}

function parseMarketTopic(topic) {
  const match = /^tournament:([^:]+):market:(\d+)$/.exec(topic ?? '');
  return match ? { tournamentId: match[1], marketId: match[2] } : null;
}

export function createRealtimeOfiDiagnosticV1({ splitIndex, totalRecords, throughAt, labelGraceMs = REALTIME_OFI_LABEL_GRACE_MS_V1 }) {
  requireValue(Number.isSafeInteger(totalRecords) && totalRecords > 0, 'OFI_TOTAL_RECORDS');
  requireValue(totalRecords <= MAX_TOTAL_RECORDS, 'OFI_RECORD_COUNT_BOUND');
  requireValue(Number.isSafeInteger(splitIndex) && splitIndex > 0 && splitIndex < totalRecords, 'OFI_SPLIT_INDEX');
  requireValue(Number.isSafeInteger(labelGraceMs) && labelGraceMs >= 0 && labelGraceMs <= MAX_LABEL_GRACE_MS, 'OFI_LABEL_GRACE');
  const throughMs = parsedTimestamp(throughAt, 'OFI_THROUGH_AT');
  const histograms = Object.fromEntries(FEATURE_NAMES.map(name => [name, newHistogram()]));
  const thresholds = Object.fromEntries(FEATURE_NAMES.map(name => [name, null]));
  const cells = Object.fromEntries(FEATURE_NAMES.map(name => [name,
    Object.fromEntries(REALTIME_OFI_HORIZONS_MS_V1.map(horizon => [horizon, emptyCell(labelGraceMs)])),
  ]));
  const states = new Map();
  const exchangeToMarket = new Map();
  const marketToExchange = new Map();
  const rejectionReasons = Object.create(null);
  const boundaryReasons = Object.create(null);
  const foldCounts = {
    discovery: { records: 0, marketBatchRecords: 0, publicBookRows: 0, acceptedPublicBooks: 0, queueSignals: 0, ofiSignals: 0 },
    holdout: { records: 0, marketBatchRecords: 0, publicBookRows: 0, acceptedPublicBooks: 0, queueSignals: 0, ofiSignals: 0 },
  };
  let expectedIndex = 0;
  let previousObservedMs = -Infinity;
  let splitAt = null;
  let tournamentId = null;
  let thresholdFitDone = false;
  let pendingObservations = 0;

  function count(target, key) {
    target[key] = (target[key] ?? 0) + 1;
  }

  function stateFor(marketId) {
    let state = states.get(marketId);
    if (!state) {
      requireValue(states.size < MAX_MARKETS, 'OFI_MARKET_COUNT_BOUND');
      state = { previous: null, epoch: null, revision: null, pending: [] };
      states.set(marketId, state);
    }
    return state;
  }

  function accountBoundaryPending(state, reason) {
    pendingObservations -= state.pending.length;
    for (const pending of state.pending) {
      if (pending.fold !== 'holdout') continue;
      const cluster = marketCell(pending.cell, pending.marketId);
      pending.cell.boundaryCensored++;
      cluster.boundaryCensored++;
      if (pending.thresholdTriggered) {
        pending.cell.thresholdBoundaryCensored++;
        cluster.thresholdBoundaryCensored++;
      }
    }
    state.pending = [];
    state.previous = null;
    count(boundaryReasons, reason);
  }

  function transitionToHoldout(at) {
    if (thresholdFitDone) return;
    splitAt = new Date(at).toISOString();
    for (const name of FEATURE_NAMES) thresholds[name] = fittedThreshold(histograms[name]);
    thresholdFitDone = true;
    for (const state of states.values()) {
      pendingObservations -= state.pending.length;
      state.pending = [];
      state.previous = null;
    }
  }

  function addCandidate(featureName, value, book, fold, state) {
    if (fold === 'discovery') {
      addDiscoverySignal(histograms[featureName], value);
      foldCounts.discovery[featureName === 'queueImbalanceTop' ? 'queueSignals' : 'ofiSignals']++;
      return;
    }
    foldCounts.holdout[featureName === 'queueImbalanceTop' ? 'queueSignals' : 'ofiSignals']++;
    const threshold = thresholds[featureName];
    const thresholdTriggered = threshold !== null && Math.abs(value) >= threshold;
    for (const horizonMs of REALTIME_OFI_HORIZONS_MS_V1) {
      const cell = cells[featureName][horizonMs];
      const targetAtMs = book.observedAtMs + horizonMs;
      const deadlineMs = targetAtMs + labelGraceMs;
      if (deadlineMs > throughMs) continue;
      cell.opportunities++;
      const cluster = marketCell(cell, book.marketId);
      cluster.opportunities++;
      if (thresholdTriggered) {
        cell.thresholdOpportunities++;
        cluster.thresholdOpportunities++;
      }
      state.pending.push({
        fold,
        cell,
        marketId: book.marketId,
        featureName,
        value,
        signalAtMs: book.observedAtMs,
        targetAtMs,
        deadlineMs,
        signalMidTicks: book.midTicks,
        spreadTicks: book.spreadTicks,
        topDepthShares: book.topDepthShares,
        depth20Shares: book.depth20Shares,
        thresholdTriggered,
      });
      pendingObservations++;
      requireValue(pendingObservations <= MAX_PENDING_OBSERVATIONS, 'OFI_PENDING_BOUND');
    }
  }

  function settlePending(state, book, fold) {
    if (!state.pending.length) return;
    const remaining = [];
    for (const pending of state.pending) {
      if (pending.fold !== fold) {
        pendingObservations--;
        continue;
      }
      if (book.observedAtMs < pending.targetAtMs) {
        remaining.push(pending);
        continue;
      }
      if (book.observedAtMs > pending.deadlineMs) {
        pendingObservations--;
        const cluster = marketCell(pending.cell, pending.marketId);
        pending.cell.noBookWithinGrace++;
        cluster.noBookWithinGrace++;
        if (pending.thresholdTriggered) {
          pending.cell.thresholdNoBookWithinGrace++;
          cluster.thresholdNoBookWithinGrace++;
        }
        continue;
      }
      const deltaMidTicks = book.midTicks - pending.signalMidTicks;
      const delayMs = book.observedAtMs - pending.targetAtMs;
      pendingObservations--;
      pending.cell.labels++;
      pending.cell.labelDelaySumMs += delayMs;
      pending.cell.labelDelayBins[delayMs]++;
      const cluster = marketCell(pending.cell, pending.marketId);
      cluster.labels++;
      addMoment(cluster.all, pending.value, deltaMidTicks, pending.spreadTicks, pending.topDepthShares, pending.depth20Shares);
      if (pending.thresholdTriggered) {
        pending.cell.thresholdLabels++;
        pending.cell.thresholdLabelDelaySumMs += delayMs;
        pending.cell.thresholdLabelDelayBins[delayMs]++;
        cluster.thresholdLabels++;
        addMoment(cluster.threshold, pending.value, deltaMidTicks, pending.spreadTicks, pending.topDepthShares, pending.depth20Shares);
      }
    }
    state.pending = remaining;
  }

  function boundaryForTopic(topic, reason, resetRevision = true) {
    const parsed = parseMarketTopic(topic);
    if (!parsed || parsed.tournamentId !== EXPECTED_TOURNAMENT_ID) return;
    const state = stateFor(parsed.marketId);
    accountBoundaryPending(state, reason);
    if (resetRevision) {
      state.epoch = null;
      state.revision = null;
    }
  }

  function processMarketBatch(record, nowMs, fold) {
    const topic = parseMarketTopic(record.topic);
    if (!topic || topic.tournamentId !== EXPECTED_TOURNAMENT_ID) return;
    tournamentId ??= topic.tournamentId;
    requireValue(topic.tournamentId === tournamentId, 'OFI_TOURNAMENT_MIX');
    const state = stateFor(topic.marketId);
    foldCounts[fold].marketBatchRecords++;
    const payload = record.payload;
    const delivery = payload?.delivery;
    if (!Number.isSafeInteger(record.epoch) || record.epoch < 1
      || !Number.isSafeInteger(delivery?.revision) || delivery.revision < 1
      || !Number.isSafeInteger(delivery?.previousRevision) || delivery.previousRevision < 0
      || delivery.model !== 'best-effort-authoritative-resync') {
      accountBoundaryPending(state, 'invalid_delivery_metadata');
      state.epoch = null;
      state.revision = null;
      count(rejectionReasons, 'invalid_delivery_metadata');
      return;
    }
    if (state.epoch !== null && record.epoch !== state.epoch) {
      accountBoundaryPending(state, 'epoch_change');
      state.revision = null;
    }
    const priorRevision = state.revision;
    if (priorRevision !== null && delivery.previousRevision !== priorRevision) {
      accountBoundaryPending(state, 'delivery_revision_gap');
      state.revision = null;
    }
    if (priorRevision !== null && delivery.revision <= priorRevision) {
      count(rejectionReasons, 'duplicate_delivery_revision');
      return;
    }
    state.epoch = record.epoch;
    state.revision = delivery.revision;
    if (payload.resyncRequired !== false || (Array.isArray(payload.marketSettled) && payload.marketSettled.length > 0)) {
      accountBoundaryPending(state, payload.marketSettled?.length ? 'market_settled' : 'resync_required');
      return;
    }
    const books = Array.isArray(payload.books) ? payload.books : [];
    foldCounts[fold].publicBookRows += books.length;
    const presentExchanges = new Set(books.map(book => typeof book?.exchangeId === 'string' ? book.exchangeId : ''));
    if (Array.isArray(payload.bookDirty) && payload.bookDirty.some(item => {
      const dirtyExchange = typeof item?.exchangeId === 'string' ? item.exchangeId : '';
      return dirtyExchange && !presentExchanges.has(dirtyExchange);
    })) {
      accountBoundaryPending(state, 'dirty_book_without_replacement');
      return;
    }
    for (const rawBook of books) {
      let book;
      try {
        book = parsePublicBook(record, rawBook, topic.marketId, nowMs);
      } catch (error) {
        accountBoundaryPending(state, 'invalid_public_book');
        count(rejectionReasons, /^[A-Z0-9_]+$/.test(error.message) ? error.message : 'invalid_public_book');
        continue;
      }
      const oldMarket = exchangeToMarket.get(book.exchangeId);
      const oldExchange = marketToExchange.get(book.marketId);
      requireValue(oldMarket === undefined || oldMarket === book.marketId, 'OFI_EXCHANGE_MARKET_CONFLICT');
      requireValue(oldExchange === undefined || oldExchange === book.exchangeId, 'OFI_MARKET_EXCHANGE_CONFLICT');
      exchangeToMarket.set(book.exchangeId, book.marketId);
      marketToExchange.set(book.marketId, book.exchangeId);
      const previous = state.previous;
      if (previous && book.sequence <= previous.sequence) {
        count(rejectionReasons, book.sequence === previous.sequence ? 'duplicate_book_sequence' : 'book_sequence_rollback');
        if (book.sequence < previous.sequence) accountBoundaryPending(state, 'book_sequence_rollback');
        continue;
      }
      if (previous && book.sourceAtMs < previous.sourceAtMs) {
        accountBoundaryPending(state, 'source_clock_rollback');
        count(rejectionReasons, 'source_clock_rollback');
        continue;
      }
      settlePending(state, book, fold);
      foldCounts[fold].acceptedPublicBooks++;
      const signals = topOfBookSignalsV1(book, previous);
      addCandidate('queueImbalanceTop', signals.queueImbalanceTop, book, fold, state);
      if (signals.normalizedBboOfi !== null) addCandidate('normalizedBboOfi', signals.normalizedBboOfi, book, fold, state);
      state.previous = book;
    }
  }

  function consume(record) {
    requireValue(record && typeof record === 'object', 'OFI_RECORD');
    requireValue(record.format === 'sig-feed-capture' && record.schemaVersion === 1, 'OFI_CAPTURE_ENVELOPE');
    requireValue(record.captureIndex === expectedIndex, 'OFI_CAPTURE_INDEX');
    const nowMs = parsedTimestamp(record.observedAt, 'OFI_CAPTURE_CLOCK');
    requireValue(nowMs >= previousObservedMs, 'OFI_CAPTURE_CLOCK_ROLLBACK');
    previousObservedMs = nowMs;
    if (expectedIndex === splitIndex) transitionToHoldout(nowMs);
    const fold = expectedIndex < splitIndex ? 'discovery' : 'holdout';
    foldCounts[fold].records++;
    if (fold === 'holdout') requireValue(thresholdFitDone, 'OFI_THRESHOLD_NOT_FIT');

    const topic = typeof record.topic === 'string' ? record.topic : '';
    if (record.kind === 'event' && record.event === 'market_batch') {
      processMarketBatch(record, nowMs, fold);
    } else if (['invalidation', 'subscribed', 'disconnected', 'rejected', 'stopped', 'snapshot'].includes(record.kind)) {
      boundaryForTopic(topic, `source_${record.kind}`);
    } else if (record.kind === 'event' && topic) {
      boundaryForTopic(topic, 'unsupported_market_event');
    }
    expectedIndex++;
  }

  function finalizeUnobserved() {
    for (const state of states.values()) {
      for (const pending of state.pending) {
        if (pending.fold !== 'holdout') continue;
        pendingObservations--;
        const cluster = marketCell(pending.cell, pending.marketId);
        pending.cell.noBookWithinGrace++;
        cluster.noBookWithinGrace++;
        if (pending.thresholdTriggered) {
          pending.cell.thresholdNoBookWithinGrace++;
          cluster.thresholdNoBookWithinGrace++;
        }
      }
      state.pending = [];
    }
    requireValue(pendingObservations === 0, 'OFI_PENDING_ACCOUNTING');
  }

  function cellReport(featureName, horizonMs) {
    const cell = cells[featureName][horizonMs];
    return {
      thresholdTriggered: thresholds[featureName] === null ? null : {
        cutoffAbs: rounded(thresholds[featureName], 8),
        discoveryQuantile: REALTIME_OFI_THRESHOLD_QUANTILE_V1,
        discoveryNonzeroSignals: histograms[featureName].nonzero,
        discoverySignals: histograms[featureName].count,
        fitRule: 'nearest-rank 80th percentile of nonzero absolute discovery features; fixed 20,001-bin [0,1] histogram; applied unchanged to holdout',
      },
      allHoldoutSignals: coverageSummary(cell, false),
      thresholdHoldoutSignals: coverageSummary(cell, true),
      allLabeledOutcomes: outcomeSummary(cell, false),
      thresholdLabeledOutcomes: outcomeSummary(cell, true),
    };
  }

  function snapshot() {
    return {
      processedRecords: expectedIndex,
      thresholdsFitted: thresholdFitDone,
      queue1s: {
        opportunities: cells.queueImbalanceTop[1_000].opportunities,
        labels: cells.queueImbalanceTop[1_000].labels,
      },
    };
  }

  function report({ source, replayRuntimeMs = null } = {}) {
    requireValue(expectedIndex === totalRecords, 'OFI_CAPTURE_COUNT_MISMATCH');
    if (!thresholdFitDone) transitionToHoldout(throughMs);
    finalizeUnobserved();
    requireValue(splitAt !== null, 'OFI_SPLIT_NOT_REACHED');
    const cellsReport = {};
    for (const featureName of FEATURE_NAMES) {
      cellsReport[featureName] = {};
      for (const horizonMs of REALTIME_OFI_HORIZONS_MS_V1) {
        cellsReport[featureName][horizonMs] = cellReport(featureName, horizonMs);
      }
    }
    return roundDeep({
      schema: 'realtime-ofi-diagnostic-v1',
      source,
      protocol: {
        input: 'Only captured public market_batch book rows. Reads capture envelope metadata and whitelisted public book fields; ignores trade, account, and unrelated payload fields.',
        split: {
          method: 'capture-record order; first floor(70% of records) discovery, remaining records holdout',
          totalRecords,
          splitIndex,
          discoveryRecords: splitIndex,
          holdoutRecords: totalRecords - splitIndex,
          holdoutFirstObservedAt: splitAt,
          noLabelMayCrossSplit: true,
        },
        horizonsMs: REALTIME_OFI_HORIZONS_MS_V1,
        labelGraceMs,
        label: 'First accepted same-market book update observed at or after signal time plus horizon, no later than the horizon target plus labelGraceMs; mid is the bid/ask midpoint in official 0.005 ticks.',
        queueImbalance: '(bestBidQuantity - bestAskQuantity) / (bestBidQuantity + bestAskQuantity)',
        orderFlowImbalance: 'Cont-style best-quote OFI: bid rise=current bid size, unchanged bid=current minus prior size, bid fall=minus prior size; ask fall=minus current size, unchanged ask=prior minus current size, ask rise=prior size; divide the sum by current plus prior best bid/ask depth.',
        thresholdSelection: 'For each signal, fit one cutoff from discovery only: nearest-rank 80th percentile of nonzero absolute signal values. No outcome tuning or parameter sweep.',
        adverseMarkout: 'Negative of sign(signal) times subsequent mid change; positive means the mid moved against the signal. This is a forecast markout, not a fill or P&L.',
        uncertainty: 'Market-clustered sandwich standard errors with finite-cluster correction and normal 1.96 intervals for markout means, coverage rates, and continuous OLS slope.',
        continuity: 'Clear pending outcomes and prior BBO across invalidation, reconnect, settlement, resync, epoch/revision gap, invalid book, source-clock rollback, and discovery/holdout boundary.',
        depth: 'Signal-time top-of-book queue depth and observed first-20-level depth; spread is in official ticks.',
        resourceBounds: { chunkBytes: CHUNK_BYTES, maxRecordBytes: MAX_RECORD_BYTES, maxPrefixBytes: MAX_PREFIX_BYTES, maxRecords: MAX_TOTAL_RECORDS, maxMarkets: MAX_MARKETS, maxPendingObservations: MAX_PENDING_OBSERVATIONS },
      },
      thresholds: Object.fromEntries(FEATURE_NAMES.map(name => [name, thresholds[name] === null ? null : rounded(thresholds[name], 8)])),
      counts: {
        byFold: foldCounts,
        markets: states.size,
        marketsWithOneExchange: marketToExchange.size,
        exchangeMarketBindings: exchangeToMarket.size,
        rejectedBooks: rejectionReasons,
        boundaries: boundaryReasons,
      },
      results: cellsReport,
      replayRuntimeMs,
      actualFillsProven: false,
      profitabilityEstablished: false,
    });
  }

  return Object.freeze({ consume, snapshot, report });
}

function scanJsonlPrefix(file, endByte, onRecord = () => {}) {
  const fd = fs.openSync(file, 'r');
  const digest = createHash('sha256');
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let initial;
  let tail = Buffer.alloc(0);
  let offset = 0;
  let count = 0;
  try {
    initial = fs.fstatSync(fd, { bigint: true });
    requireValue(Number.isSafeInteger(endByte) && endByte > 0 && endByte <= MAX_PREFIX_BYTES && initial.size >= BigInt(endByte), 'OFI_PREFIX_RANGE');
    const lastByte = Buffer.alloc(1);
    requireValue(fs.readSync(fd, lastByte, 0, 1, endByte - 1) === 1 && lastByte[0] === 10, 'OFI_PREFIX_NOT_LINE_ALIGNED');
    while (offset < endByte) {
      const buffer = Buffer.alloc(Math.min(CHUNK_BYTES, endByte - offset));
      const bytesRead = fs.readSync(fd, buffer, 0, buffer.length, offset);
      requireValue(bytesRead > 0, 'OFI_PREFIX_TRUNCATED');
      const chunk = buffer.subarray(0, bytesRead);
      digest.update(chunk);
      offset += bytesRead;
      const data = tail.length ? Buffer.concat([tail, chunk]) : chunk;
      let start = 0;
      while (true) {
        const end = data.indexOf(10, start);
        if (end < 0) break;
        requireValue(end - start > 0 && end - start <= MAX_RECORD_BYTES, 'OFI_RECORD_SIZE');
        let line;
        try {
          line = decoder.decode(data.subarray(start, end));
        } catch {
          throw new Error('OFI_RECORD_UTF8');
        }
        let record;
        try {
          record = JSON.parse(line);
        } catch {
          throw new Error('OFI_RECORD_JSON');
        }
        onRecord(record, count);
        count++;
        start = end + 1;
      }
      tail = Buffer.from(data.subarray(start));
      requireValue(tail.length <= MAX_RECORD_BYTES, 'OFI_RECORD_SIZE');
    }
    requireValue(tail.length === 0, 'OFI_PREFIX_NOT_LINE_ALIGNED');
    const final = fs.fstatSync(fd, { bigint: true });
    requireValue(final.dev === initial.dev && final.ino === initial.ino && final.size >= BigInt(endByte), 'OFI_CAPTURE_REPLACED');
    return {
      records: count,
      sha256: digest.digest('hex'),
      device: String(initial.dev),
      inode: String(initial.ino),
      sourceSizeAtScanStart: Number(initial.size),
      sourceSizeAtScanEnd: Number(final.size),
    };
  } finally {
    fs.closeSync(fd);
  }
}

function validateCapturePassOne(record, index, state) {
  requireValue(record?.format === 'sig-feed-capture' && record.schemaVersion === 1, 'OFI_CAPTURE_ENVELOPE');
  requireValue(record.captureIndex === index, 'OFI_CAPTURE_INDEX');
  const observedAtMs = parsedTimestamp(record.observedAt, 'OFI_CAPTURE_CLOCK');
  requireValue(observedAtMs >= state.previousObservedMs, 'OFI_CAPTURE_CLOCK_ROLLBACK');
  state.previousObservedMs = observedAtMs;
  state.firstObservedAt ??= record.observedAt;
  state.throughAt = record.observedAt;
}

export async function runRealtimeOfiDiagnosticV1({
  cwd = process.cwd(),
  localAppData = process.env.LOCALAPPDATA,
  captureFile = 'realtime-2026-10-13T13-28-06-669Z-a7674ae9-ed0e-41c6-90c8-390b1ce0dc8b.jsonl',
  prefixBytes = PREFIX_BYTES,
  outputFile = 'research/results/realtime-ofi-diagnostic-20261013.json',
} = {}) {
  requireValue(typeof localAppData === 'string' && localAppData.length > 0, 'OFI_LOCALAPPDATA');
  const started = performance.now();
  const sourcePath = path.join(localAppData, 'SigTradingCup', 'paper-runtime', captureFile);
  const firstPassState = { previousObservedMs: -Infinity, firstObservedAt: null, throughAt: null };
  const firstPass = scanJsonlPrefix(sourcePath, prefixBytes, (record, index) => validateCapturePassOne(record, index, firstPassState));
  requireValue(firstPass.records > 0, 'OFI_EMPTY_CAPTURE');
  if (EXPECTED_CAPTURE_RECORDS !== null) requireValue(firstPass.records === EXPECTED_CAPTURE_RECORDS, 'OFI_EXPECTED_CAPTURE_COUNT');
  if (EXPECTED_PREFIX_SHA256 !== null) requireValue(firstPass.sha256 === EXPECTED_PREFIX_SHA256, 'OFI_EXPECTED_PREFIX_SHA256');
  const splitIndex = Math.floor(firstPass.records * 0.70);
  const study = createRealtimeOfiDiagnosticV1({
    splitIndex,
    totalRecords: firstPass.records,
    throughAt: firstPassState.throughAt,
  });
  const secondPass = scanJsonlPrefix(sourcePath, prefixBytes, record => study.consume(record));
  requireValue(secondPass.records === firstPass.records && secondPass.sha256 === firstPass.sha256, 'OFI_CAPTURE_CHANGED_DURING_REPLAY');
  const report = study.report({
    source: {
      file: captureFile,
      prefixBytes,
      sha256: firstPass.sha256,
      captureRecords: firstPass.records,
      firstObservedAt: firstPassState.firstObservedAt,
      throughObservedAt: firstPassState.throughAt,
      tournamentId: EXPECTED_TOURNAMENT_ID,
      sourceSizeAtFirstScanStart: firstPass.sourceSizeAtScanStart,
      sourceSizeAtFirstScanEnd: firstPass.sourceSizeAtScanEnd,
      sourceSizeAtSecondScanStart: secondPass.sourceSizeAtScanStart,
      sourceSizeAtSecondScanEnd: secondPass.sourceSizeAtScanEnd,
      appendedBytesExcluded: firstPass.sourceSizeAtScanEnd > prefixBytes || secondPass.sourceSizeAtScanEnd > prefixBytes,
    },
    replayRuntimeMs: performance.now() - started,
  });
  const outputPath = path.resolve(cwd, outputFile);
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  return { output: path.relative(cwd, outputPath), report };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    const result = await runRealtimeOfiDiagnosticV1();
    const holdout = Object.fromEntries(FEATURE_NAMES.map(name => [name,
      Object.fromEntries(REALTIME_OFI_HORIZONS_MS_V1.map(horizon => {
        const cell = result.report.results[name][horizon];
        return [horizon, {
          coverage: cell.allHoldoutSignals.rate,
          opportunities: cell.allHoldoutSignals.opportunities,
          labeled: cell.allHoldoutSignals.labeled,
          threshold: cell.thresholdTriggered?.cutoffAbs ?? null,
          thresholdCoverage: cell.thresholdHoldoutSignals.rate,
          thresholdLabeled: cell.thresholdHoldoutSignals.labeled,
          adverseMarkoutTicks: cell.thresholdLabeledOutcomes.adverseMarkoutTicks,
          adverseMarkoutCi95: cell.thresholdLabeledOutcomes.adverseMarkoutCi95,
          signalSpreadTicks: cell.thresholdLabeledOutcomes.meanSignalSpreadTicks,
          topDepthShares: cell.thresholdLabeledOutcomes.meanTopDepthShares,
          depth20Shares: cell.thresholdLabeledOutcomes.meanDepth20Shares,
          slopeTicksPerFeatureUnit: cell.allLabeledOutcomes.continuousSlopeTicksPerFeatureUnit,
          slopeCi95: cell.allLabeledOutcomes.continuousSlopeCi95,
        }];
      })),
    ]));
    console.log(JSON.stringify({ output: result.output, source: result.report.source, split: result.report.protocol.split, counts: result.report.counts, thresholds: result.report.thresholds, holdout }, null, 2));
  } catch (error) {
    console.error(`realtime-ofi-diagnostic failed: ${error.message}`);
    process.exitCode = 1;
  }
}

