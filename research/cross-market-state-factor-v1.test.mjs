import test from 'node:test';
import assert from 'node:assert/strict';
import {
  aggregateStateClustersV1,
  crossMarketStatePairsV1,
  fitStateFactorV1,
  pairExecutableMarkoutV1,
  stateClusterCoverageV1,
  stateFactorSignalV1,
  synchronizedPairSnapshotV1,
} from './cross-market-state-factor-v1.mjs';

function market(id, exchangeId, title) {
  return { id: String(id), exchanges: [{ id: String(exchangeId) }], title };
}

function makeBooks({ aBid = 0.39, aAsk = 0.41, bBid = 0.59, bAsk = 0.61, aQty = 10, bQty = 10 } = {}) {
  const at = '2026-10-13T06:00:01.000Z';
  const source = '2026-10-13T06:00:00.500Z';
  const book = (bid, ask, qty, sequence) => ({
    receivedAt: at, sourceAt: source, sequence: String(sequence), segment: 1,
    bids: [{ price: bid, quantity: qty }], asks: [{ price: ask, quantity: qty }],
  });
  return { a: book(aBid, aAsk, aQty, 10), b: book(bBid, bAsk, bQty, 20) };
}

test('pairs only exact statewide same-party Senate and Governor contracts', () => {
  const result = crossMarketStatePairsV1([
    market(1, 11, 'Will the Democratic Party win the Florida Senate?'),
    market(2, 12, 'Will the Democratic Party win the Florida Governor?'),
    market(3, 13, 'Will the Republican Party win the Florida Senate?'),
    market(4, 14, 'Will the Republican Party win the Florida Governor?'),
    market(5, 15, 'Will the Democratic Party win the Georgia Senate?'),
    market(6, 16, 'Will the Democratic Party win the FL-07 House race?'),
  ]);
  assert.equal(result.pairs.length, 2);
  assert.equal(result.statewideContracts, 5);
  assert.deepEqual(result.pairs.map(pair => pair.id), ['Florida|Democratic', 'Florida|Republican']);
  assert.equal(result.pairs[0].marketA, '1');
  assert.equal(result.pairs[0].marketB, '2');
});

test('rejects duplicated exchange identity instead of silently forming a pair', () => {
  assert.throws(() => crossMarketStatePairsV1([
    market(1, 11, 'Will the Democratic Party win the Florida Senate?'),
    market(2, 11, 'Will the Democratic Party win the Florida Governor?'),
  ]), /CMSTATE_MARKET_IDENTITY/);
});

test('state-cluster coverage reconciles candidate pair rows and groups parties within each state', () => {
  const candidates = [
    { id: 'Florida|Democratic', state: 'Florida' },
    { id: 'Florida|Republican', state: 'Florida' },
    { id: 'Georgia|Democratic', state: 'Georgia' },
  ];
  const coverage = stateClusterCoverageV1(candidates, candidates.map(pair => ({ ...pair })));
  assert.equal(coverage.matches, true);
  assert.equal(coverage.candidatePairs, 3);
  assert.equal(coverage.analyzedPairs, 3);
  assert.equal(coverage.candidateStateClusters, 2);
  assert.equal(coverage.analyzedStateClusters, 2);
  assert.deepEqual(coverage.candidateStates, ['Florida', 'Georgia']);

  const droppedPair = stateClusterCoverageV1(candidates, candidates.slice(1));
  assert.equal(droppedPair.matches, false);
  assert.deepEqual(droppedPair.missingPairIds, ['Florida|Democratic']);
  const misgroupedPair = stateClusterCoverageV1(candidates, [
    { id: 'Florida|Democratic', state: 'Florida' },
    { id: 'Florida|Republican', state: 'Florida' },
    { id: 'Georgia|Democratic', state: 'Nevada' },
  ]);
  assert.equal(misgroupedPair.matches, false);
  assert.deepEqual(misgroupedPair.missingStates, ['Georgia']);
  assert.deepEqual(misgroupedPair.extraStates, ['Nevada']);
  const reassignedPair = stateClusterCoverageV1(candidates, [
    { id: 'Florida|Democratic', state: 'Florida' },
    { id: 'Florida|Republican', state: 'Georgia' },
    { id: 'Georgia|Democratic', state: 'Georgia' },
  ]);
  assert.equal(reassignedPair.candidateStateClusters, reassignedPair.analyzedStateClusters);
  assert.equal(reassignedPair.matches, false);
  assert.deepEqual(reassignedPair.pairStateMismatches, [
    { id: 'Florida|Republican', candidateState: 'Florida', analyzedState: 'Georgia' },
  ]);
});

