import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { createConservativeTouchQueueFillV1 } from './conservative-touch-queue-fill-v1.mjs';
import { compactPersistedReport, hashFrozenPrefix, writeCompactReport } from '../data/run-conservative-touch-queue-fill-20261013.mjs';

const member = Object.freeze({
  tournamentId: 'tournament-test', marketId: 'market-test', exchangeId: 'exchange-test',
  topic: 'tournament:tournament-test:market:market-test',
});
const t0 = Date.parse('2026-10-05T12:00:00.000Z');
const iso = offset => new Date(t0 + offset).toISOString();

function book(offset, { sequence = 1, bid = '0.400', ask = '0.410', bidQty = 10, askQty = 12 } = {}) {
  return {
    tournamentId: member.tournamentId, marketId: member.marketId, exchangeId: member.exchangeId,
    sequence: String(sequence), sourceAt: iso(offset), receivedAt: iso(offset),
    bids: [{ price: bid, quantity: bidQty }], asks: [{ price: ask, quantity: askQty }],
  };
}

function newAudit() { return createConservativeTouchQueueFillV1({ members: [member] }); }

test('queue-ahead adds observed same-price increases and never treats decreases or prints as queue depletion', () => {
  const audit = newAudit();
  audit.observeBook(book(0), { observedAt: iso(0) });
  const candidate = audit.candidates.find(row => row.action === 'buy');
  assert.equal(candidate.initialQueueShares, 10);
  audit.observeBook(book(1000, { sequence: 2, bidQty: 14 }), { observedAt: iso(1000) });
  audit.observeBook(book(2000, { sequence: 3, bidQty: 7 }), { observedAt: iso(2000) });
  audit.observeBook(book(2500, { sequence: 4, bidQty: 9 }), { observedAt: iso(2500) });
  assert.equal(candidate.queueIncreaseShares, 6);
  assert.equal(candidate.queueAheadShares, 16);
  assert.equal(candidate.displayReductionsIgnored, 1);

  audit.advance(iso(3000), member.topic);
  assert.equal(candidate.status, 'active');
  assert.equal(candidate.activated, true);
  assert.equal(audit.observeTrade({
    tournamentId: member.tournamentId, marketId: member.marketId, exchangeId: member.exchangeId,
    id: 'stable-trade-id-is-not-the-sequence', sequence: 55,
    price: '0.400', quantity: 1000, executedAt: iso(3100),
  }, { receivedAt: iso(3200), observedAt: iso(3300) }), true);
  assert.equal(candidate.touchPriceReportsUnknownSide, 1);
  assert.equal(candidate.touchPriceQuantityUnknownSide, 1000);
  assert.equal(candidate.queueAheadShares, 16);
  assert.equal(audit.report().counts.invalidTradeReports, 0);
  assert.equal(audit.report().counts.confirmedQueueFills, 0);
});

test('three-second activation is causal; execution before activation does not count even if receipt is later', () => {
  const audit = newAudit();
  audit.observeBook(book(0), { observedAt: iso(0) });
  const candidate = audit.candidates.find(row => row.action === 'buy');
  audit.observeTrade({
    tournamentId: member.tournamentId, marketId: member.marketId, exchangeId: member.exchangeId,
    id: 'pre-activation', sequence: 2, price: '0.400', quantity: 1, executedAt: iso(2999),
  }, { receivedAt: iso(3100), observedAt: iso(3200) });
  assert.equal(candidate.touchPriceReportsUnknownSide, 0);
  audit.advance(iso(3000), member.topic);
  audit.observeTrade({
    tournamentId: member.tournamentId, marketId: member.marketId, exchangeId: member.exchangeId,
    id: 'post-activation', sequence: 3, price: '0.400', quantity: 1, executedAt: iso(3001),
  }, { receivedAt: iso(3100), observedAt: iso(3200) });
  assert.equal(candidate.touchPriceReportsUnknownSide, 1);
});

