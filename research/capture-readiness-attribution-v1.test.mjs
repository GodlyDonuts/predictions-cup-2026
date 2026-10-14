import test from 'node:test';
import assert from 'node:assert/strict';
import {
  assertCaptureReadinessReconciliationV1,
  createCaptureReadinessAttributionV1,
} from './capture-readiness-attribution-v1.mjs';

const tournamentId = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const topic = marketId => `tournament:${tournamentId}:market:${marketId}`;
const instant = second => `2026-10-13T02:55:${String(second).padStart(2, '0')}.000Z`;

function captureRows(items) {
  return items.map((row, captureIndex) => ({
    format: 'sig-feed-capture', schemaVersion: 1, captureIndex,
    observedAt: row.observedAt ?? instant(captureIndex), ...row,
  }));
}

function config(marketId, exchangeId) {
  return {kind: 'config', topic: topic(marketId), scope: {tournamentId, marketId, exchangeIds: [exchangeId]}};
}

function lifecycle(kind, marketId, epoch, at, extra = {}) {
  return {kind, topic: marketId === null ? undefined : topic(marketId), epoch, observedAt: at, ...extra};
}

function marketBatch(marketId, exchangeId, epoch, revision, previousRevision, at) {
  return {
    kind: 'event', event: 'market_batch', topic: topic(marketId), epoch, receivedAt: at, observedAt: at,
    payload: {
      delivery: {model: 'best-effort-authoritative-resync', revision, previousRevision,
        sourceSequenceFrom: null, sourceSequenceThrough: null},
      resyncRequired: false, marketSettled: [], bookDirty: [],
      books: [{exchangeId, asOf: {sequence: revision, at}, nextExpiryAt: null, expiryKnown: true,
        bids: [{price: 0.4, quantity: 3}], asks: [{price: 0.41, quantity: 4}]}],
      trades: [],
    },
  };
}

function analyze(rows, expectedMarkets = 1) {
  const tracker = createCaptureReadinessAttributionV1({tournamentId, expectedMarkets});
  for (const row of captureRows(rows)) tracker.push(row);
  return tracker.finish();
}

test('subscription data stays unusable until successful same-epoch recovery', () => {
  const result = analyze([
    config('10', '100'),
    lifecycle('subscribed', '10', 1, instant(1), {reason: 'INITIAL'}),
    marketBatch('10', '100', 1, 1, 0, instant(2)),
    lifecycle('recovery_finished', '10', 1, instant(3), {booksReady: true}),
    marketBatch('10', '100', 1, 2, 1, instant(4)),
  ]);
  assert.equal(result.totals.notReadyBatches, 1);
  assert.deepEqual(result.totals.notReadyBatchesByCause, {'subscribed:INITIAL': 1});
  assert.equal(result.totals.validRecoveryCompletions, 1);
  assert.equal(result.totals.acceptedBookPoints, 1);
  assert.equal(result.markets[0].readinessFraction, 1 / 3);
});

test('not-ready recovery completion remains blocked and is attributed until a later success', () => {
  const result = analyze([
    config('10', '100'),
    lifecycle('subscribed', '10', 1, instant(1), {reason: 'INITIAL'}),
    lifecycle('recovery_finished', '10', 1, instant(2), {booksReady: false}),
    marketBatch('10', '100', 1, 1, 0, instant(3)),
    lifecycle('recovery_finished', '10', 1, instant(4), {booksReady: true}),
    marketBatch('10', '100', 1, 2, 1, instant(5)),
  ]);
  assert.equal(result.totals.recoveryNotReadyCompletions, 1);
  assert.equal(result.totals.notReadyBatches, 1);
  assert.deepEqual(result.totals.notReadyBatchesByCause, {'recovery_finished:false': 1});
  assert.equal(result.totals.acceptedBookPoints, 1);
});

test('global invalidation blocks every configured topic until each recovers', () => {
  const result = analyze([
    config('10', '100'), config('20', '200'),
    lifecycle('subscribed', '10', 1, instant(1)), lifecycle('subscribed', '20', 1, instant(1)),
    lifecycle('recovery_finished', '10', 1, instant(2), {booksReady: true}),
    lifecycle('recovery_finished', '20', 1, instant(2), {booksReady: true}),
    lifecycle('disconnected', null, null, instant(3)),
    marketBatch('10', '100', 1, 1, 0, instant(4)),
    marketBatch('20', '200', 1, 1, 0, instant(4)),
    lifecycle('recovery_finished', '10', 1, instant(5), {booksReady: true}),
    lifecycle('recovery_finished', '20', 1, instant(5), {booksReady: true}),
    marketBatch('10', '100', 1, 2, 1, instant(6)),
    marketBatch('20', '200', 1, 2, 1, instant(6)),
  ], 2);
  assert.equal(result.totals.globalInvalidations, 1);
  assert.equal(result.totals.notReadyBatches, 2);
  assert.deepEqual(result.totals.notReadyBatchesByCause, {'global:disconnected': 2});
  assert.equal(result.totals.acceptedBookPoints, 2);
});

test('resubscription changes epoch; old-epoch events cannot restore state', () => {
  const result = analyze([
    config('10', '100'),
    lifecycle('subscribed', '10', 1, instant(1), {reason: 'INITIAL'}),
    lifecycle('recovery_finished', '10', 1, instant(2), {booksReady: true}),
    marketBatch('10', '100', 1, 1, 0, instant(3)),
    lifecycle('subscribed', '10', 2, instant(4), {reason: 'RECONNECT'}),
    marketBatch('10', '100', 1, 2, 1, instant(5)),
    marketBatch('10', '100', 2, 1, 0, instant(6)),
    lifecycle('recovery_finished', '10', 2, instant(7), {booksReady: true}),
    marketBatch('10', '100', 2, 2, 1, instant(8)),
  ]);
  assert.equal(result.totals.epochMismatchRows, 1);
  assert.equal(result.totals.notReadyBatches, 1);
  assert.deepEqual(result.totals.notReadyBatchesByCause, {epoch_mismatch: 1});
  assert.equal(result.totals.acceptedBookPoints, 2);
  assert.equal(result.markets[0].repeatedSubscriptions, 1);
});

