import { amount, TICK } from '../paper/fixed.mjs';
import { instantNs } from '../feed/contract.mjs';

export const subsecondMicropriceHorizonsMsV1 = Object.freeze([100, 250, 500, 1_000, 5_000]);

const MAX_LATENCY_MS = 10_000;
const spreadBucket = ticks => ticks <= 1 ? '1tick' : ticks <= 2 ? '2ticks' : '3plusTicks';
const sign = value => value > 0 ? 'up' : value < 0 ? 'down' : 'flat';

function makeHistogram(maximum) {
  return { bins: new Uint32Array(maximum + 1), count: 0, sum: 0, max: 0, overflow: 0, negative: 0 };
}

function addHistogram(histogram, value) {
  if (!Number.isFinite(value) || value < 0) {
    histogram.negative++;
    return;
  }
  const n = Math.round(value);
  if (n >= histogram.bins.length) {
    histogram.overflow++;
    return;
  }
  histogram.bins[n]++;
  histogram.count++;
  histogram.sum += n;
  histogram.max = Math.max(histogram.max, n);
}

function histogramSummary(histogram) {
  const quantile = p => {
    if (!histogram.count) return null;
    const target = Math.ceil(histogram.count * p);
    let seen = 0;
    for (let i = 0; i < histogram.bins.length; i++) {
      seen += histogram.bins[i];
      if (seen >= target) return i;
    }
    return null;
  };
  return {
    samples: histogram.count,
    meanMs: histogram.count ? histogram.sum / histogram.count : null,
    p50Ms: quantile(0.5),
    p95Ms: quantile(0.95),
    maxMs: histogram.count ? histogram.max : null,
    overRange: histogram.overflow,
    negativeOrInvalid: histogram.negative,
  };
}

function emptyAggregate() {
  return {
    samples: 0,
    markets: new Set(),
    pressureMoveTicks: 0,
    directionalWins: 0,
    afterSpreadTicks: 0,
    afterSpreadWins: 0,
    longExecutableTicks: 0,
    longPositive: 0,
    shortExecutableTicks: 0,
    shortPositive: 0,
    pressureExecutableTicks: 0,
    pressureExecutableSamples: 0,
    pressureExecutablePositive: 0,
    signalSpreadTicks: 0,
    futureSpreadTicks: 0,
  };
}

function foldFor(at, splitAt, purgeMs) {
  if (at < splitAt - purgeMs) return 'development';
  if (at < splitAt) return 'purged';
  return 'heldOut';
}

function percent(numerator, denominator) {
  return denominator ? numerator / denominator : null;
}

/**
 * Full-update, top-of-book microprice pressure study. Outcomes use the first
 * later exchange-sequenced book whose source clock reaches the fixed horizon.
 * Markouts are quote-price counterfactuals, not proof of actual execution.
 */
