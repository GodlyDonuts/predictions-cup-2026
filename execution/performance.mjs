import {amount, money, price, instant} from '../paper/fixed.mjs';
import {instantNs, timestamp} from '../feed/contract.mjs';

const MAX_QUANTITY = 2_147_483_647;
const check = (ok, code) => { if (!ok) throw Error(code); };
const id = value => typeof value === 'string' && /^[1-9]\d*$/.test(value) && Number.isSafeInteger(Number(value));
const quantity = value => Number.isSafeInteger(value) && value > 0 && value <= MAX_QUANTITY;
const plain = value => value !== null && typeof value === 'object' && !Array.isArray(value)
  && Object.getPrototypeOf(value) === Object.prototype && !['__proto__', 'constructor', 'prototype'].some(key => Object.hasOwn(value, key));
const freeze = value => { if (value && typeof value === 'object') { Object.values(value).forEach(freeze); Object.freeze(value); } return value; };

function reconcileOwned(positions, fills, tournamentId, nowNs) {
  check(plain(positions) && Object.keys(positions).length <= 10000 && Array.isArray(fills) && fills.length <= 100000, 'INVALID_PERFORMANCE_INPUT');
  const ledger = new Map(), fillKeys = new Set(), orders = new Map();
  const ordered = fills.map((fill, index) => {
    check(plain(fill) && id(fill.marketId) && id(fill.exchangeId) && quantity(fill.quantity)
      && ['buy', 'sell'].includes(fill.action) && (fill.side === undefined || fill.side === 'yes')
      && (fill.tournamentId === undefined || fill.tournamentId === tournamentId)
      && Number.isSafeInteger(fill.orderId) && fill.orderId > 0 && Number.isSafeInteger(fill.id) && fill.id > 0, 'INVALID_RECONCILED_FILL');
    const key = fill.orderId + ':' + fill.id, terms = [fill.marketId, fill.exchangeId, fill.action].join(':');
    check(!fillKeys.has(key) && (!orders.has(fill.orderId) || orders.get(fill.orderId) === terms), 'DUPLICATE_OR_CONFLICTING_FILL');
    fillKeys.add(key); orders.set(fill.orderId, terms);
    const at = instantNs(fill.filledAt), p = price(fill.price);
    check(at <= nowNs, 'FUTURE_RECONCILED_FILL');
    return {fill, index, at, price: p};
  }).sort((a, b) => a.at < b.at ? -1 : a.at > b.at ? 1 : a.index - b.index);
  for (const {fill, at, price: p} of ordered) {
    const row = ledger.get(fill.exchangeId) ?? {marketId: fill.marketId, quantity: 0, cost: 0n, cashFlow: 0n, lots: [], lastOwnFillAt: null};
    check(row.marketId === fill.marketId, 'FILL_MARKET_IDENTITY_MISMATCH');
    const notional = p * BigInt(fill.quantity);
    if (fill.action === 'buy') {
      row.quantity += fill.quantity; row.cost += notional; row.cashFlow -= notional;
      row.lots.push({quantity: fill.quantity, cost: notional});
    } else {
      check(fill.quantity <= row.quantity, 'RECONCILED_FILL_OVERSELL');
      let left = fill.quantity;
      while (left) {
        const lot = row.lots[0], usedQuantity = Math.min(left, lot.quantity);
        const cost = lot.cost * BigInt(usedQuantity) / BigInt(lot.quantity);
        row.cost -= cost; lot.cost -= cost; lot.quantity -= usedQuantity; left -= usedQuantity;
        if (!lot.quantity) row.lots.shift();
      }
      row.quantity -= fill.quantity; row.cashFlow += notional;
    }
    check(Number.isSafeInteger(row.quantity), 'OWNED_QUANTITY_OVERFLOW');
    row.lastOwnFillAt = timestamp(fill.filledAt); row.lastOwnFillNs = at;
    ledger.set(fill.exchangeId, row);
  }
  for (const [exchangeId, position] of Object.entries(positions)) {
    check(id(exchangeId) && plain(position) && id(position.marketId) && quantity(position.quantity)
      && Array.isArray(position.lots), 'INVALID_OWNED_POSITION');
    const replay = ledger.get(exchangeId);
    check(replay && replay.marketId === position.marketId && replay.quantity === position.quantity
      && replay.cost === amount(position.cost) && position.lots.length === replay.lots.length
      && position.lots.every((lot, index) => plain(lot) && quantity(lot.quantity)
        && lot.quantity === replay.lots[index].quantity && amount(lot.cost) === replay.lots[index].cost), 'OWNERSHIP_PROVENANCE_MISMATCH');
  }
  for (const [exchangeId, row] of ledger) check(row.quantity === 0 || Object.hasOwn(positions, exchangeId), 'OWNED_POSITION_MISSING');
  return ledger;
}

