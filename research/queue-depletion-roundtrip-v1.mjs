import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { amount, money, SCALE, TICK } from '../paper/fixed.mjs';
import { instantNs } from '../feed/contract.mjs';
import { normalizeOfficialFillTick } from '../execution/api-tick-price.mjs';

const NS_PER_MS = 1_000_000n;
const LATENCY_NS = 3_000n * NS_PER_MS;
const ORDER_LIFE_NS = 10_000n * NS_PER_MS;
const FRESH_NS = 10_000n * NS_PER_MS;
const asIso = ns => new Date(Number(ns / NS_PER_MS)).toISOString();
const marketKey = (marketId, exchangeId) => `${marketId}\u0000${exchangeId}`;

function priceMicros(value) {
  return amount(normalizeOfficialFillTick(value).price);
}

function parseBook(raw, observedNs) {
  if (!raw || typeof raw.marketId !== 'string' || !raw.marketId || typeof raw.exchangeId !== 'string' || !raw.exchangeId) throw Error('QUEUE_BOOK_SCOPE');
  if (typeof raw.sequence !== 'string' || !/^(0|[1-9]\d*)$/.test(raw.sequence)) throw Error('QUEUE_BOOK_SEQUENCE');
  const sourceNs = instantNs(raw.sourceAt), receivedNs = instantNs(raw.receivedAt);
  if (sourceNs > observedNs || receivedNs > observedNs) throw Error('QUEUE_BOOK_CLOCK');
  if (!Array.isArray(raw.bids) || !raw.bids.length || !Array.isArray(raw.asks) || !raw.asks.length) throw Error('QUEUE_BOOK_DEPTH');
  const readLevels = (levels, descending) => {
    let prior = null;
    return levels.map(level => {
      if (!Number.isSafeInteger(level?.quantity) || level.quantity < 1) throw Error('QUEUE_BOOK_DEPTH');
      const px = priceMicros(level.price);
      if (prior !== null && (descending ? px >= prior : px <= prior)) throw Error('QUEUE_BOOK_ORDER');
      prior = px;
      return { price: px, quantity: level.quantity };
    });
  };
  const bids = readLevels(raw.bids, true), asks = readLevels(raw.asks, false);
  if (bids[0].price >= asks[0].price) throw Error('QUEUE_BOOK_CROSSED');
  return { ...raw, sequenceValue: BigInt(raw.sequence), sourceNs, receivedNs, observedNs, bids, asks };
}

function parseTrade(raw, observedNs) {
  if (!raw || typeof raw.marketId !== 'string' || !raw.marketId || typeof raw.exchangeId !== 'string' || !raw.exchangeId || typeof raw.id !== 'string' || !raw.id || !Number.isSafeInteger(raw.sequence) || raw.sequence < 0 || String(raw.sequence) !== raw.id || !Number.isSafeInteger(raw.quantity) || raw.quantity < 1) throw Error('QUEUE_TRADE_SCHEMA');
  const executedNs = instantNs(raw.executedAt);
  if (executedNs > observedNs) throw Error('QUEUE_TRADE_FUTURE');
  return { ...raw, priceValue: priceMicros(raw.price), executedNs };
}

function signature(trade) { return `${trade.executedAt}|${trade.priceValue}|${trade.quantity}`; }

function aggressorAt(state, trade) {
  let best = null;
  for (const book of state.bookHistory) {
    if (book.sourceNs > trade.executedNs || trade.executedNs - book.sourceNs > FRESH_NS || book.observedNs > state.currentObservedNs) continue;
    if (!best || book.sourceNs > best.sourceNs || book.sourceNs === best.sourceNs && book.sequenceValue > best.sequenceValue) best = book;
  }
  if (!best) return { side: null, book: null };
  const bid = best.bids[0].price, ask = best.asks[0].price;
  if (trade.priceValue <= bid) return { side: 'sell', book: best };
  if (trade.priceValue >= ask) return { side: 'buy', book: best };
  return { side: null, book: best };
}

function grossPnl(side, entry, exit) { return side === 'buy' ? exit - entry : entry - exit; }

