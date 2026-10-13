import {amount, money, price, SCALE} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';

export const COMPLEMENT_REPLAY_V1 = Object.freeze({
  fees: '0.000000',
  payoutPerNetPair: '1.000000',
  depthFractionBps: 100,
  maxBookAgeMs: 5000,
  decisionSpacingMs: 60000,
  maxFirstBookDelayMs: 10000,
  maxHedgeBookDelayMs: 15000,
  firstLegLatenciesMs: [0, 250, 1000, 3000],
  hedgeLatenciesMs: [0, 250, 1000, 3000],
});
export const SCREENED_COMPLEMENT_BOOK_V1 = 'screened_same_market_complement_book_v1';

function timeNs(value, code) {
  try { return instantNs(value); } catch { throw Error(code); }
}
function timeMs(ns) { return Number(ns / 1_000_000n); }
function validateLadder(levels, ascending, code) {
  if (!Array.isArray(levels) || !levels.length || levels.length > 20) throw Error(code);
  let prior = null;
  return levels.map(level => {
    const p = price(level.price);
    if (!Number.isSafeInteger(level.quantity) || level.quantity < 1
      || prior !== null && (ascending ? p <= prior : p >= prior)) throw Error(code);
    prior = p;
    return {price: p, quantity: BigInt(level.quantity)};
  });
}
function depthAtFraction(levels, fractionBps) {
  return levels.reduce((n, level) => n + level.quantity * BigInt(fractionBps) / 10000n, 0n);
}
function sweepCost(levels, quantity) {
  let remaining = quantity, notional = 0n;
  for (const level of levels) {
    const used = remaining < level.quantity ? remaining : level.quantity;
    notional += used * level.price;
    remaining -= used;
    if (!remaining) break;
  }
  if (remaining) throw Error('COMPLEMENT_DEPTH_SHORTFALL');
  return notional;
}

// YES and NO are the two sides of one exact binary contract. Captured books
// use YES prices: a NO ask is therefore the complement of a YES bid.
export function screenComplementBookV1(book, options = {}) {
  if (!book || typeof book.marketId !== 'string' || !book.marketId
    || typeof book.exchangeId !== 'string' || !book.exchangeId) throw Error('COMPLEMENT_SCOPE');
  const availableNs = timeNs(book.availableAt, 'COMPLEMENT_CLOCK');
  const sourceNs = timeNs(book.sourceAt, 'COMPLEMENT_CLOCK');
  const maxBookAgeMs = options.maxBookAgeMs ?? COMPLEMENT_REPLAY_V1.maxBookAgeMs;
  if (!Number.isSafeInteger(maxBookAgeMs) || maxBookAgeMs < 0 || sourceNs > availableNs
    || availableNs - sourceNs > BigInt(maxBookAgeMs) * 1_000_000n) throw Error('COMPLEMENT_STALE');
  const expiryNs = book.nextExpiryAt == null ? null : timeNs(book.nextExpiryAt, 'COMPLEMENT_EXPIRY');
  if (expiryNs !== null && expiryNs <= availableNs) throw Error('COMPLEMENT_EXPIRED');
  if (typeof book.sequence !== 'string' || !/^(0|[1-9]\d*)$/.test(book.sequence)) throw Error('COMPLEMENT_SEQUENCE');

  const bids = validateLadder(book.bids, false, 'COMPLEMENT_BIDS');
  const asks = validateLadder(book.asks, true, 'COMPLEMENT_ASKS');
  const bid = bids[0].price, ask = asks[0].price;
  if (bid >= ask) throw Error('COMPLEMENT_CROSSED');

  const noAsks = bids.map(level => ({price: SCALE - level.price, quantity: level.quantity}));
  const fractionBps = options.depthFractionBps ?? COMPLEMENT_REPLAY_V1.depthFractionBps;
  if (!Number.isSafeInteger(fractionBps) || fractionBps < 1 || fractionBps > 10000) throw Error('COMPLEMENT_DEPTH_FRACTION');
  const participatingYes = depthAtFraction(asks, fractionBps), participatingNo = depthAtFraction(noAsks, fractionBps);
  const participationPairs = participatingYes < participatingNo ? participatingYes : participatingNo;
  const participationCost = participationPairs
    ? sweepCost(asks, participationPairs) + sweepCost(noAsks, participationPairs) : null;
  const participationPnl = participationPairs
    ? participationPairs * SCALE - participationCost : null;

  return {
    mode: SCREENED_COMPLEMENT_BOOK_V1,
    marketId: book.marketId, exchangeId: book.exchangeId, availableAt: book.availableAt,
    sourceAt: book.sourceAt, nextExpiryAt: book.nextExpiryAt ?? null, sequence: book.sequence,
    bid: money(bid), ask: money(ask), bidSize: Number(bids[0].quantity), askSize: Number(asks[0].quantity),
    topSpread: money(ask - bid), topPairAcquisitionCost: money(ask + SCALE - bid),
    topPairNetIfBothFill: money(bid - ask),
    displayedTopPairDepth: String(bids[0].quantity < asks[0].quantity ? bids[0].quantity : asks[0].quantity),
    depthFractionBps: fractionBps,
    participationPairDepth: String(participationPairs),
    participationCostTotal: participationCost === null ? null : money(participationCost),
    participationNetTotal: participationPnl === null ? null : money(participationPnl),
    participationNetPerPair: participationPnl === null ? null : Number(participationPnl) / Number(participationPairs) / Number(SCALE),
  };
}

