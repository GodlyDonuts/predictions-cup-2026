import test from 'node:test';
import assert from 'node:assert/strict';
import { createCausalDipReversionV1 } from './causal-dip-reversion-v1.mjs';

const SPLIT = 4 * 60 * 60_000;
const point = (at, bid = '0.495000', ask = '0.500000', bidQuantity = 100, askQuantity = 100) => ({
  marketId: '1', exchangeId: '2', at, bid, ask, bidQuantity, askQuantity,
});
function warm(engine, { count = 20, interval = 65_000, bid = '0.495000', ask = '0.500000', at = 0 } = {}) {
  for (let i = 0; i < count; i++) engine.observeBook(point(at + i * interval, bid, ask));
  return at + (count - 1) * interval;
}
function dip(engine, at, { bid = '0.480000', ask = '0.490000', qty = 100 } = {}) {
  engine.observeBook(point(at, bid, ask, qty, qty));
}

test('signal anchor is the upper median of strictly prior asks, not current ask', () => {
  const engine = createCausalDipReversionV1({ splitAt: SPLIT });
  let at = 0;
  for (let i = 0; i < 20; i++) {
    const ask = i < 10 ? '0.480000' : '0.500000';
    engine.observeBook(point(at, '0.475000', ask));
    at += 65_000;
  }
  dip(engine, at, { bid: '0.480000', ask: '0.490000' });
  const result = engine.report();
  assert.equal(result.folds.development.signals, 1);
  assert.equal(result.folds.development.entryFillProxies, 0);
});

test('insufficient prior count or span cannot create a dip signal', () => {
  const engine = createCausalDipReversionV1({ splitAt: SPLIT });
  warm(engine, { count: 19, interval: 65_000 });
  dip(engine, 1_300_000);
  const result = engine.report();
  assert.equal(result.folds.development.signals, 0);
});

test('entry waits for the declared latency and requires displayed ask depth', () => {
  const engine = createCausalDipReversionV1({ splitAt: SPLIT });
  const last = warm(engine);
  const signalAt = last + 65_000;
  dip(engine, signalAt);
  engine.observeBook(point(signalAt + 2_999, '0.480000', '0.490000'));
  assert.equal(engine.report().folds.development.entryFillProxies, 0);
  engine.observeBook(point(signalAt + 10_000, '0.480000', '0.490000', 100, 0));
  assert.equal(engine.report().folds.development.entryFillProxies, 0);
  engine.observeBook(point(signalAt + 20_000, '0.480000', '0.490000'));
  assert.equal(engine.report().folds.development.entryFillProxies, 1);
});

test('rebound exit sells at a later executable bid after exit latency', () => {
  const engine = createCausalDipReversionV1({ splitAt: SPLIT });
  const signalAt = warm(engine) + 65_000;
  dip(engine, signalAt);
  engine.observeBook(point(signalAt + 10_000, '0.480000', '0.490000'));
  engine.observeBook(point(signalAt + 11_000, '0.495000', '0.505000'));
  assert.equal(engine.report().folds.development.completed, 0);
  engine.observeBook(point(signalAt + 15_000, '0.495000', '0.505000'));
  const row = engine.report().folds.development;
  assert.equal(row.completed, 1);
  assert.equal(row.reboundExits, 1);
  assert.equal(row.meanExecutableNetPerShare, '0.005000');
  assert.equal(row.oneTickEachWayStressMean, '-0.005000');
});

test('horizon exit uses the future bid and includes spread loss with zero fees', () => {
  const engine = createCausalDipReversionV1({ splitAt: SPLIT });
  const signalAt = warm(engine) + 65_000;
  dip(engine, signalAt);
  const entryAt = signalAt + 10_000;
  engine.observeBook(point(entryAt, '0.480000', '0.490000'));
  const horizonAt = entryAt + 15 * 60_000;
  for (let at = entryAt + 60_000; at < horizonAt; at += 60_000) {
    engine.observeBook(point(at, '0.470000', '0.480000'));
  }
  engine.observeBook(point(horizonAt, '0.470000', '0.480000'));
  engine.observeBook(point(horizonAt + 5_000, '0.470000', '0.480000'));
  const row = engine.report().folds.development;
  assert.equal(row.completed, 1);
  assert.equal(row.horizonExits, 1);
  assert.equal(row.meanExecutableNetPerShare, '-0.020000');
});

test('source gaps censor open positions and reset the prior-only anchor', () => {
  const engine = createCausalDipReversionV1({ splitAt: SPLIT });
  const signalAt = warm(engine) + 65_000;
  dip(engine, signalAt);
  engine.observeBook(point(signalAt + 10_000, '0.480000', '0.490000'));
  engine.observeBook(point(signalAt + 10_000 + 120_001, '0.480000', '0.490000'));
  const row = engine.report().folds.development;
  assert.equal(row.censoredAfterEntry, 1);
  assert.deepEqual(row.censoredAfterEntryByReason, { source_gap: 1 });
  assert.equal(engine.report().counts.gapResets, 1);
});

test('purge suppresses boundary-period entries while heldout decisions remain chronological', () => {
  const splitAt = 60 * 60_000;
  const engine = createCausalDipReversionV1({ splitAt });
  let at = warm(engine) + 10_000;
  const purgeSignalAt = splitAt - 20 * 60_000;
  for (; at < purgeSignalAt; at += 10_000) engine.observeBook(point(at));
  dip(engine, purgeSignalAt);
  const before = engine.report();
  assert.equal(before.folds.purged.signals, 0);
  for (at = purgeSignalAt + 10_000; at <= splitAt; at += 10_000) engine.observeBook(point(at));
  at = splitAt + 10_000;
  dip(engine, at);
  assert.equal(engine.report().folds.heldOut.signals, 1);
});

test('boundary purges a pending entry without calling it a fill', () => {
  const engine = createCausalDipReversionV1({ splitAt: SPLIT });
  const signalAt = warm(engine) + 65_000;
  dip(engine, signalAt);
  engine.boundary('2', signalAt + 1_000, 'feed_gap');
  const row = engine.report().folds.development;
  assert.equal(row.signals, 1);
  assert.equal(row.entryFillProxies, 0);
  assert.equal(row.entryExpired, 1);
  assert.deepEqual(row.entryExpiredByReason, { feed_gap: 1 });
  assert.deepEqual(engine.report().folds.development.entryExpiredByReason, { feed_gap: 1 });
});