function checkedBook(book, position, exchangeId, tournamentId, now, lastOwnFillNs, policy) {
  check(plain(book) && book.tournamentId === tournamentId && book.marketId === position.marketId
    && book.exchangeId === exchangeId && book.instrument === exchangeId + ':YES', 'BOOK_SCOPE_MISMATCH');
  check(book.freshnessPolicy === 'authoritative_receipt_v1' && !book.invalidated && book.valid !== false, 'BOOK_NOT_AUTHORITATIVE');
  const received = instant(book.receivedAt), started = instant(book.requestStartedAt), source = instantNs(book.sourceAt), nowNs = BigInt(now) * 1000000n;
  check(received <= now && started <= now, 'FUTURE_BOOK_RECEIPT');
  check(started <= received && received - started <= policy.maxBookLatencyMs, 'INVALID_BOOK_LATENCY');
  check(now - received <= policy.maxBookAgeMs, 'STALE_BOOK');
  check(BigInt(received) * 1000000n >= lastOwnFillNs, 'BOOK_PRECEDES_OWN_FILL');
  // Preserve the raw engine clock. Even the normalizer's bounded source-clock
  // skew does not permit a future source mark at this measurement's as-of time.
  check(source <= nowNs && source <= BigInt(received + 2000) * 1000000n, 'FUTURE_BOOK_SOURCE');
  check(typeof book.sequence === 'string' && /^(0|[1-9]\d*)$/.test(book.sequence) && Number.isSafeInteger(Number(book.sequence))
    && book.versionKey === book.sequence + ':' + timestamp(book.sourceAt), 'INVALID_BOOK_VERSION');
  const levels = (rows, descending) => {
    check(Array.isArray(rows) && rows.length > 0 && rows.length <= 100000, 'INVALID_BOOK_DEPTH');
    let previous = null, total = 0;
    return rows.map(row => {
      check(plain(row) && Number.isSafeInteger(row.quantity) && row.quantity > 0, 'INVALID_BOOK_DEPTH');
      const p = price(row.price);
      check(previous === null || (descending ? p < previous : p > previous), 'INVALID_BOOK_DEPTH');
      previous = p; total += row.quantity; check(Number.isSafeInteger(total), 'INVALID_BOOK_DEPTH');
      return {price: p, quantity: row.quantity};
    });
  };
  const bids = levels(book.bids, true), asks = levels(book.asks, false);
  check(bids[0].price < asks[0].price, 'LOCKED_OR_CROSSED_BOOK');
  return {bids, asks, bookAgeMs: now - received, sourceEventAgeMs: Number(nowNs - source) / 1000000};
}

