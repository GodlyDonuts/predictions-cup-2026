import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {analyzeCapture, bookFeatures, firstAtOrAfter, fitRidge, simulateLongOnly, targetDirtyHasFreshBook, validCaptureEnvelope} from './realtime-book-pressure-holdout-v1.mjs';

const book = ({bid = 0.4, ask = 0.5, bidQuantity = 3, askQuantity = 1, at = '2026-10-14T12:48:50.000Z'} = {}) => ({
  asOf: {sequence: 1, at},
  bids: [{price: bid, quantity: bidQuantity}, {price: bid - 0.01, quantity: 1}],
  asks: [{price: ask, quantity: askQuantity}, {price: ask + 0.01, quantity: 1}],
});

test('book pressure uses opposite-side queue weighting and bounded depth imbalance', () => {
  const row = bookFeatures(book(), Date.parse('2026-10-14T12:48:51.000Z'));
  assert.equal(row.mid, 0.45);
  assert.ok(Math.abs(row.spread - 0.1) < 1e-12);
  assert.ok(Math.abs(row.micropricePressure - 0.25) < 1e-12);
  assert.ok(Math.abs(row.topImbalance - 0.5) < 1e-12);
  assert.ok(Math.abs(row.depth5Imbalance - 1 / 3) < 1e-12);
});

test('crossed, stale, unversioned, empty, and zero-depth books fail closed', () => {
  const at = Date.parse('2026-10-14T12:48:51.000Z');
  assert.equal(bookFeatures(book({bid: 0.51, ask: 0.5}), at), null);
  assert.equal(bookFeatures(book({at: '2026-10-14T12:48:20.000Z'}), at), null);
  assert.equal(bookFeatures({...book(), asOf: null}, at), null);
  assert.equal(bookFeatures({...book(), asks: []}, at), null);
  assert.equal(bookFeatures(book({bidQuantity: 0}), at), null);
});

test('future observation lookup enforces latency, maximum delay, and continuity', () => {
  const rows = [{at: 1000, continuity: 1}, {at: 2000, continuity: 2}];
  assert.equal(firstAtOrAfter(rows, 1000, 0, 1), rows[0]);
  assert.equal(firstAtOrAfter(rows, 1500, 500, 2), rows[1]);
  assert.equal(firstAtOrAfter(rows, 1500, 499, 2), null);
  assert.equal(firstAtOrAfter(rows, 1000, 1000, 2), null);
});

test('capture reader accepts the actual versioned feed format and rejects corrupt envelopes', () => {
  assert.equal(validCaptureEnvelope({format: 'sig-feed-capture', schemaVersion: 1, captureIndex: 0, kind: 'config', observedAt: '2026-10-14T12:00:00.000Z'}), true);
  assert.equal(validCaptureEnvelope({format: 'sig-feed-replay', schemaVersion: 1, kind: 'config', observedAt: '2026-10-14T12:00:00.000Z'}), true);
  assert.equal(validCaptureEnvelope({format: 'other', schemaVersion: 1, kind: 'config', observedAt: '2026-10-14T12:00:00.000Z'}), false);
  assert.equal(validCaptureEnvelope({format: 'sig-feed-capture', schemaVersion: 1, kind: 'config', observedAt: 'not-a-time'}), false);
});

test('a target book in the same batch reconciles book-dirty at nanosecond precision', () => {
  const dirty = {exchangeId: '840', at: '2026-10-14T12:00:00.123456789Z'};
  const payload = {bookDirty: [dirty], books: [{exchangeId: '840', asOf: {at: '2026-10-14T12:00:00.123456790Z'}}]};
  assert.equal(targetDirtyHasFreshBook(payload, '840'), true);
  assert.equal(targetDirtyHasFreshBook({bookDirty: [dirty], books: [{exchangeId: '840', asOf: {at: dirty.at}}]}, '840'), true);
  assert.equal(targetDirtyHasFreshBook({bookDirty: [dirty], books: [{exchangeId: '840', asOf: {at: '2026-10-14T12:00:00.123456788Z'}}]}, '840'), false);
  assert.equal(targetDirtyHasFreshBook({bookDirty: [dirty], books: []}, '840'), false);
  assert.equal(targetDirtyHasFreshBook({bookDirty: [{...dirty, exchangeId: '841'}], books: []}, '840'), true);
  assert.equal(targetDirtyHasFreshBook({bookDirty: [{...dirty, at: 'invalid'}], books: [{exchangeId: '840', asOf: {at: dirty.at}}]}, '840'), false);
});

