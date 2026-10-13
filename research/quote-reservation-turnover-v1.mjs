import {payloadHash} from '../execution/ledger.mjs';
import {amount, money, instant} from '../paper/fixed.mjs';
import {makerProjectionRetirementV1} from '../paper/maker-projection-barrier-v1.mjs';

const SCALE = 1_000_000n;
const TICK = 5_000n;
const need = (ok, code) => { if (!ok) throw Error(`QUOTE_TURNOVER_${code}`); };
const ms = value => typeof value === 'number' ? value : instant(value);

function decimal(n, denominator, digits = 3) {
  need(typeof n === 'bigint' && typeof denominator === 'bigint' && denominator > 0n, 'DECIMAL');
  const neg = n < 0n;
  const scale = 10n ** BigInt(digits);
  const rounded = (abs(n) * scale + denominator / 2n) / denominator;
  const whole = rounded / scale;
  const fraction = (rounded % scale).toString().padStart(digits, '0');
  return `${neg ? '-' : ''}${whole}.${fraction}`;
}
const abs = x => x < 0n ? -x : x;
const dollars = micro => money(micro);

function distribution(values) {
  if (!values.length) return {n: 0, min: null, median: null, p90: null, max: null};
  const sorted = values.slice().sort((a, b) => a - b);
  const q = p => sorted[Math.floor((sorted.length - 1) * p)];
  return {n: sorted.length, min: sorted[0], median: q(.5), p90: q(.9), max: sorted.at(-1)};
}

function topBookMetrics(quote) {
  const book = quote.queueDecisionBook;
  if (!book || !Array.isArray(book.bids) || !Array.isArray(book.asks) || !book.bids.length || !book.asks.length)
    return {available: false, reason: 'decision_book_not_retained'};
  need(book.exchangeId === quote.exchangeId && book.marketId === quote.marketId, 'BOOK_SCOPE');
  need(book.versionKey === quote.decisionVersion, 'BOOK_VERSION');
  need(quote.decisionBookHash === payloadHash(book), 'BOOK_HASH');
  const first = (levels, name) => {
    const row = levels[0]; need(row && Number.isSafeInteger(row.quantity) && row.quantity > 0, `BOOK_${name}`);
    return {price: amount(row.price), quantity: row.quantity};
  };
  const yesBid = first(book.bids, 'BID');
  const yesAsk = first(book.asks, 'ASK');
  need(yesBid.price < yesAsk.price, 'CROSSED_BOOK');
  const bid = quote.side === 'YES' ? yesBid : {price: SCALE - yesAsk.price, quantity: yesAsk.quantity};
  const ask = quote.side === 'YES' ? yesAsk : {price: SCALE - yesBid.price, quantity: yesBid.quantity};
  const limit = amount(quote.limit);
  const ahead = quote.queueStudy?.models?.[0]?.initialDisplayedAhead;
  const binding = book.sourceBinding;
  const requestStartedAt = ms(book.requestStartedAt);
  const receivedAt = ms(book.receivedAt);
  const createdAt = ms(quote.createdAt);
  const activationAt = ms(quote.activationAt);
  need(requestStartedAt <= receivedAt && receivedAt <= createdAt && createdAt <= activationAt, 'BOOK_CLOCK');
  const sameBestBid = limit === bid.price;
  const insideSpread = limit > bid.price && limit < ask.price;
  return {
    available: true,
    sourceBound: !!binding && typeof binding.recordHash === 'string' && typeof binding.rawBookHash === 'string',
    sourceContinuityProven: binding?.continuityProven === true,
    requestStartedAt,
    receivedAt,
    createdAt,
    activationAt,
    ageAtCreationMs: createdAt - requestStartedAt,
    ageAtActivationMs: activationAt - requestStartedAt,
    spreadTicks: Number(ask.price - bid.price) / Number(TICK),
    quoteToBestBidTicks: Number(limit - bid.price) / Number(TICK),
    quoteToBestAskTicks: Number(ask.price - limit) / Number(TICK),
    topBidShares: bid.quantity,
    topAskShares: ask.quantity,
    topQueueAheadShares: sameBestBid ? bid.quantity : insideSpread ? 0 : null,
    queueStudyInitialDisplayedAhead: Number.isSafeInteger(ahead) ? ahead : null,
    storedBidLevels: book.bids.length,
    storedAskLevels: book.asks.length,
    sourceObservedBidLevels: Number.isSafeInteger(binding?.observedDepth?.bids) ? binding.observedDepth.bids : null,
    sourceObservedAskLevels: Number.isSafeInteger(binding?.observedDepth?.asks) ? binding.observedDepth.asks : null,
    insideSpread,
    atBestBid: sameBestBid,
    atBestAsk: limit === ask.price,
    betterThanBestBid: limit > bid.price,
    sourceRecordHash: binding?.recordHash ?? null,
    sourceRawBookHash: binding?.rawBookHash ?? null,
    sourceOriginalBookHash: book.originalSourceBookHash ?? null,
  };
}