function validateScreenedBookV1(book, options) {
  if (!book || book.mode !== SCREENED_COMPLEMENT_BOOK_V1 || typeof book.marketId !== 'string' || !book.marketId
    || typeof book.exchangeId !== 'string' || !book.exchangeId) throw Error('COMPLEMENT_SCREENED_SCOPE');
  const availableNs = timeNs(book.availableAt, 'COMPLEMENT_CLOCK'), sourceNs = timeNs(book.sourceAt, 'COMPLEMENT_CLOCK');
  const maxAge = BigInt(options.maxBookAgeMs) * 1_000_000n;
  if (sourceNs > availableNs || availableNs - sourceNs > maxAge) throw Error('COMPLEMENT_STALE');
  if (book.nextExpiryAt != null && timeNs(book.nextExpiryAt, 'COMPLEMENT_EXPIRY') <= availableNs) throw Error('COMPLEMENT_EXPIRED');
  if (typeof book.sequence !== 'string' || !/^(0|[1-9]\d*)$/.test(book.sequence)) throw Error('COMPLEMENT_SEQUENCE');
  const bid = price(book.bid), ask = price(book.ask);
  if (bid >= ask || !Number.isSafeInteger(book.bidSize) || book.bidSize < 1
    || !Number.isSafeInteger(book.askSize) || book.askSize < 1) throw Error('COMPLEMENT_SCREENED_BOOK');
  const expectedTopCost = ask + SCALE - bid, expectedTopNet = bid - ask;
  if (book.topPairAcquisitionCost !== money(expectedTopCost) || book.topPairNetIfBothFill !== money(expectedTopNet)
    || book.displayedTopPairDepth !== String(Math.min(book.bidSize, book.askSize))) throw Error('COMPLEMENT_SCREENED_ATOMIC');
  if (book.depthFractionBps !== options.depthFractionBps || !/^(0|[1-9]\d*)$/.test(book.participationPairDepth)
    || (BigInt(book.participationPairDepth) === 0n) !== (book.participationNetTotal === null)
    || (BigInt(book.participationPairDepth) === 0n) !== (book.participationCostTotal === null)) throw Error('COMPLEMENT_SCREENED_DEPTH');
  if (book.participationNetTotal !== null && (typeof book.participationNetTotal !== 'string'
    || typeof book.participationCostTotal !== 'string' || !Number.isFinite(book.participationNetPerPair))) throw Error('COMPLEMENT_SCREENED_DEPTH');
  return book;
}

function upperBound(rows, targetNs) {
  let lo = 0, hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (rows[mid].availableNs <= targetNs) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}
function breakIn(breaks, startNs, endNs) {
  if (!breaks?.length) return false;
  let lo = 0, hi = breaks.length;
  while (lo < hi) { const mid = (lo + hi) >> 1; if (breaks[mid] <= startNs) lo = mid + 1; else hi = mid; }
  return lo < breaks.length && breaks[lo] <= endNs;
}
function percentile(sorted, q) {
  if (!sorted.length) return null;
  return sorted[Math.floor((sorted.length - 1) * q)];
}
function stats(values) {
  if (!values.length) return {count: 0, mean: null, median: null, p10: null, p90: null, min: null, max: null};
  const sorted = values.slice().sort((a, b) => a - b);
  return {count: sorted.length, mean: sorted.reduce((a, b) => a + b, 0) / sorted.length,
    median: percentile(sorted, 0.5), p10: percentile(sorted, 0.1), p90: percentile(sorted, 0.9),
    min: sorted[0], max: sorted.at(-1)};
}

