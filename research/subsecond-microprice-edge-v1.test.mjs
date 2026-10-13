import test from 'node:test';
import assert from 'node:assert/strict';
import { createSubsecondMicropriceEdgeV1, subsecondMicropriceHorizonsMsV1 } from './subsecond-microprice-edge-v1.mjs';

const BASE = Date.parse('2026-10-13T00:00:00.000Z');
const point = (sequence, offsetMs, {
  sourceOffsetMs = offsetMs,
  bid = '0.495000',
  ask = '0.505000',
  bidQuantity = 300,
  askQuantity = 100,
  exchangeId = '2',
  marketId = '1',
} = {}) => ({
  marketId,
  exchangeId,
  sequence: String(sequence),
  sourceAt: new Date(BASE + sourceOffsetMs).toISOString(),
  bids: [{ price: bid, quantity: bidQuantity }],
  asks: [{ price: ask, quantity: askQuantity }],
});
const clocks = offsetMs => ({
  observedAt: new Date(BASE + offsetMs).toISOString(),
  receivedAt: new Date(BASE + offsetMs).toISOString(),
});
const makeStudy = options => createSubsecondMicropriceEdgeV1({
  splitAt: new Date(BASE + 3_600_000).toISOString(),
  ...options,
});

test('uses every unique exchange sequence and labels all five fixed horizons', () => {
  const study = makeStudy();
  assert.deepEqual(study.horizonsMs, [100, 250, 500, 1_000, 5_000]);
  study.observe(point(1, 0), clocks(0));
  study.observe(point(1, 50, { sourceOffsetMs: 50 }), clocks(50));
  assert.equal(study.report().counts.duplicateSequences, 1);
  study.observe(point(2, 100, { sourceOffsetMs: 100, bid: '0.505000', ask: '0.515000' }), clocks(100));
  const report = study.report();
  const row = report.groups.find(x => x.period === 'development' && x.horizonMs === 100
    && x.pressure === 'up' && x.signalSpread === '2ticks');
  assert.equal(row.samples, 1);
  assert.equal(row.meanPressureDirectedMidMoveTicks, 2);
  assert.equal(row.meanAskToLaterBidTicks, 0);
  assert.equal(row.meanBidToLaterAskTicks, -4);
  assert.equal(row.meanPressureDirectedExecutableTicks, 0);
  assert.equal(report.folds.development[100].updates, 2);
});

test('does not label on an observed update whose source clock is before the horizon', () => {
  const study = makeStudy({ maximumGapMs: 2_000 });
  study.observe(point(1, 0), clocks(0));
  study.observe(point(2, 500, { sourceOffsetMs: 50 }), clocks(500));
  assert.equal(study.report().folds.development[100].labels, 0);
  study.observe(point(3, 600, { sourceOffsetMs: 150 }), clocks(600));
  const row = study.report().groups.find(x => x.horizonMs === 100 && x.pressure === 'up');
  assert.equal(row.samples, 1);
  assert.equal(study.report().folds.development[100].labelTiming.observedDelayMs.p50Ms, 500);
  assert.equal(study.report().folds.development[100].labelTiming.sourceOvershootMs.p50Ms, 50);
});

test('separates chronological development, purged interval, and holdout labels', () => {
  const split = BASE + 10_000;
  const study = makeStudy({ splitAt: new Date(split).toISOString() });
  study.observe(point(1, 0), clocks(0));
  study.observe(point(2, 5_000), clocks(5_000));
  study.observe(point(3, 6_000), clocks(6_000));
  study.observe(point(4, 10_000), clocks(10_000));
  study.observe(point(5, 15_000), clocks(15_000));
  const report = study.report();
  assert.equal(report.folds.development[5_000].labels, 1);
  assert.equal(report.folds.purged[5_000].labels, 0);
  assert.equal(report.folds.heldOut[5_000].labels, 1);
  assert.equal(report.protocol.chronologicalPurgeMs, 5_000);
});

test('censors a development label whose future source point crosses the holdout boundary', () => {
  const split = BASE + 10_000;
  const study = makeStudy({ splitAt: new Date(split).toISOString() });
  study.observe(point(1, 4_999), clocks(4_999));
  study.observe(point(2, 10_000), clocks(10_000));
  const report = study.report();
  assert.equal(report.folds.development[5_000].labels, 0);
  assert.equal(report.counts.censorReasons['split_boundary:development:5000'], 1);
});

test('latency cap censors stale target labels; source age is reported rather than hidden', () => {
  const study = makeStudy({ maximumLabelDelayMs: 500, maximumGapMs: 2_000 });
  study.observe(point(1, 0), clocks(0));
  study.observe(point(2, 601), clocks(601));
  const report = study.report();
  assert.equal(report.folds.development[100].labels, 0);
  assert.equal(report.counts.censorReasons['label_delay:development:100'], 1);
  assert.equal(report.pointLatencyByPeriod.development.sourceAgeMs.p50Ms, 0);
});

test('feed boundary and sequence rollback censor pending observations without fabricated fills', () => {
  const study = makeStudy();
  study.observe(point(10, 0), clocks(0));
  study.boundary('2', BASE + 50, 'disconnect');
  const before = study.report();
  assert.equal(before.folds.development[100].labels, 0);
  assert.equal(before.counts.censorReasons['disconnect:development:100'], 1);
  study.observe(point(11, 100), clocks(100));
  study.observe(point(1, 200), clocks(200));
  assert.equal(study.report().counts.sequenceRollbacks, 1);
});

test('capture-end censoring and reports are stable across repeated reads', () => {
  const study = makeStudy();
  study.observe(point(1, 0), clocks(0));
  const before = study.report();
  assert.equal(before.folds.development[5_000].pendingAtEnd, 1);
  study.finish();
  const after = study.report();
  assert.equal(after.folds.development[5_000].pendingAtEnd, 1);
  assert.equal(after.folds.development[5_000].censored, 1);
  assert.equal(after.counts.censorReasons['capture_end:development:5000'], 1);
  assert.deepEqual(study.report(), after);
});

test('rejects horizons that would change the predeclared comparison', () => {
  assert.throws(() => createSubsecondMicropriceEdgeV1({
    splitAt: new Date(BASE + 60_000).toISOString(),
    horizonsMs: [100, 250, 500, 1_000],
  }), /OPTIONS/);
  assert.equal(subsecondMicropriceHorizonsMsV1.length, 5);
});