export function hashByteRange(file, startByte, endByte) {
  if (!Number.isSafeInteger(startByte) || !Number.isSafeInteger(endByte) || startByte < 0 || endByte <= startByte) throw Error('QUEUE_PREFIX_RANGE');
  const fd = fs.openSync(file, 'r');
  try {
    const before = fs.fstatSync(fd);
    if (before.size < endByte) throw Error('QUEUE_PREFIX_TRUNCATED');
    const hash = createHash('sha256'), buffer = Buffer.alloc(1024 * 1024);
    let offset = startByte;
    while (offset < endByte) {
      const n = Math.min(buffer.length, endByte - offset);
      const read = fs.readSync(fd, buffer, 0, n, offset);
      if (read !== n) throw Error('QUEUE_PREFIX_TRUNCATED');
      hash.update(buffer.subarray(0, read));
      offset += read;
    }
    const after = fs.fstatSync(fd);
    if (before.dev !== after.dev || before.ino !== after.ino || after.size < endByte) throw Error('QUEUE_PREFIX_REPLACED');
    return { startByte, endByte, bytes: endByte - startByte, sha256: hash.digest('hex'), dev: after.dev, ino: after.ino, fileSizeAtCheck: after.size };
  } finally {
    fs.closeSync(fd);
  }
}

export function verifyByteRange(file, frozen) {
  if (!frozen || !/^[a-f0-9]{64}$/.test(frozen.sha256)) throw Error('QUEUE_PREFIX_EXPECTED');
  const actual = hashByteRange(file, frozen.startByte, frozen.endByte);
  if (actual.sha256 !== frozen.sha256) throw Error('QUEUE_PREFIX_HASH');
  if (actual.dev !== frozen.dev || actual.ino !== frozen.ino) throw Error('QUEUE_PREFIX_REPLACED');
  return actual;
}

