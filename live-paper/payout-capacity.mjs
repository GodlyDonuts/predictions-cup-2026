import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Offline conditional-payout capacity analysis. No network, credentials or orders.
const SCALE = 1_000_000n;
const fail = code => { throw new Error(code); };
const hash = value => createHash('sha256').update(JSON.stringify(value)).digest('hex');
function money(value) {
  if (typeof value !== 'string' || !/^(0|[1-9]\d{0,12})(\.\d{1,6})?$/.test(value)) fail('INVALID_DECIMAL');
  const [whole, fraction = ''] = value.split('.');
  return BigInt(whole) * SCALE + BigInt(fraction.padEnd(6, '0'));
}
const fmt = value => `${value < 0n ? '-' : ''}${(value < 0n ? -value : value) / SCALE}.${((value < 0n ? -value : value) % SCALE).toString().padStart(6, '0')}`;
function integer(value, max = Number.MAX_SAFE_INTEGER) {
  if (!Number.isSafeInteger(value) || value < 0 || value > max) fail('INVALID_INTEGER');
  return value;
}
function token(value) {
  if (typeof value !== 'string' || !/^[A-Za-z0-9:_./-]{1,200}$/.test(value)) fail('INVALID_ID');
  return value;
}
function sha(value) { if (typeof value !== 'string' || !/^[a-f0-9]{64}$/.test(value)) fail('INVALID_HASH'); return value; }
function clock(value) {
  if (typeof value !== 'string' || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/.test(value)) fail('INVALID_CLOCK');
  const n = Date.parse(value);
  if (!Number.isFinite(n) || new Date(n).toISOString() !== value) fail('INVALID_CLOCK');
  return n;
}
const min = (...xs) => xs.reduce((a, b) => a < b ? a : b);

/**
 * Evaluate equal-quantity purchases of two explicitly reviewed native outcomes.
 * Scenario completeness is a caller assertion, never inferred or proved here.
 * Returns hypothetical costs, NOT fills, executable orders or guaranteed profit.
 */
