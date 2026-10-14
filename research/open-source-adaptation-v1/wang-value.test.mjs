import test from 'node:test';
import assert from 'node:assert/strict';
import {evaluateBinaryValue, summarizeMarketMetadata, wangTransform} from './wang-value.mjs';

test('Wang transform is identity at zero lambda and increases prices for positive lambda', () => {
  for (const p of [0.001, 0.05, 0.2, 0.5, 0.8, 0.95, 0.999]) {
    assert.ok(Math.abs(wangTransform(p, 0) - p) < 3e-7, `p=${p}`);
    assert.ok(wangTransform(p, 0.2) > p, `p=${p}`);
    assert.ok(wangTransform(p, -0.2) < p, `p=${p}`);
  }
  assert.ok(Math.abs(wangTransform(0.5, 0.2) - 0.579259709439103) < 2e-7);
  assert.ok(Math.abs(wangTransform(wangTransform(0.23, 0.2), -0.2) - 0.23) < 5e-7);
});

test('Wang lambda is mandatory and physical input must be interior', () => {
  assert.throws(() => wangTransform(0.5), /lambda must be a finite number/);
  assert.throws(() => wangTransform(0, 0.2), /physicalProbability must be in \(0, 1\)/);
  assert.throws(() => wangTransform(1, 0.2), /physicalProbability must be in \(0, 1\)/);
  assert.throws(() => wangTransform(0.5, Number.NaN), /lambda must be a finite number/);
});

test('market-price distortion residual and payout edge remain separate measurements', () => {
  const result = evaluateBinaryValue({
    referencePhysicalProbability: 0.5,
    independentPhysicalProbability: 0.4,
    lambda: 0.2,
    marketMid: 0.55,
    asks: [{price: 0.49, quantity: 1}],
    targetQuantity: 1,
    quoteReceivedAtMs: 10_000,
    decisionAtMs: 10_500,
    maxQuoteAgeMs: 1_000,
  });
  assert.ok(result.marketDistortion.marketPriceResidual < 0);
  assert.ok(result.payoutEdge.expectedGrossEdgePerShare < 0);
  assert.equal(result.payoutEdge.candidateScreenPasses, false);
  assert.equal(result.orderCreated, false);
  assert.equal(result.fillRecorded, false);
});

test('without an independent payout forecast there is never a value candidate', () => {
  const result = evaluateBinaryValue({
    referencePhysicalProbability: 0.55,
    lambda: 0.2,
    marketMid: 0.6,
    asks: [{price: 0.1, quantity: 100}],
    targetQuantity: 1,
    quoteReceivedAtMs: 1_000,
    decisionAtMs: 1_000,
    maxQuoteAgeMs: 1_000,
  });
  assert.equal(result.payoutEdge.reason, 'NO_INDEPENDENT_FORECAST');
  assert.equal(result.payoutEdge.candidateScreenPasses, false);
  assert.notEqual(result.marketDistortion.wangBenchmarkPrice, null);
});

test('positive edge uses observed ask depth, zero fees, and freshness as an eligibility gate', () => {
  const result = evaluateBinaryValue({
    independentPhysicalProbability: 0.7,
    asks: [{price: 0.4, quantity: 2}, {price: 0.5, quantity: 2}],
    targetQuantity: 3,
    quoteReceivedAtMs: 20_000,
    decisionAtMs: 21_000,
    maxQuoteAgeMs: 1_000,
  });
  assert.equal(result.payoutEdge.observedDepthCost, 1.3);
  assert.ok(Math.abs(result.payoutEdge.observedAskVwap - (1.3 / 3)) < 1e-12);
  assert.ok(Math.abs(result.payoutEdge.expectedGrossEdgeTotal - 0.8) < 1e-12);
  assert.equal(result.payoutEdge.zeroFees, true);
  assert.equal(result.payoutEdge.candidateScreenPasses, true);
});

test('stale, future, or depth-short quotes cannot pass the screen', () => {
  const base = {
    independentPhysicalProbability: 0.9,
    asks: [{price: 0.2, quantity: 1}],
    targetQuantity: 1,
    quoteReceivedAtMs: 1_000,
    decisionAtMs: 2_000,
    maxQuoteAgeMs: 500,
  };
  assert.equal(evaluateBinaryValue(base).payoutEdge.reason, 'STALE_QUOTE');
  assert.equal(evaluateBinaryValue({...base, quoteReceivedAtMs: 3_000}).payoutEdge.reason, 'QUOTE_RECEIPT_AFTER_DECISION');
  assert.equal(evaluateBinaryValue({...base, decisionAtMs: 1_500, asks: [{price: 0.2, quantity: 0.5}]}).payoutEdge.reason, 'INSUFFICIENT_OBSERVED_ASK_DEPTH');
});

test('metadata inventory reports settlement labels and timestamped prices separately', () => {
  const summary = summarizeMarketMetadata({markets: [
    {status: 'open'},
    {status: 'settled', settledWith: 'YES', history: []},
    {status: 'open', prices: [{timestamp: '2026-01-01T00:00:00Z', price: 0.4}]},
  ]});
  assert.deepEqual(summary.statuses, {open: 2, settled: 1});
  assert.equal(summary.rowsWithSettlementLabel, 1);
  assert.equal(summary.containsTimestampedPriceHistory, true);
});

test('empty arrays do not count as timestamped price history', () => {
  assert.equal(summarizeMarketMetadata([{status: 'open', prices: []}]).containsTimestampedPriceHistory, false);
});

