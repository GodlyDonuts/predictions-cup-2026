import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  POLICY,
  buildReplayData,
  evaluateCapture,
  groupAndDeduplicate,
  makeHoldoutSignals,
  parseSnapshot,
  percentile,
  completeLineBound,
  readCapturePrefix,
  simulateRoundTrip,
} from './taker-reversal-latency-v1.mjs';

function record({ at, market = '248', exchange = '937', sequence, bid, ask, bidQty = 5, askQty = 5, startAt, receivedAt }) {
  const timestamp = new Date(at).toISOString();
  return {
    kind: 'snapshot',
    topic: `tournament:test:market:${market}`,
    observedAt: timestamp,
    receivedAt: new Date(receivedAt ?? at).toISOString(),
    requestStartedAt: new Date(startAt ?? (receivedAt ?? at) - 25).toISOString(),
    books: [{
      exchangeId: exchange,
      asOf: { sequence, at: timestamp },
      bids: [{ price: bid, quantity: bidQty }],
      asks: [{ price: ask, quantity: askQty }],
    }],
  };
}

test('snapshot parsing keeps only valid market books and measures captured request latency', () => {
  const valid = record({ at: 100_000, sequence: 1, bid: .49, ask: .50, startAt: 99_900, receivedAt: 100_000 });
  assert.equal(parseSnapshot(valid)[0].requestLatencyMs, 100);
  const invalid = record({ at: 100_000, sequence: 2, bid: .51, ask: .50 });
  assert.equal(parseSnapshot(invalid).length, 0);
});

test('public-books journal envelope parses its nested book with capture timing', () => {
  const publicBook = {
    kind: 'book',
    at: '2026-10-13T14:20:00.100Z',
    payload: {
      book: {
        marketId: 'wv-governor',
        exchangeId: 'exchange-1',
        sequence: 11,
        sourceAt: '2026-10-13T14:20:00.000Z',
        requestStartedAt: '2026-10-13T14:19:59.900Z',
        receivedAt: '2026-10-13T14:20:00.100Z',
        bids: [{ price: .49, quantity: 3 }],
        asks: [{ price: .51, quantity: 4 }],
      },
    },
  };
  const [row] = parseSnapshot(publicBook);
  assert.equal(row.marketId, 'wv-governor');
  assert.equal(row.sequence, '11');
  assert.equal(row.requestLatencyMs, 200);
  assert.equal(row.at, Date.parse(publicBook.at));
  assert.equal(row.sourceAt, Date.parse(publicBook.payload.book.sourceAt));
  assert.equal(row.mid, .5);
});

test('repeated book sequence is not counted as a new independent observation', () => {
  const a = record({ at: 100_000, sequence: 7, bid: .49, ask: .50 });
  const duplicate = { ...a, receivedAt: new Date(101_000).toISOString() };
  const data = groupAndDeduplicate([a, duplicate]);
  assert.equal(data.parsedSnapshots, 2);
  assert.equal(data.duplicateSequences, 1);
  assert.equal([...data.groups.values()][0].length, 1);
});

test('signal is contrarian only after a causal 10s four-tick dislocation and re-arms inside band', () => {
  const t = Date.parse(POLICY.taskCutoffAt) + 60_000;
  const records = [
    record({ at: t, sequence: 1, bid: .48, ask: .49 }),
    record({ at: t + 10_000, sequence: 2, bid: .50, ask: .51 }), // +.02
    record({ at: t + 11_000, sequence: 3, bid: .50, ask: .51 }), // same episode
    record({ at: t + 21_000, sequence: 4, bid: .49, ask: .50 }), // inside neutral band
    record({ at: t + 31_000, sequence: 5, bid: .47, ask: .48 }), // new - .02 episode
  ];
  const { groups } = groupAndDeduplicate(records);
  const signals = makeHoldoutSignals(groups, t, POLICY);
  assert.deepEqual(signals.map(x => Math.sign(x.move)), [1, -1]);
  assert.deepEqual(signals.map(x => x.at), [t + 10_000, t + 31_000]);
});

