import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';

const SQRT_TWO = Math.SQRT2;
const PROBIT_A = [
  -39.69683028665376, 220.9460984245205, -275.9285104469687,
  138.357751867269, -30.66479806614716, 2.506628277459239,
];
const PROBIT_B = [
  -54.47609879822406, 161.5858368580409, -155.6989798598866,
  66.80131188771972, -13.28068155288572,
];
const PROBIT_C = [
  -0.007784894002430293, -0.3223964580411365, -2.400758277161838,
  -2.549732539343734, 4.374664141464968, 2.938163982698783,
];
const PROBIT_D = [
  0.007784695709041462, 0.3224671290700398, 2.445134137142996,
  3.754408661907416,
];
const PROBIT_TAIL = 0.02425;
const DEPTH_EPSILON = 1e-9;

function finiteNumber(value, name) {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    throw new TypeError(`${name} must be a finite number`);
  }
  return value;
}

function probability(value, name, {openInterval = false} = {}) {
  finiteNumber(value, name);
  const invalid = openInterval ? value <= 0 || value >= 1 : value < 0 || value > 1;
  if (invalid) {
    const range = openInterval ? '(0, 1)' : '[0, 1]';
    throw new RangeError(`${name} must be in ${range}`);
  }
  return value;
}

function horner(coefficients, x) {
  let value = coefficients.at(-1);
  for (let index = coefficients.length - 2; index >= 0; index -= 1) {
    value = coefficients[index] + x * value;
  }
  return value;
}

function standardNormalCdf(x) {
  if (x === 0) return 0.5;
  const z = Math.abs(x) / SQRT_TWO;
  const t = 1 / (1 + 0.5 * z);
  const erfc = t * Math.exp(-z * z - 1.26551223 + t * horner([
    1.00002368, 0.37409196, 0.09678418, -0.18628806,
    0.27886807, -1.13520398, 1.48851587, -0.82215223, 0.17087277,
  ], t));
  return x < 0 ? 0.5 * erfc : 1 - 0.5 * erfc;
}

function inverseStandardNormal(p) {
  probability(p, 'probability', {openInterval: true});
  let q;
  let r;
  if (p < PROBIT_TAIL) {
    q = Math.sqrt(-2 * Math.log(p));
    return (((((PROBIT_C[0] * q + PROBIT_C[1]) * q + PROBIT_C[2]) * q + PROBIT_C[3]) * q + PROBIT_C[4]) * q + PROBIT_C[5]) /
      ((((PROBIT_D[0] * q + PROBIT_D[1]) * q + PROBIT_D[2]) * q + PROBIT_D[3]) * q + 1);
  }
  if (p > 1 - PROBIT_TAIL) {
    q = Math.sqrt(-2 * Math.log1p(-p));
    return -(((((PROBIT_C[0] * q + PROBIT_C[1]) * q + PROBIT_C[2]) * q + PROBIT_C[3]) * q + PROBIT_C[4]) * q + PROBIT_C[5]) /
      ((((PROBIT_D[0] * q + PROBIT_D[1]) * q + PROBIT_D[2]) * q + PROBIT_D[3]) * q + 1);
  }
  q = p - 0.5;
  r = q * q;
  return (((((PROBIT_A[0] * r + PROBIT_A[1]) * r + PROBIT_A[2]) * r + PROBIT_A[3]) * r + PROBIT_A[4]) * r + PROBIT_A[5]) * q /
    (((((PROBIT_B[0] * r + PROBIT_B[1]) * r + PROBIT_B[2]) * r + PROBIT_B[3]) * r + PROBIT_B[4]) * r + 1);
}

/**
 * Map a physical event probability to a risk-adjusted binary price.
 * `lambda` is mandatory and must be an explicitly selected sensitivity input;
 * this helper has no SIG default or fitted venue parameter.
 * Mathematical reference: https://papers.ssrn.com/sol3/papers.cfm?abstract_id=6468338
 */
export function wangTransform(physicalProbability, lambda) {
  probability(physicalProbability, 'physicalProbability', {openInterval: true});
  finiteNumber(lambda, 'lambda');
  return standardNormalCdf(inverseStandardNormal(physicalProbability) + lambda);
}

function optionalProbability(value, name, options) {
  return value == null ? null : probability(value, name, options);
}