test('stale, price-moved, gap, and capture-boundary quotes are censored', () => {
  const stale = newAudit();
  stale.observeBook(book(0), { observedAt: iso(0) });
  stale.advance(iso(3001), member.topic);
  assert.equal(stale.candidates[0].status, 'censored_stale');

  const moved = newAudit();
  moved.observeBook(book(0), { observedAt: iso(0) });
  moved.observeBook(book(1000, { sequence: 2, bid: '0.405' }), { observedAt: iso(1000) });
  assert.equal(moved.candidates.every(row => row.status === 'censored_price_move'), true);

  const gap = newAudit();
  gap.observeBook(book(0), { observedAt: iso(0) });
  gap.boundary(member.topic, 'gap', iso(1000));
  assert.equal(gap.candidates.every(row => row.status === 'censored_gap'), true);

  const boundary = newAudit();
  boundary.observeBook(book(0), { observedAt: iso(0) });
  boundary.endCapture(iso(1000));
  assert.equal(boundary.candidates.every(row => row.status === 'censored_capture_boundary'), true);
});

test('a topic revision gap censors prior quotes before accepting the next current book', () => {
  const audit = newAudit();
  audit.observeBatch({
    topic: member.topic, observedAt: iso(1), receivedAt: iso(0),
    books: [book(0)], revision: 1, previousRevision: 0, resyncRequired: false,
  });
  const original = audit.candidates.slice();
  audit.observeBatch({
    topic: member.topic, observedAt: iso(1001), receivedAt: iso(1000),
    books: [book(1000, { sequence: 2 })], revision: 3, previousRevision: 2, resyncRequired: false,
  });
  assert.equal(original.every(row => row.status === 'censored_gap'), true);
  assert.equal(audit.report().counts.gapCensoredQuotes, 2);
  assert.equal(audit.candidates.length, 4);
});

test('expiry follows activation plus TTL when fresh books keep the hypothetical quote observable', () => {
  const audit = newAudit();
  audit.observeBook(book(0), { observedAt: iso(0) });
  audit.advance(iso(3000), member.topic);
  audit.observeBook(book(12_000, { sequence: 2 }), { observedAt: iso(12_000) });
  audit.advance(iso(13_000), member.topic);
  assert.equal(audit.candidates.every(row => row.status === 'expired'), true);
});

test('book-dirty censors the old quote but a valid same-packet versioned book can seed a fresh one', () => {
  const audit = newAudit();
  const makeRecord = (offset, revision, previousRevision) => ({
    format: 'sig-feed-capture', schemaVersion: 1, captureIndex: revision, epoch: 1,
    kind: 'event', event: 'market_batch', topic: member.topic,
    observedAt: iso(offset + 1), receivedAt: iso(offset),
    payload: {
      resyncRequired: false,
      delivery: { revision, previousRevision },
      bookDirty: [{ exchangeId: member.exchangeId }],
      marketSettled: [], trades: [],
    },
  });
  const first = makeRecord(0, 1, 0);
  first.payload.books = [book(0)];
  audit.observeRecord(first, [book(0)]);
  assert.equal(audit.candidates.length, 2);

  const second = makeRecord(1000, 2, 1);
  const refreshed = book(1000, { sequence: 2 });
  second.payload.books = [refreshed];
  audit.observeRecord(second, [refreshed]);
  assert.equal(audit.candidates.slice(0, 2).every(row => row.status === 'censored_invalidation'), true);
  assert.equal(audit.report().counts.acceptedBookPoints, 2);
  assert.equal(audit.candidates.length, 2);
});

