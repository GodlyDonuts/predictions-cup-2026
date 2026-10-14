import test from 'node:test';
import assert from 'node:assert/strict';
import {
  evaluateOracle3WangSensitivity,
  oracle3HierarchicalLambda,
  oracle3TimeVaryingLambda,
  ORACLE3_WANG_SENSITIVITY,
} from './oracle3-hierarchical-wang.mjs';

test('Table 3 coefficients reproduce the source formula and feature directions', () => {
  const lambda = oracle3HierarchicalLambda({marketPrice: 0.57, volume: 5000, durationHours: 720, spread: 0.04});
  const expected = 0.2590 - 0.0716 * Math.log1p(5000) + 0.1431 * Math.log1p(720)
    - 0.4772 * Math.abs(0.57 - 0.5) + 0.1273 * 0.04;
  assert.ok(Math.abs(lambda - expected) < 1e-12);
  assert.ok(oracle3HierarchicalLambda({marketPrice: 0.5, volume: 100, durationHours: 24})
    > oracle3HierarchicalLambda({marketPrice: 0.5, volume: 10_000, durationHours: 24}));
  assert.ok(oracle3HierarchicalLambda({marketPrice: 0.5, durationHours: 720})
    > oracle3HierarchicalLambda({marketPrice: 0.5, durationHours: 24}));
  assert.ok(oracle3HierarchicalLambda({marketPrice: 0.5})
    > oracle3HierarchicalLambda({marketPrice: 0.9}));
});

test('Table 5 lifecycle formula uses its own coefficients and strict tau range', () => {
  const lambda = oracle3TimeVaryingLambda({marketPrice: 0.57, volume: 5000, durationHours: 720, elapsedFraction: 0.4});
  const expected = 0.2531 - 0.1555 * 0.4 + 0.0744 * 0.4 ** 2
    - 0.0568 * Math.log1p(5000) + 0.1085 * Math.log1p(720) - 0.2902 * Math.abs(0.57 - 0.5);
  assert.ok(Math.abs(lambda - expected) < 1e-12);
  assert.throws(() => oracle3TimeVaryingLambda({marketPrice: 0.5, elapsedFraction: 1.01}), /elapsedFraction/);
});

test('sensitivity output compares only independent probability with a transformed benchmark', () => {
  const result = evaluateOracle3WangSensitivity({
    independentPhysicalProbability: 0.6,
    marketPrice: 0.55,
    volume: 2000,
    durationHours: 1000,
    spread: 0.03,
    elapsedFraction: 0.25,
  });
  assert.equal(result.sourceVenue, 'Polymarket');
  assert.equal(result.sigCalibrated, false);
  assert.equal(result.executionAuthority, false);
  assert.equal(result.orderCreated, false);
  assert.equal(result.fillRecorded, false);
  assert.equal(result.scenarios.length, 2);
  for (const scenario of result.scenarios) {
    assert.ok(Number.isFinite(scenario.lambda));
    assert.ok(scenario.benchmarkPrice > 0 && scenario.benchmarkPrice < 1);
    assert.equal(scenario.marketPriceResidual, 0.55 - scenario.benchmarkPrice);
  }
  assert.equal(ORACLE3_WANG_SENSITIVITY.sigCalibrated, false);
});

test('missing covariates are omitted and invalid inputs fail closed', () => {
  assert.equal(oracle3HierarchicalLambda({marketPrice: 0.5}), 0.259);
  assert.equal(oracle3TimeVaryingLambda({marketPrice: 0.5, elapsedFraction: 0}), 0.2531);
  assert.throws(() => oracle3HierarchicalLambda({marketPrice: 1.1}), /marketPrice/);
  assert.throws(() => oracle3HierarchicalLambda({marketPrice: 0.5, volume: -1}), /volume/);
  assert.throws(() => oracle3HierarchicalLambda({marketPrice: 0.5, spread: 1.1}), /spread/);
  assert.throws(() => evaluateOracle3WangSensitivity({independentPhysicalProbability: 0, marketPrice: 0.5}), /independentPhysicalProbability/);
});