test('state-level markout aggregation reconciles multiple party-pair clusters', () => {
  const states = aggregateStateClustersV1([
    { state: 'Florida', marks: 2, profitable: 1, grossPnl: 0.03, stressedPnl: 0.01 },
    { state: 'Florida', marks: 1, profitable: 0, grossPnl: -0.02, stressedPnl: -0.04 },
    { state: 'Georgia', marks: 1, profitable: 1, grossPnl: 0.04, stressedPnl: 0.02 },
  ]);
  assert.equal(states.length, 2);
  assert.deepEqual(states.map(({ state, pairClusters, marks, profitable }) => ({ state, pairClusters, marks, profitable })), [
    { state: 'Florida', pairClusters: 2, marks: 3, profitable: 1 },
    { state: 'Georgia', pairClusters: 1, marks: 1, profitable: 1 },
  ]);
  assert.ok(Math.abs(states[0].grossPnl - 0.01) < 1e-12);
  assert.ok(Math.abs(states[0].stressedPnl + 0.03) < 1e-12);
  assert.ok(Math.abs(states[1].grossPnl - 0.04) < 1e-12);
  assert.ok(Math.abs(states[1].stressedPnl - 0.02) < 1e-12);
});

test('development-only regression recovers positive common-factor hedge beta', () => {
  const points = [];
  for (let i = 0; i < 240; i++) {
    const bMid = 0.45 + 0.08 * Math.sin(i / 9) + 0.03 * Math.sin(i / 2.7);
    const residual = 0.006 * Math.sin(i / 5);
    points.push({ atMs: i * 1000, aMid: 0.18 + 1.3 * bMid + residual, bMid });
  }
  const fit = fitStateFactorV1(points);
  assert.equal(fit.usable, true);
  assert.ok(Math.abs(fit.beta - 1.3) < 0.08);
  assert.ok(fit.sigma >= 0.01);
  assert.equal(fit.observations, 240);
});

test('fit requires enough chronological development observations', () => {
  const points = Array.from({ length: 20 }, (_, i) => ({ atMs: i * 1000, aMid: 0.3 + i * 0.001, bMid: 0.4 + i * 0.001 }));
  assert.deepEqual(fitStateFactorV1(points), { usable: false, reason: 'too_few_development_points', observations: 20 });
});

test('fit rejects unstable, negative-sign, and low-variation hedge relationships', () => {
  const negative = Array.from({ length: 120 }, (_, i) => ({ atMs: i * 1000, aMid: 0.8 - i * 0.002, bMid: 0.2 + i * 0.002 }));
  assert.equal(fitStateFactorV1(negative).reason, 'unstable_or_wrong_sign_hedge');
  const flatB = Array.from({ length: 120 }, (_, i) => ({ atMs: i * 1000, aMid: 0.25 + 0.02 * Math.sin(i), bMid: 0.4 }));
  assert.equal(fitStateFactorV1(flatB).reason, 'insufficient_joint_price_variation');
  const equalShare = fitStateFactorV1(flatB, { betaOverride: 1 });
  assert.equal(equalShare.usable, true);
  assert.equal(equalShare.betaSource, 'fixed_equal_share_comparator');
});