export function createQueueDepletionRoundtripV1({ members, startAt }) {
  if (!Array.isArray(members) || !members.length) throw Error('QUEUE_MEMBERS');
  const startNs = instantNs(startAt), allowed = new Map();
  for (const member of members) {
    if (!member || typeof member.marketId !== 'string' || !member.marketId || typeof member.exchangeId !== 'string' || !member.exchangeId || allowed.has(member.marketId)) throw Error('QUEUE_MEMBERS');
    allowed.set(member.marketId, member.exchangeId);
  }
  const markets = new Map([...allowed].map(([marketId, exchangeId]) => [marketId, {
    exchangeId, bookHistory: [], latestBook: null, version: null, entryOrders: [], position: null,
    lastTradeNs: null, currentObservedNs: -1n,
  }]));
  const seenTrades = new Map(), completions = [];
  const counts = {
    quotePairs: 0, entryFillProxies: 0, completedExitProxies: 0, expiredEntryPairs: 0, censoredEntryPairs: 0,
    expiredExitQuotes: 0, censoredExitQuotes: 0, boundaryCanceledEntryPairs: 0, boundaryCanceledExitQuotes: 0,
    uniqueScopedTrades: 0, duplicateTradeRows: 0, outOfOrderTrades: 0, preStartTrades: 0,
    printsWithBook: 0, printsWithoutContemporaneousBook: 0, ambiguousPrints: 0, inferredBuyAggressorPrints: 0,
    inferredSellAggressorPrints: 0, invalidTrades: 0, invalidBooks: 0, entryQueuePrintEvents: 0,
    exitQueuePrintEvents: 0, entryQueueVolumeObserved: 0, exitQueueVolumeObserved: 0,
    quotePairsByMarket: new Map(), entryFillsByMarket: new Map(), completedByMarket: new Map(),
    expiredByMarket: new Map(), censoredByMarket: new Map(), queueBidByMarket: new Map(), queueAskByMarket: new Map(),
  };
  let lastObservedNs = -1n;

  function stateFor(marketId, exchangeId) {
    const state = markets.get(marketId);
    if (!state || state.exchangeId !== exchangeId) throw Error('QUEUE_MARKET_SCOPE');
    return state;
  }

  function expire(marketId, state, observedNs) {
    if (state.entryOrders.length && observedNs >= state.entryOrders[0].expiresNs) {
      state.entryOrders = [];
      counts.expiredEntryPairs++;
      counts.expiredByMarket.set(marketId, (counts.expiredByMarket.get(marketId) ?? 0) + 1);
    }
    if (state.position?.exitOrder && observedNs >= state.position.exitOrder.expiresNs) {
      state.position.exitOrder = null;
      counts.expiredExitQuotes++;
    }
  }

  function postEntryPair(marketId, state, book, decisionNs) {
    if (state.position || state.entryOrders.length) return;
    const bid = book.bids[0], ask = book.asks[0], activationNs = decisionNs + LATENCY_NS, expiresNs = activationNs + ORDER_LIFE_NS;
    counts.quotePairs++;
    counts.quotePairsByMarket.set(marketId, (counts.quotePairsByMarket.get(marketId) ?? 0) + 1);
    counts.queueBidByMarket.set(marketId, (counts.queueBidByMarket.get(marketId) ?? 0) + bid.quantity);
    counts.queueAskByMarket.set(marketId, (counts.queueAskByMarket.get(marketId) ?? 0) + ask.quantity);
    state.entryOrders = [
      { side: 'buy', limit: bid.price, queueAhead: bid.quantity, initialQueueAhead: bid.quantity, decisionNs, activationNs, expiresNs },
      { side: 'sell', limit: ask.price, queueAhead: ask.quantity, initialQueueAhead: ask.quantity, decisionNs, activationNs, expiresNs },
    ];
  }

  function postExit(state, entry, decisionNs) {
    const book = state.latestBook;
    if (!book || decisionNs < book.receivedNs || decisionNs - book.receivedNs > FRESH_NS) return null;
    const limit = entry.side === 'buy' ? entry.limit + TICK : entry.limit - TICK;
    if (limit < TICK || limit > SCALE - TICK) return null;
    const passive = entry.side === 'buy' ? limit > book.bids[0].price : limit < book.asks[0].price;
    if (!passive) return null;
    const levels = entry.side === 'buy' ? book.asks : book.bids;
    const queueAhead = levels.find(level => level.price === limit)?.quantity ?? 0;
    return { side: entry.side === 'buy' ? 'sell' : 'buy', limit, queueAhead, initialQueueAhead: queueAhead, decisionNs, activationNs: decisionNs + LATENCY_NS, expiresNs: decisionNs + LATENCY_NS + ORDER_LIFE_NS };
  }

  function fillEntry(marketId, state, order, trade, observedNs, aggressorBook) {
    counts.entryFillProxies++;
    counts.entryFillsByMarket.set(marketId, (counts.entryFillsByMarket.get(marketId) ?? 0) + 1);
    const position = {
      side: order.side, entryPrice: order.limit, entryDecisionAt: asIso(order.decisionNs),
      entryActivationAt: asIso(order.activationNs), entryExpiresAt: asIso(order.expiresNs),
      entryFillExecutedAt: trade.executedAt, entryFillObservedAt: asIso(observedNs),
      entryDisplayedQueueAhead: order.initialQueueAhead, entryQueueRemainingAtFill: order.queueAhead,
      entryTradeId: trade.id, entryAggressorBookAt: asIso(aggressorBook.sourceNs), entryAggressorBookAgeMs: Number((trade.executedNs - aggressorBook.sourceNs) / NS_PER_MS),
      entryObservedAtNs: observedNs, exitOrder: null, latestPostFillBook: null,
    };
    position.exitOrder = postExit(state, order, observedNs);
    state.position = position;
  }

  function eligible(order, trade, observedNs) {
    return observedNs > order.decisionNs && observedNs < order.expiresNs && trade.executedNs >= order.activationNs && trade.executedNs < order.expiresNs && trade.executedNs > order.decisionNs;
  }

  function consume(order, trade) {
    const atOrThrough = order.side === 'buy' ? trade.priceValue <= order.limit : trade.priceValue >= order.limit;
    if (!atOrThrough) return false;
    if (order.queueAhead >= trade.quantity) {
      order.queueAhead -= trade.quantity;
      return false;
    }
    order.queueAhead = 0;
    return true;
  }

  function observeBatch({ observedAt, books = [], trades = [] }) {
    const observedNs = instantNs(observedAt);
    if (observedNs < lastObservedNs) throw Error('QUEUE_CLOCK_ROLLBACK');
    lastObservedNs = observedNs;
    if (!Array.isArray(books) || !Array.isArray(trades)) throw Error('QUEUE_BATCH');

    const accepted = [];
    for (const raw of books) {
      try {
        const book = parseBook(raw, observedNs), state = stateFor(book.marketId, book.exchangeId);
        if (observedNs - book.receivedNs > FRESH_NS) { counts.invalidBooks++; continue; }
        if (state.version && (book.sequenceValue < state.version.sequence || book.sequenceValue === state.version.sequence && book.sourceNs <= state.version.sourceNs)) continue;
        state.version = { sequence: book.sequenceValue, sourceNs: book.sourceNs };
        state.latestBook = book;
        state.bookHistory.push(book);
        state.bookHistory = state.bookHistory.filter(point => observedNs - point.sourceNs <= FRESH_NS && point.sourceNs <= observedNs);
        state.currentObservedNs = observedNs;
        accepted.push([book.marketId, state, book]);
      } catch (error) {
        if (error.message === 'QUEUE_MARKET_SCOPE') throw error;
        counts.invalidBooks++;
      }
    }
    for (const [marketId, state, book] of accepted) {
      expire(marketId, state, observedNs);
      if (state.position && observedNs > state.position.entryObservedAtNs && book.sourceNs > instantNs(state.position.entryFillExecutedAt)) state.position.latestPostFillBook = book;
      if (!state.position) postEntryPair(marketId, state, book, observedNs);
    }

    const parsed = [];
    for (const raw of trades) {
      try {
        const trade = parseTrade(raw, observedNs);
        const state = stateFor(trade.marketId, trade.exchangeId);
        parsed.push({ trade, state });
      } catch (error) {
        if (error.message === 'QUEUE_MARKET_SCOPE') throw error;
        counts.invalidTrades++;
      }
    }
    parsed.sort((a, b) => a.trade.executedNs < b.trade.executedNs ? -1 : a.trade.executedNs > b.trade.executedNs ? 1 : a.trade.id.localeCompare(b.trade.id));
    for (const { trade, state } of parsed) {
      const identity = `${marketKey(trade.marketId, trade.exchangeId)}\u0000${trade.id}`;
      const sig = signature(trade), priorSig = seenTrades.get(identity);
      if (priorSig !== undefined) {
        if (priorSig !== sig) throw Error('QUEUE_TRADE_IDENTITY_CONFLICT');
        counts.duplicateTradeRows++;
        continue;
      }
      seenTrades.set(identity, sig);
      if (trade.executedNs < startNs) { counts.preStartTrades++; continue; }
      counts.uniqueScopedTrades++;
      if (state.lastTradeNs !== null && trade.executedNs < state.lastTradeNs) { counts.outOfOrderTrades++; continue; }
      state.lastTradeNs = trade.executedNs;
      state.currentObservedNs = observedNs;
      expire(trade.marketId, state, observedNs);
      const inferred = aggressorAt(state, trade);
      if (!inferred.book) { counts.printsWithoutContemporaneousBook++; continue; }
      counts.printsWithBook++;
      if (!inferred.side) { counts.ambiguousPrints++; continue; }
      if (inferred.side === 'buy') counts.inferredBuyAggressorPrints++;
      else counts.inferredSellAggressorPrints++;

      const position = state.position;
      const targetOrder = position?.exitOrder;
      if (targetOrder && eligible(targetOrder, trade, observedNs) && (targetOrder.side === 'buy' ? inferred.side === 'sell' : inferred.side === 'buy')) {
        const before = targetOrder.queueAhead;
        counts.exitQueuePrintEvents++;
        counts.exitQueueVolumeObserved += trade.quantity;
        if (consume(targetOrder, trade)) {
          const gross = grossPnl(position.side, position.entryPrice, targetOrder.limit);
          completions.push({
            marketId: trade.marketId, exchangeId: trade.exchangeId, side: position.side,
            entryPrice: position.entryPrice, exitPrice: targetOrder.limit,
            entryDecisionAt: position.entryDecisionAt, entryActivationAt: position.entryActivationAt, entryExpiresAt: position.entryExpiresAt,
            entryFillExecutedAt: position.entryFillExecutedAt, entryFillObservedAt: position.entryFillObservedAt,
            entryDisplayedQueueAhead: position.entryDisplayedQueueAhead, entryQueueRemainingAtFill: position.entryQueueRemainingAtFill,
            entryTradeId: position.entryTradeId, entryAggressorBookAt: position.entryAggressorBookAt,
            exitDecisionAt: asIso(targetOrder.decisionNs), exitActivationAt: asIso(targetOrder.activationNs), exitExpiresAt: asIso(targetOrder.expiresNs),
            exitFillExecutedAt: trade.executedAt, exitFillObservedAt: asIso(observedNs),
            exitDisplayedQueueAhead: targetOrder.initialQueueAhead, exitQueueRemainingAtFill: before,
            exitTradeId: trade.id, grossPnl: gross, stressOneTickPerFill: gross - 2n * TICK,
          });
          state.position = null;
          counts.completedExitProxies++;
          counts.completedByMarket.set(trade.marketId, (counts.completedByMarket.get(trade.marketId) ?? 0) + 1);
        }
        continue;
      }

      if (position || !state.entryOrders.length) continue;
      const order = state.entryOrders.find(candidate => eligible(candidate, trade, observedNs) && (candidate.side === 'buy' ? inferred.side === 'sell' : inferred.side === 'buy'));
      if (!order) continue;
      counts.entryQueuePrintEvents++;
      counts.entryQueueVolumeObserved += trade.quantity;
      order.initialQueueAhead ??= order.queueAhead;
      if (!consume(order, trade)) continue;
      state.entryOrders = [];
      fillEntry(trade.marketId, state, order, trade, observedNs, inferred.book);
    }
    return { acceptedBooks: accepted.length, parsedTrades: parsed.length };
  }

  function boundary({ marketId = null, exchangeId = null, preserveInventoryMarks = false } = {}) {
    for (const [id, state] of markets) {
      if ((marketId !== null && id !== marketId) || (exchangeId !== null && state.exchangeId !== exchangeId)) continue;
      if (state.entryOrders.length) { state.entryOrders = []; counts.boundaryCanceledEntryPairs++; }
      if (state.position?.exitOrder) { state.position.exitOrder = null; counts.boundaryCanceledExitQuotes++; }
      state.bookHistory = [];
      state.latestBook = null;
      state.version = null;
      state.currentObservedNs = -1n;
      state.lastTradeNs = null;
      if (state.position && !preserveInventoryMarks) state.position.latestPostFillBook = null;
    }
  }

  function report({ throughAt, source = null }) {
    const throughNs = instantNs(throughAt);
    if (lastObservedNs > throughNs) throw Error('QUEUE_REPORT_BEFORE_DATA');
    for (const [marketId, state] of markets) {
      if (state.entryOrders.length) {
        if (state.entryOrders[0].expiresNs > throughNs) {
          counts.censoredEntryPairs++;
          counts.censoredByMarket.set(marketId, (counts.censoredByMarket.get(marketId) ?? 0) + 1);
        } else {
          counts.expiredEntryPairs++;
          counts.expiredByMarket.set(marketId, (counts.expiredByMarket.get(marketId) ?? 0) + 1);
        }
        state.entryOrders = [];
      }
      if (state.position?.exitOrder) {
        if (state.position.exitOrder.expiresNs > throughNs) counts.censoredExitQuotes++;
        else counts.expiredExitQuotes++;
        state.position.exitOrder = null;
      }
    }
    let realizedGross = 0n, realizedStress = 0n, openMark = 0n, openStress = 0n, valued = 0, unvalued = 0;
    const perMarket = [];
    for (const [marketId, state] of markets) {
      const rows = completions.filter(row => row.marketId === marketId);
      const gross = rows.reduce((sum, row) => sum + row.grossPnl, 0n);
      const stress = rows.reduce((sum, row) => sum + row.stressOneTickPerFill, 0n);
      realizedGross += gross; realizedStress += stress;
      let inventoryMark = null, inventoryStress = null, markAt = null, executableDepth = null;
      const position = state.position;
      if (position) {
        const book = position.latestPostFillBook;
        if (book && throughNs >= book.receivedNs && throughNs - book.receivedNs <= FRESH_NS) {
          const level = position.side === 'buy' ? book.bids[0] : book.asks[0];
          if (level?.quantity >= 1) {
            inventoryMark = grossPnl(position.side, position.entryPrice, level.price);
            inventoryStress = inventoryMark - TICK;
            markAt = asIso(book.sourceNs); executableDepth = level.quantity; valued++;
            openMark += inventoryMark; openStress += inventoryStress;
          } else unvalued++;
        } else unvalued++;
      }
      perMarket.push({
        marketId, exchangeId: state.exchangeId,
        quotePairs: counts.quotePairsByMarket.get(marketId) ?? 0,
        entryFillProxies: counts.entryFillsByMarket.get(marketId) ?? 0,
        completedExitProxies: rows.length,
        unresolvedInventory: position ? 1 : 0,
        expiredEntryPairs: counts.expiredByMarket.get(marketId) ?? 0,
        censoredEntryPairs: counts.censoredByMarket.get(marketId) ?? 0,
        meanDisplayedBidQueueAhead: counts.quotePairsByMarket.get(marketId) ? counts.queueBidByMarket.get(marketId) / counts.quotePairsByMarket.get(marketId) : null,
        meanDisplayedAskQueueAhead: counts.quotePairsByMarket.get(marketId) ? counts.queueAskByMarket.get(marketId) / counts.quotePairsByMarket.get(marketId) : null,
        realizedGrossPnl: money(gross), realizedStressOneTickPerFillPnl: money(stress),
        unresolvedInventoryExecutableMarkPnl: inventoryMark === null ? null : money(inventoryMark),
        unresolvedInventoryStressPerFillPnl: inventoryStress === null ? null : money(inventoryStress),
        inventoryMarkSourceAt: markAt, inventoryExecutableDepth: executableDepth,
      });
    }
    const verdict = counts.uniqueScopedTrades < 250 ? 'insufficient_sample_diagnostic_only' : 'queue_depletion_proxy_only_not_profitability_evidence';
    return {
      schemaVersion: 1,
      mode: 'observed_aggressor_queue_depletion_maker_roundtrip_v1',
      executionAuthority: false,
      verdict,
      source,
      policy: {
        quantity: 1, feePerShare: '0.000000', feeBps: 0,
        entry: 'first fresh accepted top-of-book point while flat and without a live entry pair; join best bid and ask',
        activationLatencyMs: 3000, orderLifetimeAfterActivationMs: 10000,
        queueAhead: 'displayed quantity at the posted price at quote decision; reduce only by deduplicated, scoped public-print quantity at/through the limit with an inferred aggressor; book-size changes and cancellations never reduce queue',
        aggressorInference: 'use the freshest captured same-market book with sourceAt at/before print execution and age no more than 10s; price at/below best bid implies sell aggressor, at/above best ask implies buy aggressor; inside-spread prints are ambiguous and excluded',
        entryFill: 'one share only after eligible aggressor print volume exceeds the remaining displayed queue ahead; fill at the maker limit',
        positionLimit: 'one active one-share position per market; first entry proxy cancels the opposite entry quote',
        exit: 'one passive target quote one tick favorable to entry, with the same activation/lifetime and queue-depletion rule',
        stress: 'one adverse tick per modeled fill; two ticks for a completed round trip, one tick for an unresolved entry fill marked to the executable side',
        noTuning: true,
      },
      counts: {
        markets: markets.size, quotePairs: counts.quotePairs, entryQuoteSides: counts.quotePairs * 2,
        uniqueScopedTrades: counts.uniqueScopedTrades, duplicateTradeRows: counts.duplicateTradeRows,
        outOfOrderTrades: counts.outOfOrderTrades, tradesExecutedBeforeWindow: counts.preStartTrades,
        printsWithContemporaneousBook: counts.printsWithBook, printsWithoutContemporaneousBook: counts.printsWithoutContemporaneousBook,
        ambiguousPrints: counts.ambiguousPrints, inferredBuyAggressorPrints: counts.inferredBuyAggressorPrints,
        inferredSellAggressorPrints: counts.inferredSellAggressorPrints, invalidTrades: counts.invalidTrades, invalidBooks: counts.invalidBooks,
        entryQueuePrintEvents: counts.entryQueuePrintEvents, entryQueueVolumeObserved: counts.entryQueueVolumeObserved,
        entryFillProxies: counts.entryFillProxies, completedExitProxies: counts.completedExitProxies,
        unresolvedInventory: [...markets.values()].filter(state => state.position).length,
        valuedUnresolvedInventory: valued, unvaluedUnresolvedInventory: unvalued,
        expiredEntryPairs: counts.expiredEntryPairs, censoredEntryPairs: counts.censoredEntryPairs,
        expiredExitQuotes: counts.expiredExitQuotes, censoredExitQuotes: counts.censoredExitQuotes,
        boundaryCanceledEntryPairs: counts.boundaryCanceledEntryPairs, boundaryCanceledExitQuotes: counts.boundaryCanceledExitQuotes,
      },
      aggregate: {
        realizedGrossPnl: money(realizedGross), realizedStressOneTickPerFillPnl: money(realizedStress),
        openInventoryExecutableMarkPnl: money(openMark), openInventoryStressPerFillPnl: money(openStress),
        totalGrossIncludingExecutableInventoryMark: money(realizedGross + openMark),
        totalStressOneTickPerFillPnl: money(realizedStress + openStress), allFees: '0.000000',
      },
      completions: completions.map(row => ({
        marketId: row.marketId, exchangeId: row.exchangeId, side: row.side,
        entryPrice: money(row.entryPrice), exitPrice: money(row.exitPrice),
        entryDecisionAt: row.entryDecisionAt, entryActivationAt: row.entryActivationAt, entryExpiresAt: row.entryExpiresAt,
        entryFillExecutedAt: row.entryFillExecutedAt, entryFillObservedAt: row.entryFillObservedAt,
        entryDisplayedQueueAhead: row.entryDisplayedQueueAhead, entryQueueRemainingAtFill: row.entryQueueRemainingAtFill,
        entryTradeId: row.entryTradeId, entryAggressorBookAt: row.entryAggressorBookAt,
        exitDecisionAt: row.exitDecisionAt, exitActivationAt: row.exitActivationAt, exitExpiresAt: row.exitExpiresAt,
        exitFillExecutedAt: row.exitFillExecutedAt, exitFillObservedAt: row.exitFillObservedAt,
        exitDisplayedQueueAhead: row.exitDisplayedQueueAhead, exitQueueRemainingAtFill: row.exitQueueRemainingAtFill,
        exitTradeId: row.exitTradeId, grossPnl: money(row.grossPnl), stressOneTickPerFillPnl: money(row.stressOneTickPerFill),
        fillEvidence: 'observed queue-depletion proxy; no actual order/queue rank evidence',
      })),
      perMarket,
      limitations: [
        'Aggressor is inferred from event-time public quotes, not provided by the trade feed; ambiguous and stale-book prints are excluded.',
        'Displayed quantity is a queue-ahead proxy; hidden liquidity, priority changes, feed loss, cancellations, and partial execution detail are not observed.',
        'Print size at/through the limit is conservatively applied to the recorded queue; maker execution remains counterfactual and is not an actual fill.',
        'No fees, rebates, queue latency, market impact, or order-cancel costs are modeled; the one-tick-per-fill stress does not establish a deployable edge.',
        ...(counts.uniqueScopedTrades < 250 ? ['Fewer than 250 unique scoped prints; report is diagnostic only and not evidence of performance.'] : []),
      ],
    };
  }

  return { observeBatch, boundary, report };
}