test('stale recovery completion cannot mark a newer subscription epoch ready', () => {
  const result = analyze([
    config('10', '100'),
    lifecycle('subscribed', '10', 1, instant(1), {reason: 'INITIAL'}),
    lifecycle('recovery_finished', '10', 1, instant(2), {booksReady: true}),
    marketBatch('10', '100', 1, 1, 0, instant(3)),
    lifecycle('subscribed', '10', 2, instant(4), {reason: 'RECONNECT'}),
    lifecycle('recovery_finished', '10', 1, instant(5), {booksReady: true}),
    marketBatch('10', '100', 2, 1, 0, instant(6)),
    lifecycle('recovery_finished', '10', 2, instant(7), {booksReady: true}),
    marketBatch('10', '100', 2, 2, 1, instant(8)),
  ]);
  assert.equal(result.totals.staleRecoveryCompletions, 1);
  assert.equal(result.markets[0].staleRecoveryCompletions, 1);
  assert.equal(result.totals.notReadyBatches, 1);
  assert.deepEqual(result.totals.notReadyBatchesByCause, {'subscribed:RECONNECT': 1});
  assert.equal(result.totals.validRecoveryCompletions, 2);
  assert.equal(result.totals.acceptedBookPoints, 2);
});

test('any invalidation in the same observed millisecond invalidates every batch in that group', () => {
  const sameMs = '2026-10-13T02:55:10.500Z';
  const result = analyze([
    config('10', '100'),
    lifecycle('subscribed', '10', 1, instant(1)),
    lifecycle('recovery_finished', '10', 1, instant(2), {booksReady: true}),
    marketBatch('10', '100', 1, 1, 0, sameMs),
    lifecycle('invalidation', '10', 1, sameMs, {reason: 'GAP'}),
    marketBatch('10', '100', 1, 2, 1, sameMs),
  ]);
  assert.equal(result.totals.sameMillisecondInvalidationBatches, 1);
  assert.equal(result.totals.notReadyBatches, 1);
  assert.equal(result.totals.acceptedBookPoints, 0);
});

test('prefix-level aggregate reconciliation rejects any mismatch or per-market sum drift', () => {
  const result = analyze([
    config('10', '100'), lifecycle('subscribed', '10', 1, instant(1)),
    marketBatch('10', '100', 1, 1, 0, instant(2)),
    lifecycle('recovery_finished', '10', 1, instant(3), {booksReady: true}),
    marketBatch('10', '100', 1, 2, 1, instant(4)),
  ]);
  assert.equal(assertCaptureReadinessReconciliationV1(result, {
    notReadyBatches: 1, validRecoveryCompletions: 1, acceptedBookPoints: 1, lifecycleInvalidations: 1,
  }), true);
  assert.throws(() => assertCaptureReadinessReconciliationV1(result, {
    notReadyBatches: 2, validRecoveryCompletions: 1, acceptedBookPoints: 1, lifecycleInvalidations: 1,
  }), /RECONCILIATION_notReadyBatches/);
  const corrupted = structuredClone(result);
  corrupted.perMarketSummary.acceptedBookPointCountSum++;
  assert.throws(() => assertCaptureReadinessReconciliationV1(corrupted, {
    notReadyBatches: 1, validRecoveryCompletions: 1, acceptedBookPoints: 1, lifecycleInvalidations: 1,
  }), /RECONCILIATION_SUMS/);
});

test('lifecycle invalidation kind totals reconcile separately from parser readiness blockers', () => {
  const result = analyze([
    config('10', '100'),
    lifecycle('subscribed', '10', 1, instant(1), {reason: 'INITIAL'}),
    lifecycle('recovery_finished', '10', 1, instant(2), {booksReady: true}),
    marketBatch('10', '100', 1, 1, 0, instant(3)),
    marketBatch('10', '100', 1, 3, 2, instant(4)),
    lifecycle('recovery_finished', '10', 1, instant(5), {booksReady: false}),
    lifecycle('invalidation', '10', 1, instant(6), {reason: 'GAP'}),
    lifecycle('disconnected', null, null, instant(7)),
    lifecycle('recovery_started', '10', 1, instant(8)),
    marketBatch('10', '100', 1, 4, 3, instant(9)),
  ]);
  assert.deepEqual(result.totals.lifecycleInvalidationsByKind, {
    subscribed: 1, invalidation: 1, disconnected: 1, recovery_started: 1,
  });
  assert.equal(Object.values(result.totals.lifecycleInvalidationsByKind).reduce((sum, count) => sum + count, 0), 4);
  assert.equal(result.totals.lifecycleInvalidations, 4);
  assert.equal(result.totals.readinessBlockerApplicationsByCause.revision_gap, 1);
  assert.equal(result.totals.readinessBlockerApplicationsByCause['recovery_finished:false'], 1);
  assert.equal(result.totals.readinessBlockerApplicationsByCause['subscribed:INITIAL'], 1);
  assert.equal(assertCaptureReadinessReconciliationV1(result, {
    notReadyBatches: 1, validRecoveryCompletions: 1, acceptedBookPoints: 1, lifecycleInvalidations: 4,
  }), true);
});