test('entry and exit use opposing executable prices; NO is a binary complement; zero fees', () => {
  const rows = [
    { at: 1_000, sourceAt: 1_000, bid: .50, bidQty: 10, ask: .51, askQty: 10 },
    { at: 2_000, sourceAt: 2_000, bid: .48, bidQty: 10, ask: .49, askQty: 10 },
    { at: 4_000, sourceAt: 4_000, bid: .46, bidQty: 10, ask: .47, askQty: 10 },
  ];
  const signal = { at: 1_000, marketId: '248', exchangeId: '937' };
  const yes = simulateRoundTrip(rows, signal, 'YES', 1_000, 0, POLICY);
  assert.equal(yes.status, 'complete');
  assert.ok(Math.abs(yes.netPnl - (-.03)) < 1e-12); // ask .51 to bid .48
  const no = simulateRoundTrip(rows, signal, 'NO', 1_000, 0, POLICY);
  assert.equal(no.status, 'complete');
  assert.ok(Math.abs(no.netPnl - .01) < 1e-12); // NO .50 to .51
});

test('sub-share top depth and stale or missing arrival books are never treated as fills', () => {
  const signal = { at: 1_000, marketId: '248', exchangeId: '937' };
  const shallow = [
    { at: 1_000, sourceAt: 1_000, bid: .49, bidQty: 10, ask: .50, askQty: .5 },
  ];
  assert.equal(simulateRoundTrip(shallow, signal, 'YES', 1_000, 0, POLICY).status, 'insufficient_entry_depth');
  const missing = [{ at: 10_000, sourceAt: 10_000, bid: .49, bidQty: 10, ask: .50, askQty: 10 }];
  assert.equal(simulateRoundTrip(missing, signal, 'YES', 1_000, 0, POLICY).status, 'observation_gap');
  const stale = [
    { at: 1_000, sourceAt: -5_000, bid: .49, bidQty: 10, ask: .50, askQty: 10 },
  ];
  assert.equal(simulateRoundTrip(stale, signal, 'YES', 1_000, 0, POLICY).status, 'stale_quote');
});

test('holdout starts after the task cutoff and purges signals without a complete holdout lookback', () => {
  const start = Date.parse(POLICY.taskCutoffAt);
  const records = [
    record({ at: start + 9_999, sequence: 1, bid: .48, ask: .49 }),
    record({ at: start + 20_000, sequence: 2, bid: .50, ask: .51 }),
    record({ at: start + 30_000, sequence: 3, bid: .50, ask: .51 }),
  ];
  const replay = buildReplayData(records);
  const result = evaluateCapture({ ...replay, captureStartMs: start - 100_000, captureEndMs: start + 30_000, latencyByTime: replay.latencyByTime });
  assert.ok(Date.parse(result.split.holdoutStartAt) >= start);
  assert.equal(result.split.firstSignalAtOrAfter, new Date(Date.parse(result.split.holdoutStartAt) + POLICY.lookbackMs).toISOString());
  assert.equal(makeHoldoutSignals(replay.groups, start, POLICY).some(x => x.at < start + POLICY.lookbackMs), false);
});

test('nearest-rank percentile is deterministic and empty samples stay missing', () => {
  assert.equal(percentile([4, 1, 3, 2], .5), 2);
  assert.equal(percentile([], .5), null);
});

test('capture time bounds scale without spreading a large book array onto the call stack', () => {
  const rows = Array.from({ length: 200_000 }, (_, at) => ({ at }));
  const replay = buildReplayData({ groups: new Map([['market:exchange', rows]]) });
  assert.equal(replay.captureStartMs, 0);
  assert.equal(replay.captureEndMs, 199_999);
});

test('capture freezer stops at a complete line and hashes exactly the parsed byte prefix', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'taker-reversal-'));
  const file = join(dir, 'capture.jsonl');
  const first = JSON.stringify(record({ at: 100_000, sequence: 1, bid: .49, ask: .50 })) + '\n';
  const second = JSON.stringify(record({ at: 101_000, sequence: 2, bid: .49, ask: .50 })) + '\n';
  try {
    writeFileSync(file, first + second.slice(0, -1));
    const bound = completeLineBound(file);
    assert.equal(bound, Buffer.byteLength(first));
    const frozen = await readCapturePrefix(file, bound);
    assert.equal(frozen.bytesRead, bound);
    assert.equal(frozen.parsedSnapshots, 1);
    assert.equal(frozen.sha256, createHash('sha256').update(Buffer.from(readFileSync(file).subarray(0, bound))).digest('hex'));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
