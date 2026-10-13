import { instantNs } from '../feed/contract.mjs';
import {
  MULTILEVEL_BOOK_FLOW_HORIZONS_MS_V1,
  MULTILEVEL_BOOK_FLOW_LEVELS_V1,
  multiLevelOfiV1,
  parseMultiLevelBookV1,
} from './multilevel-book-flow-v1.mjs';

export const FLOW_ENTRY_LATENCIES_MS_V1 = Object.freeze([0, 1000, 3000]);
export const FLOW_ENTRY_PURGE_MS_V1 = 5000;
export const FLOW_MAX_ENTRY_OVERRUN_MS_V1 = 15000;
export const FLOW_MAX_MARK_DELAY_MS_V1 = 15000;
export const FLOW_MAX_BOOK_AGE_MS_V1 = 3000;
export const FLOW_MAX_OBSERVATION_GAP_MS_V1 = 10000;

const MS_NS = 1_000_000n;
const HORIZONS = MULTILEVEL_BOOK_FLOW_HORIZONS_MS_V1;
const FEATURES = Object.freeze([
  ...MULTILEVEL_BOOK_FLOW_LEVELS_V1.map(level => `ofiL${level}`),
  'ofiDecay20',
]);
const need = (condition, code) => { if (!condition) throw Error(code); };
const parseNs = (value, code) => { try { return instantNs(value); } catch { throw Error(code); } };
const elapsedMs = (start, end) => Number(end - start) / Number(MS_NS);
const sign = value => Math.sign(value ?? 0);

function clusterSummary(markets) {
  const perMarket = [...markets.values()].filter(market => market.labels > 0)
    .map(market => ({
      selected: market.selected / market.labels,
      pairedDelta: market.pairedDelta / market.labels,
    }));
  if (!perMarket.length) return {
    clusters: 0,
    effectiveClusters: 0,
    selectedOpportunityMeanTicks: null,
    selectedOpportunitySeTicks: null,
    selectedOpportunityCi95Ticks: null,
    pairedDeltaMeanVsTop1Ticks: null,
    pairedDeltaSeTicks: null,
    pairedDeltaCi95Ticks: null,
  };
  const mean = key => perMarket.reduce((total, row) => total + row[key], 0) / perMarket.length;
  const se = key => {
    if (perMarket.length < 2) return null;
    const avg = mean(key);
    const variance = perMarket.reduce((total, row) => total + (row[key] - avg) ** 2, 0) / (perMarket.length - 1);
    return Math.sqrt(variance / perMarket.length);
  };
  const selected = mean('selected'), delta = mean('pairedDelta');
  const selectedSe = se('selected'), deltaSe = se('pairedDelta');
  const rounded = value => Number(value.toFixed(6));
  return {
    clusters: perMarket.length,
    effectiveClusters: perMarket.length,
    selectedOpportunityMeanTicks: rounded(selected),
    selectedOpportunitySeTicks: selectedSe === null ? null : rounded(selectedSe),
    selectedOpportunityCi95Ticks: selectedSe === null ? null : [rounded(selected - 1.96 * selectedSe), rounded(selected + 1.96 * selectedSe)],
    pairedDeltaMeanVsTop1Ticks: rounded(delta),
    pairedDeltaSeTicks: deltaSe === null ? null : rounded(deltaSe),
    pairedDeltaCi95Ticks: deltaSe === null ? null : [rounded(delta - 1.96 * deltaSe), rounded(delta + 1.96 * deltaSe)],
  };
}

function periodAt(receivedNs, splitNs, purgeNs) {
  if (receivedNs < splitNs - purgeNs) return 'development';
  if (receivedNs < splitNs) return 'purged';
  return 'heldOut';
}