// Read-only measurement of reconciled owned fills, not an execution simulation.
// Each supplied normalized book must carry its explicitly verified tournamentId.
export function measureExecutionPerformance({tournamentId, positions, fills, books = [], now,
  maxBookAgeMs = 20000, maxBookLatencyMs = 20000} = {}) {
  check(typeof tournamentId === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(tournamentId), 'INVALID_PERFORMANCE_SCOPE');
  check(Number.isSafeInteger(now) && now >= 0 && now <= 8640000000000000, 'INVALID_PERFORMANCE_CLOCK');
  check([maxBookAgeMs, maxBookLatencyMs].every(value => Number.isSafeInteger(value) && value > 0 && value <= 20000), 'INVALID_PERFORMANCE_FRESHNESS');
  check(Array.isArray(books) && books.length <= 10000, 'INVALID_PERFORMANCE_BOOKS');
  const ledger = reconcileOwned(positions, fills, tournamentId, BigInt(now) * 1000000n), rows = [];
  let cashFlow = 0n, ownedCost = 0n, observedValue = 0n, midpointValue = 0n, liquidationComplete = true, midpointComplete = true;
  const coverage = {ownedPositions: Object.keys(positions).length, validBooks: 0, liquidationCompletePositions: 0,
    midpointMarkedPositions: 0, ownedQuantity: 0, observedLiquidationQuantity: 0, unsoldQuantity: 0, reasonCounts: {}};
  for (const row of ledger.values()) { cashFlow += row.cashFlow; ownedCost += row.cost; }
  for (const [exchangeId, position] of Object.entries(positions).sort((a, b) => Number(a[0]) - Number(b[0]))) {
    const own = ledger.get(exchangeId), candidates = books.filter(book => book?.exchangeId === exchangeId);
    const out = {marketId: position.marketId, exchangeId, side: 'YES', quantity: position.quantity, costBasis: money(own.cost),
      lastOwnFillAt: own.lastOwnFillAt, reasonCode: 'MISSING_BOOK', book: null, availableBidQuantity: null,
      observedLiquidationQuantity: 0, unsoldQuantity: position.quantity, observedAvailableDepthValue: null,
      observedLiquidationValue: null, observedLiquidationUnrealizedPnl: null,
      midpointPrice: null, midpointValue: null, midpointUnrealizedPnl: null};
    try {
      check(candidates.length <= 1, 'DUPLICATE_BOOK');
      check(candidates.length === 1, books.some(book => book?.marketId === position.marketId) ? 'BOOK_SCOPE_MISMATCH' : 'MISSING_BOOK');
      const book = candidates[0], checked = checkedBook(book, position, exchangeId, tournamentId, now, own.lastOwnFillNs, {maxBookAgeMs, maxBookLatencyMs});
      out.book = {receivedAt: book.receivedAt, requestStartedAt: book.requestStartedAt, sourceAt: book.sourceAt,
        versionKey: book.versionKey, freshnessPolicy: book.freshnessPolicy, ageMs: checked.bookAgeMs, sourceEventAgeMs: checked.sourceEventAgeMs};
      out.availableBidQuantity = checked.bids.reduce((total, level) => total + level.quantity, 0);
      let remaining = position.quantity, proceeds = 0n;
      for (const level of checked.bids) {
        const take = Math.min(remaining, level.quantity); proceeds += level.price * BigInt(take); remaining -= take;
        if (!remaining) break;
      }
      out.observedLiquidationQuantity = position.quantity - remaining; out.unsoldQuantity = remaining;
      out.observedAvailableDepthValue = money(proceeds); out.reasonCode = remaining ? 'INSUFFICIENT_BID_DEPTH' : 'COMPLETE';
      if (!remaining) { out.observedLiquidationValue = money(proceeds); out.observedLiquidationUnrealizedPnl = money(proceeds - own.cost); observedValue += proceeds; coverage.liquidationCompletePositions++; }
      const mid = (checked.bids[0].price + checked.asks[0].price) / 2n, mark = mid * BigInt(position.quantity);
      out.midpointPrice = money(mid); out.midpointValue = money(mark); out.midpointUnrealizedPnl = money(mark - own.cost);
      midpointValue += mark; coverage.validBooks++; coverage.midpointMarkedPositions++;
    } catch (error) {
      const known = new Set(['MISSING_BOOK', 'DUPLICATE_BOOK', 'BOOK_SCOPE_MISMATCH', 'BOOK_NOT_AUTHORITATIVE', 'FUTURE_BOOK_RECEIPT',
        'INVALID_BOOK_LATENCY', 'STALE_BOOK', 'BOOK_PRECEDES_OWN_FILL', 'FUTURE_BOOK_SOURCE', 'INVALID_BOOK_VERSION', 'INVALID_BOOK_DEPTH', 'LOCKED_OR_CROSSED_BOOK']);
      out.reasonCode = known.has(error?.message) ? error.message : 'INVALID_BOOK';
    }
    if (out.observedLiquidationValue === null) liquidationComplete = false;
    if (out.midpointValue === null) midpointComplete = false;
    coverage.ownedQuantity += position.quantity; coverage.observedLiquidationQuantity += out.observedLiquidationQuantity; coverage.unsoldQuantity += out.unsoldQuantity;
    coverage.reasonCounts[out.reasonCode] = (coverage.reasonCounts[out.reasonCode] ?? 0) + 1; rows.push(out);
  }
  const realized = cashFlow + ownedCost;
  return freeze({schemaVersion: 1, measurementKind: 'reconciled_owned_observed_depth_estimate', tournamentId, asOf: new Date(now).toISOString(),
    executionGuaranteed: false, addedFees: '0.000000', ownedCost: money(ownedCost), realizedPnl: money(realized),
    observedLiquidationValue: liquidationComplete ? money(observedValue) : null,
    observedLiquidationUnrealizedPnl: liquidationComplete ? money(observedValue - ownedCost) : null,
    observedLiquidationTotalPnl: liquidationComplete ? money(realized + observedValue - ownedCost) : null,
    midpointValue: midpointComplete ? money(midpointValue) : null,
    midpointUnrealizedPnl: midpointComplete ? money(midpointValue - ownedCost) : null,
    midpointTotalPnl: midpointComplete ? money(realized + midpointValue - ownedCost) : null,
    coverage: {...coverage, liquidationComplete, midpointComplete}, positions: rows});
}
