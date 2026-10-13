import {amount, instant, money, price, SCALE, TICK} from '../paper/fixed.mjs';

export const OUTCOME_BASKET_REPLAY_V1 = Object.freeze({
  latenciesMs: [0, 250, 1000, 3000],
  fees: '0.000000',
  payoutPerWinningYes: '1.000000',
  maxBookAgeMs: 5000,
  maxQuoteDelayMs: 5000,
  maxOutcomeBookSkewMs: 1000,
  decisionSpacingMs: 250,
  depthFractionBps: 10000,
  oneTickStress: '0.005000',
});

const HASH = /^[a-f0-9]{64}$/;

function validateBasket(basket) {
  if (!basket || typeof basket.basketId !== 'string' || !basket.basketId
    || basket.payoutPerWinningYes !== '1.000000' || !Array.isArray(basket.outcomes)
    || basket.outcomes.length < 2) throw Error('OUTCOME_BASKET_MANIFEST');
  const proof = basket.settlementProof;
  if (!proof || proof.status !== 'settlement_verified' || proof.mutuallyExclusive !== true
    || proof.exhaustive !== true || proof.payoutPerWinningYes !== '1.000000'
    || typeof proof.sourceUrl !== 'string' || !/^https:\/\//.test(proof.sourceUrl)
    || !HASH.test(proof.sourceSha256 ?? '') || !HASH.test(proof.rulesSha256 ?? '')
    || !Array.isArray(proof.coveredOutcomeIds)) throw Error('OUTCOME_BASKET_SETTLEMENT_PROOF');
  const ids = new Set(), markets = new Set();
  for (const outcome of basket.outcomes) {
    if (!outcome || typeof outcome.outcomeId !== 'string' || !outcome.outcomeId
      || typeof outcome.marketId !== 'string' || !outcome.marketId
      || typeof outcome.exchangeId !== 'string' || !outcome.exchangeId
      || !HASH.test(outcome.contractRootSha256 ?? '')) throw Error('OUTCOME_BASKET_OUTCOME');
    if (ids.has(outcome.outcomeId) || markets.has(outcome.marketId)) throw Error('OUTCOME_BASKET_DUPLICATE');
    ids.add(outcome.outcomeId); markets.add(outcome.marketId);
  }
  const covered = proof.coveredOutcomeIds;
  if (covered.length !== ids.size || new Set(covered).size !== ids.size
    || covered.some(id => !ids.has(id))) throw Error('OUTCOME_BASKET_INCOMPLETE_PROOF');
  return basket;
}

function normalizeBook(book, outcome, options) {
  if (!book || book.marketId !== outcome.marketId || book.exchangeId !== outcome.exchangeId
    || typeof book.sequence !== 'string' || !/^(0|[1-9]\d*)$/.test(book.sequence)) throw Error('OUTCOME_BOOK_IDENTITY');
  const availableAt = instant(book.availableAt), sourceAt = instant(book.sourceAt);
  if (sourceAt > availableAt || availableAt - sourceAt > options.maxBookAgeMs) throw Error('OUTCOME_BOOK_STALE');
  if (book.nextExpiryAt != null && instant(book.nextExpiryAt) <= availableAt) throw Error('OUTCOME_BOOK_EXPIRED');
  if (!Array.isArray(book.asks) || !book.asks.length || book.asks.length > 20) throw Error('OUTCOME_BOOK_ASKS');
  let previous = null;
  const asks = book.asks.map(level => {
    const p = price(level.price);
    if (!Number.isSafeInteger(level.quantity) || level.quantity < 1 || (previous !== null && p <= previous)) throw Error('OUTCOME_BOOK_ASK_LEVEL');
    previous = p;
    return {price: p, quantity: BigInt(level.quantity)};
  });
  return {marketId: book.marketId, exchangeId: book.exchangeId, availableAt, sourceAt,
    nextExpiryAt: book.nextExpiryAt == null ? null : instant(book.nextExpiryAt), sequence: BigInt(book.sequence), asks};
}

function sweep(asks, quantity) {
  let remaining = quantity, cost = 0n;
  for (const level of asks) {
    const used = remaining < level.quantity ? remaining : level.quantity;
    cost += used * level.price;
    remaining -= used;
    if (!remaining) break;
  }
  if (remaining) throw Error('OUTCOME_BOOK_DEPTH');
  return cost;
}

