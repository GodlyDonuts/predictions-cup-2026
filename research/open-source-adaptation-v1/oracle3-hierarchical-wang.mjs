import {wangTransform} from './wang-value.mjs';

// Clean-room sensitivity reproduction of Oracle3's Table 3/5 Wang models.
// Coefficients are cross-venue Polymarket estimates, not SIG calibration.
// This module intentionally returns benchmarks only: it cannot create orders,
// estimate executable edge, or authorize execution.
export const ORACLE3_WANG_SENSITIVITY = Object.freeze({
  name: 'oracle3-hierarchical-wang-sensitivity-v1',
  sourceVenue: 'Polymarket',
  sigCalibrated: false,
  source: 'Oracle3 fair_value.py, Tables 3 and 5; Yang (2026) working paper',
  static: Object.freeze({
    intercept: 0.2590,
    logVolume: -0.0716,
    logDurationHours: 0.1431,
    extremity: -0.4772,
    spread: 0.1273,
  }),
  timeVarying: Object.freeze({
    tau: -0.1555,
    tauSquared: 0.0744,
    intercept: 0.2531,
    logVolume: -0.0568,
    logDurationHours: 0.1085,
    extremity: -0.2902,
  }),
});

function finite(value, name) {
  if (typeof value !== 'number' || !Number.isFinite(value)) throw new TypeError(`${name} must be finite`);
  return value;
}

function probability(value, name) {
  finite(value, name);
  if (value < 0 || value > 1) throw new RangeError(`${name} must be in [0, 1]`);
  return value;
}

function optionalNonnegative(value, name) {
  if (value == null) return null;
  finite(value, name);
  if (value < 0) throw new RangeError(`${name} must be nonnegative`);
  return value;
}

/** Reproduce Oracle3's Table 3 feature-conditioned lambda. */
export function oracle3HierarchicalLambda({marketPrice, volume = null, durationHours = null, spread = null} = {}) {
  probability(marketPrice, 'marketPrice');
  const v = optionalNonnegative(volume, 'volume');
  const d = optionalNonnegative(durationHours, 'durationHours');
  const s = spread == null ? null : probability(spread, 'spread');
  let lambda = ORACLE3_WANG_SENSITIVITY.static.intercept
    + ORACLE3_WANG_SENSITIVITY.static.extremity * Math.abs(marketPrice - 0.5);
  if (v !== null) lambda += ORACLE3_WANG_SENSITIVITY.static.logVolume * Math.log1p(v);
  if (d !== null) lambda += ORACLE3_WANG_SENSITIVITY.static.logDurationHours * Math.log1p(d);
  if (s !== null) lambda += ORACLE3_WANG_SENSITIVITY.static.spread * s;
  return lambda;
}

/** Reproduce Oracle3's Table 5 lifecycle-conditioned lambda. */
export function oracle3TimeVaryingLambda({marketPrice, volume = null, durationHours = null, elapsedFraction} = {}) {
  probability(marketPrice, 'marketPrice');
  const v = optionalNonnegative(volume, 'volume');
  const d = optionalNonnegative(durationHours, 'durationHours');
  const tau = finite(elapsedFraction, 'elapsedFraction');
  if (tau < 0 || tau > 1) throw new RangeError('elapsedFraction must be in [0, 1]');
  const c = ORACLE3_WANG_SENSITIVITY.timeVarying;
  let lambda = c.intercept + c.tau * tau + c.tauSquared * tau * tau
    + c.extremity * Math.abs(marketPrice - 0.5);
  if (v !== null) lambda += c.logVolume * Math.log1p(v);
  if (d !== null) lambda += c.logDurationHours * Math.log1p(d);
  return lambda;
}

/**
 * Produce explicitly cross-venue price benchmarks from an independent forecast.
 * The reported price residual is not expected value, a fill, or a trade signal.
 */
export function evaluateOracle3WangSensitivity({
  independentPhysicalProbability,
  marketPrice,
  volume = null,
  durationHours = null,
  spread = null,
  elapsedFraction = null,
} = {}) {
  probability(independentPhysicalProbability, 'independentPhysicalProbability');
  probability(marketPrice, 'marketPrice');
  if (independentPhysicalProbability === 0 || independentPhysicalProbability === 1) {
    throw new RangeError('independentPhysicalProbability must be in (0, 1)');
  }
  const staticLambda = oracle3HierarchicalLambda({marketPrice, volume, durationHours, spread});
  const scenarios = [{
    model: 'oracle3_table3_static_hierarchical',
    lambda: staticLambda,
    benchmarkPrice: wangTransform(independentPhysicalProbability, staticLambda),
  }];
  if (elapsedFraction !== null) {
    const lifecycleLambda = oracle3TimeVaryingLambda({marketPrice, volume, durationHours, elapsedFraction});
    scenarios.push({
      model: 'oracle3_table5_time_varying_hierarchical',
      lambda: lifecycleLambda,
      benchmarkPrice: wangTransform(independentPhysicalProbability, lifecycleLambda),
    });
  }
  for (const scenario of scenarios) scenario.marketPriceResidual = marketPrice - scenario.benchmarkPrice;
  return {
    researchOnly: true,
    sourceVenue: ORACLE3_WANG_SENSITIVITY.sourceVenue,
    sigCalibrated: false,
    orderCreated: false,
    fillRecorded: false,
    executionAuthority: false,
    independentPhysicalProbability,
    marketPrice,
    scenarios,
    limitation: 'Cross-venue Polymarket sensitivity only; no SIG calibration, executable depth, expected payout edge, or profitability evidence.',
  };
}

