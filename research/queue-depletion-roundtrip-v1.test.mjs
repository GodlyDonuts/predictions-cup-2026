import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { createQueueDepletionRoundtripV1, hashByteRange, verifyByteRange } from './queue-depletion-roundtrip-v1.mjs';
import { fileSizeAndCompleteBound, forEachCompleteLine, hasUnresolvedBookForTape, lastCompleteRecordAt, runQueueDepletionCapture } from '../data/run-queue-depletion-roundtrip-20261013.mjs';

const base = Date.parse('2026-10-13T15:00:00.000Z');
const at = ms => new Date(base + ms).toISOString();
const book = (ms, { marketId = 'M', exchangeId = 'E', sequence = ms + 1, bid = '0.500000', ask = '0.510000', bidQuantity = 10, askQuantity = 10 } = {}) => ({
  marketId, exchangeId, sequence: String(sequence), sourceAt: at(ms), receivedAt: at(ms),
  bids: [{ price: bid, quantity: bidQuantity }], asks: [{ price: ask, quantity: askQuantity }],
});
const trade = (sequence, ms, price, quantity, { marketId = 'M', exchangeId = 'E' } = {}) => ({
  id: String(sequence), sequence, marketId, exchangeId, price, quantity, executedAt: at(ms),
});
const replay = (members = [{ marketId: 'M', exchangeId: 'E' }]) => createQueueDepletionRoundtripV1({ members, startAt: at(0) });
const observe = (engine, ms, books = [], trades = []) => engine.observeBatch({ observedAt: at(ms), books, trades });
const seed = (engine, books = [book(0)]) => observe(engine, 0, books);

test('activation is delayed, queue must be strictly depleted, and completed roundtrip pays two adverse ticks', () => {
  const engine = replay();
  seed(engine);
  observe(engine, 2999, [], [trade(1, 2999, '0.500000', 100)]);
  observe(engine, 3000, [book(3000, { sequence: 2 })], [trade(2, 3000, '0.500000', 10)]);
  observe(engine, 3001, [book(3001, { sequence: 3 })], [trade(3, 3001, '0.500000', 1)]);
  observe(engine, 6002, [book(6002, { sequence: 4 })], [trade(4, 6002, '0.510000', 1)]);
  const result = engine.report({ throughAt: at(7000) });
  assert.equal(result.counts.uniqueScopedTrades, 4);
  assert.equal(result.counts.entryFillProxies, 1);
  assert.equal(result.counts.entryQueueVolumeObserved, 11);
  assert.equal(result.counts.completedExitProxies, 1);
  assert.equal(result.completions[0].grossPnl, '0.005000');
  assert.equal(result.completions[0].stressOneTickPerFillPnl, '-0.005000');
  assert.equal(result.aggregate.allFees, '0.000000');
  assert.equal(result.verdict, 'insufficient_sample_diagnostic_only');
});

test('displayed-size reductions do not reduce queue ahead or create fills', () => {
  const engine = replay();
  seed(engine, [book(0, { bidQuantity: 10, askQuantity: 10 })]);
  observe(engine, 3000, [book(3000, { sequence: 2, bidQuantity: 1, askQuantity: 1 })], [trade(1, 3000, '0.500000', 10)]);
  const result = engine.report({ throughAt: at(5000) });
  assert.equal(result.counts.entryFillProxies, 0);
  assert.equal(result.counts.entryQueuePrintEvents, 1);
  assert.equal(result.counts.entryQueueVolumeObserved, 10);
});

test('inside-spread prints are excluded as ambiguous', () => {
  const engine = replay();
  seed(engine);
  observe(engine, 3001, [], [trade(1, 3001, '0.505000', 100)]);
  const result = engine.report({ throughAt: at(4000) });
  assert.equal(result.counts.ambiguousPrints, 1);
  assert.equal(result.counts.entryQueuePrintEvents, 0);
  assert.equal(result.counts.entryFillProxies, 0);
});

