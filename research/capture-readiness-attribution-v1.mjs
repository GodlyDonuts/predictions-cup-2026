import {instantNs} from '../feed/contract.mjs';
import {engineBookPointsV1} from '../paper/maker-engine-book-point-v1.mjs';

export const CAPTURE_READINESS_PROTOCOL_V1 = Object.freeze({
  format: 'sig-feed-capture',
  schemaVersion: 1,
  deliveryModel: 'best-effort-authoritative-resync',
  lifecycleAppliedAfterFullConfig: true,
  batchGrouping: 'capture-observed UTC millisecond',
  readyRule: 'same-topic subscription epoch, recovery_finished.booksReady=true, and no later invalidation',
  lifecycleInvalidationCountRule: 'count subscribed, invalidation, disconnected, and recovery_started lifecycle rows; recovery_finished is reported separately',
  excludedRowsAreNotFilledOrMarketReturns: true,
});

const fail = code => { throw Error(`CAPTURE_READINESS_${code}`); };
const asMs = value => Number(instantNs(value) / 1_000_000n);

function topicIdentity(topic, tournamentId) {
  const match = new RegExp(`^tournament:${tournamentId.replaceAll('-', '\\-')}:market:(\\d+)$`).exec(topic ?? '');
  return match ? {marketId: match[1]} : null;
}

function freshMarket(marketId, exchangeId) {
  return {
    marketId, exchangeId, subscribedCount: 0, lifecycleInvalidations: 0,
    recoveryStarted: 0, recoveryFinishedReady: 0, recoveryFinishedNotReady: 0,
    staleRecoveryCompletions: 0,
    marketBatchRows: 0, acceptedBookPoints: 0, batchesExcludedWhileNotReady: 0,
    excludedByCause: {}, readinessBlockerApplicationsByCause: {},
    readyMs: 0, notReadyMs: 0, longestNotReadyMs: 0, readySinceMs: null,
    notReadySinceMs: null, firstSubscriptionMs: null, lastObservedMs: null,
    readinessTransitions: 0, repeatedSubscriptions: 0,
    maxEpochSeen: null, state: null,
  };
}

function closeNotReady(m, atMs) {
  if (m.notReadySinceMs !== null) {
    const duration = Math.max(0, atMs - m.notReadySinceMs);
    m.notReadyMs += duration;
    m.longestNotReadyMs = Math.max(m.longestNotReadyMs, duration);
    m.notReadySinceMs = null;
  }
}

function closeReady(m, atMs) {
  if (m.readySinceMs !== null) {
    m.readyMs += Math.max(0, atMs - m.readySinceMs);
    m.readySinceMs = null;
  }
}

function transition(m, next, atMs, cause) {
  const prior = m.state;
  if (prior === null) {
    m.state = false;
    m.notReadySinceMs = atMs;
    m.lastCause = cause;
    m.readinessTransitions++;
  } else if (prior !== next) {
    if (next) {
      closeNotReady(m, atMs);
      m.readySinceMs = atMs;
    } else {
      closeReady(m, atMs);
      m.notReadySinceMs = atMs;
    }
    m.state = next;
    m.lastCause = cause;
    m.readinessTransitions++;
  } else if (!next) {
    // Preserve the most recent concrete blocker for subsequent excluded batches.
    m.lastCause = cause;
  }
}

function increment(object, key, amount = 1) {
  object[key] = (object[key] ?? 0) + amount;
}

