import test from 'node:test';
import assert from 'node:assert/strict';
import { createConditionalFillMarkoutV1 } from './conditional-fill-markout-v1.mjs';

const base = Date.parse('2026-10-13T05:00:00.000Z');
const at = n => new Date(base + n).toISOString();
const atPrecise = (n, microsWithinMs) => {
  const date = new Date(base + n).toISOString();
  const micros = Number(date.slice(20, 23)) * 1000 + microsWithinMs;
  return `${date.slice(0, 19)}.${String(micros).padStart(6, '0')}Z`;
};
const book = (n, bid = '0.500000', ask = '0.510000', bidQty = 100, askQty = 100, sequence = n + 1) => ({
  marketId: 'M', exchangeId: 'E', sequence: String(sequence), sourceAt: at(n), requestStartedAt: at(n), receivedAt: at(n),
  bids: [{ price: bid, quantity: bidQty }], asks: [{ price: ask, quantity: askQty }],
});
const trade = (n, price, id = n) => ({ marketId: 'M', exchangeId: 'E', id: String(id), sequence: id, price, quantity: 1, executedAt: at(n) });
const study = options => createConditionalFillMarkoutV1({ markoutHorizonsMs: [1000, 5000, 10000], purgeMs: 36000, ...options });

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
  assert.equal(buy.eventTimeDiagnosticSamples, 1);
  assert.equal(buy.executable.meanExecutableNet, '-0.005000');
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
  assert.equal(out.definition.chronologicalSplit.purgeMs, 36000);
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

test('overall and grouped markout summaries count distinct markets', () => {
  const s = study();
  const scopedBook = (marketId, exchangeId, n, bid, ask, sequence) => ({
    ...book(n, bid, ask, 100, 100, sequence), marketId, exchangeId,
  });
  const scopedTrade = (marketId, exchangeId, n, price) => ({
    marketId, exchangeId, id: '1', sequence: 1, price, quantity: 1, executedAt: at(n),
  });
  s.observeBatch({ observedAt: at(0), books: [
    scopedBook('M1', 'E1', 0, '0.500000', '0.510000', 1),
    scopedBook('M2', 'E2', 0, '0.600000', '0.610000', 1),
  ] });
  s.observeBatch({ observedAt: at(4000), trades: [
    scopedTrade('M1', 'E1', 3500, '0.490000'),
    scopedTrade('M2', 'E2', 3500, '0.590000'),
  ] });
  s.observeBatch({ observedAt: at(8000), books: [
    scopedBook('M1', 'E1', 8000, '0.495000', '0.500000', 2),
    scopedBook('M2', 'E2', 8000, '0.595000', '0.600000', 2),
  ] });
  const row = s.report({ splitAt: at(0) }).tables.find(x => x.fold === 'heldOut' && x.action === 'buy' && x.horizonMs === 1000);
  assert.equal(row.strictThroughProxies, 2);
  assert.equal(row.overall.markets, 2);
  assert.equal(row.bySpread.find(x => x.bin === '2 ticks').markout.markets, 2);
});

test('receipt time and exit response exclude an old event-time book; executable marks require a fresh later book', () => {
  const s = study({ markoutHorizonsMs: [1000], purgeMs: 36000, maximumExecutableBookAgeMs: 1000 });
  s.observeBatch({ observedAt: at(0), books: [book(0)] });
  s.observeBatch({ observedAt: at(8000), trades: [trade(3500, 0.490)] });

  const delayedReportBook = { ...book(4500), requestStartedAt: at(8500), receivedAt: at(8500) };
  s.observeBatch({ observedAt: at(8500), books: [delayedReportBook] });
  let row = s.report({ splitAt: at(0) }).tables.find(x => x.fold === 'heldOut' && x.action === 'buy' && x.horizonMs === 1000);
  assert.equal(row.eventTimeDiagnosticSamples, 1);
  assert.equal(row.markoutSamples, 0, 'source time before trade-report receipt/response must not mark an executable exit');

  s.observeBatch({ observedAt: at(10500), books: [book(10500, '0.495000', '0.500000')] });
  row = s.report({ splitAt: at(0) }).tables.find(x => x.fold === 'heldOut' && x.action === 'buy' && x.horizonMs === 1000);
  assert.equal(row.markoutSamples, 0, 'a book before the 11s receipt-plus-response target is too early');

  const staleAfterTarget = { ...book(11500), requestStartedAt: at(13500), receivedAt: at(13500), sequence: '13501' };
  s.observeBatch({ observedAt: at(13500), books: [staleAfterTarget] });
  row = s.report({ splitAt: at(0) }).tables.find(x => x.fold === 'heldOut' && x.action === 'buy' && x.horizonMs === 1000);
  assert.equal(row.markoutSamples, 0, 'a source point beyond the target is still rejected when stale at receipt');

  const freshAfterTarget = { ...book(13300, '0.495000', '0.500000'), requestStartedAt: at(14000), receivedAt: at(14000), sequence: '14002' };
  s.observeBatch({ observedAt: at(14000), books: [freshAfterTarget] });
  row = s.report({ splitAt: at(0) }).tables.find(x => x.fold === 'heldOut' && x.action === 'buy' && x.horizonMs === 1000);
  assert.equal(row.markoutSamples, 1);
  assert.equal(row.overall.meanExecutableNet, '-0.005000');
});