test('a book sourced after a print cannot classify that earlier print', () => {
  const engine = replay();
  seed(engine, [book(0, { bidQuantity: 100, askQuantity: 100 })]);
  observe(engine, 3100, [book(3100, { sequence: 2, bid: '0.600000', ask: '0.610000', bidQuantity: 100, askQuantity: 100 })], [trade(1, 3050, '0.550000', 1)]);
  const result = engine.report({ throughAt: at(4000) });
  assert.equal(result.counts.inferredBuyAggressorPrints, 1);
  assert.equal(result.counts.ambiguousPrints, 0);
  assert.equal(result.counts.entryFillProxies, 0);
});

test('print depletion is isolated by market and exchange', () => {
  const engine = replay([{ marketId: 'M1', exchangeId: 'E1' }, { marketId: 'M2', exchangeId: 'E2' }]);
  seed(engine, [
    book(0, { marketId: 'M1', exchangeId: 'E1', bidQuantity: 1, askQuantity: 1 }),
    book(0, { marketId: 'M2', exchangeId: 'E2', sequence: 2, bid: '0.800000', ask: '0.810000', bidQuantity: 1, askQuantity: 1 }),
  ]);
  observe(engine, 3001, [], [trade(1, 3001, '0.500000', 2, { marketId: 'M2', exchangeId: 'E2' })]);
  observe(engine, 3002, [], [trade(2, 3002, '0.500000', 1, { marketId: 'M1', exchangeId: 'E1' })]);
  const result = engine.report({ throughAt: at(4000) });
  assert.equal(result.counts.entryFillProxies, 1);
  assert.equal(result.perMarket.find(row => row.marketId === 'M1').entryFillProxies, 0);
  assert.equal(result.perMarket.find(row => row.marketId === 'M2').entryFillProxies, 1);
});

test('capture boundary cancels resting entry and exit quotes and leaves fills censored', () => {
  const entryOnly = replay();
  seed(entryOnly);
  entryOnly.boundary();
  const canceled = entryOnly.report({ throughAt: at(5000) });
  assert.equal(canceled.counts.boundaryCanceledEntryPairs, 1);
  assert.equal(canceled.counts.entryFillProxies, 0);

  const open = replay();
  seed(open, [book(0, { bidQuantity: 1, askQuantity: 1 })]);
  observe(open, 3001, [], [trade(1, 3001, '0.500000', 2)]);
  open.boundary();
  const result = open.report({ throughAt: at(4000) });
  assert.equal(result.counts.boundaryCanceledExitQuotes, 1);
  assert.equal(result.counts.unresolvedInventory, 1);
  assert.equal(result.counts.unvaluedUnresolvedInventory, 1);
  assert.equal(result.aggregate.openInventoryExecutableMarkPnl, '0.000000');
});

test('unresolved inventory is valued only from a later executable-side book and gets one adverse tick', () => {
  const engine = replay();
  seed(engine, [book(0, { bidQuantity: 1, askQuantity: 1 })]);
  observe(engine, 3001, [], [trade(1, 3001, '0.500000', 2)]);
  observe(engine, 4000, [book(4000, { sequence: 2, bid: '0.495000', ask: '0.500000', bidQuantity: 3, askQuantity: 3 })]);
  engine.boundary({ preserveInventoryMarks: true });
  const result = engine.report({ throughAt: at(5000) });
  assert.equal(result.counts.valuedUnresolvedInventory, 1);
  assert.equal(result.aggregate.openInventoryExecutableMarkPnl, '-0.005000');
  assert.equal(result.aggregate.openInventoryStressPerFillPnl, '-0.010000');
  assert.equal(result.counts.unresolvedInventory, 1);
});

test('pending entry quotes are censored at the observation boundary', () => {
  const engine = replay();
  seed(engine);
  const result = engine.report({ throughAt: at(5000) });
  assert.equal(result.counts.censoredEntryPairs, 1);
  assert.equal(result.counts.expiredEntryPairs, 0);
});

