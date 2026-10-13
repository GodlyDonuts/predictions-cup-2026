import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { amount, money, SCALE, TICK } from '../paper/fixed.mjs';
import { instantNs } from '../feed/contract.mjs';
import { normalizeOfficialFillTick } from '../execution/api-tick-price.mjs';

const NS_PER_MS = 1_000_000n;
const LATENCY_NS = 3_000n * NS_PER_MS;
const LIFE_NS = 10_000n * NS_PER_MS;
const FRESH_NS = 10_000n * NS_PER_MS;
const iso = ns => new Date(Number(ns / NS_PER_MS)).toISOString();
const keyOf = (marketId, exchangeId) => `${marketId}\u0000${exchangeId}`;

function normalizePrice(value) {
  const normalized = normalizeOfficialFillTick(value);
  return amount(normalized.price);
}

function validateBook(book, observedNs) {
  if (!book || typeof book.marketId !== 'string' || !book.marketId || typeof book.exchangeId !== 'string' || !book.exchangeId) throw Error('ROUNDTRIP_BOOK_SCOPE');
  if (typeof book.sequence !== 'string' || !/^(0|[1-9]\d*)$/.test(book.sequence)) throw Error('ROUNDTRIP_BOOK_SEQUENCE');
  const sourceNs = instantNs(book.sourceAt), requestNs = instantNs(book.requestStartedAt), receivedNs = instantNs(book.receivedAt);
  if (requestNs > receivedNs || sourceNs > observedNs || receivedNs > observedNs) throw Error('ROUNDTRIP_BOOK_CLOCK');
  if (!Array.isArray(book.bids) || !book.bids.length || !Array.isArray(book.asks) || !book.asks.length) throw Error('ROUNDTRIP_BOOK_DEPTH');
  const parseLevels = levels => {
    let previous = null;
    return levels.map(level => {
      if (!Number.isSafeInteger(level?.quantity) || level.quantity < 1) throw Error('ROUNDTRIP_BOOK_DEPTH');
      const px = normalizePrice(level.price);
      if (previous !== null && (levels === book.bids ? px >= previous : px <= previous)) throw Error('ROUNDTRIP_BOOK_ORDER');
      previous = px;
      return { price: px, quantity: level.quantity };
    });
  };
  const bids = parseLevels(book.bids), asks = parseLevels(book.asks);
  if (bids[0].price >= asks[0].price) throw Error('ROUNDTRIP_BOOK_CROSSED');
  return { ...book, sequenceValue: BigInt(book.sequence), sourceNs, requestNs, receivedNs, bids, asks };
}

function validateTrade(trade, observedNs) {
  if (!trade || typeof trade.marketId !== 'string' || !trade.marketId || typeof trade.exchangeId !== 'string' || !trade.exchangeId || typeof trade.id !== 'string' || !trade.id || !Number.isSafeInteger(trade.sequence) || trade.sequence < 0 || String(trade.sequence) !== trade.id || !Number.isSafeInteger(trade.quantity) || trade.quantity < 1) throw Error('ROUNDTRIP_TRADE_SCHEMA');
  const executedNs = instantNs(trade.executedAt);
  if (executedNs > observedNs) throw Error('ROUNDTRIP_TRADE_FUTURE');
  return { ...trade, priceMicros: normalizePrice(trade.price), executedNs };
}

function pAndL(entrySide, entryPrice, exitPrice) {
  return entrySide === 'buy' ? exitPrice - entryPrice : entryPrice - exitPrice;
}