function outcomeDepth(book, side, action) {
  const source = action === 'buy' ? (side === 'YES' ? book.bids : book.asks) : (side === 'YES' ? book.asks : book.bids);
  const rows = source.map(level => ({
    price: side === 'YES' ? amount(level.price) : SCALE - amount(level.price),
    quantity: level.quantity,
  }));
  rows.sort((a, b) => action === 'buy' ? (a.price < b.price ? 1 : -1) : (a.price > b.price ? 1 : -1));
  return rows;
}

function walk(rows, quantity) {
  let remaining = quantity;
  let value = 0n;
  for (const row of rows) {
    const n = Math.min(remaining, row.quantity);
    value += row.price * BigInt(n);
    remaining -= n;
    if (!remaining) break;
  }
  return {quantity: quantity - remaining, remaining, value};
}

function markouts(fills, bookReceipts, horizonsMs) {
  const results = horizonsMs.map(horizonMs => ({horizonMs, n: 0, censored: 0, meanPerShare: null, medianPerShare: null, depthComplete: 0}));
  const byExchange = new Map();
  for (const b of bookReceipts) {
    need(b && typeof b.exchangeId === 'string' && typeof b.marketId === 'string', 'MARKOUT_BOOK_SCOPE');
    need(ms(b.requestStartedAt) <= ms(b.receivedAt), 'MARKOUT_BOOK_CLOCK');
    if (!byExchange.has(b.exchangeId)) byExchange.set(b.exchangeId, []);
    byExchange.get(b.exchangeId).push(b);
  }
  for (const rows of byExchange.values()) rows.sort((a, b) => ms(a.requestStartedAt) - ms(b.requestStartedAt));
  for (const fill of fills) {
    for (const result of results) {
      const target = ms(fill.at) + result.horizonMs;
      const candidates = (byExchange.get(fill.exchangeId) ?? []).filter(b => b.marketId === fill.marketId
        && ms(b.requestStartedAt) >= target && b.versionKey !== fill.source?.version);
      const book = candidates[0];
      if (!book) { result.censored++; continue; }
      const rows = outcomeDepth(book, fill.side, fill.action);
      const walkResult = walk(rows, fill.quantity);
      if (!walkResult.quantity) { result.censored++; continue; }
      const vwap = walkResult.value / BigInt(walkResult.quantity);
      const fillPrice = amount(fill.price);
      const edge = fill.action === 'buy' ? vwap - fillPrice : fillPrice - vwap;
      result.n++;
      result.depthComplete += walkResult.remaining === 0 ? 1 : 0;
      result._sum = (result._sum ?? 0n) + edge;
      result._values ??= [];
      result._values.push(edge);
    }
  }
  return results.map(r => {
    const values = (r._values ?? []).sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
    const out = {...r, meanPerShare: r.n ? money(r._sum / BigInt(r.n)) : null,
      medianPerShare: r.n ? money(values[Math.floor((r.n - 1) / 2)]) : null};
    delete out._sum; delete out._values; return out;
  });
}

/**
 * Analyze the final frozen local quote-wallet checkpoint without replaying or
 * mutating the paper worker. Reservation time follows the wallet's actual
 * `!closedAt` cash-reservation rule; effective cancel/expiry does not itself
 * release cash. Fill time is the local accounting/knowledge clock (`fill.at`),
 * while trade-created time is separately reported as a source-time age.
 */