test('all folds, actions, horizons and prespecified bins remain visible with zero confirmed outcomes', () => {
  const audit = newAudit();
  audit.setSourceDay('2026-10-05');
  audit.observeBook(book(0), { observedAt: iso(0) });
  audit.endCapture(iso(1000));
  audit.setFold('untouched_held_out');
  audit.setSourceDay('2026-10-13');
  audit.observeBook(book(40_000, { sequence: 2 }), { observedAt: iso(40_000) });
  audit.endCapture(iso(41_000));
  const report = audit.report();
  assert.equal(report.status, 'indeterminate_no_confirmed_queue_fills');
  assert.equal(report.outcomeTables.length, 12);
  assert.equal(report.prespecifiedCells.length, 96);
  assert.equal(report.prespecifiedCells.every(cell => cell.confirmedQueueFills === 0 && cell.markoutSamples === 0), true);
  assert.equal(report.counts.confirmedQueueFills, 0);
  assert.equal(report.actualFillsProven, false);
  const snapshots = audit.queueAheadSnapshotRows();
  assert.equal(snapshots.rows.length, report.counts.candidateQuotes);
  assert.equal(snapshots.rows.every(row => typeof row[snapshots.columns.indexOf('decisionReceiptAt')] === 'string'), true);
  assert.deepEqual([...new Set(snapshots.rows.map(row => row[snapshots.columns.indexOf('sourceDay')]))], ['2026-10-05', '2026-10-13']);
});

test('market-cluster counts use actual candidate market identities, not missing top-level fields', () => {
  const other = {
    tournamentId: member.tournamentId, marketId: 'market-other', exchangeId: 'exchange-other',
    topic: 'tournament:tournament-test:market:market-other',
  };
  const audit = createConservativeTouchQueueFillV1({ members: [member, other] });
  audit.observeBook(book(0), { observedAt: iso(0) });
  const otherBook = book(0);
  otherBook.marketId = other.marketId;
  otherBook.exchangeId = other.exchangeId;
  audit.observeBook(otherBook, { observedAt: iso(0) });
  const cell = audit.report().outcomeTables.find(row => row.fold === 'development' && row.action === 'buy' && row.horizonMs === 1000);
  assert.equal(cell.marketClusterUncertainty.quoteMarketClusters, 2);
  assert.equal(cell.marketClusterUncertainty.effectiveIndependentQuoteMarkets, 2);
  assert.equal(cell.marketClusterUncertainty.fillOutcomeMarketClusters, 0);
});

test('frozen-prefix verifier hashes exact complete lines and rejects a partial final row', t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'queue-fill-prefix-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const file = path.join(directory, 'capture.jsonl');
  const prefix = Buffer.from('{"captureIndex":0}\n{"captureIndex":1}\n');
  fs.writeFileSync(file, prefix);
  const expected = createHash('sha256').update(prefix).digest('hex');
  assert.deepEqual(hashFrozenPrefix(file, prefix.length), { bytes: prefix.length, lines: 2, sha256: expected });
  assert.throws(() => hashFrozenPrefix(file, prefix.length - 1), /MULTIDAY_INCOMPLETE_PREFIX|MULTIDAY_PREFIX_RANGE/);
});