function summarize(markets, completions, counts, throughNs, source) {
  const byMarket = [];
  let completedGross = 0n, stressPerFill = 0n, stressOneTickRoundTrip = 0n, openExecutable = 0n, openStressPerFill = 0n;
  let valuedInventory = 0, unvaluedInventory = 0;
  for (const [marketId, state] of markets) {
    const marketCompletions = completions.filter(row => row.marketId === marketId);
    const marketPosition = state.position;
    const gross = marketCompletions.reduce((sum, row) => sum + row.grossPnlMicros, 0n);
    const perFill = marketCompletions.reduce((sum, row) => sum + row.stressOneTickPerFillMicros, 0n);
    const oneTick = marketCompletions.reduce((sum, row) => sum + row.stressOneTickTotalMicros, 0n);
    completedGross += gross;
    stressPerFill += perFill;
    stressOneTickRoundTrip += oneTick;
    let mark = null, stressedMark = null, markSourceAt = null, markDepth = null;
    if (marketPosition) {
      const executable = marketPosition.latestPostFillBook;
      if (executable && throughNs >= executable.receivedNs && throughNs - executable.receivedNs <= FRESH_NS) {
        const level = marketPosition.side === 'buy' ? executable.bids[0] : executable.asks[0];
        if (level && level.quantity >= 1) {
          const liquidation = marketPosition.side === 'buy' ? level.price : level.price;
          mark = pAndL(marketPosition.side, marketPosition.entryPriceMicros, liquidation);
          stressedMark = mark - 2n * TICK;
          markSourceAt = iso(executable.sourceNs);
          markDepth = level.quantity;
          valuedInventory++;
          openExecutable += mark;
          openStressPerFill += stressedMark;
        } else unvaluedInventory++;
      } else unvaluedInventory++;
    }
    byMarket.push({
      marketId,
      exchangeId: state.exchangeId,
      entryQuotePairs: counts.entryQuotePairsByMarket.get(marketId) ?? 0,
      entryProxies: counts.entryProxiesByMarket.get(marketId) ?? 0,
      completedExits: marketCompletions.length,
      unresolvedInventory: marketPosition ? 1 : 0,
      expiredUnfilledEntryPairs: counts.expiredPairsByMarket.get(marketId) ?? 0,
      boundaryPurgedEntryPairs: counts.purgedPairsByMarket.get(marketId) ?? 0,
      realizedGrossPnl: money(gross),
      stressOneTickPerFillPnl: money(perFill),
      stressOneTickTotalRoundTripPnl: money(oneTick),
      openInventoryExecutableMarkPnl: mark === null ? null : money(mark),
      openInventoryStressOneTickPerFillPnl: stressedMark === null ? null : money(stressedMark),
      meanDisplayedBidQueueAhead: counts.queueSamplesByMarket.get(marketId) ? counts.bidQueueByMarket.get(marketId) / counts.queueSamplesByMarket.get(marketId) : null,
      meanDisplayedAskQueueAhead: counts.queueSamplesByMarket.get(marketId) ? counts.askQueueByMarket.get(marketId) / counts.queueSamplesByMarket.get(marketId) : null,
      openInventoryMarkSourceAt: markSourceAt,
      openInventoryExecutableDepth: markDepth,
    });
  }
  const totalGross = completedGross + openExecutable;
  return {
    schemaVersion: 1,
    mode: 'fixed_prefix_fee_free_maker_to_maker_proxy_replay_v1',
    executionAuthority: false,
    source,
    policy: {
      quantity: 1,
      feePerShare: '0.000000',
      feeBps: 0,
      entry: 'first fresh book while flat with no live entry pair; post one-share bid at best bid and ask at best ask',
      activationLatencyMs: 3000,
      quoteLifeAfterActivationMs: 10000,
      fillProxy: 'same-market public print strictly through the limit; execution and observation must both occur after activation, before expiry, and observation must be after decision',
      positionLimit: 'one active position per market; first entry proxy cancels the other entry quote',
      exit: 'one passive target quote, one tick favorable to entry, with the same 3000 ms activation latency and 10000 ms quote life',
      queueProxy: 'displayed quantity at the joined entry price is reported as queue-ahead context; strict-through print is still not queue-ranked execution proof',
      primaryStress: 'one adverse tick per fill; a completed round trip is stressed by two ticks',
      secondaryStress: 'one adverse tick total per completed round trip',
      noHoldoutOrTuning: true,
    },
    counts: {
      markets: markets.size,
      entryQuotePairs: counts.entryQuotePairs,
      entryQuoteOrders: counts.entryQuotePairs * 2,
      entryProxies: counts.entryProxies,
      completedExits: completions.length,
      unresolvedInventory: [...markets.values()].filter(state => state.position).length,
      valuedUnresolvedInventory: valuedInventory,
      unvaluedUnresolvedInventory: unvaluedInventory,
      expiredUnfilledEntryPairs: counts.expiredEntryPairs,
      boundaryPurgedEntryPairs: counts.purgedEntryPairs,
      boundaryPurgedExitQuotes: counts.purgedExitQuotes,
      expiredUnfilledExitQuotes: counts.expiredExitQuotes,
      canceledByBoundaryEntryPairs: counts.canceledEntryPairs,
      canceledByBoundaryExitQuotes: counts.canceledExitQuotes,
      uniqueTradePrints: counts.uniqueTrades,
      duplicateTrades: counts.duplicateTrades,
      rejectedBooks: counts.rejectedBooks,
    },
    aggregate: {
      realizedGrossPnl: money(completedGross),
      realizedStressOneTickPerFillPnl: money(stressPerFill),
      realizedStressOneTickTotalRoundTripPnl: money(stressOneTickRoundTrip),
      openInventoryExecutableMarkPnl: money(openExecutable),
      openInventoryStressOneTickPerFillPnl: money(openStressPerFill),
      totalGrossIncludingExecutableInventoryMark: money(totalGross),
      totalStressOneTickPerFillPnl: money(stressPerFill + openStressPerFill),
      totalStressOneTickTotalRoundTripPnl: money(stressOneTickRoundTrip + openExecutable - BigInt(valuedInventory) * TICK),
      allFees: '0.000000',
    },
    completions: completions.map(row => ({
      marketId: row.marketId,
      exchangeId: row.exchangeId,
      side: row.side,
      entryPrice: money(row.entryPriceMicros),
      exitPrice: money(row.exitPriceMicros),
      entryDecisionAt: row.entryDecisionAt,
      entryActivationAt: row.entryActivationAt,
      entryExpiresAt: row.entryExpiresAt,
      entryFillExecutedAt: row.entryFillExecutedAt,
      entryFillObservedAt: row.entryFillObservedAt,
      exitDecisionAt: row.exitDecisionAt,
      exitActivationAt: row.exitActivationAt,
      exitExpiresAt: row.exitExpiresAt,
      exitFillExecutedAt: row.exitFillExecutedAt,
      exitFillObservedAt: row.exitFillObservedAt,
      entryDisplayedQueueAhead: row.entryDisplayedQueueAhead,
      exitDisplayedQueueAhead: row.exitDisplayedQueueAhead,
      grossPnl: money(row.grossPnlMicros),
      stressOneTickPerFillPnl: money(row.stressOneTickPerFillMicros),
      stressOneTickTotalRoundTripPnl: money(row.stressOneTickTotalMicros),
      fillEvidence: 'strict-through public-print proxy only; no aggressor side or queue rank',
    })),
    perMarket: byMarket,
    limitations: [
      'Anonymous public prints do not establish aggressor side, queue position, or actual fills; all entries and exits are strict-through proxies.',
      'Fee-free results omit venue fees, rebates, queue latency, order priority, partial fills, and cancel/replace costs.',
      'A one-share top-of-book depth mark is executable only as a displayed-depth proxy at the cited later book; no market impact is modeled.',
      'The primary adverse stress applies one tick against each fill, two ticks per completed round trip.',
    ],
  };
}