test('capture finalization reports expired quotes without a synthetic terminal cancellation', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'queue-roundtrip-finalize-'));
  const file = path.join(directory, 'capture.jsonl');
  const output = path.join(directory, 'report.json');
  const tournamentId = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
  const marketId = 'M1', exchangeId = 'E1';
  const topic = `tournament:${tournamentId}:market:${marketId}`;
  const cutoff = '2026-10-13T14:08:44.314Z';
  const through = '2026-10-13T14:28:58.395Z';
  const config = { kind: 'config', topic, scope: { tournamentId, marketId, exchangeIds: [exchangeId], topic } };
  const batch = (observedAt, books = []) => ({
    kind: 'event', event: 'market_batch', topic, observedAt, receivedAt: observedAt,
    payload: { books, trades: [] },
  });
  const firstBook = {
    exchangeId, asOf: { at: cutoff, sequence: 1 },
    bids: [{ price: '0.500000', quantity: 2 }], asks: [{ price: '0.510000', quantity: 2 }],
  };
  try {
    fs.writeFileSync(file, `${JSON.stringify(config)}\n${JSON.stringify(batch(cutoff, [firstBook]))}\n${JSON.stringify(batch(through))}\n`, 'utf8');
    const result = runQueueDepletionCapture({ file, output });
    assert.equal(result.counts.quotePairs, 1);
    assert.equal(result.counts.expiredEntryPairs, 1);
    assert.equal(result.counts.censoredEntryPairs, 0);
    assert.equal(result.counts.boundaryCanceledEntryPairs, 0);
    assert.equal(result.source.captureFileName, 'capture.jsonl');
    assert.equal('file' in result.source, false);
    assert.equal(fs.readFileSync(output, 'utf8').includes(directory), false);

    const originalOutput = fs.readFileSync(output);
    assert.throws(() => runQueueDepletionCapture({ file, output }), /QUEUE_OUTPUT_EXISTS/);
    assert.deepEqual(fs.readFileSync(output), originalOutput);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('byte-range hash is stable across append and detects changed frozen bytes', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'queue-depletion-'));
  const file = path.join(dir, 'capture.jsonl');
  try {
    fs.writeFileSync(file, 'header\nrecord\n', 'utf8');
    const frozen = hashByteRange(file, 7, 14);
    fs.appendFileSync(file, 'later\n', 'utf8');
    assert.equal(verifyByteRange(file, frozen).sha256, frozen.sha256);
    const fd = fs.openSync(file, 'r+');
    try { fs.writeSync(fd, Buffer.from('X'), 0, 1, 8); }
    finally { fs.closeSync(fd); }
    assert.throws(() => verifyByteRange(file, frozen), /QUEUE_PREFIX_HASH/);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('stream framing stops at the last complete line and reports exact byte offsets', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'queue-lines-'));
  const file = path.join(dir, 'capture.jsonl');
  try {
    fs.writeFileSync(file, 'first\nsecond\npartial', 'utf8');
    const bound = fileSizeAndCompleteBound(file);
    const rows = [];
    forEachCompleteLine(file, 0, bound.endByte, (bytes, startByte, endByte) => rows.push({ text: bytes.toString('utf8'), startByte, endByte }));
    assert.deepEqual(rows, [
      { text: 'first', startByte: 0, endByte: 6 },
      { text: 'second', startByte: 6, endByte: 13 },
    ]);
    assert.equal(bound.endByte, 13);
    fs.writeFileSync(file, `{"observedAt":"${at(1)}"}\n{"observedAt":"${at(2)}"}\n{"observedAt":"partial"`, 'utf8');
    const jsonBound = fileSizeAndCompleteBound(file);
    assert.equal(lastCompleteRecordAt(file, jsonBound.endByte), at(2));
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('a dirty hint paired with a full same-batch book is resolved; missing named books fail closed', () => {
  assert.equal(hasUnresolvedBookForTape({ books: [{ exchangeId: 'E' }], bookDirty: [{ exchangeId: 'E' }], trades: [{ exchangeId: 'E' }] }), false);
  assert.equal(hasUnresolvedBookForTape({ books: [{ exchangeId: 'E' }], bookDirty: [{ exchangeId: 'OTHER' }], trades: [] }), true);
  assert.equal(hasUnresolvedBookForTape({ books: [], bookDirty: [], trades: [{ exchangeId: 'E' }] }), true);
});