export function evaluatePayoutCapacity(input) {
  const { now, policy: p, review: r, books } = input ?? {};
  const at = clock(now);
  if (!p || !r || !Array.isArray(books) || books.length !== 2) fail('INVALID_INPUT');
  integer(p.maxAgeMs); integer(p.maxSkewMs); integer(p.maxReviewAgeMs);
  const maxQuantity = BigInt(integer(p.maxQuantity));
  const budget = money(p.maxCost), partialCap = money(p.maxUnmatchedCost);
  const edgeFloor = money(p.minMarginalEdge), profitFloor = money(p.minTotalProfit);
  if (r.kind !== 'reviewed_conditional_scenarios' || r.completeUnderAssumptions !== true
      || !Array.isArray(r.assumptions) || !r.assumptions.length
      || r.assumptions.some(x => typeof x !== 'string' || !x.trim() || x.length > 2000)
      || !Array.isArray(r.legs) || r.legs.length !== 2
      || !Array.isArray(r.scenarios) || !r.scenarios.length || r.scenarios.length > 100) fail('INVALID_REVIEW');
  token(r.id); token(r.tournamentId); sha(r.evidenceHash);
  const reviewed = clock(r.reviewedAt), expires = clock(r.expiresAt);
  if (reviewed > at || at >= expires || expires <= reviewed || at - reviewed > p.maxReviewAgeMs) fail('EXPIRED_REVIEW');
  const identities = r.legs.map(l => {
    if (!l || !['YES', 'NO'].includes(l.outcome)) fail('INVALID_LEG');
    token(l.marketId); token(l.exchangeId); sha(l.rulesHash);
    return `${r.tournamentId}:${l.exchangeId}:${l.outcome}`;
  });
  if (new Set(identities).size !== 2) fail('DUPLICATE_LEG');
  if (r.legs[0].exchangeId === r.legs[1].exchangeId && r.legs[0].marketId !== r.legs[1].marketId) fail('EXCHANGE_MARKET_CONFLICT');
  const scenarioIds = new Set();
  const scenarios = r.scenarios.map(s => {
    token(s.id);
    if (scenarioIds.has(s.id) || !Array.isArray(s.payouts) || s.payouts.length !== 2
        || s.payouts.some(v => v !== 0 && v !== 1)) fail('INVALID_SCENARIO');
    scenarioIds.add(s.id);
    if (r.legs[0].exchangeId === r.legs[1].exchangeId && s.payouts[0] + s.payouts[1] > 1) fail('CONTRADICTORY_NATIVE_PAYOUT');
    return { id: s.id, payout: BigInt(s.payouts[0] + s.payouts[1]) * SCALE };
  });
  const floor = min(...scenarios.map(s => s.payout));
  const receipts = [], starts = [];
  const legs = books.map((b, i) => {
    const l = r.legs[i];
    if (!b || b.tournamentId !== r.tournamentId || b.marketId !== l.marketId
        || b.exchangeId !== l.exchangeId || b.outcome !== l.outcome
        || b.rulesHash !== l.rulesHash || b.nativeOutcome !== true || b.status !== 'active') fail('BOOK_SCOPE');
    sha(b.sourceHash); token(b.version);
    const started = clock(b.requestStartedAt), received = clock(b.receivedAt);
    if (started > received || received > at || at - started > p.maxAgeMs) fail('STALE_BOOK');
    starts.push(started); receipts.push(received);
    if (!Array.isArray(b.asks) || !b.asks.length || b.asks.length > 10000) fail('INVALID_DEPTH');
    const tick = money(b.tickSize), slip = money(b.adversePricePerShare);
    const fee = money(b.feePerShare), fixedFee = money(b.fixedFee);
    if (tick <= 0n || tick > SCALE || SCALE % tick !== 0n || slip % tick !== 0n) fail('INVALID_TICK');
    let previous = -1n;
    const asks = b.asks.map(level => {
      const price = money(level.price), quantity = BigInt(integer(level.quantity));
      if (!quantity || price <= 0n || price >= SCALE || price % tick !== 0n || price <= previous) fail('INVALID_DEPTH');
      previous = price;
      // An adverse-price sensitivity beyond the binary domain is not a valid cost model.
      if (price + slip > SCALE) fail('INVALID_ADVERSE_PRICE');
      return { price, quantity, effective: price + slip + fee };
    });
    return { asks, fixedFee };
  });
  // Bound the entire acquisition window, not only last-response timestamps.
  if (Math.max(...receipts) - Math.min(...starts) > p.maxSkewMs) fail('SKEWED_BOOKS');
  const fixed = legs[0].fixedFee + legs[1].fixedFee;
  let costs = legs.map(l => l.fixedFee), quantity = 0n;
  let indexes = [0, 0], remaining = legs.map(l => l.asks[0].quantity);
  let stopReason = 'DEPTH_EXHAUSTED';
  const segments = [];
  while (indexes.every((j, i) => j < legs[i].asks.length) && quantity < maxQuantity) {
    const levels = indexes.map((j, i) => legs[i].asks[j]);
    const unitCost = levels[0].effective + levels[1].effective;
    if (floor - unitCost < edgeFloor || floor <= unitCost) { stopReason = 'MARGINAL_EDGE'; break; }
    if (costs[0] + costs[1] > budget || costs.some(c => c > partialCap)) { stopReason = 'CAPITAL_LIMIT'; break; }
    let take = min(...remaining, maxQuantity - quantity,
      (budget - costs[0] - costs[1]) / unitCost,
      ...costs.map((c, i) => (partialCap - c) / levels[i].effective));
    if (take <= 0n) { stopReason = 'CAPITAL_LIMIT'; break; }
    levels.forEach((l, i) => { costs[i] += take * l.effective; remaining[i] -= take; });
    quantity += take;
    segments.push({ quantity: take.toString(), prices: levels.map(l => fmt(l.price)), adverseUnitCost: fmt(unitCost) });
    remaining.forEach((q, i) => {
      if (q === 0n) { indexes[i]++; remaining[i] = legs[i].asks[indexes[i]]?.quantity ?? 0n; }
    });
  }
  if (quantity === maxQuantity) stopReason = 'QUANTITY_LIMIT';
  const cost = quantity ? costs[0] + costs[1] : 0n;
  const profit = quantity * floor - cost;
  const accepted = quantity > 0n && profit >= profitFloor && profit > 0n;
  return {
    schemaVersion: 1, kind: 'conditional_payout_capacity', evaluatedAt: now,
    inputHash: hash(input), reviewId: r.id, evidenceHash: r.evidenceHash,
    sourceHashes: books.map(b => b.sourceHash), versions: books.map(b => b.version),
    assumptions: [...r.assumptions], scenarioCompletenessVerified: false,
    executionAuthority: false, ordersSubmitted: 0, networkRequests: 0, fills: null, realizedPnl: null,
    acceptedForResearch: accepted, reason: accepted ? 'CONDITIONAL_POSITIVE_CAPACITY' : quantity ? 'TOTAL_PROFIT' : stopReason,
    stopReason, matchedQuantity: quantity.toString(), acquisitionCost: fmt(cost),
    fixedFeesIncluded: fmt(quantity ? fixed : 0n), conditionalMinimumPayout: fmt(quantity * floor),
    conditionalMinimumProfit: fmt(profit), maximumSingleLegCostAtRisk: fmt(quantity ? (costs[0] > costs[1] ? costs[0] : costs[1]) : 0n),
    scenarioProfits: scenarios.map(s => ({ id: s.id, profit: fmt(quantity * s.payout - cost) })),
    segments,
    limitations: ['Conditional on caller-reviewed scenario completeness and unchanged rules.',
      'Native snapshot depth is not an atomic package, a queue model or a fill guarantee.',
      'Unmatched-leg bound assumes a worthless lone leg; it is not a live liquidation estimate.',
      'Fees must be explicit conservative inputs; unknown fees must not be entered as zero.',
      'Refund/override outcomes require separate scenarios or exclusion; no guaranteed arbitrage is asserted.']
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 3 || process.argv[2].startsWith('--')) fail('USAGE: node payout-capacity.mjs input.json');
    console.log(JSON.stringify(evaluatePayoutCapacity(JSON.parse(readFileSync(process.argv[2], 'utf8'))), null, 2));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