export function createSubsecondMicropriceEdgeV1({
  splitAt,
  horizonsMs = subsecondMicropriceHorizonsMsV1,
  purgeMs = 5_000,
  maximumLabelDelayMs = 15_000,
  maximumGapMs = 10_000,
} = {}) {
  const split = Date.parse(splitAt);
  if (!Number.isFinite(split) || !Array.isArray(horizonsMs) || horizonsMs.length !== 5
    || horizonsMs.some((h, i) => h !== subsecondMicropriceHorizonsMsV1[i])
    || !Number.isSafeInteger(purgeMs) || purgeMs < Math.max(...horizonsMs)
    || !Number.isSafeInteger(maximumLabelDelayMs) || maximumLabelDelayMs < 0
    || !Number.isSafeInteger(maximumGapMs) || maximumGapMs < 1) throw Error('SUBSECOND_OPTIONS');

  const versions = new Map();
  const lastObserved = new Map();
  const pending = new Map();
  const aggregates = new Map();
  const periodCounts = Object.fromEntries(['development', 'purged', 'heldOut'].map(period => [period,
    Object.fromEntries(horizonsMs.map(h => [h, { updates: 0, labels: 0, censored: 0, pendingAtEnd: 0 }]))]));
  const pointLatency = Object.fromEntries(['development', 'purged', 'heldOut'].map(period => [period, {
    sourceAgeMs: makeHistogram(MAX_LATENCY_MS),
    sourceToReceiptMs: makeHistogram(MAX_LATENCY_MS),
    receiptToObservedMs: makeHistogram(MAX_LATENCY_MS),
  }]));
  const labelLatency = Object.fromEntries(['development', 'purged', 'heldOut'].map(period => [period,
    Object.fromEntries(horizonsMs.map(h => [h, {
      observedDelayMs: makeHistogram(maximumLabelDelayMs),
      sourceOvershootMs: makeHistogram(maximumLabelDelayMs),
    }]))]));
  const counts = {
    acceptedUpdates: 0,
    duplicateSequences: 0,
    sequenceRollbacks: 0,
    invalidBooks: 0,
    sourceBoundaries: 0,
    observationGaps: 0,
    labels: 0,
    censoredLabels: 0,
    censorReasons: Object.create(null),
  };
  let endPendingCounts = null;
  let lastGlobalObserved = -1;

  const keyFor = (period, horizon, pressure, spread) => [period, horizon, pressure, spread].join('|');
  const getLane = exchangeId => {
    let lane = pending.get(exchangeId);
    if (!lane) {
      lane = new Map(horizonsMs.map(h => [h, []]));
      pending.set(exchangeId, lane);
    }
    return lane;
  };

  function censorQueue(exchangeId, reason) {
    const lane = pending.get(exchangeId);
    if (!lane) return;
    for (const [horizon, queue] of lane) {
      for (const item of queue) {
        periodCounts[item.period][horizon].censored++;
        counts.censoredLabels++;
        const key = `${reason}:${item.period}:${horizon}`;
        counts.censorReasons[key] = (counts.censorReasons[key] ?? 0) + 1;
      }
      queue.length = 0;
    }
  }

  function boundary(exchangeId, at, reason = 'source_boundary') {
    const ms = typeof at === 'number' ? at : Date.parse(at);
    if (!Number.isSafeInteger(ms) || ms < 0 || ms < lastGlobalObserved) throw Error('SUBSECOND_CLOCK');
    lastGlobalObserved = ms;
    const ids = exchangeId === null ? [...versions.keys(), ...pending.keys()] : [String(exchangeId)];
    for (const id of new Set(ids)) {
      censorQueue(id, reason);
      versions.delete(id);
      lastObserved.delete(id);
    }
    counts.sourceBoundaries++;
    return true;
  }

  function observe(book, { observedAt, receivedAt = observedAt } = {}) {
    const observedMs = Date.parse(observedAt);
    const receivedMs = Date.parse(receivedAt);
    const sourceNs = instantNs(book?.sourceAt);
    const sourceMs = Number(sourceNs / 1_000_000n);
    if (!Number.isSafeInteger(observedMs) || !Number.isSafeInteger(receivedMs)
      || observedMs < lastGlobalObserved || receivedMs > observedMs || sourceMs > observedMs) throw Error('SUBSECOND_CLOCK');
    lastGlobalObserved = observedMs;
    if (typeof book?.marketId !== 'string' || typeof book?.exchangeId !== 'string') throw Error('SUBSECOND_SCOPE');
    const exchangeId = book.exchangeId;
    const sequence = BigInt(book.sequence);
    const previous = versions.get(exchangeId);
    if (previous && sequence <= previous.sequence) {
      if (sequence < previous.sequence) {
        censorQueue(exchangeId, 'sequence_rollback');
        versions.delete(exchangeId);
        lastObserved.delete(exchangeId);
        counts.sequenceRollbacks++;
      } else counts.duplicateSequences++;
      return false;
    }
    if (previous && sourceNs < previous.sourceNs) {
      censorQueue(exchangeId, 'source_clock_rollback');
      versions.delete(exchangeId);
      lastObserved.delete(exchangeId);
      counts.sequenceRollbacks++;
      return false;
    }
    const priorObserved = lastObserved.get(exchangeId);
    if (priorObserved !== undefined && observedMs - priorObserved > maximumGapMs) {
      censorQueue(exchangeId, 'observation_gap');
      counts.observationGaps++;
    }

    const bid = amount(book.bids?.[0]?.price);
    const ask = amount(book.asks?.[0]?.price);
    const bidQuantity = book.bids?.[0]?.quantity;
    const askQuantity = book.asks?.[0]?.quantity;
    if (!(bid > 0n && ask > bid && Number.isSafeInteger(bidQuantity) && bidQuantity > 0
      && Number.isSafeInteger(askQuantity) && askQuantity > 0)) {
      counts.invalidBooks++;
      return false;
    }
    versions.set(exchangeId, { sequence, sourceNs });
    lastObserved.set(exchangeId, observedMs);
    counts.acceptedUpdates++;

    const pressure = sign(bidQuantity - askQuantity);
    const spreadTicks = Number(ask - bid) / Number(TICK);
    const spread = spreadBucket(spreadTicks);
    const midTicks = (Number(bid) + Number(ask)) / (2 * Number(TICK));
    const lane = getLane(exchangeId);

    for (const horizon of horizonsMs) {
      const queue = lane.get(horizon);
      while (queue.length) {
        const item = queue[0];
        const targetAt = item.observedAt + horizon;
        if (item.period === 'development' && (sourceNs >= BigInt(split) * 1_000_000n || observedMs >= split)) {
          queue.shift();
          periodCounts[item.period][horizon].censored++;
          counts.censoredLabels++;
          const key = `split_boundary:${item.period}:${horizon}`;
          counts.censorReasons[key] = (counts.censorReasons[key] ?? 0) + 1;
          continue;
        }
        if (observedMs > targetAt + maximumLabelDelayMs) {
          queue.shift();
          periodCounts[item.period][horizon].censored++;
          counts.censoredLabels++;
          counts.censorReasons[`label_delay:${item.period}:${horizon}`] =
            (counts.censorReasons[`label_delay:${item.period}:${horizon}`] ?? 0) + 1;
          continue;
        }
        if (sourceNs < BigInt(targetAt) * 1_000_000n) break;
        queue.shift();
        const nextMidTicks = (Number(bid) + Number(ask)) / (2 * Number(TICK));
        const midMoveTicks = nextMidTicks - item.midTicks;
        const futureSpreadTicks = Number(ask - bid) / Number(TICK);
        const longTicks = Number(bid - item.ask) / Number(TICK);
        const shortTicks = Number(item.bid - ask) / Number(TICK);
        const pressureTicks = item.pressure === 'up' ? longTicks : item.pressure === 'down' ? shortTicks : null;
        const groupKey = keyFor(item.period, horizon, item.pressure, item.spread);
        let aggregate = aggregates.get(groupKey);
        if (!aggregate) {
          aggregate = emptyAggregate();
          aggregates.set(groupKey, aggregate);
        }
        aggregate.samples++;
        aggregate.markets.add(item.marketId);
        aggregate.pressureMoveTicks += item.pressure === 'up' ? midMoveTicks : item.pressure === 'down' ? -midMoveTicks : 0;
        if (pressureTicks !== null) {
          aggregate.pressureExecutableTicks += pressureTicks;
          aggregate.pressureExecutableSamples++;
          if (pressureTicks > 0) aggregate.pressureExecutablePositive++;
        }
        aggregate.longExecutableTicks += longTicks;
        if (longTicks > 0) aggregate.longPositive++;
        aggregate.shortExecutableTicks += shortTicks;
        if (shortTicks > 0) aggregate.shortPositive++;
        const midDirectional = item.pressure === 'up' ? midMoveTicks : item.pressure === 'down' ? -midMoveTicks : 0;
        const afterSpread = midDirectional - item.spreadTicks;
        aggregate.afterSpreadTicks += afterSpread;
        if (afterSpread > 0) aggregate.afterSpreadWins++;
        if (midDirectional > 0) aggregate.directionalWins++;
        aggregate.signalSpreadTicks += item.spreadTicks;
        aggregate.futureSpreadTicks += futureSpreadTicks;
        periodCounts[item.period][horizon].labels++;
        addHistogram(labelLatency[item.period][horizon].observedDelayMs, observedMs - targetAt);
        addHistogram(labelLatency[item.period][horizon].sourceOvershootMs,
          sourceMs - targetAt);
        counts.labels++;
      }
      const period = foldFor(observedMs, split, purgeMs);
      periodCounts[period][horizon].updates++;
      if (period !== 'purged') {
        lane.get(horizon).push({
          period,
          horizon,
          observedAt: observedMs,
          targetAt: observedMs + horizon,
          marketId: book.marketId,
          pressure,
          spread,
          spreadTicks,
          midTicks,
          bid,
          ask,
        });
      }
    }

    const period = foldFor(observedMs, split, purgeMs);
    const latency = pointLatency[period];
    addHistogram(latency.sourceAgeMs, observedMs - sourceMs);
    addHistogram(latency.sourceToReceiptMs, receivedMs - sourceMs);
    addHistogram(latency.receiptToObservedMs, observedMs - receivedMs);
    return true;
  }

  function report({ source = null } = {}) {
    const pendingAtEnd = Object.fromEntries(['development', 'purged', 'heldOut'].map(period => [period,
      Object.fromEntries(horizonsMs.map(h => [h, 0]))]));
    if (endPendingCounts) {
      for (const period of Object.keys(pendingAtEnd)) for (const horizon of horizonsMs) {
        pendingAtEnd[period][horizon] = endPendingCounts[period][horizon];
      }
    } else {
      for (const lane of pending.values()) for (const [horizon, queue] of lane) {
        for (const item of queue) pendingAtEnd[item.period][horizon]++;
      }
    }
    const groups = [];
    for (const [key, value] of aggregates) {
      const [period, h, pressure, spread] = key.split('|');
      groups.push({
        period,
        horizonMs: Number(h),
        pressure,
        signalSpread: spread,
        samples: value.samples,
        markets: value.markets.size,
        meanPressureDirectedMidMoveTicks: percent(value.pressureMoveTicks, value.samples),
        pressureDirectionWinRate: percent(value.directionalWins, value.samples),
        meanAfterSignalSpreadTicks: percent(value.afterSpreadTicks, value.samples),
        afterSignalSpreadPositiveRate: percent(value.afterSpreadWins, value.samples),
        meanAskToLaterBidTicks: percent(value.longExecutableTicks, value.samples),
        askToLaterBidPositiveRate: percent(value.longPositive, value.samples),
        meanBidToLaterAskTicks: percent(value.shortExecutableTicks, value.samples),
        bidToLaterAskPositiveRate: percent(value.shortPositive, value.samples),
        meanPressureDirectedExecutableTicks: percent(value.pressureExecutableTicks, value.pressureExecutableSamples),
        pressureDirectedExecutableSamples: value.pressureExecutableSamples,
        pressureDirectedExecutablePositiveRate: percent(value.pressureExecutablePositive, value.pressureExecutableSamples),
        meanSignalSpreadTicks: percent(value.signalSpreadTicks, value.samples),
        meanFutureSpreadTicks: percent(value.futureSpreadTicks, value.samples),
      });
    }
    groups.sort((a, b) => a.horizonMs - b.horizonMs || a.period.localeCompare(b.period)
      || a.signalSpread.localeCompare(b.signalSpread) || a.pressure.localeCompare(b.pressure));
    const folds = {};
    for (const period of ['development', 'purged', 'heldOut']) {
      folds[period] = Object.fromEntries(horizonsMs.map(h => [h, {
        ...periodCounts[period][h],
        pendingAtEnd: pendingAtEnd[period][h],
        labelTiming: {
          observedDelayMs: histogramSummary(labelLatency[period][h].observedDelayMs),
          sourceOvershootMs: histogramSummary(labelLatency[period][h].sourceOvershootMs),
        },
      }]));
    }
    return {
      schemaVersion: 1,
      mode: 'subsecond_full_update_microprice_executable_markouts',
      source,
      protocol: {
        horizonsMs: [...horizonsMs],
        splitAt: new Date(split).toISOString(),
        chronologicalPurgeMs: purgeMs,
        maximumLabelDelayMs,
        maximumObservationGapMs: maximumGapMs,
        feesPerShare: '0.000000',
        updateRule: 'all valid sequenced source book updates; each exchange sequence used at most once',
        feature: 'top-of-book pressure sign from bid-quantity versus ask-quantity; fixed spread buckets; no fitted or swept thresholds',
        label: 'first later exchange-sequenced book with sourceAt >= decision observedAt + horizon; never use an earlier source clock',
        executableMarkouts: 'buy at signal ask then sell at later best bid; sell at signal bid then buy at later best ask; zero fees, no actual fill or order-latency claim',
        oneSpreadProxy: 'pressure-directed mid move minus the signal spread; exact ask-to-bid and bid-to-ask markouts include both observed top-of-book sides',
      },
      counts: {
        ...counts,
        censorReasons: Object.fromEntries(Object.entries(counts.censorReasons).sort(([a], [b]) => a.localeCompare(b))),
        marketCount: source?.scopeMarkets ?? null,
        groups: groups.length,
      },
      folds,
      pointLatencyByPeriod: Object.fromEntries(Object.entries(pointLatency).map(([period, stats]) => [period, {
        sourceAgeMs: histogramSummary(stats.sourceAgeMs),
        sourceToReceiptMs: histogramSummary(stats.sourceToReceiptMs),
        receiptToObservedMs: histogramSummary(stats.receiptToObservedMs),
      }])),
      groups,
      actualFillsProven: false,
      profitabilityEstablished: false,
      contestOrders: 0,
      networkRequests: 0,
      limitations: [
        'This is a descriptive, overlapping-event markout study on one historical capture, not an independent profitability test.',
        'A full book update and a later displayed opposite quote do not prove order acceptance, queue priority, fill quantity, or achievable execution latency.',
        'The source capture was previously used for the 1-second microprice study; this is a frequency-resolution comparison, not untouched data.',
        'Top-of-book pressure bins and spread buckets are predeclared; no thresholds or strategy parameters were tuned.',
        'Rows overlap across time and markets. They are correlated observations, not independent trials or evidence of scalable capacity.',
        'All monetary markouts are one-share displayed-price counterfactuals, zero fees, and exclude market impact.',
      ],
    };
  }

  function finish() {
    if (endPendingCounts) return;
    endPendingCounts = Object.fromEntries(['development', 'purged', 'heldOut'].map(period => [period,
      Object.fromEntries(horizonsMs.map(h => [h, 0]))]));
    for (const [exchangeId, lane] of pending) {
      for (const [horizon, queue] of lane) {
        for (const item of queue) {
          endPendingCounts[item.period][horizon]++;
          periodCounts[item.period][horizon].censored++;
          counts.censoredLabels++;
          const key = `capture_end:${item.period}:${horizon}`;
          counts.censorReasons[key] = (counts.censorReasons[key] ?? 0) + 1;
        }
        queue.length = 0;
      }
      pending.delete(exchangeId);
    }
  }

  return { observe, boundary, report, finish, horizonsMs: [...horizonsMs] };
}
