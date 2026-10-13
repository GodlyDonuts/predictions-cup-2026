import test from 'node:test';
import assert from 'node:assert/strict';
import { createConditionalFillMarkoutV1 } from './conditional-fill-markout-v1.mjs';

const base = Date.parse('2026-10-13T05:00:00.000Z');
const at = n => new Date(base + n).toISOString();
const book = (n, bid = '0.500000', ask = '0.510000', bidQty = 100, askQty = 100, sequence = n + 1) => ({
  marketId: 'M', exchangeId: 'E', sequence: String(sequence), sourceAt: at(n), requestStartedAt: at(n), receivedAt: at(n),
  bids: [{ price: bid, quantity: bidQty }], asks: [{ price: ask, quantity: askQty }],
});
const trade = (n, price, id = n) => ({ marketId: 'M', exchangeId: 'E', id: String(id), sequence: id, price, quantity: 1, executedAt: at(n) });
const study = () => createConditionalFillMarkoutV1({ markoutHorizonsMs: [1000, 5000, 10000], purgeMs: 25000 });

test('pre-activation prints do not fill; strict through after latency produces a proxy and executable markout', () => {
  const s = study();
  s.observeBatch({ observedAt: at(0), books: [book(0)] });
  s.observeBatch({ observedAt: at(2500), trades: [trade(2000, 0.490)] });
  s.observeBatch({ observedAt: at(4000), trades: [trade(3500, 0.490)] });
  s.observeBatch({ observedAt: at(5000), books: [book(5000, '0.495000', '0.500000', 80, 120)] });
  s.observeBatch({ observedAt: at(10000), books: [book(10000, '0.495000', '0.500000', 80, 120)] });
  const out = s.report({ splitAt: at(0) });
  const buy = out.tables.find(x => x.fold === 'heldOut' && x.action === 'buy' && x.horizonMs === 5000);
  assert.equal(out.counts.strictThroughProxies, 1);
  assert.equal(buy.markoutSamples, 1);
  assert.equal(buy.overall.meanExecutableNet, '-0.005000');
  assert.equal(buy.overall.meanMidMarkout, '-0.002500');
  assert.equal(buy.overall.oneTickEachWayStressMean, '-0.015000');
});

test('sell markout buys back at the future ask and has symmetric spread/stress accounting', () => {
  const s = study();
  s.observeBatch({ observedAt: at(0), books: [book(0)] });
  s.observeBatch({ observedAt: at(4000), trades: [trade(3500, 0.520, 1)] });
  s.observeBatch({ observedAt: at(10000), books: [book(10000, '0.510000', '0.515000', 100, 100)] });
  const row = s.report({ splitAt: at(0) }).tables.find(x => x.fold === 'heldOut' && x.action === 'sell' && x.horizonMs === 5000);
  assert.equal(row.markoutSamples, 1);
  assert.equal(row.overall.meanExecutableNet, '-0.005000');
  assert.equal(row.overall.meanMidMarkout, '-0.002500');
  assert.equal(row.overall.oneTickEachWayStressMean, '-0.015000');
});

test('a boundary clears queue-flow history and cancels unfilled candidate orders', () => {
  const s = study();
  s.observeBatch({ observedAt: at(0), books: [book(0)] });
  s.boundary('E');
  s.observeBatch({ observedAt: at(1000), books: [book(1000, '0.505000', '0.510000', 200, 50)] });
  const out = s.report({ splitAt: at(100000) });
  assert.equal(out.counts.boundaryCancelledQuotes, 2);
  assert.equal(out.counts.candidateQuotes, 2);
  assert.equal(out.flowThresholds, null);
});

test('a sequence gap resets flow features without falsely cancelling an independently valid point quote', () => {
  const s = study();
  s.observeBatch({ observedAt: at(0), books: [book(0)] });
  s.resetFeatures('E');
  s.observeBatch({ observedAt: at(4000), trades: [trade(3500, 0.490)] });
  s.observeBatch({ observedAt: at(10000), books: [book(10000, '0.495000', '0.500000')] });
  const out = s.report({ splitAt: at(0) });
  assert.equal(out.counts.strictThroughProxies, 1);
  assert.equal(out.counts.boundaryCancelledQuotes, 0);
});

test('chronological report purges the boundary window and keeps development-only flow thresholds', () => {
  const s = study();
  for (let n = 0; n <= 360000; n += 30000) {
    const bid = n < 180000 ? '0.500000' : '0.505000';
    s.observeBatch({ observedAt: at(n), books: [book(n, bid, '0.510000', n < 180000 ? 100 : 200, n < 180000 ? 100 : 50, n + 1)] });
  }
  const out = s.report({ splitAt: at(295000) });
  assert.ok(out.counts.candidateQuotesByFold.development > 0);
  assert.ok(out.counts.candidateQuotesByFold.heldOut > 0);
  assert.ok(out.counts.candidateQuotesByFold.purged > 0);
  assert.equal(out.definition.chronologicalSplit.purgeMs, 25000);
  assert.equal(out.definition.bins.alignedFlow, 'tercile cutpoints from development quote features only; when both cutpoints tie, split strictly below / equal / strictly above the tied value; no outcome-guided selection');
  const flow = out.tables.find(x => x.fold === 'development' && x.action === 'buy' && x.horizonMs === 1000).byAlignedFlowTercile;
  assert.ok(flow.some(x => x.bin !== 'missing' && x.candidateQuotes > 0));
  assert.ok(flow.find(x => x.bin === 'middle tercile').candidateQuotes > 0);
});

test('clock rollback and conflicting duplicated trade identities fail closed', () => {
  const s = study();
  s.observeBatch({ observedAt: at(1000), books: [book(1000)] });
  assert.throws(() => s.observeBatch({ observedAt: at(999), books: [] }), /MARKOUT_CLOCK_ROLLBACK/);
  const t = trade(2000, 0.490, 7);
  s.observeBatch({ observedAt: at(2001), trades: [t] });
  assert.throws(() => s.observeBatch({ observedAt: at(2002), trades: [{ ...t, price: 0.495 }] }), /TRADE_IDENTITY_CONFLICT/);
});
