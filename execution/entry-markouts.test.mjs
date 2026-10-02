import test from 'node:test';
import assert from 'node:assert/strict';
import {analyzeEntryMarkouts, ENTRY_MARKOUT_HORIZONS_MINUTES} from './entry-markouts.mjs';
import {timestamp} from '../feed/contract.mjs';
import {normalizeBook} from '../live-paper/model.mjs';

const TID = 'bda92870-621e-47b0-bc3c-3602c5c26f55', START = Date.parse('2026-10-02T11:16:36.114Z'), M = 60000;
const at = ms => new Date(START + ms).toISOString();
const fill = changes => ({id: 1, orderId: 100, marketId: '369', exchangeId: '1058', action: 'buy',
  quantity: 16, price: '0.245000', filledAt: at(0), ...changes});
function book(ms, changes = {}) {
  const b = {tournamentId: TID, marketId: '369', exchangeId: '1058', instrument: '1058:YES',
    freshnessPolicy: 'authoritative_receipt_v1', receivedAt: at(ms), requestStartedAt: at(ms - 100),
    sourceAt: timestamp(at(ms - 50)), sequence: String(ms), bids: [{price: '0.235000', quantity: 100}],
    asks: [{price: '0.275000', quantity: 100}], ...changes};
  return {...b, versionKey: changes.versionKey ?? b.sequence + ':' + timestamp(b.sourceAt)};
}
const input = changes => ({tournamentId: TID, fill: fill(), books: [], now: START + 61 * M, ...changes});
const horizon = (out, minutes = 5) => out.horizons.find(h => h.minutes === minutes);

test('fixed horizons walk the full bid ladder and preserve original paid cost and clocks', () => {
  const data = input({books: [book(5 * M + 200), book(15 * M + 100, {bids: [{price: '0.245000', quantity: 12}, {price: '0.240000', quantity: 4}]}),
    book(30 * M, {bids: [{price: '0.245000', quantity: 100}]}), book(60 * M + 500, {bids: [{price: '0.240000', quantity: 100}]})]});
  const before = JSON.stringify(data), result = analyzeEntryMarkouts(data);
  assert.deepEqual(result.horizons.map(x => x.minutes), [5, 15, 30, 60]);
  assert.deepEqual(result.horizons.map(x => x.entryMarkout), ['-0.160000', '-0.020000', '0.000000', '-0.080000']);
  assert.equal(horizon(result, 15).observedLiquidationValue, '3.900000');
  assert.deepEqual(horizon(result, 15).segments, [{price: '0.245000', quantity: 12}, {price: '0.240000', quantity: 4}]);
  assert.equal(horizon(result).delayMs, 200); assert.equal(result.entry.cost, '3.920000');
  assert.equal(result.entry.filledAt, at(0)); assert.equal(horizon(result).evidence.sourceAt, data.books[0].sourceAt);
  assert.equal(result.addedFees, '0.000000'); assert.equal(result.executionGuaranteed, false);
  assert.equal(result.profitabilityEstablished, false); assert.equal(JSON.stringify(data), before);
  assert.ok(Object.isFrozen(result.horizons[0].evidence)); assert.ok(Object.isFrozen(ENTRY_MARKOUT_HORIZONS_MINUTES));
  assert.equal(JSON.stringify(result).includes('orderId'), false);
  assert.deepEqual(analyzeEntryMarkouts(JSON.parse(before)), result);
});

test('horizons never carry an earlier quote forward, use a future receipt, or hide a late measurement', () => {
  const before = analyzeEntryMarkouts(input({books: [book(5 * M - 1), book(5 * M + 1)], now: START + 5 * M}));
  assert.equal(horizon(before).reasonCode, 'AWAITING_HORIZON_RECEIPT'); assert.equal(before.sampling.futureReceipts, 1);
  assert.equal(horizon(before, 15).reasonCode, 'PENDING_HORIZON');
  const missing = analyzeEntryMarkouts(input({books: [book(5 * M - 1)], now: START + 6 * M + 1}));
  assert.equal(horizon(missing).reasonCode, 'MISSING_HORIZON_EVIDENCE');
  const stale = analyzeEntryMarkouts(input({books: [book(6 * M + 1)]}));
  assert.equal(horizon(stale).reasonCode, 'STALE_HORIZON_EVIDENCE'); assert.equal(horizon(stale).entryMarkout, null);
  const boundary = analyzeEntryMarkouts(input({books: [book(6 * M)]}));
  assert.equal(horizon(boundary).available, true); assert.equal(horizon(boundary).delayMs, M);
});

test('a thin first horizon observation cannot be replaced by favorable later liquidity', () => {
  const result = analyzeEntryMarkouts(input({books: [book(5 * M, {bids: [{price: '0.250000', quantity: 5}]}),
    book(5 * M + 1000, {bids: [{price: '0.260000', quantity: 100}]})]}));
  const h = horizon(result); assert.equal(h.reasonCode, 'INSUFFICIENT_BID_DEPTH'); assert.equal(h.available, false);
  assert.equal(h.observedAvailableDepthValue, '1.250000'); assert.equal(h.observedLiquidationValue, null);
  assert.equal(h.entryMarkout, null); assert.equal(h.observedQuantity, 5); assert.equal(h.unsoldQuantity, 11);
  assert.equal(result.passages.takeProfit.firstObservedAt, at(5 * M + 1000));
});