export function analyzeQuoteReservationTurnoverV1({bank, state, asOf = state?.wallet?.wallet?.lastAt,
  bookReceipts = [], horizonsMs = [1_000, 5_000, 30_000, 60_000]}) {
  need(bank?.schemaVersion === 1 && bank.executionAuthority === false, 'BANK_AUTHORITY');
  need(typeof bank.bankHash === 'string' && /^[a-f0-9]{64}$/.test(bank.bankHash), 'BANK_HASH');
  need(state?.bankHash === bank.bankHash, 'STATE_BANK');
  need(bank.policy?.wallet?.fillModel === 'strict_tape_through', 'FILL_MODEL');
  need(Array.isArray(horizonsMs) && horizonsMs.every(x => Number.isSafeInteger(x) && x > 0), 'HORIZONS');
  const wallet = state.wallet?.wallet;
  need(wallet && Array.isArray(wallet.quotes) && Array.isArray(wallet.fills), 'WALLET');
  const cutoffMs = instant(bank.cutoff);
  const asOfMs = ms(asOf);
  need(asOfMs >= cutoffMs, 'CLOCK');
  const quoteById = new Map();
  for (const q of wallet.quotes) {
    need(Number.isSafeInteger(q.id) && !quoteById.has(q.id), 'QUOTE_ID');
    need(['YES', 'NO'].includes(q.side) && ['buy', 'sell'].includes(q.action), 'QUOTE_SIDE_ACTION');
    need(Number.isSafeInteger(q.quantity) && q.quantity > 0 && Number.isSafeInteger(q.remaining)
      && q.remaining >= 0 && q.remaining <= q.quantity, 'QUOTE_QUANTITY');
    need(ms(q.createdAt) <= asOfMs && ms(q.activationAt) >= ms(q.createdAt), 'QUOTE_CLOCK');
    need(ms(q.expiresAt) >= ms(q.activationAt), 'QUOTE_EXPIRY');
    if (q.cancelEffectiveAt !== undefined) need(ms(q.cancelEffectiveAt) >= ms(q.createdAt), 'CANCEL_CLOCK');
    if (q.closedAt !== null && q.closedAt !== undefined) need(ms(q.closedAt) <= asOfMs, 'CLOSE_CLOCK');
    quoteById.set(q.id, q);
  }
  const fillsByQuote = new Map([...quoteById.keys()].map(id => [id, []]));
  for (const f of wallet.fills) {
    need(f.actualFill === false, 'NON_PAPER_FILL');
    const q = quoteById.get(f.quoteId);
    need(q && f.action === q.action && f.side === q.side && f.marketId === q.marketId
      && f.exchangeId === q.exchangeId && f.price === q.limit, 'FILL_BINDING');
    need(Number.isSafeInteger(f.quantity) && f.quantity > 0 && f.source?.strictTradeThrough === true
      && f.source?.queuePositionProven === false, 'FILL_SEMANTICS');
    const sourceAt = ms(f.source.createdAt);
    const knownAt = ms(f.source.knownAt);
    const fillAt = ms(f.at);
    const quoteEnd = Math.min(ms(q.expiresAt), q.cancelEffectiveAt === undefined ? Infinity : ms(q.cancelEffectiveAt));
    need(sourceAt >= ms(q.activationAt) && sourceAt < quoteEnd && knownAt >= sourceAt && fillAt >= knownAt, 'FILL_CLOCK');
    fillsByQuote.get(q.id).push(f);
  }
  const currentReserved = wallet.quotes.filter(q => !q.closedAt && q.action === 'buy')
    .reduce((n, q) => n + amount(q.limit) * BigInt(q.remaining), 0n);
  let reservedMicroMs = 0n;
  let postEffectiveMicroMs = 0n;
  let reservedShareMs = 0n;
  let decisionBookHashMatches = 0;
  let sourceBoundBooks = 0;
  let sourceContinuityProven = 0;
  let staleAtActivation = 0;
  let currentCancelBound = 0;
  let currentExpiredBound = 0;
  let cancelBoundMicro = 0n;
  let expiredBoundMicro = 0n;
  const ageAtTerminalMs = [];
  const unresolvedAgeMs = [];
  const bookAgeAtCreation = [];
  const bookAgeAtActivation = [];
  const topBidDepth = [];
  const topAskDepth = [];
  const queueAhead = [];
  const observedBidLevels = [];
  const observedAskLevels = [];
  const quotePricePosition = {atBestBid: 0, insideSpread: 0, betterThanBestBid: 0, atBestAsk: 0, belowBestBid: 0};
  const retirementBlockers = {};
  const outcomeCounts = {filled: 0, partialFill: 0, closedWithoutFullFill: 0, unresolved: 0,
    unresolvedAfterCancelEffective: 0, unresolvedAfterExpiry: 0, censored: 0,
    modeledCancelFlag: 0, modeledCancelAcknowledged: 0};
  const fillSourceAges = [];
  const fillKnowledgeDelays = [];
  let buyFilledMicro = 0n;
  let sellFilledMicro = 0n;
  let paperFeesMicro = 0n;
  let fillFeeFields = 0;
  const captureBindings = [];

  for (const q of wallet.quotes) {
    const createdAt = ms(q.createdAt);
    const terminalAt = q.closedAt === null || q.closedAt === undefined ? asOfMs : ms(q.closedAt);
    const effectiveEnd = Math.min(ms(q.expiresAt), q.cancelEffectiveAt === undefined ? Infinity : ms(q.cancelEffectiveAt));
    const startAt = Math.max(cutoffMs, createdAt);
    const stopAt = Math.min(asOfMs, terminalAt);
    const orderedFills = fillsByQuote.get(q.id).slice().sort((a, b) => ms(a.at) - ms(b.at));
    let remaining = q.quantity;
    let cursor = createdAt;
    let filled = 0;
    for (const f of orderedFills) {
      const fillAt = ms(f.at);
      const from = Math.max(cursor, startAt);
      const to = Math.min(fillAt, stopAt);
      if (q.action === 'buy' && to > from && remaining > 0) {
        const notional = amount(q.limit) * BigInt(remaining);
        reservedMicroMs += notional * BigInt(to - from);
        reservedShareMs += BigInt(remaining) * BigInt(to - from);
        const afterEnd = Math.max(from, effectiveEnd);
        if (to > afterEnd) postEffectiveMicroMs += notional * BigInt(to - afterEnd);
      }
      remaining -= f.quantity;
      filled += f.quantity;
      cursor = fillAt;
      const sourceAt = ms(f.source.createdAt);
      fillSourceAges.push(sourceAt - ms(q.activationAt));
      fillKnowledgeDelays.push(ms(f.at) - sourceAt);
      const notional = amount(f.price) * BigInt(f.quantity);
      if (q.action === 'buy') buyFilledMicro += notional; else sellFilledMicro += notional;
      if (f.fee !== undefined) { paperFeesMicro += amount(f.fee); fillFeeFields++; }
    }
    const from = Math.max(cursor, startAt);
    if (q.action === 'buy' && stopAt > from && remaining > 0) {
      const notional = amount(q.limit) * BigInt(remaining);
      reservedMicroMs += notional * BigInt(stopAt - from);
      reservedShareMs += BigInt(remaining) * BigInt(stopAt - from);
      const afterEnd = Math.max(from, effectiveEnd);
      if (stopAt > afterEnd) postEffectiveMicroMs += notional * BigInt(stopAt - afterEnd);
    }
    need(remaining === q.remaining, 'FILL_RECONCILIATION');
    if (q.closedAt) need(filled > 0 || q.remaining > 0, 'CLOSED_ZERO');
    if (filled >= q.quantity) outcomeCounts.filled++;
    else if (filled > 0) outcomeCounts.partialFill++;
    else if (q.closedAt) outcomeCounts.closedWithoutFullFill++;
    else outcomeCounts.unresolved++;
    if (q.censored) outcomeCounts.censored++;
    if (q.cancelEffectiveAt !== undefined) outcomeCounts.modeledCancelFlag++;
    if (q.modeledCancelAcknowledgedAt !== undefined) outcomeCounts.modeledCancelAcknowledged++;
    const liveEnd = q.closedAt ? Math.min(terminalAt, asOfMs) : asOfMs;
    ageAtTerminalMs.push(Math.max(0, liveEnd - ms(q.activationAt)));
    if (!q.closedAt) {
      unresolvedAgeMs.push(Math.max(0, asOfMs - ms(q.activationAt)));
      if (q.action === 'buy') {
        const reserve = amount(q.limit) * BigInt(q.remaining);
        if (q.cancelEffectiveAt !== undefined && ms(q.cancelEffectiveAt) <= asOfMs) { currentCancelBound++; cancelBoundMicro += reserve; outcomeCounts.unresolvedAfterCancelEffective++; }
        if (ms(q.expiresAt) <= asOfMs) { currentExpiredBound++; expiredBoundMicro += reserve; outcomeCounts.unresolvedAfterExpiry++; }
      }
      const blocker = !q.projection ? 'missing_projection' : makerProjectionRetirementV1(q.projection).reason;
      retirementBlockers[blocker ?? 'retirement_ready'] = (retirementBlockers[blocker ?? 'retirement_ready'] ?? 0) + 1;
    }
    const depth = topBookMetrics(q);
    if (depth.available) {
      decisionBookHashMatches++;
      bookAgeAtCreation.push(depth.ageAtCreationMs);
      bookAgeAtActivation.push(depth.ageAtActivationMs);
      if (depth.ageAtActivationMs > 10_000) staleAtActivation++;
      topBidDepth.push(depth.topBidShares);
      topAskDepth.push(depth.topAskShares);
      if (depth.queueStudyInitialDisplayedAhead !== null) queueAhead.push(depth.queueStudyInitialDisplayedAhead);
      if (depth.sourceObservedBidLevels !== null) observedBidLevels.push(depth.sourceObservedBidLevels);
      if (depth.sourceObservedAskLevels !== null) observedAskLevels.push(depth.sourceObservedAskLevels);
      if (depth.atBestBid) quotePricePosition.atBestBid++;
      if (depth.insideSpread) quotePricePosition.insideSpread++;
      if (depth.betterThanBestBid) quotePricePosition.betterThanBestBid++;
      if (depth.atBestAsk) quotePricePosition.atBestAsk++;
      if (!depth.atBestBid && !depth.insideSpread && !depth.betterThanBestBid && !depth.atBestAsk) quotePricePosition.belowBestBid++;
      if (depth.sourceBound) sourceBoundBooks++;
      if (depth.sourceContinuityProven) sourceContinuityProven++;
      if (depth.sourceRecordHash && depth.sourceRawBookHash)
        captureBindings.push({recordHash: depth.sourceRecordHash, rawBookHash: depth.sourceRawBookHash,
          originalBookHash: depth.sourceOriginalBookHash, version: q.decisionVersion});
    }
  }
  need(currentReserved === wallet.quotes.filter(q => !q.closedAt && q.action === 'buy')
    .reduce((n, q) => n + amount(q.limit) * BigInt(q.remaining), 0n), 'RESERVE');
  const fills = wallet.fills;
  const openBuyQuotes = wallet.quotes.filter(q => !q.closedAt && q.action === 'buy');
  const totalWindowMs = asOfMs - cutoffMs;
  const markoutRows = markouts(fills, bookReceipts, horizonsMs);
  const actualFillRows = fills.filter(f => f.actualFill === true).length;
  const fundedQuotes = wallet.quotes.length;
  const captureBindingsDigest = captureBindings.length ? payloadHash(captureBindings) : null;
  const avgReserved = totalWindowMs > 0 ? decimal(reservedMicroMs, SCALE * BigInt(totalWindowMs), 6) : '0.000000';

  return {
    schemaVersion: 1,
    mode: 'offline_frozen_local_paper_quote_reservation_diagnosis',
    cohort: {name: bank.cohort, bankHash: bank.bankHash, cutoff: bank.cutoff,
      asOf: new Date(asOfMs).toISOString(), windowMs: totalWindowMs,
      fillModel: bank.policy.wallet.fillModel, modeledLatencyMs: bank.policy.wallet.latencyMs,
      quoteLifetimeMs: bank.policy.wallet.lifetimeMs, tapeParticipationBps: bank.policy.participationBps,
      executionAuthority: false},
    wallet: {initialCash: bank.policy.wallet.initialCash, cash: wallet.cash,
      reservedCash: dollars(currentReserved), ownedMarkets: Object.keys(wallet.positions).length,
      realized: wallet.realized, paperFillEvents: fills.length, actualFillEvents: actualFillRows,
      paperBuyFillNotional: dollars(buyFilledMicro), paperSellFillNotional: dollars(sellFilledMicro),
      modeledFees: dollars(paperFeesMicro), fillFeeFields, feeInterpretation: fillFeeFields ? 'fee fields present on some paper fills' : 'no explicit fee recorded on paper fill records'},
    quoteLifecycle: {totalQuotes: fundedQuotes, buys: wallet.quotes.filter(q => q.action === 'buy').length,
      sells: wallet.quotes.filter(q => q.action === 'sell').length, openBuyReservations: openBuyQuotes.length,
      currentReservedCash: dollars(currentReserved), currentReservedBuyShares: openBuyQuotes.reduce((n, q) => n + q.remaining, 0),
      unresolvedExpiredBuyQuotes: currentExpiredBound, reservedCashOnExpiredBuyQuotes: dollars(expiredBoundMicro),
      unresolvedAfterModeledCancel: currentCancelBound, reservedCashAfterModeledCancel: dollars(cancelBoundMicro),
      outcomeCounts, retirementBlockers,
      meanCashReservationUsd: avgReserved,
      reservationCapitalDollarHours: decimal(reservedMicroMs, 3_600_000_000_000n, 3),
      reservedShareHours: decimal(reservedShareMs, 3_600_000n, 3),
      cashReservationDollarHoursAfterEffectiveCancelOrExpiry: decimal(postEffectiveMicroMs, 3_600_000_000_000n, 3),
      ageSinceActivationMs: distribution(ageAtTerminalMs),
      unresolvedAgeSinceActivationMs: distribution(unresolvedAgeMs)},
    priceAndDepth: {quotesWithHashBoundDecisionBook: decisionBookHashMatches,
      sourceBoundReceipts: sourceBoundBooks, sourceContinuityProven, decisionBookContinuityNotProven: sourceBoundBooks - sourceContinuityProven,
      sourceCaptureBindingsDigest: captureBindingsDigest,
      bookAgeAtQuoteCreationMs: distribution(bookAgeAtCreation), bookAgeAtActivationMs: distribution(bookAgeAtActivation),
      activationBookOlderThan10s: staleAtActivation,
      topOfBookSameOutcomeBidShares: distribution(topBidDepth), topOfBookSameOutcomeAskShares: distribution(topAskDepth),
      paperQueueStudyInitialDisplayedAheadShares: distribution(queueAhead),
      sourceObservedBidLevelCounts: distribution(observedBidLevels), sourceObservedAskLevelCounts: distribution(observedAskLevels),
      quotePricePosition, quotePositionLimitations: 'Decision snapshots preserve the top level for each outcome; displayed depth and the queue-study estimate do not prove queue priority or fills.'},
    fillsAndMarkouts: {paperFillEvents: fills.length, eligiblePrints: walletStateParticipation(state)?.eligiblePrints ?? null,
      observedTapePrints: walletStateParticipation(state)?.prints ?? null,
      grantedParticipationShares: walletStateParticipation(state)?.grantedShares ?? null,
      fillSourceAgeMs: distribution(fillSourceAges), fillKnowledgeDelayMs: distribution(fillKnowledgeDelays),
      queuePositionProvenFillEvents: fills.filter(f => f.source?.queuePositionProven === true).length,
      markouts: markoutRows,
      associationStatus: fills.length ? 'Descriptive post-fill markouts only; no independent profitability inference.' : 'Not identifiable: zero paper fills; age/depth versus fill or post-fill markout cannot be estimated.'},
    interpretation: {reservationSemantics: 'Buy cash remains reserved while its quote is not closed; modeled cancel-effective time or quote expiry alone does not release it. The simulator retires only after its projection/tape barrier allows closure.',
      fillSemantics: 'A paper fill requires strict one-tick tape trade-through after modeled activation and applies the configured tape participation fraction. Queue priority and actual contest fills are not established.',
      markoutSemantics: 'Markouts are counterfactual top/depth liquidation values at later captured books, after the local paper fill is known; they are not realized cash or contest executions.',
      profitabilityEstablished: false, networkRequests: 0, contestOrders: 0},
  };
}

function walletStateParticipation(state) {
  return state.wallet?.participationCounts ?? null;
}