test('persisted queue report is compact while retaining all 96 cells and exact replay summaries', t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'queue-fill-compact-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const output = path.join(directory, 'report.json');
  const columns = [
    'sourceDay', 'fold', 'marketId', 'exchangeId', 'action', 'decisionReceiptAt', 'activationAt',
    'lastBookReceivedAt', 'status', 'endAt', 'spreadTicks', 'initialQueueAheadShares',
    'observedQueueIncreaseSharesAdded', 'observedDisplayReductionsIgnored', 'finalQueueAheadShares',
    'touchPriceTradeReportsUnknownSide', 'touchPriceTradeQuantityUnknownSide',
  ];
  const row = ({ day, fold, market, exchange, action, status, queue }) => [
    day, fold, market, exchange, action, iso(0), iso(3000), iso(0), status, iso(10000), 1,
    queue, 2, 3, queue + 2, 0, 0,
  ];
  const rows = [
    row({ day: '2026-10-05', fold: 'development', market: 'market-a', exchange: 'exchange-a', action: 'buy', status: 'censored_stale', queue: 10 }),
    row({ day: '2026-10-06', fold: 'development', market: 'market-a', exchange: 'exchange-a', action: 'sell', status: 'censored_stale', queue: 20 }),
    row({ day: '2026-10-13', fold: 'untouched_held_out', market: 'market-b', exchange: 'exchange-b', action: 'buy', status: 'censored_invalidation', queue: 30 }),
  ];
  const cells = Array.from({ length: 96 }, (_, index) => ({ cell: index, confirmedQueueFills: 0, markoutSamples: 0 }));
  const sequenceDiagnostic = { tradeReports: 123, exactBookTradeBookAdjacency: 0, interpretation: 'sequence never establishes a fill' };
  const sourceHashes = ['1'.repeat(64), '2'.repeat(64), '3'.repeat(64)];
  const report = {
    schemaVersion: 1,
    status: 'indeterminate_no_confirmed_queue_fills',
    split: { developmentThrough: iso(1), untouchedHeldOutFrom: iso(40_000) },
    sources: sourceHashes.map((sha256, index) => ({ day: `2026-10-${['05', '06', '13'][index]}`, sha256 })),
    sequenceDiagnostic,
    outcomes: {
      counts: { candidateQuotes: rows.length, confirmedQueueFills: 0, markoutSamples: 0 },
      outcomeTables: [{ fold: 'development', confirmedQueueFills: 0 }],
      prespecifiedCells: cells,
      queueAheadSnapshots: { columns, rows },
      actualFillsProven: false,
      profitabilityEstablished: false,
      limitations: ['no fills inferred'],
    },
    interpretation: ['no-fill interpretation is retained'],
  };
  const originalRows = structuredClone(report.outcomes.queueAheadSnapshots.rows);

  const compact = compactPersistedReport(report);
  const written = writeCompactReport(report, output);
  const persisted = JSON.parse(fs.readFileSync(output, 'utf8'));

  assert.equal(written.bytes, fs.statSync(output).size);
  assert.ok(written.bytes < 100 * 1024 * 1024);
  assert.equal(persisted.schemaVersion, 2);
  assert.deepEqual(persisted.sources, report.sources);
  assert.deepEqual(persisted.sequenceDiagnostic, sequenceDiagnostic);
  assert.equal(Object.hasOwn(persisted.sequenceDiagnostic, 'supplementalNotInFrozenSourceMetrics'), false);
  assert.deepEqual(persisted.split, report.split);
  assert.deepEqual(persisted.outcomes.prespecifiedCells, cells);
  assert.equal(persisted.outcomes.prespecifiedCells.length, 96);
  assert.deepEqual(persisted.outcomes.counts, report.outcomes.counts);
  assert.deepEqual(persisted.interpretation, report.interpretation);
  assert.equal(persisted.outcomes.actualFillsProven, false);
  assert.equal(persisted.outcomes.profitabilityEstablished, false);
  assert.equal(persisted.outcomes.queueAheadSnapshots.rowCount, rows.length);
  assert.equal(persisted.outcomes.queueAheadSnapshots.totals.metrics.initialQueueAheadShares.sum, 60);
  assert.equal(persisted.outcomes.queueAheadSnapshots.totals.metrics.initialQueueAheadShares.min, 10);
  assert.equal(persisted.outcomes.queueAheadSnapshots.totals.metrics.initialQueueAheadShares.max, 30);
  assert.deepEqual(persisted.outcomes.queueAheadSnapshots.groupings.byStatus.map(group => [group.status, group.rowCount]), [
    ['censored_invalidation', 1], ['censored_stale', 2],
  ]);
  assert.equal(persisted.outcomes.queueAheadSnapshots.groupings.bySourceDayFoldActionStatus.length, 3);
  assert.equal(persisted.outcomes.queueAheadSnapshots.groupings.bySourceDayFoldActionStatus.find(group => group.sourceDay === '2026-10-13').uniqueMarketCount, 1);
  assert.equal(Object.hasOwn(persisted.outcomes.queueAheadSnapshots, 'rows'), false);
  assert.deepEqual(report.outcomes.queueAheadSnapshots.rows, originalRows);
  assert.deepEqual(compact.outcomes.queueAheadSnapshots, persisted.outcomes.queueAheadSnapshots);
});