test('observed top-bid triggers expose thin profitable depth and never imply full fills at the trigger', () => {
  const result = analyzeEntryMarkouts(input({books: [book(M, {bids: [{price: '0.255000', quantity: 1}, {price: '0.235000', quantity: 15}]}),
    book(2 * M, {bids: [{price: '0.215000', quantity: 5}]})]}));
  const tp = result.passages.takeProfit, stop = result.passages.stopLoss;
  assert.equal(tp.observed, true); assert.equal(tp.quantityAtOrBetterThreshold, 1);
  assert.equal(tp.fullQuantityAtOrBetterThreshold, false); assert.equal(tp.entryMarkout, '-0.140000');
  assert.equal(tp.observationGapMs, M); assert.equal(tp.previousValidReceiptAt, null);
  assert.equal(stop.observed, true); assert.equal(stop.observedLiquidationValue, null); assert.equal(stop.unsoldQuantity, 11);
  assert.equal(stop.previousValidReceiptAt, at(M)); assert.equal(result.passages.continuousCrossingClaim, false);
});

test('passages stop at the fixed 60 minute window while the 60 minute mark discloses allowed receipt delay', () => {
  const result = analyzeEntryMarkouts(input({books: [book(60 * M + 1, {bids: [{price: '0.260000', quantity: 100}]})]}));
  assert.equal(result.passages.takeProfit.observed, false); assert.equal(result.sampling.passageWindowValidReceipts, 0);
  assert.equal(horizon(result, 60).entryMarkout, '0.240000'); assert.equal(horizon(result, 60).delayMs, 1);
  assert.equal(result.sampling.gapsMs.maximumIncludingUnobservedTail, 60 * M);
});

test('sampling reports unobserved leading, inter-receipt and trailing intervals rather than continuous crossing evidence', () => {
  const result = analyzeEntryMarkouts(input({books: [book(M), book(10 * M)]}));
  assert.equal(result.sampling.observedReceipts, 2); assert.equal(result.sampling.gapsMs.firstObservationDelay, M);
  assert.equal(result.sampling.gapsMs.unobservedTail, 50 * M);
  assert.equal(result.sampling.gapsMs.medianObservedInterval, 9 * M);
  assert.equal(result.sampling.gapsMs.maximumIncludingUnobservedTail, 50 * M);
  assert.equal(result.passages.takeProfit.observed, false); assert.equal(result.passages.stopLoss.observed, false);
});

test('repeated authoritative quiet-book receipts can mark horizons but do not become distinct source observations', () => {
  const sourceAt = timestamp(at(-86400000));
  const a = book(5 * M, {sourceAt, sequence: '1'}), b = book(15 * M, {sourceAt, sequence: '1'});
  const result = analyzeEntryMarkouts(input({books: [b, a, structuredClone(a)]}));
  assert.equal(horizon(result).available, true); assert.equal(horizon(result, 15).available, true);
  assert.equal(result.sampling.observedReceipts, 2); assert.equal(result.sampling.duplicateReceipts, 1);
  assert.equal(result.sampling.distinctSourceVersions, 1); assert.equal(result.sampling.distinctBookContents, 1);
});

test('median sampling interval averages the two central intervals and excludes the first observation delay', () => {
  const result = analyzeEntryMarkouts(input({books: [book(M), book(2 * M), book(12 * M)]}));
  assert.equal(result.sampling.gapsMs.medianObservedInterval, 5.5 * M);
  assert.equal(result.sampling.gapsMs.firstObservationDelay, M);
  const single = analyzeEntryMarkouts(input({books: [book(M)]}));
  assert.equal(single.sampling.gapsMs.medianObservedInterval, null);
});

test('conflicting same-receipt evidence cannot choose the favorable book and version rollback stays invalid', () => {
  const conflict = analyzeEntryMarkouts(input({books: [book(5 * M), book(5 * M, {bids: [{price: '0.260000', quantity: 100}]})]}));
  assert.equal(horizon(conflict).reasonCode, 'CONFLICTING_RECEIPT'); assert.equal(conflict.passages.takeProfit.observed, false);
  const rollback = analyzeEntryMarkouts(input({books: [book(M, {sequence: '9'}), book(5 * M, {sequence: '8'})]}));
  assert.equal(horizon(rollback).reasonCode, 'BOOK_VERSION_OR_REQUEST_ROLLBACK');
  const sourceAt = timestamp(at(M - 50));
  const changed = analyzeEntryMarkouts(input({books: [book(M, {sourceAt, sequence: '9'}),
    book(5 * M, {sourceAt, sequence: '9', bids: [{price: '0.260000', quantity: 100}]})]}));
  assert.equal(horizon(changed).reasonCode, 'BOOK_VERSION_CONTENT_CONFLICT');
  const movedClock = analyzeEntryMarkouts(input({books: [book(M, {sequence: '9'}),
    book(5 * M, {sequence: '9', bids: [{price: '0.260000', quantity: 100}]})]}));
  assert.equal(horizon(movedClock).reasonCode, 'BOOK_SEQUENCE_IDENTITY_CONFLICT');
  assert.equal(movedClock.passages.takeProfit.observed, false);
});