export function createMultiLevelBookFlowLatencyV1({
  scope,
  splitAt,
  purgeMs = FLOW_ENTRY_PURGE_MS_V1,
  maximumEntryOverrunMs = FLOW_MAX_ENTRY_OVERRUN_MS_V1,
  maximumMarkDelayMs = FLOW_MAX_MARK_DELAY_MS_V1,
  maximumBookAgeMs = FLOW_MAX_BOOK_AGE_MS_V1,
  maximumObservationGapMs = FLOW_MAX_OBSERVATION_GAP_MS_V1,
} = {}) {
  need(scope && typeof scope.tournamentId === 'string' && scope.tournamentId.length > 0
    && Array.isArray(scope.exchanges) && scope.exchanges.length > 0, 'MLOFI_LATENCY_SCOPE');
  const members = new Map();
  for (const member of scope.exchanges) {
    need(typeof member?.marketId === 'string' && member.marketId.length > 0
      && typeof member?.exchangeId === 'string' && member.exchangeId.length > 0, 'MLOFI_LATENCY_MEMBER');
    need(!members.has(member.exchangeId), 'MLOFI_LATENCY_AMBIGUOUS_EXCHANGE');
    members.set(member.exchangeId, member.marketId);
  }
  const splitNs = parseNs(splitAt, 'MLOFI_LATENCY_SPLIT');
  need(Number.isSafeInteger(purgeMs) && purgeMs >= Math.max(...HORIZONS), 'MLOFI_LATENCY_PURGE');
  for (const [value, code] of [
    [maximumEntryOverrunMs, 'MLOFI_LATENCY_ENTRY_OVERRUN'],
    [maximumMarkDelayMs, 'MLOFI_LATENCY_MARK_DELAY'],
    [maximumBookAgeMs, 'MLOFI_LATENCY_BOOK_AGE'],
    [maximumObservationGapMs, 'MLOFI_LATENCY_OBSERVATION_GAP'],
  ]) need(Number.isSafeInteger(value) && value >= 0 && (code === 'MLOFI_LATENCY_OBSERVATION_GAP' ? value > 0 : true), code);

  const purgeNs = BigInt(purgeMs) * MS_NS;
  const entryOverrunNs = BigInt(maximumEntryOverrunMs) * MS_NS;
  const markDelayNs = BigInt(maximumMarkDelayMs) * MS_NS;
  const bookAgeNs = BigInt(maximumBookAgeMs) * MS_NS;
  const observationGapNs = BigInt(maximumObservationGapMs) * MS_NS;
  const priorBooks = new Map(), lastObserved = new Map(), pending = new Map(), cells = new Map();
  const counts = {
    booksSeen: 0,
    acceptedBooks: 0,
    insufficientDepth: 0,
    duplicateSequences: 0,
    sequenceRollbacks: 0,
    sourceClockRollbacks: 0,
    receiptClockRollbacks: 0,
    observationClockRollbacks: 0,
    observationGaps: 0,
    staleBooks: 0,
    decisions: 0,
    entryProxies: Object.fromEntries(FLOW_ENTRY_LATENCIES_MS_V1.map(ms => [ms, 0])),
    labels: 0,
    censoredEntries: 0,
    censoredLabels: 0,
    labelAfterDevelopmentCutoff: 0,
    pendingAtEnd: 0,
    boundaries: 0,
    boundaryReasons: Object.create(null),
  };

  function cell(period, latency, horizon, feature) {
    const key = `${period}|${latency}|${horizon}|${feature}`;
    let row = cells.get(key);
    if (!row) {
      row = {
        period, responseLatencyMs: latency, horizonMs: horizon, feature,
        decisionUpdates: 0, proposedSignals: 0, entryProxies: 0,
        labeledOutcomes: 0, nonzeroSignals: 0,
        sumOpportunityNetTicks: 0, sumTop1OpportunityNetTicks: 0, sumPairedDeltaTicks: 0,
        sumSignalDirectedNetTicks: 0, sumSignalDirectedMidTicks: 0,
        sumTop1OnSameSignalsTicks: 0, positiveExecutable: 0,
        markets: new Map(), censorReasons: Object.create(null),
      };
      cells.set(key, row);
    }
    return row;
  }

  function markCensored(item, reason, horizon = null) {
    if (!item.entry) counts.censoredEntries++;
    const selectedHorizons = horizon === null ? HORIZONS : [horizon];
    for (const name of item.featureNames) for (const holdMs of selectedHorizons) {
      const row = cell(item.period, item.latencyMs, holdMs, name);
      row.censorReasons[reason] = (row.censorReasons[reason] ?? 0) + 1;
      counts.censoredLabels++;
      if (reason === 'development_cutoff') counts.labelAfterDevelopmentCutoff++;
    }
    if (horizon !== null) item.doneHorizons.add(horizon);
    else item.cancelled = true;
  }

  function cancel(exchangeId, reason) {
    const queue = pending.get(exchangeId) ?? [];
    for (const item of queue) {
      if (item.entry) {
        for (const horizon of HORIZONS) if (!item.doneHorizons.has(horizon)) markCensored(item, reason, horizon);
      } else markCensored(item, reason);
      item.cancelled = true;
    }
    pending.set(exchangeId, []);
  }

  function boundary(exchangeId, at, reason = 'source_boundary') {
    parseNs(at, 'MLOFI_LATENCY_BOUNDARY_CLOCK');
    const ids = exchangeId === null
      ? new Set([...priorBooks.keys(), ...pending.keys()])
      : new Set([String(exchangeId)]);
    for (const id of ids) {
      cancel(id, reason);
      priorBooks.delete(id);
      lastObserved.delete(id);
    }
    counts.boundaries++;
    counts.boundaryReasons[reason] = (counts.boundaryReasons[reason] ?? 0) + 1;
  }

  function observe(bookInput, { observedAt } = {}) {
    counts.booksSeen++;
    const observedNs = parseNs(observedAt, 'MLOFI_LATENCY_OBSERVED_CLOCK');
    need(bookInput?.tournamentId === scope.tournamentId
      && members.get(bookInput?.exchangeId) === bookInput?.marketId, 'MLOFI_LATENCY_SCOPE_MISMATCH');
    const current = parseMultiLevelBookV1(bookInput);
    need(current.receivedNs <= observedNs && current.sourceNs <= observedNs, 'MLOFI_LATENCY_FUTURE_BOOK');
    if (current.receivedNs - current.sourceNs > bookAgeNs
      || observedNs - current.receivedNs > bookAgeNs
      || observedNs - current.sourceNs > bookAgeNs) {
      counts.staleBooks++;
      throw Error('MLOFI_LATENCY_STALE_BOOK');
    }

    const previousObserved = lastObserved.get(current.exchangeId);
    if (previousObserved !== undefined && observedNs < previousObserved) {
      cancel(current.exchangeId, 'observation_clock_rollback');
      priorBooks.delete(current.exchangeId);
      lastObserved.delete(current.exchangeId);
      counts.observationClockRollbacks++;
      return false;
    }
    if (previousObserved !== undefined && observedNs - previousObserved > observationGapNs) {
      cancel(current.exchangeId, 'observation_gap');
      priorBooks.delete(current.exchangeId);
      counts.observationGaps++;
    }
    const previous = priorBooks.get(current.exchangeId);
    if (previous && current.sequence <= previous.sequence) {
      if (current.sequence < previous.sequence) {
        cancel(current.exchangeId, 'sequence_rollback');
        priorBooks.delete(current.exchangeId);
        lastObserved.delete(current.exchangeId);
        counts.sequenceRollbacks++;
      } else counts.duplicateSequences++;
      return false;
    }
    if (previous && current.sourceNs < previous.sourceNs) {
      cancel(current.exchangeId, 'source_clock_rollback');
      priorBooks.delete(current.exchangeId);
      lastObserved.delete(current.exchangeId);
      counts.sourceClockRollbacks++;
      return false;
    }
    if (previous && current.receivedNs < previous.receivedNs) {
      cancel(current.exchangeId, 'receipt_clock_rollback');
      priorBooks.delete(current.exchangeId);
      lastObserved.delete(current.exchangeId);
      counts.receiptClockRollbacks++;
      return false;
    }
    counts.acceptedBooks++;
    lastObserved.set(current.exchangeId, observedNs);

    const queue = pending.get(current.exchangeId) ?? [];
    const keep = [];
    for (const item of queue) {
      if (item.cancelled) continue;
      if (!item.entry) {
        const entryLateByObservation = observedNs - item.entryTargetNs > entryOverrunNs;
        if (entryLateByObservation) {
          markCensored(item, 'entry_delay');
          continue;
        }
        const isLater = current.sequence > item.decisionSequence
          && current.sourceNs > item.decisionSourceNs && current.receivedNs > item.decisionNs;
        if (!isLater || current.sourceNs < item.entryTargetNs || current.receivedNs < item.entryTargetNs) {
          keep.push(item);
          continue;
        }
        const eventOverrun = current.sourceNs - item.entryTargetNs;
        const receiveOverrun = current.receivedNs - item.entryTargetNs;
        if (eventOverrun > entryOverrunNs || receiveOverrun > entryOverrunNs) {
          markCensored(item, 'entry_delay');
          continue;
        }
        if (item.period === 'development' && current.receivedNs >= splitNs - purgeNs) {
          markCensored(item, 'development_cutoff');
          continue;
        }
        item.entry = current;
        counts.entryProxies[item.latencyMs]++;
        for (const name of item.featureNames) for (const horizon of HORIZONS) {
          cell(item.period, item.latencyMs, horizon, name).entryProxies++;
        }
        keep.push(item);
        continue;
      }

      let complete = true;
      for (const horizon of HORIZONS) {
        if (item.doneHorizons.has(horizon)) continue;
        const targetNs = item.entry.receivedNs + BigInt(horizon) * MS_NS;
        if (observedNs - targetNs > markDelayNs) {
          markCensored(item, 'mark_delay', horizon);
          continue;
        }
        const isLater = current.sequence > item.entry.sequence && current.sourceNs > item.entry.sourceNs
          && current.receivedNs > item.entry.receivedNs;
        if (!isLater || current.sourceNs < targetNs || current.receivedNs < targetNs) {
          complete = false;
          continue;
        }
        const sourceDelay = current.sourceNs - targetNs;
        const receiveDelay = current.receivedNs - targetNs;
        const observationDelay = observedNs - targetNs;
        if (sourceDelay > markDelayNs || receiveDelay > markDelayNs || observationDelay > markDelayNs) {
          markCensored(item, 'mark_delay', horizon);
          continue;
        }
        if (item.period === 'development' && current.receivedNs >= splitNs - purgeNs) {
          markCensored(item, 'development_cutoff', horizon);
          continue;
        }
        thisLabel(item, current, horizon);
        item.doneHorizons.add(horizon);
      }
      for (const horizon of HORIZONS) if (!item.doneHorizons.has(horizon)) complete = false;
      if (!complete) keep.push(item);
    }
    pending.set(current.exchangeId, keep);

    let features = Object.create(null);
    if (previous) features = multiLevelOfiV1(current, previous);
    if (!Object.keys(features).length) {
      if (previous) counts.insufficientDepth++;
      priorBooks.set(current.exchangeId, current);
      return true;
    }
    const period = periodAt(current.receivedNs, splitNs, purgeNs);
    const featureNames = FEATURES.filter(name => Object.hasOwn(features, name));
    for (const latencyMs of FLOW_ENTRY_LATENCIES_MS_V1) {
      const item = {
        marketId: current.marketId,
        exchangeId: current.exchangeId,
        period,
        latencyMs,
        decisionNs: current.receivedNs,
        decisionSourceNs: current.sourceNs,
        decisionSequence: current.sequence,
        entryTargetNs: current.receivedNs + BigInt(latencyMs) * MS_NS,
        featureValues: features,
        featureNames,
        top1Signal: sign(features.ofiL1),
        entry: null,
        doneHorizons: new Set(),
        cancelled: false,
      };
      for (const name of featureNames) for (const horizon of HORIZONS) {
        const row = cell(period, latencyMs, horizon, name);
        row.decisionUpdates++;
        if (sign(features[name])) row.proposedSignals++;
      }
      counts.decisions++;
      const items = pending.get(current.exchangeId) ?? [];
      items.push(item);
      pending.set(current.exchangeId, items);
    }
    priorBooks.set(current.exchangeId, current);
    return true;
  }

  function thisLabel(item, current, horizon) {
    const entry = item.entry;
    const deltaMid = current.midTicks - entry.midTicks;
    const longExec = current.bidTick - entry.askTick;
    const shortExec = entry.bidTick - current.askTick;
    for (const name of item.featureNames) {
      const signal = sign(item.featureValues[name]);
      const selected = signal > 0 ? longExec : signal < 0 ? shortExec : 0;
      const top1 = item.top1Signal > 0 ? longExec : item.top1Signal < 0 ? shortExec : 0;
      const directional = signal * deltaMid;
      const row = cell(item.period, item.latencyMs, horizon, name);
      row.labeledOutcomes++;
      row.sumOpportunityNetTicks += selected;
      row.sumTop1OpportunityNetTicks += top1;
      row.sumPairedDeltaTicks += selected - top1;
      let market = row.markets.get(item.marketId);
      if (!market) { market = { labels: 0, selected: 0, pairedDelta: 0 }; row.markets.set(item.marketId, market); }
      market.labels++;
      market.selected += selected;
      market.pairedDelta += selected - top1;
      counts.labels++;
      if (!signal) continue;
      row.nonzeroSignals++;
      row.sumSignalDirectedNetTicks += selected;
      row.sumSignalDirectedMidTicks += directional;
      row.sumTop1OnSameSignalsTicks += top1;
      row.positiveExecutable += selected > 0 ? 1 : 0;
    }
  }

  function finish() {
    for (const [exchangeId, queue] of pending) {
      for (const item of queue) {
        if (item.entry) {
          for (const horizon of HORIZONS) if (!item.doneHorizons.has(horizon)) markCensored(item, 'capture_end', horizon);
        } else markCensored(item, 'capture_end');
      }
      counts.pendingAtEnd += queue.length;
      pending.set(exchangeId, []);
    }
  }

  function report({ source = null } = {}) {
    const round = value => Number(value.toFixed(6));
    const rows = [...cells.values()].map(row => {
      const labels = row.labeledOutcomes, signals = row.nonzeroSignals;
      const opportunityMean = labels ? row.sumOpportunityNetTicks / labels : null;
      const signalMean = signals ? row.sumSignalDirectedNetTicks / signals : null;
      return {
        period: row.period,
        responseLatencyMs: row.responseLatencyMs,
        horizonMs: row.horizonMs,
        feature: row.feature,
        decisionUpdates: row.decisionUpdates,
        proposedSignals: row.proposedSignals,
        entryProxies: row.entryProxies,
        labeledOutcomes: labels,
        nonzeroSignals: signals,
        signalRate: labels ? round(signals / labels) : null,
        uniqueMarkets: row.markets.size,
        meanSignalDirectedMidMoveTicks: signals ? round(row.sumSignalDirectedMidTicks / signals) : null,
        meanSignalDirectedExecutableTicks: signalMean === null ? null : round(signalMean),
        oneTickEachWayStressSignalMeanTicks: signalMean === null ? null : round(signalMean - 2),
        meanOpportunityNetPerDecisionTicks: opportunityMean === null ? null : round(opportunityMean),
        oneTickEachWayStressOpportunityMeanTicks: opportunityMean === null ? null
          : round(opportunityMean - 2 * (signals / labels)),
        meanTop1OnSameSignalsTicks: signals ? round(row.sumTop1OnSameSignalsTicks / signals) : null,
        meanTop1OpportunityNetPerDecisionTicks: labels ? round(row.sumTop1OpportunityNetTicks / labels) : null,
        meanPairedDeltaVsTop1Ticks: labels ? round(row.sumPairedDeltaTicks / labels) : null,
        positiveExecutableRate: signals ? round(row.positiveExecutable / signals) : null,
        clusterInference: clusterSummary(row.markets),
        censoredByReason: Object.fromEntries(Object.entries(row.censorReasons).sort(([a], [b]) => a.localeCompare(b))),
      };
    }).sort((a, b) => ['development', 'purged', 'heldOut'].indexOf(a.period) - ['development', 'purged', 'heldOut'].indexOf(b.period)
      || a.responseLatencyMs - b.responseLatencyMs || a.horizonMs - b.horizonMs || a.feature.localeCompare(b.feature));
    return {
      schemaVersion: 1,
      mode: 'receipt_anchored_multilevel_order_flow_latency_diagnostic',
      source,
      definition: {
        featureSource: 'fixed rank-aligned OFI L1/L5/L10/L20 and 1/sqrt(rank)-weighted 20-level OFI from the preceding accepted sequenced book',
        decisionAvailability: 'each signal is timestamped at the current book packet receivedAt; fold assignment uses this local receipt time',
        responseLatencyMs: FLOW_ENTRY_LATENCIES_MS_V1,
        entry: 'for each latency, first strictly later sequenced book whose sourceAt and receivedAt are both at or after signal receipt plus latency; the signal book is never reused as its own entry',
        holding: 'fixed horizon begins at the entry book local receivedAt; exit mark requires both later sourceAt and receivedAt at or after that target',
        freshness: { maximumBookAgeMs, maximumObservationGapMs, maximumEntryOverrunMs, maximumMarkDelayMs },
        horizonsMs: HORIZONS,
        splitAt: new Date(Number(splitNs / MS_NS)).toISOString(),
        purgeMs,
        developmentLabelsMustBeReceivedBefore: new Date(Number((splitNs - purgeNs) / MS_NS)).toISOString(),
        top1Comparison: 'OFI L1 sign is compared on the exact same entry book, horizon and feature-labeled decision sample; zero signal is flat at zero return',
        execution: 'one-share counterfactual YES ask-to-later-bid or bid-to-later-ask, zero fees; one tick adverse entry plus one tick adverse exit stress',
        feePerShare: '0.000000',
        selection: 'fixed signal sign only; all fixed latency, horizon, feature, and chronological cells are emitted; no tuned thresholds or strategy selection',
      },
      counts: {
        ...counts,
        entryProxies: Object.fromEntries(Object.entries(counts.entryProxies).sort(([a], [b]) => Number(a) - Number(b))),
        boundaryReasons: Object.fromEntries(Object.entries(counts.boundaryReasons).sort(([a], [b]) => a.localeCompare(b))),
      },
      cells: rows,
      limitations: [
        'Entry and exit book prices are observable-price proxies, not proof that an order could be accepted or filled at that exact level.',
        'One share is evaluated without portfolio funding, inventory constraints, queue priority, adverse selection beyond the spread, or concurrency.',
        'Signals and horizons overlap; per-market intervals do not remove common election/race shocks and the independent event count is unknown.',
        'This single captured session is diagnostic evidence only; it cannot establish persistent alpha or production readiness.',
      ],
      actualFillsProven: false,
      profitabilityEstablished: false,
      contestOrders: 0,
      networkRequests: 0,
    };
  }

  return { observe, boundary, finish, report };
}
