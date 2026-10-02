import {amount, money, price} from '../paper/fixed.mjs';
import {instantNs, timestamp} from '../feed/contract.mjs';

export const ENTRY_MARKOUT_HORIZONS_MINUTES = Object.freeze([5, 15, 30, 60]);
const MS = 1_000_000n, MINUTE = 60_000n * MS;
const check = (ok, code) => { if (!ok) throw Error(code); };
const plain = x => x !== null && typeof x === 'object' && !Array.isArray(x);
const id = x => typeof x === 'string' && /^[1-9]\d*$/.test(x) && Number.isSafeInteger(Number(x));
const quantity = x => Number.isSafeInteger(x) && x > 0 && x <= 2_147_483_647;
const freeze = x => { if (x && typeof x === 'object') { Object.values(x).forEach(freeze); Object.freeze(x); } return x; };
const iso = ns => new Date(Number(ns / MS)).toISOString().slice(0, 19) + '.' + String(ns % 1_000_000_000n).padStart(9, '0') + 'Z';
const elapsed = (a, b) => Number(a - b) / 1e6;
const count = (object, key) => { object[key] = (object[key] || 0) + 1; };

function levels(rows, descending) {
  check(Array.isArray(rows) && rows.length > 0 && rows.length <= 100000, 'INVALID_BOOK_DEPTH');
  let prior = null, total = 0;
  return rows.map(row => {
    check(plain(row) && quantity(row.quantity), 'INVALID_BOOK_DEPTH');
    const p = price(row.price);
    check(prior === null || (descending ? p < prior : p > prior), 'INVALID_BOOK_DEPTH');
    prior = p; total += row.quantity; check(Number.isSafeInteger(total), 'INVALID_BOOK_DEPTH');
    return {price: p, quantity: row.quantity};
  });
}

function inspect(book, entry, tournamentId, received, nowNs, policy) {
  check(book.tournamentId === tournamentId && book.marketId === entry.marketId && book.exchangeId === entry.exchangeId
    && book.instrument === entry.exchangeId + ':YES', 'BOOK_SCOPE_MISMATCH');
  check(book.freshnessPolicy === 'authoritative_receipt_v1' && !book.invalidated && book.valid !== false, 'BOOK_NOT_AUTHORITATIVE');
  const started = instantNs(book.requestStartedAt), source = instantNs(book.sourceAt);
  check(started <= received && received - started <= BigInt(policy.maxBookLatencyMs) * MS, 'INVALID_BOOK_LATENCY');
  // A response to a request started before this fill is not a subsequent mark.
  check(started > entry.at, 'REQUEST_NOT_AFTER_FILL');
  check(source <= nowNs && source <= received + 2000n * MS, 'FUTURE_BOOK_SOURCE');
  check(typeof book.sequence === 'string' && /^(0|[1-9]\d*)$/.test(book.sequence) && Number.isSafeInteger(Number(book.sequence))
    && book.versionKey === book.sequence + ':' + timestamp(book.sourceAt), 'INVALID_BOOK_VERSION');
  const bids = levels(book.bids, true), asks = levels(book.asks, false);
  check(bids[0].price < asks[0].price, 'LOCKED_OR_CROSSED_BOOK');
  return {started, source, sequence: BigInt(book.sequence), bids, asks,
    content: JSON.stringify([bids, asks], (_, v) => typeof v === 'bigint' ? v.toString() : v)};
}

function mark(row, entry) {
  let remaining = entry.quantity, proceeds = 0n;
  const segments = [];
  for (const level of row.checked.bids) {
    const q = Math.min(remaining, level.quantity);
    if (q) { proceeds += level.price * BigInt(q); segments.push({price: money(level.price), quantity: q}); remaining -= q; }
    if (!remaining) break;
  }
  return {reasonCode: remaining ? 'INSUFFICIENT_BID_DEPTH' : 'COMPLETE', observedQuantity: entry.quantity - remaining,
    unsoldQuantity: remaining, observedAvailableDepthValue: money(proceeds),
    observedLiquidationValue: remaining ? null : money(proceeds),
    entryMarkout: remaining ? null : money(proceeds - entry.cost), segments};
}

const evidence = row => ({receivedAt: row.book.receivedAt, requestStartedAt: row.book.requestStartedAt,
  sourceAt: row.book.sourceAt, versionKey: row.book.versionKey});