function lowerBound(rows, target) {
  let lo = 0, hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (rows[mid].availableAt < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

function breaksBetween(breaks, outcome, from, through) {
  return breaks.some(item => (item.marketId == null || item.marketId === outcome.marketId)
    && instant(item.at) > from && instant(item.at) <= through);
}

function scenarioResult(latencyMs) {
  return {latencyMs, consideredDecisionSnapshots: 0, completeCofreshDecisions: 0,
    censoredMissingDecisionBook: 0, censoredStaleDecisionBook: 0, censoredDecisionSkew: 0,
    censoredLatencyBook: 0, censoredLatencyDelay: 0, censoredLatencyStale: 0,
    censoredLatencySkew: 0, censoredBreak: 0, visibleDepthCandidates: 0,
    noQuoteDepth: 0, packageOpportunities: 0, positiveAtObservedAsks: 0,
    positiveAfterOneTickStress: 0, quantityAcrossCandidates: '0',
    netSurplusAcrossCandidates: '0.000000', stressedNetSurplusAcrossCandidates: '0.000000',
    maximumOutcomeCount: 0};
}

// This is a quote-sweep counterfactual only. It never creates fills, orders,
// portfolio entries, or account credit. A supplied settlement manifest is an
// external proof assertion; this function validates its shape and exact set
// coverage, not the truth of the source documents themselves.
export function analyzeOutcomeBasketV1({basket, observations, breaks = [], options = {}}) {
  validateBasket(basket);
  if (!Array.isArray(observations) || !Array.isArray(breaks)) throw Error('OUTCOME_BASKET_INPUT');
  const cfg = {
    latenciesMs: options.latenciesMs ?? OUTCOME_BASKET_REPLAY_V1.latenciesMs,
    maxBookAgeMs: options.maxBookAgeMs ?? OUTCOME_BASKET_REPLAY_V1.maxBookAgeMs,
    maxQuoteDelayMs: options.maxQuoteDelayMs ?? OUTCOME_BASKET_REPLAY_V1.maxQuoteDelayMs,
    maxOutcomeBookSkewMs: options.maxOutcomeBookSkewMs ?? OUTCOME_BASKET_REPLAY_V1.maxOutcomeBookSkewMs,
    decisionSpacingMs: options.decisionSpacingMs ?? OUTCOME_BASKET_REPLAY_V1.decisionSpacingMs,
    depthFractionBps: options.depthFractionBps ?? OUTCOME_BASKET_REPLAY_V1.depthFractionBps,
  };
  for (const [name, value] of Object.entries({maxBookAgeMs: cfg.maxBookAgeMs, maxQuoteDelayMs: cfg.maxQuoteDelayMs,
    maxOutcomeBookSkewMs: cfg.maxOutcomeBookSkewMs, decisionSpacingMs: cfg.decisionSpacingMs})) {
    if (!Number.isSafeInteger(value) || value < 0) throw Error(`OUTCOME_${name.toUpperCase()}`);
  }
  if (!Array.isArray(cfg.latenciesMs) || !cfg.latenciesMs.length
    || cfg.latenciesMs.some(ms => !Number.isSafeInteger(ms) || ms < 0)
    || new Set(cfg.latenciesMs).size !== cfg.latenciesMs.length) throw Error('OUTCOME_LATENCIES');
  if (!Number.isSafeInteger(cfg.depthFractionBps) || cfg.depthFractionBps < 1 || cfg.depthFractionBps > 10000) throw Error('OUTCOME_DEPTH_FRACTION');

  const memberByMarket = new Map(basket.outcomes.map(outcome => [outcome.marketId, outcome]));
  const streams = new Map(basket.outcomes.map(outcome => [outcome.marketId, []]));
  for (const raw of observations) {
    const outcome = memberByMarket.get(raw?.marketId);
    if (!outcome) continue;
    const book = normalizeBook(raw, outcome, cfg), rows = streams.get(outcome.marketId);
    const previous = rows.at(-1);
    if (previous && (book.availableAt < previous.availableAt
      || book.availableAt === previous.availableAt && book.sequence <= previous.sequence)) throw Error('OUTCOME_INPUT_ORDER');
    rows.push(book);
  }
  const decisionTimes = [...new Set([...streams.values()].flatMap(rows => rows.map(row => row.availableAt)))].sort((a, b) => a - b);
  const results = cfg.latenciesMs.map(scenarioResult), resultByLatency = new Map(results.map(x => [x.latencyMs, x]));
  let lastDecision = -Infinity;
  for (const decisionAt of decisionTimes) {
    if (decisionAt - lastDecision < cfg.decisionSpacingMs) continue;
    lastDecision = decisionAt;
    const decisionBooks = [];
    let missing = false, stale = false;
    for (const outcome of basket.outcomes) {
      const rows = streams.get(outcome.marketId), index = lowerBound(rows, decisionAt + 1) - 1, book = rows[index];
      if (!book) { missing = true; break; }
      if (decisionAt - book.availableAt > cfg.maxBookAgeMs || book.sourceAt > decisionAt
        || decisionAt - book.sourceAt > cfg.maxBookAgeMs || book.nextExpiryAt !== null && book.nextExpiryAt <= decisionAt) { stale = true; break; }
      decisionBooks.push(book);
    }
    if (missing || stale) {
      for (const result of results) { result.consideredDecisionSnapshots++; if (missing) result.censoredMissingDecisionBook++; else result.censoredStaleDecisionBook++; }
      continue;
    }
    const decisionSkew = Math.max(...decisionBooks.map(x => x.availableAt)) - Math.min(...decisionBooks.map(x => x.availableAt));
    if (decisionSkew > cfg.maxOutcomeBookSkewMs) {
      for (const result of results) { result.consideredDecisionSnapshots++; result.censoredDecisionSkew++; }
      continue;
    }
    for (const latencyMs of cfg.latenciesMs) {
      const result = resultByLatency.get(latencyMs);
      result.consideredDecisionSnapshots++;
      result.completeCofreshDecisions++;
      const target = decisionAt + latencyMs, selected = [];
      let reason = null;
      for (const outcome of basket.outcomes) {
        const rows = streams.get(outcome.marketId);
        const book = latencyMs === 0
          ? rows[lowerBound(rows, decisionAt + 1) - 1]
          : rows[lowerBound(rows, target)];
        if (!book) { reason = 'book'; break; }
        if (book.availableAt - target > cfg.maxQuoteDelayMs) { reason = 'delay'; break; }
        if (book.sourceAt > book.availableAt || book.availableAt - book.sourceAt > cfg.maxBookAgeMs
          || book.availableAt - decisionAt > latencyMs + cfg.maxQuoteDelayMs
          || book.nextExpiryAt !== null && book.nextExpiryAt <= book.availableAt) { reason = 'stale'; break; }
        selected.push(book);
      }
      if (reason) {
        if (reason === 'book') result.censoredLatencyBook++;
        else if (reason === 'delay') result.censoredLatencyDelay++;
        else result.censoredLatencyStale++;
        continue;
      }
      const quoteTimes = selected.map(x => x.availableAt), through = Math.max(...quoteTimes);
      if (through - Math.min(...quoteTimes) > cfg.maxOutcomeBookSkewMs) { result.censoredLatencySkew++; continue; }
      if (basket.outcomes.some(outcome => breaksBetween(breaks, outcome, decisionAt, through))) { result.censoredBreak++; continue; }

      const visible = selected.map(book => book.asks.reduce((n, level) => n + level.quantity, 0n));
      const quantity = visible.reduce((a, b) => a < b ? a : b) * BigInt(cfg.depthFractionBps) / 10000n;
      if (quantity < 1n) { result.noQuoteDepth++; continue; }
      const cost = selected.reduce((n, book) => n + sweep(book.asks, quantity), 0n);
      const stressedCost = cost + quantity * BigInt(basket.outcomes.length) * TICK;
      const net = quantity * SCALE - cost, stressedNet = quantity * SCALE - stressedCost;
      result.visibleDepthCandidates++;
      result.packageOpportunities++;
      if (net > 0n) result.positiveAtObservedAsks++;
      if (stressedNet > 0n) result.positiveAfterOneTickStress++;
      result.quantityAcrossCandidates = (BigInt(result.quantityAcrossCandidates) + quantity).toString();
      result.netSurplusAcrossCandidates = money(amount(result.netSurplusAcrossCandidates) + net);
      result.stressedNetSurplusAcrossCandidates = money(amount(result.stressedNetSurplusAcrossCandidates) + stressedNet);
      result.maximumOutcomeCount = Math.max(result.maximumOutcomeCount, basket.outcomes.length);
    }
  }
  return {mode: 'settlement_verified_exhaustive_yes_basket_quote_replay_v1', basketId: basket.basketId,
    settlementProof: {sourceUrl: basket.settlementProof.sourceUrl, sourceSha256: basket.settlementProof.sourceSha256,
      rulesSha256: basket.settlementProof.rulesSha256, coveredOutcomeCount: basket.settlementProof.coveredOutcomeIds.length,
      manifestComplete: true, externalProofAuthenticityIndependentlyVerifiedByCode: false},
    payoutPerWinningYes: '1.000000', fees: '0.000000', latenciesMs: cfg.latenciesMs,
    depthFractionBps: cfg.depthFractionBps, oneTickStressPerOutcome: money(TICK),
    scenarios: results, actualFills: 0, actualProfit: null, ordersSubmitted: 0,
    executionAuthority: false, profitabilityEstablished: false,
    limitation: 'Displayed-ask sweeps are counterfactuals, not fills; settlement proof must be independently established before this analyzer is meaningful.'};
}

export function emptyOutcomeBasketScenarioV1(latencyMs, reason = 'NO_SETTLEMENT_VERIFIED_COMPLETE_BASKETS') {
  if (!Number.isSafeInteger(latencyMs) || latencyMs < 0 || typeof reason !== 'string' || !reason) throw Error('OUTCOME_EMPTY_SCENARIO');
  return {...scenarioResult(latencyMs), reason};
}