test('invalid or foreign first horizon evidence remains unavailable without skipping to the next valid quote', () => {
  const bad = [book(5 * M, {invalidated: true}), book(5 * M, {freshnessPolicy: 'event_age_v1'}), book(5 * M, {tournamentId: 'foreign'}),
    book(5 * M, {marketId: '377'}), book(5 * M, {exchangeId: '1066'}), book(5 * M, {versionKey: 'wrong'}),
    book(5 * M, {requestStartedAt: at(5 * M + 1)}), book(5 * M, {requestStartedAt: at(5 * M - 20001)}),
    book(5 * M, {bids: [{price: '0.236000', quantity: 100}]}), book(5 * M, {bids: [{price: '0.235000', quantity: 1.5}]}),
    book(5 * M, {asks: [{price: '0.235000', quantity: 100}]})];
  for (const b of bad) {
    const r = analyzeEntryMarkouts(input({books: [b, book(5 * M + 1000)]}));
    assert.equal(horizon(r).available, false); assert.equal(horizon(r).entryMarkout, null);
  }
  const unrelated = analyzeEntryMarkouts(input({books: [book(5 * M, {marketId: '999', exchangeId: '999'})]}));
  assert.equal(unrelated.sampling.unrelatedBooks, 1); assert.equal(horizon(unrelated).reasonCode, 'MISSING_HORIZON_EVIDENCE');
});

test('subsequent requests preserve nanosecond fill causality and source skew bounds', () => {
  const nsFill = '2026-10-02T11:16:36.114000123Z';
  const bad = book(1, {requestStartedAt: '2026-10-02T11:16:36.114000122Z'});
  const r = analyzeEntryMarkouts(input({fill: fill({filledAt: nsFill}), books: [bad]}));
  assert.equal(r.sampling.reasonCounts.REQUEST_NOT_AFTER_FILL, 1); assert.equal(r.entry.filledAt, nsFill);
  assert.equal(horizon(r).targetAt, '2026-10-02T11:21:36.114000123Z');
  const accept = analyzeEntryMarkouts(input({books: [book(5 * M, {sourceAt: timestamp(at(5 * M + 130))})]}));
  assert.equal(horizon(accept).available, true);
  const reject = analyzeEntryMarkouts(input({books: [book(5 * M, {sourceAt: timestamp(at(5 * M + 2001))})]}));
  assert.equal(horizon(reject).reasonCode, 'FUTURE_BOOK_SOURCE');
  const futureNs = timestamp(at(5 * M)).replace('000000Z', '000001Z');
  const future = analyzeEntryMarkouts(input({books: [book(5 * M, {sourceAt: futureNs})], now: START + 5 * M}));
  assert.equal(horizon(future).reasonCode, 'FUTURE_BOOK_SOURCE');
});

test('actual normalized REST book shape with explicit tournament scope is supported', () => {
  const raw = {contexts: [{type: 'tournament', tournament: {id: TID}, orderbook: {exchanges: [{exchangeId: '1058', option: 'YES',
    asOf: {sequence: 12, at: at(5 * M + 130)}, bids: [{price: .235, quantity: 100}], asks: [{price: .24, quantity: 100}]}]}}]};
  const normalized = normalizeBook(raw, '369', TID, at(5 * M), at(5 * M - 100), 20000, new Set(), 'authoritative_receipt_v1');
  const result = analyzeEntryMarkouts(input({books: [{...normalized, tournamentId: TID}]}));
  assert.equal(horizon(result).entryMarkout, '-0.160000'); assert.equal(horizon(result).evidence.sourceAt, normalized.sourceAt);
});

test('malformed ownership and tunable policy expansion are rejected before producing measurements', () => {
  for (const changed of [fill({id: null}), fill({orderId: 0}), fill({action: 'sell'}), fill({side: 'no'}),
    fill({tournamentId: 'foreign'}), fill({quantity: 1.5}), fill({quantity: 0}), fill({price: '0.246000'}), fill({filledAt: at(62 * M)})]) {
    assert.throws(() => analyzeEntryMarkouts(input({fill: changed})));
  }
  for (const changed of [{now: NaN}, {maxHorizonDelayMs: 60001}, {maxBookLatencyMs: 20001}, {takeProfit: '0.000000'}, {stopLoss: '-0.030000'}]) {
    assert.throws(() => analyzeEntryMarkouts(input(changed)));
  }
});
