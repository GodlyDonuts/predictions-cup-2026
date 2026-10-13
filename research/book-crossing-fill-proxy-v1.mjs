import { amount, money, TICK } from '../paper/fixed.mjs';
import { instantNs } from '../feed/contract.mjs';
import { normalizeOfficialFillTick } from '../execution/api-tick-price.mjs';

const NS_PER_MS = 1_000_000n;
const DEFAULTS = Object.freeze({
  activationLatencyMs: 3000,
  quoteTtlMs: 10000,
  quoteEveryMs: 30000,
  exitResponseLatencyMs: 3000,
  maximumBookAgeMs: 3000,
  maximumMarkoutDelayMs: 10000,
  markoutHorizonsMs: [1000, 5000, 30000],
});

const elapsedMs = (a, b) => b > a ? Number(b - a) / Number(NS_PER_MS) : 0;
const quantile = (values, p) => values.length ? values[Math.max(0, Math.ceil(values.length * p) - 1)] : null;

function summary(rows) {
  if (!rows.length) return {
    samples: 0, markets: 0, meanExecutableNet: null, medianExecutableNet: null,
    oneTickEachWayStressMean: null, positiveFraction: null,
  };
  const nets = rows.map(row => row.net).sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  const sum = nets.reduce((total, value) => total + value, 0n);
  const mean = sum / BigInt(nets.length);
  const middle = nets.length % 2
    ? nets[(nets.length - 1) / 2]
    : (nets[nets.length / 2 - 1] + nets[nets.length / 2]) / 2n;
  return {
    samples: rows.length,
    markets: new Set(rows.map(row => row.candidate.marketId)).size,
    meanExecutableNet: money(mean),
    medianExecutableNet: money(middle),
    oneTickEachWayStressMean: money(mean - 2n * TICK),
    positiveFraction: rows.filter(row => row.net > 0n).length / rows.length,
  };
}

function validateScope(scope) {
  if (!scope || typeof scope.tournamentId !== 'string' || !scope.tournamentId || !Array.isArray(scope.exchanges) || !scope.exchanges.length) {
    throw Error('BOOK_CROSSING_SCOPE');
  }
  const pairs = new Map();
  for (const member of scope.exchanges) {
    if (!member || typeof member.marketId !== 'string' || typeof member.exchangeId !== 'string' || !member.marketId || !member.exchangeId) {
      throw Error('BOOK_CROSSING_MEMBER');
    }
    if (pairs.has(member.exchangeId)) throw Error('BOOK_CROSSING_AMBIGUOUS_EXCHANGE');
    pairs.set(member.exchangeId, member.marketId);
  }
  return pairs;
}