// One reconciled buy-fill, measured at its original quantity even after an actual
// exit. This is an entry diagnostic, never portfolio P&L or a simulated sale.
// Callers verify attribution from their owned journal and attach tournamentId to
// normalized books. Multiple fills of one intent are NOT independent trades.
export function analyzeEntryMarkouts({tournamentId, fill, books = [], now, takeProfit = '0.010000', stopLoss = '0.030000',
  maxHorizonDelayMs = 60000, maxBookLatencyMs = 20000} = {}) {
  check(typeof tournamentId === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(tournamentId), 'INVALID_MARKOUT_SCOPE');
  check(Number.isSafeInteger(now) && now >= 0 && now <= 8640000000000000, 'INVALID_MARKOUT_CLOCK');
  check(Number.isSafeInteger(maxHorizonDelayMs) && maxHorizonDelayMs >= 0 && maxHorizonDelayMs <= 60000
    && Number.isSafeInteger(maxBookLatencyMs) && maxBookLatencyMs > 0 && maxBookLatencyMs <= 20000, 'INVALID_MARKOUT_POLICY');
  check(plain(fill) && id(fill.marketId) && id(fill.exchangeId) && fill.action === 'buy'
    && (fill.side === undefined || fill.side === 'yes') && (fill.tournamentId === undefined || fill.tournamentId === tournamentId)
    && Number.isSafeInteger(fill.id) && fill.id > 0 && Number.isSafeInteger(fill.orderId) && fill.orderId > 0
    && quantity(fill.quantity), 'INVALID_ATTRIBUTED_ENTRY_FILL');
  const nowNs = BigInt(now) * MS, entry = {...fill, at: instantNs(fill.filledAt), priceMicros: price(fill.price)};
  check(entry.at <= nowNs, 'FUTURE_ENTRY_FILL');
  entry.cost = entry.priceMicros * BigInt(entry.quantity);
  const profit = amount(takeProfit), loss = amount(stopLoss);
  check(profit > 0n && profit < 1_000_000n && loss > 0n && loss < 1_000_000n, 'INVALID_MARKOUT_THRESHOLDS');
  check(Array.isArray(books) && books.length <= 100000, 'INVALID_MARKOUT_BOOKS');
  const policy = {horizonsMinutes: [...ENTRY_MARKOUT_HORIZONS_MINUTES], maxHorizonDelayMs, maxBookLatencyMs,
    maxSourceClockSkewMs: 2000, takeProfit: money(profit), stopLoss: money(loss),
    horizonSelection: 'first_receipt_at_or_after_target_no_skipping_invalid_or_thin_evidence',
    passageWindowMinutes: 60, addedFees: '0.000000'};
  const counts = {suppliedBooks: books.length, unrelatedBooks: 0, preFillReceipts: 0, futureReceipts: 0,
    invalidReceiptClocks: 0, duplicateReceipts: 0, observedReceipts: 0, validReceipts: 0,
    distinctSourceVersions: 0, distinctBookContents: 0, reasonCounts: {}};
  const candidates = [];
  for (const book of books) {
    if (!plain(book) || (book.exchangeId !== entry.exchangeId && book.marketId !== entry.marketId)) { counts.unrelatedBooks++; continue; }
    let received;
    try { received = instantNs(book.receivedAt); } catch { counts.invalidReceiptClocks++; continue; }
    if (received <= entry.at) { counts.preFillReceipts++; continue; }
    if (received > nowNs) { counts.futureReceipts++; continue; }
    candidates.push({book, received});
  }
  // Input may come from interleaved core/supplemental capture arrays. Order by
  // actual receipt, retaining the original clocks and rejecting version rollback.
  candidates.sort((a, b) => a.received < b.received ? -1 : a.received > b.received ? 1 : 0);
  const rows = [], versions = new Map(), contents = new Set();
  let previous = null;
  for (let index = 0; index < candidates.length;) {
    const row = candidates[index++], group = [row];
    while (index < candidates.length && candidates[index].received === row.received) group.push(candidates[index++]);
    counts.duplicateReceipts += group.length - 1;
    try {
      row.checked = inspect(row.book, entry, tournamentId, row.received, nowNs, policy);
      for (const duplicate of group.slice(1)) {
        const other = inspect(duplicate.book, entry, tournamentId, duplicate.received, nowNs, policy);
        check(other.content === row.checked.content && other.started === row.checked.started
          && duplicate.book.versionKey === row.book.versionKey, 'CONFLICTING_RECEIPT');
      }
      const c = row.checked;
      check(!previous || (c.sequence >= previous.sequence && c.source >= previous.source && c.started >= previous.started), 'BOOK_VERSION_OR_REQUEST_ROLLBACK');
      check(!previous || c.sequence !== previous.sequence || c.source === previous.source, 'BOOK_SEQUENCE_IDENTITY_CONFLICT');
      check(!versions.has(row.book.versionKey) || versions.get(row.book.versionKey) === c.content, 'BOOK_VERSION_CONTENT_CONFLICT');
      versions.set(row.book.versionKey, c.content); contents.add(c.content); previous = c;
      row.reasonCode = 'VALID'; counts.validReceipts++;
    } catch (error) { row.checked = null; row.reasonCode = /^[A-Z][A-Z_]+$/.test(error.message) ? error.message : 'INVALID_BOOK'; }
    count(counts.reasonCounts, row.reasonCode); rows.push(row);
  }
  counts.observedReceipts = rows.length; counts.distinctSourceVersions = versions.size; counts.distinctBookContents = contents.size;
  const horizonRows = policy.horizonsMinutes.map(minutes => {
    const target = entry.at + BigInt(minutes) * MINUTE, row = rows.find(x => x.received >= target);
    const out = {minutes, targetAt: iso(target), available: false, reasonCode: 'PENDING_HORIZON', evidence: null,
      delayMs: null, observedQuantity: 0, unsoldQuantity: entry.quantity, observedAvailableDepthValue: null,
      observedLiquidationValue: null, entryMarkout: null};
    if (nowNs < target) return out;
    if (!row) { out.reasonCode = nowNs > target + BigInt(maxHorizonDelayMs) * MS ? 'MISSING_HORIZON_EVIDENCE' : 'AWAITING_HORIZON_RECEIPT'; return out; }
    out.evidence = evidence(row); out.delayMs = elapsed(row.received, target);
    if (out.delayMs > maxHorizonDelayMs) { out.reasonCode = 'STALE_HORIZON_EVIDENCE'; return out; }
    if (!row.checked) { out.reasonCode = row.reasonCode; return out; }
    Object.assign(out, mark(row, entry)); out.available = out.reasonCode === 'COMPLETE'; return out;
  });
  const passageThrough = nowNs < entry.at + 60n * MINUTE ? nowNs : entry.at + 60n * MINUTE;
  const windowRows = rows.filter(x => x.received <= passageThrough), valid = windowRows.filter(x => x.checked);
  function passage(threshold, isProfit) {
    const row = valid.find(x => isProfit ? x.checked.bids[0].price >= threshold : x.checked.bids[0].price <= threshold);
    if (!row) return {observed: false, threshold: money(threshold), firstObservedAt: null};
    const prior = valid.filter(x => x.received < row.received).at(-1);
    const quantityAtOrBetterThreshold = row.checked.bids.filter(x => x.price >= threshold).reduce((n, x) => n + x.quantity, 0);
    return {observed: true, threshold: money(threshold), firstObservedAt: row.book.receivedAt, elapsedMs: elapsed(row.received, entry.at),
      bestBid: money(row.checked.bids[0].price), evidence: evidence(row), previousValidReceiptAt: prior?.book.receivedAt ?? null,
      observationGapMs: elapsed(row.received, prior?.received ?? entry.at), quantityAtOrBetterThreshold,
      fullQuantityAtOrBetterThreshold: quantityAtOrBetterThreshold >= entry.quantity, ...mark(row, entry)};
  }
  const gaps = valid.map((row, i) => elapsed(row.received, i ? valid[i - 1].received : entry.at));
  const tail = elapsed(passageThrough, valid.at(-1)?.received ?? entry.at), sortedIntervals = gaps.slice(1).sort((a, b) => a - b);
  const middle = Math.floor(sortedIntervals.length / 2), medianInterval = !sortedIntervals.length ? null
    : sortedIntervals.length % 2 ? sortedIntervals[middle] : (sortedIntervals[middle - 1] + sortedIntervals[middle]) / 2;
  return freeze({schemaVersion: 1, measurementKind: 'attributed_buy_fill_observed_bid_markouts', tournamentId,
    asOf: new Date(now).toISOString(), executionGuaranteed: false, profitabilityEstablished: false, addedFees: '0.000000', policy,
    entry: {marketId: entry.marketId, exchangeId: entry.exchangeId, side: 'YES', quantity: entry.quantity, price: money(entry.priceMicros),
      cost: money(entry.cost), filledAt: fill.filledAt, origin: 'caller_supplied_reconciled_owned_fill'},
    horizons: horizonRows, passages: {windowThrough: iso(passageThrough), takeProfit: passage(entry.priceMicros + profit, true),
      stopLoss: passage(entry.priceMicros - loss, false), continuousCrossingClaim: false},
    sampling: {...counts, passageWindowValidReceipts: valid.length, firstValidReceiptAt: valid[0]?.book.receivedAt ?? null,
      lastValidReceiptAt: valid.at(-1)?.book.receivedAt ?? null, gapsMs: {maximumIncludingUnobservedTail: Math.max(tail, ...gaps),
        medianObservedInterval: medianInterval,
        firstObservationDelay: gaps[0] ?? null, unobservedTail: tail}},
    limitations: ['Receipt observations are not independent trades and do not reveal crossings between samples.',
      'Historical markouts retain the original fill quantity after any actual exit; they are not realized or portfolio P&L.',
      'Displayed-depth liquidation assumes the displayed liquidity remains available; no fill, queue priority or execution is inferred.',
      'Old engine timestamps may describe a quiet book; freshness is an authoritative subsequent receipt and a bounded horizon delay.']});
}
