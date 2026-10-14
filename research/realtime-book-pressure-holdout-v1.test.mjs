import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {analyzeBookOnlyDevelopmentPrefix, analyzeCapture, bookFeatures, evaluateBookOnlyDevelopment,
  firstAtOrAfter, fitRidge, simulateLongOnly, targetDirtyHasFreshBook, trainingOnlyFoldBounds,
  validCaptureEnvelope} from './realtime-book-pressure-holdout-v1.mjs';

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

test('four-feature five-second ridge uses only chronological purged development folds', () => {
  const features = ['micropricePressure', 'topImbalance', 'depth5Imbalance', 'midOffset'];
  const samples = Array.from({length: 1_000}, (_, i) => {
    const at = i * 1_000;
    const values = [Math.sin(i * 0.13), Math.cos(i * 0.07), Math.sin(i * 0.031 + 0.2), Math.cos(i * 0.017)];
    const row = {at, marketId: String(i % 4), continuity: 1, spread: 0.01,
      ...Object.fromEntries(features.map((name, index) => [name, values[index]]))};
    row.labels = {5000: {midChange: 0.004 * values[0] - 0.003 * values[1] + 0.002 * values[2] + 0.001 * values[3],
      executableLongPnl: 0, futureAt: at + 5_000, effectiveHorizonMs: 5_000}};
    return row;
  });
  const model = fitRidge(samples.slice(0, 100), 0.1, {features, horizonMs: 5_000, minRows: 30});
  assert.equal(model.horizonMs, 5_000);
  assert.deepEqual(model.features, features);
  const folds = trainingOnlyFoldBounds(0, 1_000_000);
  assert.deepEqual(folds.map(fold => [fold.startAt, fold.endAt]), [[400_000, 500_000], [500_000, 600_000], [600_000, 700_000]]);
  const result = evaluateBookOnlyDevelopment(samples, new Map(), {firstAt: 0, lastAt: 1_000_000});
  assert.equal(result.length, 3);
  for (const fold of result) {
    assert.equal(fold.status, 'scored');
    assert.ok(Date.parse(fold.latestTrainingLabelAt) < Date.parse(fold.startAt) - fold.purgeMs);
    assert.ok(Date.parse(fold.latestValidationLabelAt) < Date.parse(fold.endAt) - fold.purgeMs);
    assert.ok(Number.isFinite(fold.validationScore.mse));
    assert.equal(fold.decisionDiagnostics.validationDecisions, fold.validationRows);
    assert.equal(fold.decisionDiagnostics.predictionsAboveFullSpread, fold.modelGatedCounterfactual.eligibleSignals);
  }
});

test('training-only prefix stops before semantic processing of its one boundary lookahead', async () => {
  const folder = mkdtempSync(join(tmpdir(), 'baal-book-only-prefix-'));
  const input = join(folder, 'capture.jsonl');
  const topic = 'tournament:123e4567-e89b-12d3-a456-426614174000:market:17';
  const baseAt = Date.parse('2026-10-14T12:00:00.000Z');
  const iso = ms => new Date(baseAt + ms).toISOString();
  let index = 0;
  const row = (kind, offset, extra = {}) => ({format: 'sig-feed-capture', schemaVersion: 1,
    captureIndex: index++, kind, observedAt: iso(offset), topic, epoch: 1, ...extra});
  const captureBook = (sequence, offset) => ({exchangeId: '840', asOf: {sequence, at: iso(offset)}, nextExpiryAt: null,
    bids: [{price: 0.4, quantity: 3}, {price: 0.39, quantity: 1}],
    asks: [{price: 0.5, quantity: 2}, {price: 0.51, quantity: 1}]});
  const rows = [
    row('config', 0, {scope: {marketId: '17', exchangeIds: ['840']}, staleMs: 20_000}),
    row('subscribed', 1), row('recovery_started', 2),
    row('snapshot', 3, {receivedAt: iso(3), books: [captureBook(1, 3)]}),
    row('recovery_finished', 4, {booksReady: true}),
  ];
  let revision = 1, sequence = 1;
  for (let offset = 5_000; offset <= 70_000; offset += 5_000) {
    rows.push(row('event', offset, {receivedAt: iso(offset), event: 'market_batch', payload: {
      delivery: {revision: ++revision, previousRevision: revision - 1}, resyncRequired: false,
      books: [captureBook(++sequence, offset)], bookDirty: [], marketSettled: [],
    }}));
  }
  rows.push(row('config', 75_000, {scope: {marketId: '17', exchangeIds: ['840']}, staleMs: 20_000}));
  for (let offset = 80_000; offset <= 100_000; offset += 5_000) {
    rows.push(row('event', offset, {receivedAt: iso(offset), event: 'market_batch', payload: {
      delivery: {revision: ++revision, previousRevision: revision - 1}, resyncRequired: false,
      books: [captureBook(++sequence, offset)], bookDirty: [], marketSettled: [],
    }}));
  }
  try {
    const content = `${rows.map(item => JSON.stringify(item)).join('\n')}\n`;
    writeFileSync(input, content, {flag: 'wx'});
    const reference = {report: 'realtime-book-pressure-holdout-20261014', source: {
      basename: 'capture.jsonl', bytes: Buffer.byteLength(content), sha256: createHash('sha256').update(content).digest('hex'),
      firstValidBookAt: iso(0), lastValidBookAt: iso(100_000),
    }};
    const result = await analyzeBookOnlyDevelopmentPrefix(input, reference);
    assert.equal(result.source.configuredMarkets, 1);
    assert.equal(result.source.boundaryLookahead.kind, 'config');
    assert.equal(result.source.boundaryLookahead.observedAt, iso(75_000));
    assert.equal(result.source.processedThrough, iso(70_000));
    assert.equal(result.actualFills, false);
    assert.equal(result.profitabilityEstablished, false);
    await assert.rejects(analyzeBookOnlyDevelopmentPrefix(input, {...reference,
      source: {...reference.source, sha256: 'a'.repeat(64)}}), /CAPTURE_SOURCE_HASH_MISMATCH/);
  } finally {
    rmSync(folder, {recursive: true, force: true});
  }
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