export function createBookCrossingFillProxyV1({ scope, ...options } = {}) {
  const members = validateScope(scope);
  const config = { ...DEFAULTS, ...options };
  const {
    activationLatencyMs, quoteTtlMs, quoteEveryMs, exitResponseLatencyMs,
    maximumBookAgeMs, maximumMarkoutDelayMs, markoutHorizonsMs,
  } = config;
  if (!Array.isArray(markoutHorizonsMs) || !markoutHorizonsMs.length
    || !markoutHorizonsMs.every((value, index) => Number.isSafeInteger(value) && value > 0 && (index === 0 || value > markoutHorizonsMs[index - 1]))) {
    throw Error('BOOK_CROSSING_HORIZONS');
  }
  if (![activationLatencyMs, quoteTtlMs, quoteEveryMs, exitResponseLatencyMs, maximumBookAgeMs, maximumMarkoutDelayMs]
    .every(Number.isSafeInteger) || activationLatencyMs < 0 || quoteTtlMs < 1 || quoteEveryMs < activationLatencyMs + quoteTtlMs
    || exitResponseLatencyMs < 0 || maximumBookAgeMs < 0 || maximumMarkoutDelayMs < 0) throw Error('BOOK_CROSSING_POLICY');

  const candidates = [];
  const live = new Map();
  const crossed = [];
  const priorBooks = new Map();
  const lastQuoteSlot = new Map();
  const tradeIds = new Map();
  let lastObservedNs = -1n;
  const counts = {
    observedBatches: 0,
    booksSeen: 0,
    acceptedFreshSequencedBooks: 0,
    rejectedScope: 0,
    rejectedClockOrFreshness: 0,
    rejectedSequence: 0,
    rejectedBookShape: 0,
    tradeReportsSeen: 0,
    duplicateTradeReports: 0,
    invalidTradeReports: 0,
    candidateQuotes: 0,
    crossedBookProxies: 0,
    tapeCorroboratedProxies: 0,
    invalidatedBeforeCrossing: 0,
    expiredWithoutCrossing: 0,
  };

  function invalidation(exchangeId, reason, at) {
    const atNs = at === undefined || at === null ? lastObservedNs : instantNs(at);
    const list = live.get(exchangeId) ?? [];
    for (const candidate of list) {
      candidate.status = 'invalidated_before_crossing';
      candidate.invalidatedAt = atNs >= 0n ? new Date(Number(atNs / NS_PER_MS)).toISOString() : null;
      candidate.invalidationReason = reason;
      candidate.activeDurationMs = Math.max(0, elapsedMs(candidate.activationNs, atNs < candidate.expiresNs ? atNs : candidate.expiresNs));
      counts.invalidatedBeforeCrossing++;
    }
    live.delete(exchangeId);
    priorBooks.delete(exchangeId);
    lastQuoteSlot.delete(exchangeId);
  }

  function clearMarket(marketId, reason, at) {
    for (const [exchangeId, scopedMarketId] of members) if (marketId === null || scopedMarketId === marketId) invalidation(exchangeId, reason, at);
  }

  function normalizeBook(book, observedNs) {
    counts.booksSeen++;
    if (!book || typeof book.exchangeId !== 'string' || members.get(book.exchangeId) !== book.marketId
      || book.tournamentId !== scope.tournamentId) {
      counts.rejectedScope++;
      return null;
    }
    try {
      const sourceNs = instantNs(book.sourceAt), receivedNs = instantNs(book.receivedAt);
      const requestNs = instantNs(book.requestStartedAt);
      if (requestNs > receivedNs || sourceNs > receivedNs || receivedNs > observedNs || sourceNs > observedNs
        || observedNs - sourceNs > BigInt(maximumBookAgeMs) * NS_PER_MS
        || observedNs - receivedNs > BigInt(maximumBookAgeMs) * NS_PER_MS) {
        counts.rejectedClockOrFreshness++;
        return null;
      }
      if (typeof book.sequence !== 'string' || !/^(0|[1-9]\d*)$/.test(book.sequence)) throw Error('BOOK_SEQUENCE');
      const sequence = BigInt(book.sequence);
      const prior = priorBooks.get(book.exchangeId);
      if (prior && (sequence <= prior.sequence || sourceNs <= prior.sourceNs)) {
        counts.rejectedSequence++;
        return null;
      }
      const bid = amount(normalizeOfficialFillTick(book.bids?.[0]?.price).price);
      const ask = amount(normalizeOfficialFillTick(book.asks?.[0]?.price).price);
      const bidQty = book.bids?.[0]?.quantity, askQty = book.asks?.[0]?.quantity;
      if (bid <= 0n || ask <= bid || !Number.isSafeInteger(bidQty) || bidQty < 1 || !Number.isSafeInteger(askQty) || askQty < 1) {
        counts.rejectedBookShape++;
        return null;
      }
      const normalized = { ...book, sourceNs, receivedNs, sequence, bid, ask, bidQty, askQty };
      priorBooks.set(book.exchangeId, { sequence, sourceNs });
      counts.acceptedFreshSequencedBooks++;
      return normalized;
    } catch {
      counts.rejectedClockOrFreshness++;
      return null;
    }
  }

  function tryCrosses(book, observedNs) {
    const list = live.get(book.exchangeId) ?? [];
    const keep = [];
    for (const candidate of list) {
      const inSourceWindow = book.sourceNs >= candidate.activationNs && book.sourceNs < candidate.expiresNs;
      const arrivedAfterActivation = book.receivedNs >= candidate.activationNs;
      const arrivedWithinAllowance = book.receivedNs < candidate.expiresNs + BigInt(maximumBookAgeMs) * NS_PER_MS;
      const beyond = candidate.action === 'buy' ? book.bid < candidate.limit : book.ask > candidate.limit;
      if (inSourceWindow && arrivedAfterActivation && arrivedWithinAllowance && beyond) {
        candidate.status = 'book_crossing_proxy_unconfirmed';
        candidate.proxyCrossSourceAt = book.sourceAt;
        candidate.proxyCrossReceivedAt = book.receivedAt;
        candidate.proxyCrossObservedAt = new Date(Number(observedNs / NS_PER_MS)).toISOString();
        candidate.crossingSidePrice = money(candidate.action === 'buy' ? book.bid : book.ask);
        candidate.activeDurationMs = elapsedMs(candidate.activationNs, book.sourceNs);
        candidate.marks = Object.fromEntries(markoutHorizonsMs.map(horizon => [horizon, null]));
        candidate.fillKnownNs = book.receivedNs;
        crossed.push(candidate);
        counts.crossedBookProxies++;
      } else {
        keep.push(candidate);
      }
    }
    live.set(book.exchangeId, keep);

    for (const candidate of crossed) {
      if (candidate.exchangeId !== book.exchangeId || candidate.marketId !== book.marketId || observedNs <= candidate.fillKnownNs) continue;
      for (const horizon of markoutHorizonsMs) {
        if (candidate.marks[horizon] !== null) continue;
        const eventTarget = instantNs(candidate.proxyCrossSourceAt) + BigInt(horizon) * NS_PER_MS;
        const responseTarget = candidate.fillKnownNs + BigInt(exitResponseLatencyMs) * NS_PER_MS;
        if (book.sourceNs < eventTarget || book.receivedNs < responseTarget) continue;
        const eventDelay = book.sourceNs - eventTarget;
        const responseDelay = book.receivedNs - responseTarget;
        const delay = eventDelay > responseDelay ? eventDelay : responseDelay;
        if (delay > BigInt(maximumMarkoutDelayMs) * NS_PER_MS) {
          candidate.marks[horizon] = { censored: true, sourceAt: book.sourceAt, availableAt: book.receivedAt };
          continue;
        }
        const net = candidate.action === 'buy' ? book.bid - candidate.limit : candidate.limit - book.ask;
        candidate.marks[horizon] = {
          net,
          mid: candidate.action === 'buy' ? (book.bid + book.ask) / 2n - candidate.limit : candidate.limit - (book.bid + book.ask) / 2n,
          sourceAt: book.sourceAt,
          availableAt: book.receivedAt,
          observedDelayMs: Math.max(elapsedMs(eventTarget, book.sourceNs), elapsedMs(responseTarget, book.receivedNs)),
        };
      }
    }
  }

  function expireQuotes(observedNs) {
    for (const [exchangeId, rows] of live) {
      const keep = [];
      for (const candidate of rows) {
        if (observedNs >= candidate.expiresNs + BigInt(maximumBookAgeMs) * NS_PER_MS) {
          candidate.status = 'expired_without_crossing';
          candidate.activeDurationMs = elapsedMs(candidate.activationNs, candidate.expiresNs);
          counts.expiredWithoutCrossing++;
        } else keep.push(candidate);
      }
      live.set(exchangeId, keep);
    }
  }

  function normalizeTrade(trade, receivedAt, observedNs) {
    counts.tradeReportsSeen++;
    try {
      if (!trade || typeof trade.marketId !== 'string' || typeof trade.exchangeId !== 'string'
        || members.get(trade.exchangeId) !== trade.marketId || trade.tournamentId !== scope.tournamentId
        || !Number.isSafeInteger(trade.sequence) || trade.sequence < 0 || String(trade.sequence) !== String(trade.id)
        || !Number.isSafeInteger(trade.quantity) || trade.quantity < 1) throw Error('TRADE_SCOPE_OR_SHAPE');
      const executedNs = instantNs(trade.executedAt), receivedNs = instantNs(receivedAt);
      if (executedNs > observedNs || receivedNs > observedNs || !Number.isSafeInteger(trade.sequence)) throw Error('TRADE_CLOCK');
      const price = amount(normalizeOfficialFillTick(trade.price).price);
      const id = `${trade.marketId}|${trade.exchangeId}|${trade.id}`;
      const signature = `${trade.executedAt}|${price}|${trade.quantity}`;
      const prior = tradeIds.get(id);
      if (prior !== undefined) {
        if (prior !== signature) throw Error('TRADE_IDENTITY_CONFLICT');
        counts.duplicateTradeReports++;
        return null;
      }
      tradeIds.set(id, signature);
      return { marketId: trade.marketId, exchangeId: trade.exchangeId, executedNs, receivedNs, price, id };
    } catch (error) {
      if (error.message === 'TRADE_IDENTITY_CONFLICT') throw error;
      counts.invalidTradeReports++;
      return null;
    }
  }

  function applyTrades(trades, receivedAt, observedNs) {
    for (const raw of trades) {
      const trade = normalizeTrade(raw, receivedAt, observedNs);
      if (!trade) continue;
      for (const candidate of crossed) {
        if (candidate.tapeCorroborated || candidate.marketId !== trade.marketId || candidate.exchangeId !== trade.exchangeId) continue;
        if (trade.executedNs < candidate.activationNs || trade.executedNs >= candidate.expiresNs
          || trade.executedNs < instantNs(candidate.proxyCrossSourceAt) || trade.receivedNs < candidate.fillKnownNs) continue;
        const strictlyThrough = candidate.action === 'buy' ? trade.price <= candidate.limit - TICK : trade.price >= candidate.limit + TICK;
        if (!strictlyThrough) continue;
        candidate.tapeCorroborated = true;
        candidate.status = 'tape_corroborated';
        candidate.tapeConfirmedAt = new Date(Number(trade.receivedNs / NS_PER_MS)).toISOString();
        candidate.tapeExecutionAt = new Date(Number(trade.executedNs / NS_PER_MS)).toISOString();
        candidate.tapeReportLagMs = elapsedMs(trade.executedNs, trade.receivedNs);
        candidate.bookObservationLeadMs = elapsedMs(candidate.fillKnownNs, trade.receivedNs);
        counts.tapeCorroboratedProxies++;
      }
    }
  }

  function observeBatch({ observedAt, receivedAt = observedAt, books = [], trades = [] }) {
    const observedNs = instantNs(observedAt), envelopeReceivedNs = instantNs(receivedAt);
    if (observedNs < lastObservedNs || envelopeReceivedNs > observedNs) throw Error('BOOK_CROSSING_CLOCK');
    lastObservedNs = observedNs;
    counts.observedBatches++;
    // Books first: a same-envelope trade report may independently corroborate a newly
    // observed crossing, but the report is never used to invent the crossing itself.
    for (const raw of books) {
      const book = normalizeBook(raw, observedNs);
      if (!book) continue;
      tryCrosses(book, observedNs);
      const slot = Math.floor(Number(book.receivedNs / NS_PER_MS) / quoteEveryMs);
      if (lastQuoteSlot.get(book.exchangeId) === slot) continue;
      lastQuoteSlot.set(book.exchangeId, slot);
      const activationNs = observedNs + BigInt(activationLatencyMs) * NS_PER_MS;
      const expiresNs = activationNs + BigInt(quoteTtlMs) * NS_PER_MS;
      for (const action of ['buy', 'sell']) {
        const candidate = {
          id: candidates.length + 1,
          marketId: book.marketId,
          exchangeId: book.exchangeId,
          action,
          decisionAt: observedAt,
          decisionReceiptAt: book.receivedAt,
          decisionNs: observedNs,
          activationAt: new Date(Number(activationNs / NS_PER_MS)).toISOString(),
          activationNs,
          expiresAt: new Date(Number(expiresNs / NS_PER_MS)).toISOString(),
          expiresNs,
          limit: action === 'buy' ? book.bid : book.ask,
          queueAheadShares: action === 'buy' ? book.bidQty : book.askQty,
          status: 'resting',
          tapeCorroborated: false,
        };
        candidates.push(candidate);
        const liveRows = live.get(book.exchangeId) ?? [];
        liveRows.push(candidate);
        live.set(book.exchangeId, liveRows);
        counts.candidateQuotes++;
      }
    }
    expireQuotes(observedNs);
    applyTrades(trades, receivedAt, observedNs);
  }

  function report({ splitAt, purgeMs = 60000, source = null } = {}) {
    const splitNs = instantNs(splitAt), developmentEndNs = splitNs - BigInt(purgeMs) * NS_PER_MS;
    const fold = candidate => candidate.decisionNs < developmentEndNs ? 'development'
      : candidate.decisionNs >= splitNs ? 'heldOut' : 'purged';
    const rows = [];
    for (const candidate of candidates) {
      if (candidate.status !== 'book_crossing_proxy_unconfirmed' && candidate.status !== 'tape_corroborated') continue;
      const candidateFold = fold(candidate);
      if (candidateFold === 'purged') continue;
      for (const horizon of markoutHorizonsMs) {
        const mark = candidate.marks?.[horizon];
        if (!mark || mark.censored) continue;
        if (candidateFold === 'development' && instantNs(mark.availableAt) >= developmentEndNs) continue;
        rows.push({ candidate, horizon, fold: candidateFold, mark });
      }
    }
    const folds = ['development', 'heldOut'].map(name => ({
      fold: name,
      actions: ['buy', 'sell'].map(action => ({
        action,
        candidateQuotes: candidates.filter(candidate => fold(candidate) === name && candidate.action === action).length,
        bookCrossingProxies: candidates.filter(candidate => fold(candidate) === name && candidate.action === action && (candidate.status === 'book_crossing_proxy_unconfirmed' || candidate.status === 'tape_corroborated')).length,
        tapeCorroboratedProxies: candidates.filter(candidate => fold(candidate) === name && candidate.action === action && candidate.tapeCorroborated).length,
        unconfirmedBookCrossings: candidates.filter(candidate => fold(candidate) === name && candidate.action === action && candidate.status === 'book_crossing_proxy_unconfirmed').length,
        horizons: markoutHorizonsMs.map(horizon => {
          const selected = rows.filter(row => row.fold === name && row.candidate.action === action && row.horizon === horizon);
          return { horizonMs: horizon, executable: summary(selected.map(row => ({ ...row.candidate, candidate: row.candidate, net: row.mark.net }))) };
        }),
      })),
    }));
    const reportLags = candidates.filter(candidate => candidate.tapeCorroborated).map(candidate => candidate.tapeReportLagMs).sort((a, b) => a - b);
    const observationLeads = candidates.filter(candidate => candidate.tapeCorroborated).map(candidate => candidate.bookObservationLeadMs).sort((a, b) => a - b);
    return {
      schemaVersion: 1,
      mode: 'causal_scoped_book_crossing_fill_proxy_v1',
      source,
      policy: {
        quote: 'hypothetical one-unit YES best-bid/best-ask join sampled every quoteEveryMs from fresh scoped sequenced books',
        activationLatencyMs,
        quoteTtlMs,
        quoteEveryMs,
        bookCrossingProxy: 'buy only when a fresh scoped sequenced best bid moves below the active limit; sell only when best ask moves above; crossing alone can be cancellation/depletion and is never called a fill',
        tapeCorroboration: 'separate later scoped strict-through trade print within candidate active event-time window, delivered no earlier than local book-crossing receipt',
        markout: 'proxy-cross source time plus horizon AND crossing-book local receipt plus exitResponseLatencyMs; fresh sequenced later scoped book, bounded event and response delay',
        exitResponseLatencyMs,
        maximumBookAgeMs,
        maximumMarkoutDelayMs,
        markoutHorizonsMs,
        fees: '0.000000',
        stress: 'subtract one tick from entry and one tick from exit (0.010000 per unit) from executable markout',
        chronologicalSplit: { splitAt, purgeMs, developmentMarksMustBeAvailableBefore: new Date(Number(developmentEndNs / NS_PER_MS)).toISOString() },
      },
      counts,
      proxyEvidence: {
        bookCrossingProxies: candidates.filter(candidate => candidate.status === 'book_crossing_proxy_unconfirmed' || candidate.status === 'tape_corroborated').length,
        tapeCorroboratedProxies: candidates.filter(candidate => candidate.tapeCorroborated).length,
        unconfirmedBookCrossings: candidates.filter(candidate => candidate.status === 'book_crossing_proxy_unconfirmed').length,
        invalidatedBeforeCrossing: candidates.filter(candidate => candidate.status === 'invalidated_before_crossing').length,
        expiredWithoutCrossing: candidates.filter(candidate => candidate.status === 'expired_without_crossing').length,
        tapeReportLagMs: { samples: reportLags.length, p50: quantile(reportLags, 0.5), p90: quantile(reportLags, 0.9), p99: quantile(reportLags, 0.99) },
        localBookObservationLeadMs: { samples: observationLeads.length, p50: quantile(observationLeads, 0.5), p90: quantile(observationLeads, 0.9), p99: quantile(observationLeads, 0.99) },
      },
      folds,
      limitations: [
        'A best-price move beyond the hypothetical limit is a depletion/crossing proxy only; cancellations, hidden liquidity, queue position, acceptance and actual fill are unknown.',
        'A later strict-through tape print corroborates market trading beyond the limit but does not prove this hypothetical order filled or establish priority.',
        'Only one unit is valued at the limit; funding, inventory, matching rules and concurrent orders are not simulated.',
        'Captured book sampling, local receipt times, delayed trade reports, clock alignment and invalidation gaps limit latency inference.',
        'Markouts are local counterfactual prices, not actual realized P&L; the analysis authorizes no contest order or runtime change.',
      ],
      actualFillsProven: false,
      profitabilityEstablished: false,
      contestOrders: 0,
      networkRequests: 0,
    };
  }

  return { observeBatch, boundary: invalidation, boundaryMarket: clearMarket, report };
}