export function createCaptureReadinessAttributionV1({tournamentId, expectedMarkets = 237} = {}) {
  if (!/^[a-f0-9-]{36}$/i.test(tournamentId ?? '') || !Number.isSafeInteger(expectedMarkets) || expectedMarkets < 1) fail('SCOPE');
  const topics = new Map(), markets = new Map(), states = new Map(), lastRevision = new Map(), lastBookVersion = new Map();
  const totals = {
    captureRows: 0, configRows: 0, eventRows: 0, marketBatchRows: 0,
    lifecycleInvalidations: 0, validRecoveryCompletions: 0,
    recoveryNotReadyCompletions: 0, staleRecoveryCompletions: 0, notReadyBatches: 0,
    lifecycleInvalidationsByKind: {}, notReadyBatchesByCause: {}, missingStateRows: 0, epochMismatchRows: 0,
    revisionGaps: 0, duplicateRevisions: 0, resyncRequired: 0,
    settlements: 0, bookDirtyMarkers: 0, dirtyWithoutBook: 0,
    sameMillisecondInvalidationBatches: 0, acceptedBookPoints: 0,
    scopeRejectedBooks: 0, duplicateBookVersions: 0, rollbackBookVersions: 0,
    invalidRows: 0, globalInvalidations: 0,
    readinessBlockerApplicationsByCause: {}, rowsByKind: {}, captureIndexErrors: 0, observedClockRegressions: 0,
  };
  let expectedIndex = 0, lastObservedNs = null, lastObservedMs = null, firstObservedAt = null, throughObservedAt = null;
  let group = [];

  function marketFor(topic) {
    const identity = topicIdentity(topic, tournamentId);
    if (!identity || !topics.has(topic)) fail('TOPIC_SCOPE');
    return markets.get(identity.marketId);
  }

  function currentState(topic, updates) {
    return updates.get(topic) ?? states.get(topic) ?? null;
  }

  function applyInvalidation(topic, reason, atMs, updates, invalidated, {lifecycle = false} = {}) {
    const safeReason = String(reason ?? 'unspecified').slice(0, 64);
    increment(totals.readinessBlockerApplicationsByCause, safeReason);
    if (topic === null) {
      totals.globalInvalidations++;
      invalidated.add(null);
      for (const known of topics.keys()) {
        const prior = currentState(known, updates) ?? {epoch: null, ready: false, lastCause: 'unknown'};
        const next = {...prior, ready: false, lastCause: `global:${safeReason}`};
        updates.set(known, next);
        const m = markets.get(topicIdentity(known, tournamentId).marketId);
        m.lifecycleInvalidations += lifecycle ? 1 : 0;
        transition(m, false, atMs, next.lastCause);
        increment(m.readinessBlockerApplicationsByCause, `global:${safeReason}`);
      }
      return;
    }
    const m = marketFor(topic);
    const parsed = topicIdentity(topic, tournamentId);
    invalidated.add(parsed.marketId);
    const prior = currentState(topic, updates) ?? {epoch: null, ready: false, lastCause: 'unknown'};
    const next = {...prior, ready: false, lastCause: safeReason};
    updates.set(topic, next);
    if (lifecycle) m.lifecycleInvalidations++;
    increment(m.readinessBlockerApplicationsByCause, safeReason);
    transition(m, false, atMs, safeReason);
  }

  function flushGroup() {
    if (!group.length) return;
    const invalidated = new Set(), updates = new Map(), usableRows = [];
    for (const row of group) {
      const topic = row.topic ?? null;
      const parsed = topic === null ? null : topicIdentity(topic, tournamentId);
      if (topic !== null && (!parsed || !topics.has(topic))) fail('ROW_TOPIC_SCOPE');
      const atMs = asMs(row.observedAt);
      if (row.kind === 'subscribed') {
        if (!parsed || !Number.isSafeInteger(row.epoch) || row.epoch < 1) fail('SUBSCRIPTION');
        const m = markets.get(parsed.marketId), prior = currentState(topic, updates);
        if (prior) m.repeatedSubscriptions++;
        m.subscribedCount++;
        m.firstSubscriptionMs ??= atMs;
        m.maxEpochSeen = Math.max(m.maxEpochSeen ?? 0, row.epoch);
        totals.lifecycleInvalidations++;
        increment(totals.lifecycleInvalidationsByKind, 'subscribed');
        const cause = `subscribed:${String(row.reason ?? 'unspecified').slice(0, 48)}`;
        invalidated.add(parsed.marketId);
        updates.set(topic, {epoch: row.epoch, ready: false, lastCause: cause});
        m.lifecycleInvalidations++;
        increment(totals.readinessBlockerApplicationsByCause, cause);
        increment(m.readinessBlockerApplicationsByCause, cause);
        transition(m, false, atMs, cause);
        continue;
      }
      if (['invalidation', 'disconnected', 'recovery_started'].includes(row.kind)) {
        const cause = row.kind === 'invalidation'
          ? `invalidation:${String(row.reason ?? 'unspecified').slice(0, 48)}`
          : row.kind;
        totals.lifecycleInvalidations++;
        increment(totals.lifecycleInvalidationsByKind, row.kind);
        if (topic === null) {
          applyInvalidation(null, cause, atMs, updates, invalidated, {lifecycle: false});
        } else {
          const m = markets.get(parsed.marketId);
          if (row.kind === 'recovery_started') m.recoveryStarted++;
          applyInvalidation(topic, cause, atMs, updates, invalidated, {lifecycle: true});
        }
        continue;
      }
      if (row.kind === 'recovery_finished') {
        if (!parsed || !Number.isSafeInteger(row.epoch) || row.epoch < 1 || typeof row.booksReady !== 'boolean') fail('RECOVERY_COMPLETION');
        const m = markets.get(parsed.marketId), prior = currentState(topic, updates);
        // A delayed completion from an obsolete subscription must not replace or
        // re-enable the state of the currently subscribed epoch.
        if (!prior || row.epoch !== prior.epoch) {
          m.staleRecoveryCompletions++;
          totals.staleRecoveryCompletions++;
          continue;
        }
        if (row.booksReady) {
          m.recoveryFinishedReady++;
          totals.validRecoveryCompletions++;
          updates.set(topic, {...prior, epoch: row.epoch, ready: true, lastCause: null});
          transition(m, true, atMs, 'recovery_finished:true');
        } else {
          m.recoveryFinishedNotReady++;
          totals.recoveryNotReadyCompletions++;
          applyInvalidation(topic, 'recovery_finished:false', atMs, updates, invalidated);
          const state = currentState(topic, updates);
          updates.set(topic, {...state, epoch: row.epoch, ready: false, lastCause: 'recovery_finished:false'});
        }
        continue;
      }
      if (row.kind !== 'event') continue;
      totals.eventRows++;
      if (row.event !== 'market_batch') continue;
      totals.marketBatchRows++;
      const m = markets.get(parsed.marketId);
      m.marketBatchRows++;
      const state = currentState(topic, updates), payload = row.payload ?? {}, delivery = payload.delivery;
      if (!state) {
        totals.missingStateRows++;
        applyInvalidation(topic, 'missing_state', atMs, updates, invalidated);
        continue;
      }
      if (state.epoch !== row.epoch) {
        totals.epochMismatchRows++;
        applyInvalidation(topic, 'epoch_mismatch', atMs, updates, invalidated);
        continue;
      }
      const revision = delivery?.revision, previous = delivery?.previousRevision;
      if (!Number.isSafeInteger(revision) || revision < 0 || !Number.isSafeInteger(previous) || previous < 0) {
        applyInvalidation(topic, 'invalid_revision', atMs, updates, invalidated);
        continue;
      }
      const revKey = `${topic}#${row.epoch}`, oldRevision = lastRevision.get(revKey);
      if (oldRevision !== undefined && revision <= oldRevision) { totals.duplicateRevisions++; continue; }
      const revisionGap = oldRevision !== undefined && previous > oldRevision;
      lastRevision.set(revKey, revision);
      if (revisionGap) {
        totals.revisionGaps++;
        applyInvalidation(topic, 'revision_gap', atMs, updates, invalidated);
        continue;
      }
      if (payload.resyncRequired !== false) {
        totals.resyncRequired++;
        applyInvalidation(topic, 'resync_required', atMs, updates, invalidated);
        continue;
      }
      if (payload.marketSettled?.length) {
        totals.settlements++;
        applyInvalidation(topic, 'market_settled', atMs, updates, invalidated);
        continue;
      }
      totals.bookDirtyMarkers += payload.bookDirty?.length ?? 0;
      if ((payload.bookDirty ?? []).some(dirty => !(payload.books ?? []).some(book => String(book.exchangeId) === String(dirty.exchangeId)))) {
        totals.dirtyWithoutBook++;
        applyInvalidation(topic, 'dirty_without_book', atMs, updates, invalidated);
        continue;
      }
      if (!state.ready) {
        totals.notReadyBatches++;
        m.batchesExcludedWhileNotReady++;
        const cause = state.lastCause ?? 'not_ready_unspecified';
        increment(totals.notReadyBatchesByCause, cause);
        increment(m.excludedByCause, cause);
        continue;
      }
      usableRows.push(row);
    }

    const candidateByExchange = new Map();
    const engineScope = {tournamentId, exchanges: [...markets.values()].map(m => ({marketId: m.marketId, exchangeId: m.exchangeId}))};
    for (const row of usableRows) {
      const marketId = topicIdentity(row.topic, tournamentId).marketId;
      if (invalidated.has(null) || invalidated.has(marketId)) {
        totals.sameMillisecondInvalidationBatches++;
        continue;
      }
      const result = engineBookPointsV1(row, engineScope, row.observedAt);
      totals.scopeRejectedBooks += result.rejected.length;
      for (const book of result.books) {
        const m = markets.get(String(book.marketId));
        if (!m || m.exchangeId !== String(book.exchangeId)) { totals.scopeRejectedBooks++; continue; }
        const sequence = BigInt(book.sequence), sourceNs = instantNs(book.sourceAt), old = lastBookVersion.get(book.exchangeId);
        if (old && (sequence < old.sequence || (sequence === old.sequence && sourceNs <= old.sourceNs))) {
          totals.duplicateBookVersions++;
          if (sequence < old.sequence) totals.rollbackBookVersions++;
          continue;
        }
        lastBookVersion.set(book.exchangeId, {sequence, sourceNs});
        const bid = Number(book.bids[0]?.price), ask = Number(book.asks[0]?.price);
        const bidDepth = (book.bids ?? []).slice(0, 1).reduce((n, level) => n + Number(level.quantity), 0);
        const askDepth = (book.asks ?? []).slice(0, 1).reduce((n, level) => n + Number(level.quantity), 0);
        if (!Number.isFinite(bid) || !Number.isFinite(ask) || bid <= 0 || ask >= 1 || bid >= ask || bidDepth < 1 || askDepth < 1) {
          totals.invalidRows++;
          continue;
        }
        const key = String(book.exchangeId), prior = candidateByExchange.get(key);
        if (!prior || sequence > prior.sequence || (sequence === prior.sequence && sourceNs > prior.sourceNs)) {
          candidateByExchange.set(key, {market: m, sequence, sourceNs});
        }
      }
    }
    for (const {market} of candidateByExchange.values()) {
      market.acceptedBookPoints++;
      totals.acceptedBookPoints++;
    }
    for (const [topic, state] of updates) states.set(topic, state);
    group = [];
  }

  function push(row) {
    if (!row || row.format !== CAPTURE_READINESS_PROTOCOL_V1.format || row.schemaVersion !== 1
      || row.captureIndex !== expectedIndex++) fail('CAPTURE_CHAIN');
    let observedNs;
    try { observedNs = instantNs(row.observedAt); } catch { fail('CAPTURE_CLOCK'); }
    if (lastObservedNs !== null && observedNs < lastObservedNs) {
      totals.observedClockRegressions++;
      fail('CAPTURE_CLOCK_ORDER');
    }
    lastObservedNs = observedNs;
    const observedMs = Number(observedNs / 1_000_000n);
    firstObservedAt ??= row.observedAt;
    throughObservedAt = row.observedAt;
    totals.captureRows++;
    increment(totals.rowsByKind, row.kind ?? 'missing_kind');

    if (row.kind === 'config') {
      if (group.length || totals.eventRows) fail('CONFIG_ORDER');
      const parsed = topicIdentity(row.topic, tournamentId), scope = row.scope;
      if (!parsed || scope?.tournamentId !== tournamentId || String(scope?.marketId) !== parsed.marketId
        || !Array.isArray(scope.exchangeIds) || scope.exchangeIds.length !== 1) fail('CONFIG_SCOPE');
      if (topics.has(row.topic) || markets.has(parsed.marketId)) fail('DUPLICATE_CONFIG');
      const exchangeId = String(scope.exchangeIds[0]);
      if (!/^\d+$/.test(exchangeId) || [...markets.values()].some(m => m.exchangeId === exchangeId)) fail('DUPLICATE_EXCHANGE');
      topics.set(row.topic, parsed.marketId);
      markets.set(parsed.marketId, freshMarket(parsed.marketId, exchangeId));
      totals.configRows++;
      return;
    }
    if (row.kind === 'event' && totals.configRows !== expectedMarkets) fail('CONFIG_INCOMPLETE');
    if (row.kind !== 'event' && totals.configRows === expectedMarkets) {
      if (lastObservedMs !== null && observedMs !== lastObservedMs) flushGroup();
      lastObservedMs = observedMs;
      group.push(row);
      return;
    }
    if (row.kind === 'event') {
      const parsed = topicIdentity(row.topic, tournamentId);
      if (!parsed || !topics.has(row.topic)) fail('EVENT_SCOPE');
      if (lastObservedMs !== null && observedMs !== lastObservedMs) flushGroup();
      lastObservedMs = observedMs;
      group.push(row);
      return;
    }
    if (row.kind !== 'event') fail('ROW_KIND');
  }

  function finish() {
    flushGroup();
    if (totals.configRows !== expectedMarkets || markets.size !== expectedMarkets) fail('MARKET_COUNT');
    if (!firstObservedAt || !throughObservedAt) fail('EMPTY_CAPTURE');
    const endMs = asMs(throughObservedAt);
    const perMarket = [...markets.values()].map(m => {
      if (m.state === true) closeReady(m, endMs);
      else if (m.state === false) closeNotReady(m, endMs);
      const spanMs = m.firstSubscriptionMs === null ? 0 : Math.max(0, endMs - m.firstSubscriptionMs);
      const denominator = m.readyMs + m.notReadyMs;
      return {
        marketId: m.marketId, exchangeId: m.exchangeId,
        subscribedCount: m.subscribedCount, repeatedSubscriptions: m.repeatedSubscriptions,
        lifecycleInvalidations: m.lifecycleInvalidations,
        recoveryStarted: m.recoveryStarted, recoveryFinishedReady: m.recoveryFinishedReady,
        recoveryFinishedNotReady: m.recoveryFinishedNotReady,
        staleRecoveryCompletions: m.staleRecoveryCompletions,
        marketBatchRows: m.marketBatchRows,
        batchesExcludedWhileNotReady: m.batchesExcludedWhileNotReady,
        excludedByCause: Object.fromEntries(Object.entries(m.excludedByCause).sort(([a],[b]) => a.localeCompare(b))),
        acceptedBookPoints: m.acceptedBookPoints,
        readyMs: m.readyMs, notReadyMs: m.notReadyMs, readinessFraction: denominator ? m.readyMs / denominator : null,
        longestNotReadyMs: m.longestNotReadyMs, spanMs, readinessTransitions: m.readinessTransitions,
        readinessBlockerApplicationsByCause: Object.fromEntries(Object.entries(m.readinessBlockerApplicationsByCause).sort(([a],[b]) => a.localeCompare(b))),
      };
    }).sort((a, b) => Number(a.marketId) - Number(b.marketId));
    const readiness = perMarket.map(m => m.readinessFraction).filter(Number.isFinite).sort((a,b) => a-b);
    const quantile = q => readiness.length ? readiness[Math.min(readiness.length - 1, Math.floor(q * readiness.length))] : null;
    return {
      protocol: CAPTURE_READINESS_PROTOCOL_V1,
      capture: {captureRows: totals.captureRows, configRows: totals.configRows, eventRows: totals.eventRows,
        marketBatchRows: totals.marketBatchRows, firstObservedAt, throughObservedAt,
        captureDurationMs: Math.max(0, endMs - asMs(firstObservedAt)), rowsByKind: Object.fromEntries(Object.entries(totals.rowsByKind).sort(([a],[b]) => a.localeCompare(b)))},
      totals: {...totals, rowsByKind: undefined},
      perMarketSummary: {
        markets: perMarket.length,
        notReadyBatchCountSum: perMarket.reduce((n, m) => n + m.batchesExcludedWhileNotReady, 0),
        acceptedBookPointCountSum: perMarket.reduce((n, m) => n + m.acceptedBookPoints, 0),
        readyCompletionCountSum: perMarket.reduce((n, m) => n + m.recoveryFinishedReady, 0),
        marketBatchCountSum: perMarket.reduce((n, m) => n + m.marketBatchRows, 0),
        notReadyBatchFraction: totals.marketBatchRows ? totals.notReadyBatches / totals.marketBatchRows : null,
        acceptedBookPointsPerMarketBatch: totals.marketBatchRows ? totals.acceptedBookPoints / totals.marketBatchRows : null,
        marketsBelowOnePercentReady: perMarket.filter(m => Number.isFinite(m.readinessFraction) && m.readinessFraction < 0.01).length,
        aggregateReadyMarketHours: perMarket.reduce((n, m) => n + m.readyMs, 0) / 3_600_000,
        aggregateNotReadyMarketHours: perMarket.reduce((n, m) => n + m.notReadyMs, 0) / 3_600_000,
        readinessFractionByMarketQuantiles: {p00: quantile(0), p25: quantile(0.25), p50: quantile(0.5), p75: quantile(0.75), p100: quantile(1)},
      },
      markets: perMarket,
    };
  }

  return Object.freeze({push, finish});
}

