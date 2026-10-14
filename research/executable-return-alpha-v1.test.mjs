import test from 'node:test';
import assert from 'node:assert/strict';
import {
  EXECUTABLE_RETURN_POLICY_V1,
  executableReturnFeaturesV1,
  executableReturnLabelsV1,
  fitExecutableReturnModelV1,
  scoreExecutableReturnModelV1,
} from './executable-return-alpha-v1.mjs';

function point({marketId = 'M', sequence, at, requestAt = at, bid, ask, buyCapacity = 1, sellCapacity = 1, imbalance = 0.2}) {
  const mid = (bid + ask) / 2, spread = ask - bid;
  return {marketId, sequence, at, requestAt, bid, ask, mid, buyCapacity, sellCapacity,
    features: [spread, imbalance, imbalance * spread / 2, sequence ? 0.001 : 0, mid * (1 - mid)]};
}

const policy = Object.freeze({...EXECUTABLE_RETURN_POLICY_V1, horizonMs: 10_000,
  maximumArrivalWaitMs: 2_000, maximumExitWaitMs: 2_000});

test('labels arrival ask to exit bid even when the ask rises after the decision', () => {
  const rows = [
    point({sequence: 1, at: 0, bid: 0.40, ask: 0.45}),
    point({sequence: 2, at: 3_000, bid: 0.48, ask: 0.52}),
    point({sequence: 3, at: 10_000, bid: 0.70, ask: 0.75}),
    point({sequence: 4, at: 13_000, bid: 0.56, ask: 0.60}),
  ];
  const {labels, excluded} = executableReturnLabelsV1([rows], policy);
  const yes = labels.find(row => row.sequence === 1 && row.side === 'YES');
  const no = labels.find(row => row.sequence === 1 && row.side === 'NO');
  assert.equal(yes.entryAt, 3_000);
  assert.equal(yes.exitAt, 13_000, 'holding horizon begins at arrival, not decision');
  assert.equal(yes.entryAsk, 0.52);
  assert.equal(yes.exitBid, 0.56);
  assert.ok(Math.abs(yes.net - 0.04) < 1e-12);
  assert.ok(Math.abs(yes.stressedNet - 0.03) < 1e-12);
  assert.ok(Math.abs(no.net - ((1 - 0.60) - (1 - 0.48))) < 1e-12);
  assert.equal(excluded.noEntryDepth, 0);
});

test('NO-side features reverse directional book features and complement the midpoint', () => {
  const row = point({sequence: 1, at: 0, bid: 0.40, ask: 0.50, imbalance: 0.6});
  const yes = executableReturnFeaturesV1(row, 'YES');
  const no = executableReturnFeaturesV1(row, 'NO');
  assert.equal(yes[0], 0.45);
  assert.equal(no[0], 0.55);
  assert.equal(no[1], yes[1]);
  assert.equal(no[2], -yes[2]);
  assert.equal(no[4], -yes[4]);
  assert.ok(Math.abs(no[5] - yes[5]) < 1e-12);
});

test('excludes entry and exit observations without one share of displayed depth', () => {
  const noEntry = [
    point({marketId: 'A', sequence: 1, at: 0, bid: 0.40, ask: 0.45}),
    point({marketId: 'A', sequence: 2, at: 3_000, bid: 0.44, ask: 0.46, buyCapacity: 0, sellCapacity: 1}),
    point({marketId: 'A', sequence: 3, at: 13_000, bid: 0.47, ask: 0.49}),
  ];
  const noExit = [
    point({marketId: 'B', sequence: 1, at: 0, bid: 0.40, ask: 0.45}),
    point({marketId: 'B', sequence: 2, at: 3_000, bid: 0.44, ask: 0.46}),
    point({marketId: 'B', sequence: 3, at: 13_000, bid: 0.47, ask: 0.49, sellCapacity: 0}),
  ];
  const {labels, excluded} = executableReturnLabelsV1([noEntry, noExit], policy);
  assert.equal(labels.some(row => row.marketId === 'A' && row.sequence === 1 && row.side === 'YES'), false);
  assert.equal(labels.some(row => row.marketId === 'A' && row.sequence === 1 && row.side === 'NO'), true);
  assert.equal(labels.some(row => row.marketId === 'B' && row.sequence === 1 && row.side === 'YES'), false);
  assert.ok(excluded.noEntryDepth > 0);
  assert.ok(excluded.noExitDepth > 0);
});

test('rejects late arrival instead of assigning a stale executable entry', () => {
  const rows = [
    point({sequence: 1, at: 0, bid: 0.40, ask: 0.45}),
    point({sequence: 2, at: 6_001, bid: 0.44, ask: 0.46}),
    point({sequence: 3, at: 16_001, bid: 0.47, ask: 0.49}),
  ];
  const {labels, excluded} = executableReturnLabelsV1([rows], policy);
  assert.equal(labels.length, 0);
  assert.ok(excluded.lateArrival > 0);
});

test('fits only fully matured labels before cutoff', () => {
  const rows = [];
  for (let i = 0; i < 41; i++) rows.push({decisionAt: i * 10, exitAt: i * 10 + 5,
    net: (i % 2 ? 0.01 : -0.01) + i * 1e-5,
    features: [i % 3, i % 5, i % 7, i % 11, i % 13, i % 17]});
  rows[39].exitAt = 1001;
  rows[40].decisionAt = 800;
  rows[40].exitAt = 801;
  rows.push({decisionAt: 1001, exitAt: 1002, net: 2, features: Array(6).fill(99)});
  const model = fitExecutableReturnModelV1(rows, 1000, {...EXECUTABLE_RETURN_POLICY_V1, ridgeAlpha: 0.01});
  assert.equal(model.trainingRows, 40);
  assert.ok(model.trainingMean < 0.01);
});

test('scores one best side per market and holds through the delayed exit window', () => {
  const model = {mean: Array(6).fill(0), scale: Array(6).fill(1),
    coefficients: [0.021, 0, 0, 0, 0, 0, 0]};
  const decisions = [
    point({sequence: 1, at: 100_000, bid: 0.40, ask: 0.45}),
    point({sequence: 2, at: 110_000, bid: 0.40, ask: 0.45}),
    point({sequence: 3, at: 120_000, bid: 0.40, ask: 0.45}),
  ];
  const labels = [1, 3].map(sequence => ({marketId: 'M', sequence, side: 'YES',
    decisionAt: sequence === 1 ? 100_000 : 120_000, net: 0.02, stressedNet: 0.01}));
  const result = scoreExecutableReturnModelV1(model, decisions, labels, 0, policy);
  assert.equal(result.selectedDecisions, 2);
  assert.equal(result.selectedWithOutcome, 2);
  assert.equal(result.selectedCensored, 0);
  assert.equal(result.selected.n, 2);
  assert.equal(result.selected.meanNet, 0.02);
});

test('does not call a sparse counterfactual sample profitability proof', () => {
  const model = {mean: Array(6).fill(0), scale: Array(6).fill(1),
    coefficients: [0.021, 0, 0, 0, 0, 0, 0]};
  const decision = point({sequence: 1, at: 100_000, bid: 0.40, ask: 0.45});
  const result = scoreExecutableReturnModelV1(model, [decision], [], 0, policy);
  assert.equal(result.selectedCensored, 1);
  assert.equal(result.profitabilityEstablished, false);
  assert.equal(result.actualFillsProven, false);
});