export function hashFixedPrefix(file, bytes) {
  if (!Number.isSafeInteger(bytes) || bytes < 1) throw Error('ROUNDTRIP_PREFIX_BOUND');
  const fd = fs.openSync(file, 'r');
  try {
    const before = fs.fstatSync(fd);
    if (before.size < bytes) throw Error('ROUNDTRIP_PREFIX_TRUNCATED');
    const hash = createHash('sha256'), buffer = Buffer.alloc(1024 * 1024);
    let offset = 0;
    while (offset < bytes) {
      const length = Math.min(buffer.length, bytes - offset);
      const read = fs.readSync(fd, buffer, 0, length, offset);
      if (read !== length) throw Error('ROUNDTRIP_PREFIX_TRUNCATED');
      hash.update(buffer.subarray(0, read));
      offset += read;
    }
    const after = fs.fstatSync(fd);
    if (before.dev !== after.dev || before.ino !== after.ino || after.size < bytes) throw Error('ROUNDTRIP_PREFIX_REPLACED');
    return { bytes, sha256: hash.digest('hex'), dev: after.dev, ino: after.ino, sourceSizeAtCheck: after.size };
  } finally {
    fs.closeSync(fd);
  }
}

export function verifyFixedPrefix(file, expected, frozenIdentity = null) {
  if (!expected || !Number.isSafeInteger(expected.bytes) || !/^[a-f0-9]{64}$/.test(expected.sha256)) throw Error('ROUNDTRIP_PREFIX_EXPECTED');
  const actual = hashFixedPrefix(file, expected.bytes);
  if (actual.sha256 !== expected.sha256) throw Error('ROUNDTRIP_PREFIX_HASH');
  if (frozenIdentity && (actual.dev !== frozenIdentity.dev || actual.ino !== frozenIdentity.ino)) throw Error('ROUNDTRIP_PREFIX_REPLACED');
  return actual;
}