test('capture replay retains a coherent same-batch dirty-book replacement', async () => {
  const folder = mkdtempSync(join(tmpdir(), 'baal-book-pressure-'));
  const input = join(folder, 'capture.jsonl');
  const topic = 'tournament:123e4567-e89b-12d3-a456-426614174000:market:17';
  const row = (captureIndex, kind, observedAt, extra = {}) => ({
    format: 'sig-feed-capture', schemaVersion: 1, captureIndex, kind, observedAt, topic, epoch: 1, ...extra,
  });
  const captureBook = (sequence, at) => ({exchangeId: '840', asOf: {sequence, at}, nextExpiryAt: null,
    bids: [{price: 0.4, quantity: 3}, {price: 0.39, quantity: 1}],
    asks: [{price: 0.5, quantity: 2}, {price: 0.51, quantity: 1}]});
  const rows = [
    row(0, 'config', '2026-10-14T12:00:00.000Z', {scope: {marketId: '17', exchangeIds: ['840']}, staleMs: 20_000}),
    row(1, 'subscribed', '2026-10-14T12:00:01.000Z'),
    row(2, 'recovery_started', '2026-10-14T12:00:02.000Z'),
    row(3, 'snapshot', '2026-10-14T12:00:03.000Z', {receivedAt: '2026-10-14T12:00:03.000Z', books: [captureBook(1, '2026-10-14T12:00:03.000Z')]}),
    row(4, 'recovery_finished', '2026-10-14T12:00:04.000Z', {booksReady: true}),
    row(5, 'event', '2026-10-14T12:00:05.000Z', {receivedAt: '2026-10-14T12:00:05.000Z', event: 'market_batch', payload: {
      delivery: {revision: 2, previousRevision: 1}, resyncRequired: false,
      books: [captureBook(2, '2026-10-14T12:00:04.500000001Z')],
      bookDirty: [{exchangeId: '840', marketId: '17', at: '2026-10-14T12:00:04.500000000Z'}], marketSettled: [],
    }}),
  ];
  try {
    writeFileSync(input, `${rows.map(item => JSON.stringify(item)).join('\n')}\n`, {flag: 'wx'});
    const result = await analyzeCapture(input);
    assert.equal(result.source.acceptedBooks, 2);
    assert.equal(result.source.targetDirtyBatchesSeen, 1);
    assert.equal(result.source.targetDirtyBatchesReconciled, 1);
    assert.equal(result.source.targetDirtyBatchesUnreconciled, 0);
    assert.equal(result.source.rejectedEvents, 0);
  } finally {
    rmSync(folder, {recursive: true, force: true});
  }
});

test('ridge fit learns a fixed linear markout without using test rows', () => {
  const train = Array.from({length: 160}, (_, i) => {
    const a = Math.sin(i * 0.17), b = Math.cos(i * 0.11), c = Math.sin(i * 0.07 + 0.3);
    const d = Math.cos(i * 0.13 + 0.8), e = Math.sin(i * 0.037 + 1.2), f = Math.cos(i * 0.019 + 0.4);
    const row = {micropricePressure: a, topImbalance: b, depth5Imbalance: c,
      momentum5s: d, momentum30s: e, midOffset: f};
    const y = 0.004 * a - 0.003 * b + 0.002 * c + 0.001 * d - 0.0015 * e + 0.0008 * f;
    row.labels = {30000: {midChange: y}};
    return row;
  });
  const model = fitRidge(train, 1e-6);
  const errors = train.map(row => Math.abs(model.predict(row) - row.labels[30000].midChange));
  assert(Math.max(...errors) < 0.0001);
});

test('one-share taker proxy observes latency, requires visible top depth, and prevents overlap', () => {
  const rows = [0, 1000, 5000, 30000, 35000, 60000].map((at, i) => ({
    at, continuity: 7, bidPrice: i < 3 ? 0.49 : 0.60, askPrice: i < 3 ? 0.50 : 0.61,
    bidQuantity: 10, askQuantity: 10,
  }));
  const signal = {marketId: '17', at: 0, continuity: 7, spread: 0.01};
  const result = simulateLongOnly([signal, {...signal, at: 5000}], new Map([['17', rows]]), {predict: () => 0.02});
  assert.equal(result.trades, 1);
  assert.ok(Math.abs(result.totalNetPnl - 0.1) < 1e-12);
  assert.equal(result.meanEntryDelayMs, 1000);
  assert.equal(result.actualFills, false);
});