test('a future-skewed source timestamp cannot make a pre-response book executable', () => {
  const s = study({ markoutHorizonsMs: [1000], purgeMs: 27001 });
  s.observeBatch({ observedAt: at(0), books: [book(0)] });
  s.observeBatch({ observedAt: at(8000), trades: [trade(3500, 0.490)] });

  const receivedBeforeResponse = { ...book(12000), requestStartedAt: at(9900), receivedAt: at(10000), sequence: '12001' };
  s.observeBatch({ observedAt: at(12000), books: [receivedBeforeResponse] });
  let row = s.report({ splitAt: at(0) }).tables.find(x => x.fold === 'heldOut' && x.action === 'buy' && x.horizonMs === 1000);
  assert.equal(row.markoutSamples, 0, 'source time after the target cannot substitute for local receipt after the response target');

  const receivedAfterResponse = { ...book(12100), requestStartedAt: at(11000), receivedAt: at(11100), sequence: '12101' };
  s.observeBatch({ observedAt: at(12100), books: [receivedAfterResponse] });
  row = s.report({ splitAt: at(0) }).tables.find(x => x.fold === 'heldOut' && x.action === 'buy' && x.horizonMs === 1000);
  assert.equal(row.markoutSamples, 1);
});

test('development labels require availability before the cutoff even when source time is earlier', () => {
  const s = study({ markoutHorizonsMs: [1000], purgeMs: 27001 });
  s.observeBatch({ observedAt: at(0), books: [book(0)] });
  s.observeBatch({ observedAt: at(3600), trades: [trade(3500, 0.490)] });
  const receivedAfterDevelopmentCutoff = { ...book(4500), requestStartedAt: at(7000), receivedAt: at(7000) };
  s.observeBatch({ observedAt: at(7000), books: [receivedAfterDevelopmentCutoff] });
  const freshExecutableAfterCutoff = { ...book(6800), requestStartedAt: at(7200), receivedAt: at(7200), sequence: '7201' };
  s.observeBatch({ observedAt: at(7200), books: [freshExecutableAfterCutoff] });
  const row = s.report({ splitAt: at(33000) }).tables.find(x => x.fold === 'development' && x.action === 'buy' && x.horizonMs === 1000);
  assert.equal(row.eventTimeDiagnosticSamples, 0, 'source-before-cutoff but receipt-after-cutoff must not enter development');
  assert.equal(row.markoutSamples, 0, 'an executable mark first available after cutoff must not enter development');
  assert.equal(row.unresolvedOrPurgedMarkouts, 1);
});

test('strict-through eligibility uses exact execution nanoseconds against activation and TTL', () => {
  const preciseBook = () => ({ ...book(0), sourceAt: atPrecise(0, 500), requestStartedAt: atPrecise(0, 500), receivedAt: atPrecise(0, 500) });
  const preciseTrade = (n, microsWithinMs, id = 1) => ({
    marketId: 'M', exchangeId: 'E', id: String(id), sequence: id, price: 0.490, quantity: 1,
    executedAt: atPrecise(n, microsWithinMs),
  });
  const run = candidateTrade => {
    const s = study();
    s.observeBatch({ observedAt: atPrecise(0, 500), books: [preciseBook()] });
    s.observeBatch({ observedAt: at(15000), trades: [candidateTrade] });
    return s.report({ splitAt: at(0) }).counts.strictThroughProxies;
  };

  assert.equal(run(preciseTrade(3000, 400)), 0, 'execution 0.1ms before activation is ineligible');
  assert.equal(run(preciseTrade(13000, 400)), 1, 'execution 0.1ms before TTL expiry remains eligible despite delayed report');
  assert.equal(run(preciseTrade(13000, 600)), 0, 'execution 0.1ms after TTL expiry is ineligible');
});