export function createPassiveRoundtripV1({ members }) {
  if (!Array.isArray(members) || !members.length) throw Error('ROUNDTRIP_MEMBERS');
  const scoped = new Map();
  for (const member of members) {
    if (!member || typeof member.marketId !== 'string' || !member.marketId || typeof member.exchangeId !== 'string' || !member.exchangeId || scoped.has(member.marketId)) throw Error('ROUNDTRIP_MEMBERS');
    scoped.set(member.marketId, member.exchangeId);
  }
  const markets = new Map([...scoped].map(([marketId, exchangeId]) => [marketId, { exchangeId, latestBook: null, sequenceValue: null, sourceNs: null, entryOrders: [], position: null }]));
  const completions = [], tradesSeen = new Map();
  const counts = { entryQuotePairs: 0, entryProxies: 0, expiredEntryPairs: 0, purgedEntryPairs: 0, expiredExitQuotes: 0, purgedExitQuotes: 0, canceledEntryPairs: 0, canceledExitQuotes: 0, uniqueTrades: 0, duplicateTrades: 0, rejectedBooks: 0, entryProxiesByMarket: new Map(), entryQuotePairsByMarket: new Map(), bidQueueByMarket: new Map(), askQueueByMarket: new Map(), queueSamplesByMarket: new Map(), expiredPairsByMarket: new Map(), purgedPairsByMarket: new Map() };
  let lastObservedNs = -1n;

  const stateFor = (marketId, exchangeId) => {
    const state = markets.get(marketId);
    if (!state || state.exchangeId !== exchangeId) throw Error('ROUNDTRIP_BOOK_SCOPE');
    return state;
  };
  const expire = (marketId, state, observedNs) => {
    if (state.entryOrders.length && observedNs >= state.entryOrders[0].expiresNs) {
      state.entryOrders = [];
      counts.expiredEntryPairs++;
      counts.expiredPairsByMarket.set(marketId, (counts.expiredPairsByMarket.get(marketId) ?? 0) + 1);
    }
    if (state.position?.exitOrder && observedNs >= state.position.exitOrder.expiresNs) {
      state.position.exitOrder = null;
      counts.expiredExitQuotes++;
    }
  };
  const postEntries = (marketId, state, book, observedNs) => {
    if (state.position || state.entryOrders.length) return;
    const bid = book.bids[0], ask = book.asks[0];
    const decisionNs = observedNs, activationNs = decisionNs + LATENCY_NS, expiresNs = activationNs + LIFE_NS;
    counts.entryQuotePairs++;
    counts.entryQuotePairsByMarket.set(marketId, (counts.entryQuotePairsByMarket.get(marketId) ?? 0) + 1);
    counts.bidQueueByMarket.set(marketId, (counts.bidQueueByMarket.get(marketId) ?? 0) + bid.quantity);
    counts.askQueueByMarket.set(marketId, (counts.askQueueByMarket.get(marketId) ?? 0) + ask.quantity);
    counts.queueSamplesByMarket.set(marketId, (counts.queueSamplesByMarket.get(marketId) ?? 0) + 1);
    state.entryOrders = [
      { side: 'buy', limitMicros: bid.price, queueAhead: bid.quantity, decisionNs, activationNs, expiresNs },
      { side: 'sell', limitMicros: ask.price, queueAhead: ask.quantity, decisionNs, activationNs, expiresNs },
    ];
  };
  const createExit = (marketId, state, entry, observedNs) => {
    const book = state.latestBook;
    if (!book || observedNs < book.receivedNs || observedNs - book.receivedNs > FRESH_NS) return null;
    const limitMicros = entry.side === 'buy' ? entry.limitMicros + TICK : entry.limitMicros - TICK;
    if (limitMicros < TICK || limitMicros > SCALE - TICK) return null;
    const passive = entry.side === 'buy' ? limitMicros > book.bids[0].price : limitMicros < book.asks[0].price;
    if (!passive) return null;
    const levels = entry.side === 'buy' ? book.asks : book.bids;
    const queueAhead = levels.find(level => level.price === limitMicros)?.quantity ?? 0;
    return { side: entry.side === 'buy' ? 'sell' : 'buy', limitMicros, queueAhead, decisionNs: observedNs, activationNs: observedNs + LATENCY_NS, expiresNs: observedNs + LATENCY_NS + LIFE_NS };
  };
  const fillEntry = (marketId, state, order, trade, observedNs) => {
    counts.entryProxies++;
    counts.entryProxiesByMarket.set(marketId, (counts.entryProxiesByMarket.get(marketId) ?? 0) + 1);
    const position = {
      side: order.side,
      entryPriceMicros: order.limitMicros,
      entryDecisionAt: iso(order.decisionNs),
      entryActivationAt: iso(order.activationNs),
      entryExpiresAt: iso(order.expiresNs),
      entryFillExecutedAt: trade.executedAt,
      entryFillObservedAt: iso(observedNs),
      entryDisplayedQueueAhead: order.queueAhead,
      entryTradeSequence: trade.id,
      fillKnownNs: observedNs,
      latestPostFillBook: null,
      exitOrder: null,
    };
    position.exitOrder = createExit(marketId, state, order, observedNs);
    state.position = position;
  };
  const throughTrade = (order, trade, observedNs) => observedNs > order.decisionNs && observedNs < order.expiresNs && trade.executedNs >= order.activationNs && trade.executedNs < order.expiresNs && (order.side === 'buy' ? trade.priceMicros < order.limitMicros : trade.priceMicros > order.limitMicros);

  function observeBatch({ observedAt, books = [], trades = [] }) {
    const observedNs = instantNs(observedAt);
    if (observedNs < lastObservedNs) throw Error('ROUNDTRIP_CLOCK_ROLLBACK');
    lastObservedNs = observedNs;
    if (!Array.isArray(books) || !Array.isArray(trades)) throw Error('ROUNDTRIP_BATCH');
    const acceptedBooks = [];
    for (const raw of books) {
      try {
        const book = validateBook(raw, observedNs), state = stateFor(book.marketId, book.exchangeId);
        if (observedNs - book.receivedNs > FRESH_NS) { counts.rejectedBooks++; continue; }
        if (state.sequenceValue !== null && (book.sequenceValue < state.sequenceValue || book.sequenceValue === state.sequenceValue && book.sourceNs <= state.sourceNs)) continue;
        state.sequenceValue = book.sequenceValue;
        state.sourceNs = book.sourceNs;
        state.latestBook = book;
        acceptedBooks.push([book.marketId, state, book]);
      } catch (error) {
        if (error.message === 'ROUNDTRIP_BOOK_SCOPE') throw error;
        counts.rejectedBooks++;
      }
    }
    for (const [marketId, state, book] of acceptedBooks) {
      expire(marketId, state, observedNs);
      if (state.position && observedNs > state.position.fillKnownNs && book.sourceNs > instantNs(state.position.entryFillExecutedAt)) state.position.latestPostFillBook = book;
      if (!state.position) postEntries(marketId, state, book, observedNs);
    }

    const orderedTrades = [...trades].map(trade => validateTrade(trade, observedNs)).sort((a, b) => a.executedNs < b.executedNs ? -1 : a.executedNs > b.executedNs ? 1 : a.id.localeCompare(b.id));
    for (const trade of orderedTrades) {
      const state = stateFor(trade.marketId, trade.exchangeId);
      const signature = `${trade.executedAt}|${trade.priceMicros}|${trade.quantity}`;
      const identity = keyOf(trade.marketId, trade.exchangeId) + `\u0000${trade.id}`;
      const prior = tradesSeen.get(identity);
      if (prior !== undefined) {
        if (prior !== signature) throw Error('ROUNDTRIP_TRADE_IDENTITY_CONFLICT');
        counts.duplicateTrades++;
        continue;
      }
      tradesSeen.set(identity, signature);
      counts.uniqueTrades++;
      expire(trade.marketId, state, observedNs);
      const position = state.position;
      if (position?.exitOrder) {
        const order = position.exitOrder;
        if (throughTrade(order, trade, observedNs) && (order.side === 'sell' ? trade.priceMicros > order.limitMicros : trade.priceMicros < order.limitMicros)) {
          const grossPnlMicros = pAndL(position.side, position.entryPriceMicros, order.limitMicros);
          completions.push({
            marketId: trade.marketId, exchangeId: trade.exchangeId, side: position.side,
            entryPriceMicros: position.entryPriceMicros, exitPriceMicros: order.limitMicros,
            entryDecisionAt: position.entryDecisionAt, entryActivationAt: position.entryActivationAt, entryExpiresAt: position.entryExpiresAt,
            entryFillExecutedAt: position.entryFillExecutedAt, entryFillObservedAt: position.entryFillObservedAt,
            exitDecisionAt: iso(order.decisionNs), exitActivationAt: iso(order.activationNs), exitExpiresAt: iso(order.expiresNs),
            exitFillExecutedAt: trade.executedAt, exitFillObservedAt: iso(observedNs),
            entryDisplayedQueueAhead: position.entryDisplayedQueueAhead, exitDisplayedQueueAhead: order.queueAhead,
            grossPnlMicros, stressOneTickPerFillMicros: grossPnlMicros - 2n * TICK, stressOneTickTotalMicros: grossPnlMicros - TICK,
          });
          state.position = null;
          continue;
        }
      }
      if (position || !state.entryOrders.length) continue;
      const index = state.entryOrders.findIndex(order => throughTrade(order, trade, observedNs));
      if (index < 0) continue;
      const [order] = state.entryOrders.splice(index, 1);
      state.entryOrders = [];
      fillEntry(trade.marketId, state, order, trade, observedNs);
    }
    return { books: acceptedBooks.length, trades: orderedTrades.length };
  }

  function boundary({ marketId = null, exchangeId = null } = {}) {
    for (const [id, state] of markets) {
      if (marketId !== null && id !== marketId || exchangeId !== null && state.exchangeId !== exchangeId) continue;
      if (state.entryOrders.length) {
        state.entryOrders = [];
        counts.canceledEntryPairs++;
      }
      if (state.position?.exitOrder) {
        state.position.exitOrder = null;
        counts.canceledExitQuotes++;
      }
    }
  }

  function report({ throughAt, source = null }) {
    const throughNs = instantNs(throughAt);
    if (lastObservedNs > throughNs) throw Error('ROUNDTRIP_REPORT_BEFORE_DATA');
    for (const [marketId, state] of markets) {
      if (!state.entryOrders.length) continue;
      if (state.entryOrders[0].expiresNs > throughNs) {
        state.entryOrders = [];
        counts.purgedEntryPairs++;
        counts.purgedPairsByMarket.set(marketId, (counts.purgedPairsByMarket.get(marketId) ?? 0) + 1);
      } else {
        state.entryOrders = [];
        counts.expiredEntryPairs++;
        counts.expiredPairsByMarket.set(marketId, (counts.expiredPairsByMarket.get(marketId) ?? 0) + 1);
      }
    }
    for (const state of markets.values()) {
      const order = state.position?.exitOrder;
      if (!order) continue;
      if (order.expiresNs > throughNs) counts.purgedExitQuotes++;
      else counts.expiredExitQuotes++;
      state.position.exitOrder = null;
    }
    return summarize(markets, completions, counts, throughNs, source);
  }

  return { observeBatch, boundary, report };
}