function validateOptions(options) {
  const latencies = (values, name) => {
    if (!Array.isArray(values) || !values.length || values.some(x => !Number.isSafeInteger(x) || x < 0)) throw Error(`COMPLEMENT_${name}`);
    return [...new Set(values)].sort((a, b) => a - b);
  };
  const cfg = {
    splitAtNs: timeNs(options.splitAt, 'COMPLEMENT_SPLIT'),
    firstLegLatenciesMs: latencies(options.firstLegLatenciesMs ?? COMPLEMENT_REPLAY_V1.firstLegLatenciesMs, 'FIRST_LATENCY'),
    hedgeLatenciesMs: latencies(options.hedgeLatenciesMs ?? COMPLEMENT_REPLAY_V1.hedgeLatenciesMs, 'HEDGE_LATENCY'),
    decisionSpacingMs: options.decisionSpacingMs ?? COMPLEMENT_REPLAY_V1.decisionSpacingMs,
    maxFirstBookDelayMs: options.maxFirstBookDelayMs ?? COMPLEMENT_REPLAY_V1.maxFirstBookDelayMs,
    maxHedgeBookDelayMs: options.maxHedgeBookDelayMs ?? COMPLEMENT_REPLAY_V1.maxHedgeBookDelayMs,
    maxBookAgeMs: options.maxBookAgeMs ?? COMPLEMENT_REPLAY_V1.maxBookAgeMs,
    depthFractionBps: options.depthFractionBps ?? COMPLEMENT_REPLAY_V1.depthFractionBps,
  };
  for (const key of ['decisionSpacingMs','maxFirstBookDelayMs','maxHedgeBookDelayMs','maxBookAgeMs']) {
    if (!Number.isSafeInteger(cfg[key]) || cfg[key] < 0 || key === 'decisionSpacingMs' && cfg[key] < 1) throw Error(`COMPLEMENT_${key.toUpperCase()}`);
  }
  if (!Number.isSafeInteger(cfg.depthFractionBps) || cfg.depthFractionBps < 1 || cfg.depthFractionBps > 10000) throw Error('COMPLEMENT_DEPTH_FRACTION');
  return cfg;
}