function inspectAskDepth(asks, targetQuantity) {
  if (!Array.isArray(asks) || asks.length === 0) return null;
  const levels = asks.map((level, index) => {
    if (!level || typeof level !== 'object') throw new TypeError(`asks[${index}] must be an object`);
    const price = probability(level.price, `asks[${index}].price`);
    const quantity = finiteNumber(level.quantity, `asks[${index}].quantity`);
    if (quantity <= 0) throw new RangeError(`asks[${index}].quantity must be positive`);
    return {price, quantity};
  });
  for (let index = 1; index < levels.length; index += 1) {
    if (levels[index].price < levels[index - 1].price) {
      throw new RangeError('asks must be ordered from lowest to highest price');
    }
  }
  finiteNumber(targetQuantity, 'targetQuantity');
  if (targetQuantity <= 0) throw new RangeError('targetQuantity must be positive');

  let remaining = targetQuantity;
  let depthBasedAskCost = 0;
  for (const level of levels) {
    const used = Math.min(remaining, level.quantity);
    depthBasedAskCost += used * level.price;
    remaining -= used;
    if (remaining <= DEPTH_EPSILON) break;
  }
  if (remaining > DEPTH_EPSILON) return null;
  return {depthBasedAskCost, askVwap: depthBasedAskCost / targetQuantity};
}

/**
 * Read-only binary-value diagnostic. It returns no order and records no fill.
 * The reference forecast and the payout-edge forecast are intentionally separate
 * fields; a Wang price residual is not a substitute for executable expected value.
 */
export function evaluateBinaryValue({
  referencePhysicalProbability = null,
  independentPhysicalProbability = null,
  lambda = null,
  marketMid = null,
  asks = null,
  targetQuantity = null,
  quoteReceivedAtMs = null,
  decisionAtMs = null,
  maxQuoteAgeMs = null,
} = {}) {
  const referenceProbability = optionalProbability(referencePhysicalProbability, 'referencePhysicalProbability', {openInterval: true});
  const payoutProbability = optionalProbability(independentPhysicalProbability, 'independentPhysicalProbability');
  const observedMid = optionalProbability(marketMid, 'marketMid');
  const explicitLambda = lambda == null ? null : finiteNumber(lambda, 'lambda');

  let wangBenchmarkPrice = null;
  let marketPriceResidual = null;
  if (referenceProbability !== null && explicitLambda !== null) {
    wangBenchmarkPrice = wangTransform(referenceProbability, explicitLambda);
    if (observedMid !== null) marketPriceResidual = observedMid - wangBenchmarkPrice;
  }

  const payoutEdge = {
    independentPhysicalProbability: payoutProbability,
    observedAskVwap: null,
    observedDepthCost: null,
    targetQuantity: targetQuantity ?? null,
    quoteAgeMs: null,
    expectedGrossEdgePerShare: null,
    expectedGrossEdgeTotal: null,
    zeroFees: true,
    candidateScreenPasses: false,
    reason: null,
  };

  if (payoutProbability === null) {
    payoutEdge.reason = 'NO_INDEPENDENT_FORECAST';
  } else if (asks == null || targetQuantity == null) {
    payoutEdge.reason = 'EXECUTABLE_ASK_DEPTH_REQUIRED';
  } else if ([quoteReceivedAtMs, decisionAtMs, maxQuoteAgeMs].some(value => value == null)) {
    payoutEdge.reason = 'QUOTE_FRESHNESS_METADATA_REQUIRED';
  } else {
    finiteNumber(quoteReceivedAtMs, 'quoteReceivedAtMs');
    finiteNumber(decisionAtMs, 'decisionAtMs');
    finiteNumber(maxQuoteAgeMs, 'maxQuoteAgeMs');
    if (maxQuoteAgeMs < 0) throw new RangeError('maxQuoteAgeMs must be nonnegative');
    const quoteAgeMs = decisionAtMs - quoteReceivedAtMs;
    payoutEdge.quoteAgeMs = quoteAgeMs;
    if (quoteAgeMs < 0) {
      payoutEdge.reason = 'QUOTE_RECEIPT_AFTER_DECISION';
    } else if (quoteAgeMs > maxQuoteAgeMs) {
      payoutEdge.reason = 'STALE_QUOTE';
    } else {
      const depth = inspectAskDepth(asks, targetQuantity);
      if (depth === null) {
        payoutEdge.reason = 'INSUFFICIENT_OBSERVED_ASK_DEPTH';
      } else {
        const expectedGrossEdgePerShare = payoutProbability - depth.askVwap;
        const expectedGrossEdgeTotal = targetQuantity * payoutProbability - depth.depthBasedAskCost;
        payoutEdge.observedAskVwap = depth.askVwap;
        payoutEdge.observedDepthCost = depth.depthBasedAskCost;
        payoutEdge.expectedGrossEdgePerShare = expectedGrossEdgePerShare;
        payoutEdge.expectedGrossEdgeTotal = expectedGrossEdgeTotal;
        payoutEdge.candidateScreenPasses = expectedGrossEdgeTotal > 0;
        payoutEdge.reason = payoutEdge.candidateScreenPasses ? 'POSITIVE_EDGE_SCREEN_ONLY' : 'NON_POSITIVE_EDGE';
      }
    }
  }

  return {
    researchOnly: true,
    orderCreated: false,
    fillRecorded: false,
    marketDistortion: {
      referencePhysicalProbability: referenceProbability,
      lambda: explicitLambda,
      wangBenchmarkPrice,
      observedMarketMid: observedMid,
      marketPriceResidual,
      reason: wangBenchmarkPrice === null ? 'REFERENCE_FORECAST_AND_EXPLICIT_LAMBDA_REQUIRED' : null,
    },
    payoutEdge,
  };
}