export function assertCaptureReadinessReconciliationV1(actual, expected) {
  for (const key of ['notReadyBatches', 'validRecoveryCompletions', 'acceptedBookPoints', 'lifecycleInvalidations', 'staleRecoveryCompletions']) {
    if (expected?.[key] !== undefined && actual?.totals?.[key] !== expected[key]) fail(`RECONCILIATION_${key}`);
  }
  if (expected?.lifecycleInvalidationsByKind) {
    const sort = object => Object.fromEntries(Object.entries(object).sort(([a],[b]) => a.localeCompare(b)));
    if (JSON.stringify(sort(actual?.totals?.lifecycleInvalidationsByKind ?? {}))
      !== JSON.stringify(sort(expected.lifecycleInvalidationsByKind))) fail('RECONCILIATION_LIFECYCLE_KIND_COUNTS');
  }
  const sums = actual.perMarketSummary;
  if (sums?.notReadyBatchCountSum !== actual.totals.notReadyBatches
    || sums?.acceptedBookPointCountSum !== actual.totals.acceptedBookPoints
    || sums?.readyCompletionCountSum !== actual.totals.validRecoveryCompletions) fail('RECONCILIATION_SUMS');
  const lifecycleKindSum = Object.values(actual.totals.lifecycleInvalidationsByKind ?? {}).reduce((n, count) => n + count, 0);
  if (lifecycleKindSum !== actual.totals.lifecycleInvalidations) fail('RECONCILIATION_LIFECYCLE_KINDS');
  return true;
}