// A fixed, untrained sequential-leg diagnostic. Both orientations are measured
// independently; positive later netting is directional price movement, not a
// locked-in arbitrage. Candidate books come from one exact market ID only.
export function analyzeSameMarketComplementV1({observations, breaks = [], options}) {
  if (!Array.isArray(observations) || !Array.isArray(breaks)) throw Error('COMPLEMENT_INPUT');
  const cfg = validateOptions(options ?? {}), input = [], markets = new Map(), breaksByMarket = new Map();
  let priorInputNs = null, acceptedPoints = 0, invalidPoints = 0;
  for (const raw of observations) {
    try {
      const inputNs = timeNs(raw?.availableAt, 'COMPLEMENT_CLOCK');
      if (priorInputNs !== null && inputNs < priorInputNs) throw Error('COMPLEMENT_INPUT_ORDER');
      priorInputNs = inputNs;
    } catch (error) {
      if (error.message === 'COMPLEMENT_INPUT_ORDER') throw error;
      invalidPoints++; continue;
    }
    let row;
    try { row = raw?.mode === SCREENED_COMPLEMENT_BOOK_V1 ? validateScreenedBookV1(raw, cfg) : screenComplementBookV1(raw, cfg); }
    catch { invalidPoints++; continue; }
    const availableNs = timeNs(row.availableAt, 'COMPLEMENT_CLOCK'), sourceNs = timeNs(row.sourceAt, 'COMPLEMENT_CLOCK');
    const market = markets.get(row.exchangeId) ?? [];
    const last = market.at(-1);
    if (last && (availableNs < last.availableNs || BigInt(row.sequence) < BigInt(last.sequence)
      || BigInt(row.sequence) === BigInt(last.sequence) && sourceNs <= last.sourceNs)) { invalidPoints++; continue; }
    row.availableNs = availableNs; row.sourceNs = sourceNs; row.bidMicros = amount(row.bid); row.askMicros = amount(row.ask);
    row.bidSizeBig = BigInt(row.bidSize); row.askSizeBig = BigInt(row.askSize);
    if (last && last.marketId !== row.marketId) throw Error('COMPLEMENT_EXCHANGE_MARKET_IDENTITY');
    market.push(row); markets.set(row.exchangeId, market); input.push(row); acceptedPoints++;
  }
  let previousBreak = null;
  for (const b of breaks) {
    const at = timeNs(b.at, 'COMPLEMENT_BREAK_CLOCK');
    if (previousBreak !== null && at < previousBreak || b.marketId !== null && typeof b.marketId !== 'string') throw Error('COMPLEMENT_BREAK_ORDER');
    previousBreak = at;
    const list = breaksByMarket.get(b.marketId) ?? []; list.push(at); breaksByMarket.set(b.marketId, list);
  }
  const globalBreaks = breaksByMarket.get(null) ?? [];

  const maxFirstLatency = cfg.firstLegLatenciesMs.at(-1), maxHedgeLatency = cfg.hedgeLatenciesMs.at(-1);
  const purgeMs = maxFirstLatency + maxHedgeLatency + cfg.maxFirstBookDelayMs + cfg.maxHedgeBookDelayMs;
  const baseDecisionCount = {development: 0, heldout: 0, purged: 0};
  const scenarios = new Map(), resultFor = (period, firstSide, firstLatencyMs, hedgeLatencyMs) => {
    const key = `${period}|${firstSide}|${firstLatencyMs}|${hedgeLatencyMs}`;
    if (!scenarios.has(key)) scenarios.set(key, {period, firstSide, firstLatencyMs, hedgeLatencyMs,
      decisions: 0, firstDepthReady: 0, completedPairs: 0, positivePairs: 0, negativePairs: 0,
      censoredFirstDelay: 0, censoredFirstDepth: 0, censoredHedgeDelay: 0, censoredHedgeDepth: 0,
      censoredBreak: 0, firstCosts: [], hedgeCosts: [], pairPnl: [], hedgeDelayActual: [],
      orphanMarkAtHedge: [], orphanWorstMark: [], completedMarkets: new Set(), positiveMarkets: new Map()});
    return scenarios.get(key);
  };
  const firstLatencyNs = cfg.firstLegLatenciesMs.map(ms => BigInt(ms) * 1_000_000n);
  const hedgeLatencyNs = cfg.hedgeLatenciesMs.map(ms => BigInt(ms) * 1_000_000n);
  const firstDelayNs = BigInt(cfg.maxFirstBookDelayMs) * 1_000_000n, hedgeDelayNs = BigInt(cfg.maxHedgeBookDelayMs) * 1_000_000n;
  const minSpacingNs = BigInt(cfg.decisionSpacingMs) * 1_000_000n;

  for (const [exchangeId, rows] of markets) {
    const marketId = rows[0].marketId;
    const lastDecision = {YES: null, NO: null}, marketBreaks = breaksByMarket.get(marketId) ?? [];
    for (const decision of rows) {
      for (const firstSide of ['YES','NO']) {
        if (lastDecision[firstSide] !== null && decision.availableNs - lastDecision[firstSide] < minSpacingNs) continue;
        lastDecision[firstSide] = decision.availableNs;
        const period = decision.availableNs + BigInt(purgeMs) * 1_000_000n <= cfg.splitAtNs ? 'development'
          : decision.availableNs >= cfg.splitAtNs ? 'heldout' : 'purged';
        baseDecisionCount[period]++;
        if (period === 'purged') continue;
        for (let fi = 0; fi < cfg.firstLegLatenciesMs.length; fi++) {
          const firstLatencyMs = cfg.firstLegLatenciesMs[fi], entryTarget = decision.availableNs + firstLatencyNs[fi];
          const entryIndex = upperBound(rows, entryTarget), entry = rows[entryIndex];
          for (let hi = 0; hi < cfg.hedgeLatenciesMs.length; hi++) {
            const hedgeLatencyMs = cfg.hedgeLatenciesMs[hi], result = resultFor(period, firstSide, firstLatencyMs, hedgeLatencyMs);
            result.decisions++;
            if (!entry || entry.availableNs - entryTarget > firstDelayNs) { result.censoredFirstDelay++; continue; }
            const firstCost = firstSide === 'YES' ? entry.askMicros : SCALE - entry.bidMicros;
            const firstDepth = firstSide === 'YES' ? entry.askSizeBig : entry.bidSizeBig;
            if (firstDepth < 1n) { result.censoredFirstDepth++; continue; }
            result.firstDepthReady++;
            if (breakIn(marketBreaks, decision.availableNs, entry.availableNs) || breakIn(globalBreaks, decision.availableNs, entry.availableNs)) {
              result.censoredBreak++; continue;
            }

            const hedgeTarget = entry.availableNs + hedgeLatencyNs[hi], hedgeIndex = upperBound(rows, hedgeTarget), hedge = rows[hedgeIndex];
            if (!hedge || hedge.availableNs - hedgeTarget > hedgeDelayNs) { result.censoredHedgeDelay++; continue; }
            if (breakIn(marketBreaks, entry.availableNs, hedge.availableNs) || breakIn(globalBreaks, entry.availableNs, hedge.availableNs)) {
              result.censoredBreak++; continue;
            }
            const hedgeCost = firstSide === 'YES' ? SCALE - hedge.bidMicros : hedge.askMicros;
            const hedgeDepth = firstSide === 'YES' ? hedge.bidSizeBig : hedge.askSizeBig;
            if (hedgeDepth < 1n) { result.censoredHedgeDepth++; continue; }

            const pairPnl = SCALE - firstCost - hedgeCost;
            const liquidateAt = firstSide === 'YES' ? hedge.bidMicros : SCALE - hedge.askMicros;
            const orphanAtHedge = liquidateAt - firstCost;
            let orphanWorst = (firstSide === 'YES' ? entry.bidMicros : SCALE - entry.askMicros) - firstCost;
            for (let k = entryIndex + 1; k <= hedgeIndex; k++) {
              const mid = rows[k], exitValue = firstSide === 'YES' ? mid.bidMicros : SCALE - mid.askMicros;
              const mark = exitValue - firstCost;
              if (mark < orphanWorst) orphanWorst = mark;
            }
            result.completedPairs++; if (pairPnl > 0n) result.positivePairs++; else if (pairPnl < 0n) result.negativePairs++;
            result.firstCosts.push(Number(firstCost) / Number(SCALE)); result.hedgeCosts.push(Number(hedgeCost) / Number(SCALE));
            result.pairPnl.push(Number(pairPnl) / Number(SCALE));
            result.hedgeDelayActual.push(Number(hedge.availableNs - entry.availableNs) / 1e6);
            result.orphanMarkAtHedge.push(Number(orphanAtHedge) / Number(SCALE));
            result.orphanWorstMark.push(Number(orphanWorst) / Number(SCALE));
            result.completedMarkets.add(exchangeId);
            const perMarket = result.positiveMarkets.get(exchangeId) ?? {n: 0, pnl: 0};
            perMarket.n++; perMarket.pnl += Number(pairPnl) / Number(SCALE); result.positiveMarkets.set(exchangeId, perMarket);
          }
        }
      }
    }
  }

  const resultRows = [...scenarios.values()].map(s => {
    const pnl = stats(s.pairPnl), orphan = stats(s.orphanMarkAtHedge), worst = stats(s.orphanWorstMark), delay = stats(s.hedgeDelayActual);
    const marketMeans = [...s.positiveMarkets.values()].map(x => x.pnl / x.n);
    return {period: s.period, firstSide: s.firstSide, hedgeSide: s.firstSide === 'YES' ? 'NO' : 'YES',
      firstLegLatencyMs: s.firstLatencyMs, secondLegLatencyMs: s.hedgeLatencyMs,
      decisionCount: s.decisions, firstLegDepthReady: s.firstDepthReady, completedPairs: s.completedPairs,
      profitablePairCount: s.positivePairs, losingPairCount: s.negativePairs,
      completedContracts: s.completedMarkets.size, positiveContractMeanCount: marketMeans.filter(x => x > 0).length,
      firstLegBookDelayCensored: s.censoredFirstDelay, firstLegDepthCensored: s.censoredFirstDepth,
      secondLegBookDelayCensored: s.censoredHedgeDelay, secondLegDepthCensored: s.censoredHedgeDepth,
      breakCensored: s.censoredBreak,
      firstLegCost: stats(s.firstCosts), secondLegCost: stats(s.hedgeCosts),
      pairNetPnlPerShare: pnl, sumOfCorrelatedOneShareOutcomes: s.pairPnl.reduce((a, b) => a + b, 0),
      secondLegObservedDelayMs: delay, firstLegUnhedgedLiquidationPnlAtHedge: orphan,
      firstLegWorstObservedLiquidationPnlBeforeHedge: worst,
      notActualOrdersOrFills: true, independentSamplesEstablished: false};
  }).sort((a, b) => a.period.localeCompare(b.period) || a.firstSide.localeCompare(b.firstSide)
    || a.firstLegLatencyMs - b.firstLegLatencyMs || a.secondLegLatencyMs - b.secondLegLatencyMs);

  const atomicRows = input;
  const topPnl = atomicRows.map(x => Number(amount(x.topPairNetIfBothFill)) / Number(SCALE));
  const participationPnl = atomicRows.filter(x => x.participationNetPerPair !== null).map(x => x.participationNetPerPair);
  const participationTotal = atomicRows.filter(x => x.participationNetTotal !== null).map(x => Number(amount(x.participationNetTotal)) / Number(SCALE));
  const atomic = {bookSnapshots: atomicRows.length,
    snapshotsWithAtLeastOneDisplayedShareOnBothSides: atomicRows.filter(x => BigInt(x.displayedTopPairDepth) >= 1n).length,
    snapshotsWithParticipatingDepthOnBothSides: participationPnl.length,
    topPairNetPerShare: stats(topPnl),
    onePercentDepthSweepNetPerPair: stats(participationPnl),
    onePercentDepthSweepNetTotalPerSnapshot: stats(participationTotal),
    positiveSameTimestampTopPairCount: topPnl.filter(x => x > 0).length,
    positiveSameTimestampOnePercentDepthCount: participationPnl.filter(x => x > 0).length,
    maximumObservedOnePercentPairQuantity: atomicRows.reduce((n, x) => BigInt(x.participationPairDepth) > n ? BigInt(x.participationPairDepth) : n, 0n).toString(),
    officialNettingPayoutPerCompletedPair: '1.000000',
    interpretation: 'At one timestamp, one YES ask plus the complementary NO ask (1 minus YES bid) costs 1 plus the YES spread. Sequential positive outcomes require a favorable later move and are not locked-in arbitrage.'};
  return {mode: 'same_market_exact_complement_sequential_depth_replay_v1', fees: '0.000000',
    splitAt: new Date(timeMs(cfg.splitAtNs)).toISOString(), holdout: 'chronological later 30% of capture time; no model fitting or threshold search',
    purgeMs, observations: {input: observations.length, accepted: acceptedPoints, invalidOrDuplicate: invalidPoints,
      markets: new Set(input.map(x=>x.marketId)).size, contracts: markets.size, explicitBreaks: breaks.length}, atomic, diagnostics: resultRows,
    assumptions: {decisionSpacingMs: cfg.decisionSpacingMs, firstLegLatenciesMs: cfg.firstLegLatenciesMs,
      secondLegLatenciesMs: cfg.hedgeLatenciesMs, maxFirstBookDelayMs: cfg.maxFirstBookDelayMs,
      maxSecondBookDelayMs: cfg.maxHedgeBookDelayMs, maxBookAgeMs: cfg.maxBookAgeMs,
      displayedDepthParticipationBpsForSameTimeSweep: cfg.depthFractionBps,
      sequentialSizing: 'one share; both top levels must display at least one share',
      sameContractOnly: true, holdoutNoTuning: true,
      sourceBookDepthIncludesOwnOrders: true, queuePriorityOrFillProbabilityKnown: false,
      exactNettingPayoutIsCreditedOnlyAfterOppositeLegCompletes: true,
      oneLegPositionRemainsDirectionallyExposedUntilThen: true,
      fees: 'zero per challenge research setup; no added slippage beyond walking visible depth',
    }, limitations: ['Public displayed depth is a conditional counterfactual, not an order acceptance or fill; books include own orders and queue priority is unknown.',
      'Sequential positive pair P&L is caused by later price movement, with first-leg directional exposure; it is not same-timestamp arbitrage.',
      'The report has no account holdings, order state, actual fills, or actual cash netting evidence.',
      'Same-time 1% depth sweep uses only the captured visible top 20 levels and does not establish executable capacity after latency.'],
    networkRequests: 0, contestOrders: 0, profitabilityEstablished: false};
}