export function summarizeMarketMetadata(raw) {
  const markets = Array.isArray(raw) ? raw : raw?.markets;
  if (!Array.isArray(markets)) throw new TypeError('market metadata must be an array or contain a markets array');
  const statuses = {};
  let settledLabelCount = 0;
  for (const market of markets) {
    if (!market || typeof market !== 'object') throw new TypeError('every market metadata row must be an object');
    const status = typeof market.status === 'string' ? market.status : 'missing';
    statuses[status] = (statuses[status] ?? 0) + 1;
    if (market.settledOn != null || market.settledWith != null || /settled|resolved/i.test(status)) settledLabelCount += 1;
  }
  const containsTimestampedPriceHistory = markets.some(market =>
    Object.values(market).some(value => Array.isArray(value) && value.some(point => {
      if (!point || typeof point !== 'object') return false;
      const timestamp = point.timestamp ?? point.timestampMs ?? point.observedAt ?? point.receivedAt ?? point.ts;
      const price = point.price ?? point.mid ?? point.marketPrice ?? point.yesPrice ?? point.yes_price;
      const validTimestamp = typeof timestamp === 'number'
        ? Number.isFinite(timestamp)
        : typeof timestamp === 'string' && Number.isFinite(Date.parse(timestamp));
      return validTimestamp && typeof price === 'number' && Number.isFinite(price);
    })),
  );
  return {
    marketMetadataRows: markets.length,
    statuses,
    rowsWithSettlementLabel: settledLabelCount,
    metadataOnly: true,
    containsTimestampedPriceHistory,
  };
}

function inventoryReport() {
  const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
  const inputPath = path.join(projectRoot, 'data', 'markets.json');
  const bytes = readFileSync(inputPath);
  const raw = JSON.parse(bytes.toString('utf8'));
  return {
    report: 'open-source-adaptation-20261014',
    analysisMode: 'read-only metadata inventory',
    input: {
      path: 'data/markets.json',
      sha256: createHash('sha256').update(bytes).digest('hex'),
      ...summarizeMarketMetadata(raw),
    },
    calibration: {
      sigLambdaEstimated: false,
      reason: 'The inspected market metadata has no resolved labels or timestamped price history; it cannot fit a SIG-specific price-distortion parameter.',
    },
    trading: {ordersCreated: 0, fillsSimulated: 0, feesPerShare: 0},
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] !== '--inventory') {
    console.error('Usage: node research/open-source-adaptation-v1/wang-value.mjs --inventory');
    process.exitCode = 2;
  } else {
    try {
      process.stdout.write(`${JSON.stringify(inventoryReport(), null, 2)}\n`);
    } catch (error) {
      console.error(`read-only inventory failed: ${error.message}`);
      process.exitCode = 1;
    }
  }
}

