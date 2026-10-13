import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { createQueueDepletionSourceAgeV1, hashByteRange, verifyByteRange } from './queue-depletion-source-age-v1.mjs';

const base = Date.parse('2026-10-13T15:00:00.000Z');
const at = ms => new Date(base + ms).toISOString();
const book = (sourceAgeMs, overrides = {}) => ({
  marketId: 'M', exchangeId: 'E', sequence: '1',
  sourceAt: overrides.sourceAt ?? at(-sourceAgeMs),
  receivedAt: overrides.receivedAt ?? at(0),
  bids: [{ price: '0.500000', quantity: 10 }],
  asks: [{ price: '0.510000', quantity: 10 }],
});
const replay = () => createQueueDepletionSourceAgeV1({
  members: [{ marketId: 'M', exchangeId: 'E' }], startAt: at(0), maxDecisionSourceAgeMs: 10_000,
});
const observe = (engine, books) => engine.observeBatch({ observedAt: at(0), books, trades: [] });

test('the fixed gate retains exactly 10 seconds and rejects any greater source age', () => {
  assert.throws(() => createQueueDepletionSourceAgeV1({ members: [{ marketId: 'M', exchangeId: 'E' }], startAt: at(0), maxDecisionSourceAgeMs: 5_000 }), /QUEUE_SOURCE_AGE_LIMIT/);
  const exactlyAtLimit = replay();
  observe(exactlyAtLimit, [book(10_000)]);
  const accepted = exactlyAtLimit.report({ throughAt: at(1) });
  assert.equal(accepted.counts.quotePairs, 1);
  assert.equal(accepted.counts.staleDecisionBooksRejected, 0);
  assert.deepEqual(accepted.decisionSourceAgeStrata.map(row => row.posted).reduce((a, b) => a + b, 0), 1);

  const overLimit = replay();
  observe(overLimit, [book(10_001)]);
  const rejected = overLimit.report({ throughAt: at(1) });
  assert.equal(rejected.counts.quotePairs, 0);
  assert.equal(rejected.counts.staleDecisionBooksRejected, 1);
  assert.equal(rejected.decisionSourceAgeStrata.find(row => row.label === '>10s').rejected, 1);
});

test('missing, malformed, future, and after-receipt source timestamps fail closed', () => {
  const cases = [
    { name: 'missing', patch: row => { delete row.sourceAt; }, invalidBooks: 1, invalidSourceTimes: 0 },
    { name: 'malformed', patch: row => { row.sourceAt = 'not-a-time'; }, invalidBooks: 1, invalidSourceTimes: 0 },
    { name: 'after decision time', patch: row => { row.sourceAt = at(1); }, invalidBooks: 1, invalidSourceTimes: 0 },
    { name: 'after receipt time', patch: row => { row.sourceAt = at(-1); row.receivedAt = at(-2); }, invalidBooks: 0, invalidSourceTimes: 1 },
  ];
  for (const item of cases) {
    const engine = replay(), invalid = book(0);
    item.patch(invalid);
    observe(engine, [invalid]);
    const result = engine.report({ throughAt: at(1) });
    assert.equal(result.counts.quotePairs, 0, item.name);
    assert.equal(result.counts.decisionCandidates, item.invalidSourceTimes, item.name);
    assert.equal(result.counts.invalidBooks, item.invalidBooks, item.name);
    assert.equal(result.counts.invalidDecisionSourceTimesRejected, item.invalidSourceTimes, item.name);
  }
});

test('each fill proxy retains its decision source and receipt timestamps and ages', () => {
  const engine = replay();
  const decisionBook = book(1_500, { receivedAt: at(-1_000) });
  observe(engine, [decisionBook]);
  engine.observeBatch({
    observedAt: at(3_001), books: [],
    trades: [{ id: '1', sequence: 1, marketId: 'M', exchangeId: 'E', price: '0.500000', quantity: 11, executedAt: at(3_001) }],
  });
  const result = engine.report({ throughAt: at(4_000) });
  assert.equal(result.counts.entryFillProxies, 1);
  assert.equal(result.unresolvedEntryProxies[0].decisionBookSourceAt, decisionBook.sourceAt);
  assert.equal(result.unresolvedEntryProxies[0].decisionBookReceivedAt, decisionBook.receivedAt);
  assert.equal(result.unresolvedEntryProxies[0].decisionBookSourceAgeMs, 1_500);
  assert.equal(result.unresolvedEntryProxies[0].decisionBookReceivedAgeMs, 1_000);
});

test('fixed byte-range verification ignores appended bytes and detects any changed frozen byte', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'queue-source-age-'));
  const file = path.join(directory, 'throwaway.jsonl');
  try {
    fs.writeFileSync(file, Buffer.from('scope\nwindow\nignored-tail\n'));
    const frozen = hashByteRange(file, 6, 13);
    assert.equal(frozen.bytes, 7);
    assert.equal(frozen.sha256, '6d3929ad0e72c76ef7f249ed7b4cb922040c51d64fe0b625a6d3c2eaea2833ae');
    fs.appendFileSync(file, 'later-data\n');
    assert.equal(verifyByteRange(file, frozen).sha256, frozen.sha256);
    const changed = Buffer.from('scope\nchange\nignored-tail\n');
    fs.writeFileSync(file, changed);
    assert.throws(() => verifyByteRange(file, frozen), /QUEUE_PREFIX_HASH/);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