test('signal direction buys the cheap contract and the complement of the rich one', () => {
  const model = { usable: true, intercept: 0, beta: 1, residualMean: 0, sigma: 0.01 };
  assert.equal(stateFactorSignalV1(model, 0.45, 0.55, 2).direction, -1);
  assert.equal(stateFactorSignalV1(model, 0.55, 0.45, 2).direction, 1);
  assert.equal(stateFactorSignalV1(model, 0.51, 0.50, 2).direction, 0);
});

test('synchronized books require fresh source and receipt clocks, same continuity segment', () => {
  const { a, b } = makeBooks();
  assert.ok(synchronizedPairSnapshotV1(a, b, Date.parse('2026-10-13T06:00:01.000Z')));
  assert.equal(synchronizedPairSnapshotV1(a, b, Date.parse('2026-10-13T06:00:04.001Z')), null);
  assert.equal(synchronizedPairSnapshotV1(a, { ...b, sourceAt: '2026-10-13T06:00:02.000Z' }, Date.parse('2026-10-13T06:00:01.000Z')), null);
  assert.equal(synchronizedPairSnapshotV1(a, { ...b, segment: 2 }, Date.parse('2026-10-13T06:00:01.000Z')), null);
});

test('synchronization rejects excessive cross-market receipt or source skew', () => {
  const { a, b } = makeBooks();
  assert.equal(synchronizedPairSnapshotV1(a, { ...b, receivedAt: '2026-10-13T06:00:04.000Z' }, Date.parse('2026-10-13T06:00:04.000Z')), null);
  assert.equal(synchronizedPairSnapshotV1(a, { ...b, sourceAt: '2026-10-13T06:00:03.000Z' }, Date.parse('2026-10-13T06:00:01.000Z')), null);
});

test('long cheap-A / short rich-B markout includes both executable spreads and zero fees', () => {
  const entry = makeBooks({ aBid: 0.39, aAsk: 0.41, bBid: 0.59, bAsk: 0.61 });
  const exit = makeBooks({ aBid: 0.45, aAsk: 0.47, bBid: 0.53, bAsk: 0.55 });
  const mark = pairExecutableMarkoutV1(-1, 1, entry, exit);
  assert.equal(mark.eligible, true);
  assert.ok(Math.abs(mark.entryCost - 0.82) < 1e-12);
  assert.ok(Math.abs(mark.exitValue - 0.90) < 1e-12);
  assert.ok(Math.abs(mark.grossPnl - 0.08) < 1e-12);
  assert.ok(Math.abs(mark.oneTickEachFillStressPnl - 0.06) < 1e-12);
});

test('long cheap-B / short rich-A markout is symmetric', () => {
  const entry = makeBooks({ aBid: 0.59, aAsk: 0.61, bBid: 0.39, bAsk: 0.41 });
  const exit = makeBooks({ aBid: 0.53, aAsk: 0.55, bBid: 0.45, bAsk: 0.47 });
  const mark = pairExecutableMarkoutV1(1, 1, entry, exit);
  assert.equal(mark.eligible, true);
  assert.ok(Math.abs(mark.entryCost - 0.82) < 1e-12);
  assert.ok(Math.abs(mark.exitValue - 0.90) < 1e-12);
  assert.ok(Math.abs(mark.grossPnl - 0.08) < 1e-12);
});

test('pair markout fails closed when either leg lacks top-level size', () => {
  const entry = makeBooks({ bQty: 0 });
  const exit = makeBooks();
  assert.deepEqual(pairExecutableMarkoutV1(-1, 1, entry, exit), { eligible: false, reason: 'insufficient_top_level_depth' });
});

test('pair markout rejects unsupported direction and hedge scale', () => {
  const books = makeBooks();
  assert.throws(() => pairExecutableMarkoutV1(0, 1, books, books), /CMSTATE_MARKOUT_INPUT/);
  assert.throws(() => pairExecutableMarkoutV1(-1, 0, books, books), /CMSTATE_MARKOUT_INPUT/);
});
