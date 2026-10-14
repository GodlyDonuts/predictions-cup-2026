import test from 'node:test';
import assert from 'node:assert/strict';
import {payloadHash} from '../execution/core.mjs';
import {createBookPublicationState} from '../live-paper/book-publication-v3.mjs';
import {createOfficialFillBookTracker, createPublicBookJournalVerifier, verifyOrderAckReceiptChain} from './official-fill-book-markout-v1.mjs';

const tournament = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const universe = {tournamentId: tournament, exchanges: [
  {marketId: '386', exchangeId: '1075', option: 'YES'}, {marketId: '390', exchangeId: '1081', option: 'YES'},
]};
const base = Date.parse('2026-10-14T18:00:00.000Z');
const iso = ms => new Date(ms).toISOString();

function appendRow(sequence, previous, fields) {
  const body = {...fields, sequence, previous};
  return {...body, hash: payloadHash(body)};
}
function ackRow(orderId, exchangeId = '1075', side = 'yes', action = 'buy') {
  return {schemaVersion: 1, kind: 'order_ack_principal', tournamentId: tournament, matchedIdentity: true,
    exchangeId, orderId, canonicalSide: side, canonicalAction: action, requestHash: 'a'.repeat(64), fields: {},
    method: 'POST', endpoint: '/orders', receivedAt: iso(base), sourceBodyHash: 'b'.repeat(64), executionAuthority: false};
}
function verifiedAcks(receipts = [ackRow('7001')]) {
  const rows = []; let previous = null;
  for (const receipt of receipts) { const row = appendRow(rows.length + 1, previous, {receipt}); rows.push(row); previous = row.hash; }
  return verifyOrderAckReceiptChain(rows);
}
function bookRecord(verifier, sequence, previousHash, atMs, kind, payload, sessionId = 'book-test') {
  const body = {schemaVersion: 1, kind, sequence, previousHash, sessionId, tournamentId: tournament,
    scopeHash: createBookPublicationState(universe).scopeHash, at: iso(atMs), payload};
  return {...body, hash: payloadHash(body)};
}
function validBook({marketId = '386', exchangeId = '1075', receivedMs, sourceMs = receivedMs - 10, bid = '0.500000', ask = '0.510000', bidQty = 20, askQty = 20, seq = 1}) {
  return {tournamentId: tournament, marketId, exchangeId, instrument: exchangeId + ':YES', receivedAt: iso(receivedMs),
    requestStartedAt: iso(receivedMs - 50), sourceAt: iso(sourceMs), sequence: String(seq), versionKey: `${seq}:${iso(sourceMs)}`,
    freshnessPolicy: 'authoritative_receipt_v1', sourceEventAgeMs: receivedMs - sourceMs,
    bids: [{price: bid, quantity: bidQty}], asks: [{price: ask, quantity: askQty}]};
}
function trackerFor({fills, acknowledgements = verifiedAcks(), hasMore = true} = {}) {
  return createOfficialFillBookTracker({fillRows: fills ?? [{id: 9001, orderId: 7001, marketId: '386', exchangeId: '1075',
    side: 'yes', quantity: 10, price: 0.5, at: iso(base + 10_000)}], fillPageHasMore: hasMore, orderAcknowledgements: acknowledgements});
}

test('receipt chain binds actual official fills to this journal and isolates conflicting order IDs', () => {
  const verified = verifiedAcks([ackRow('7001'), ackRow('7002', '1081')]);
  assert.equal(verified.receiptRowsVerified, 2);
  assert.equal(verified.orders.size, 2);
  const conflicted = verifiedAcks([ackRow('7001'), ackRow('7001', '1081')]);
  assert.equal(conflicted.orders.has('7001'), false);
  assert.equal(conflicted.conflictedOrderIds.has('7001'), true);
  assert.throws(() => verifyOrderAckReceiptChain([{...appendRow(1, '', {receipt: ackRow('7001')}), previous: 'wrong'}]), /RECEIPT_CHAIN/);
  const fills = [
    {id: 1, orderId: 7001, marketId: '386', exchangeId: '1075', side: 'yes', quantity: 1, price: 0.5, at: iso(base + 1)},
    {id: 2, orderId: 7002, marketId: '390', exchangeId: '1081', side: 'yes', quantity: 1, price: 0.5, at: iso(base + 1)},
  ];
  const tracker = trackerFor({fills, acknowledgements: conflicted});
  assert.deepEqual(tracker.diagnostics.excluded, {duplicateFillIds: 0, missingOrderId: 0, noBaalOrderAck: 1,
    conflictingOrderAck: 1, orderScopeMismatch: 0, invalidFill: 0});
});

test('causal horizons walk executable depth, leave partial shares unmarked, and use no fee', () => {
  const tracker = trackerFor();
  const pre = validBook({receivedMs: base + 9_000, sourceMs: base + 8_990, bid: '0.490000', ask: '0.510000'});
  tracker.observeBook(pre);
  const oneSecond = validBook({receivedMs: base + 11_100, sourceMs: base + 11_050, bid: '0.520000', ask: '0.540000', bidQty: 5, askQty: 20, seq: 2});
  tracker.observeBook(oneSecond);
  tracker.observeBook(validBook({receivedMs: base + 15_100, sourceMs: base + 15_050, bid: '0.510000', ask: '0.530000', seq: 3}));
  tracker.observeBook(validBook({receivedMs: base + 40_100, sourceMs: base + 40_050, bid: '0.490000', ask: '0.510000', seq: 4}));
  const result = tracker.finish();
  const one = result.horizons['1000'];
  assert.equal(result.coverage.matchedBaalFillRows, 1);
  assert.equal(result.coverage.preFillContextAtMostFiveSeconds, 1);
  assert.equal(result.fillVsAckResponse.buckets.fillAfterAckReceiptOneToThirtySeconds, 1);
  assert.equal(result.fillVsAckResponse.lagMs.positiveMeansFillFollowedAckResponse.medianMs, 10_000);
  assert.equal(one.observedLaterBooks, 1);
  assert.equal(one.partialDepthFillRows, 1);
  assert.equal(one.markedShares, 5);
  assert.equal(one.unmarkedShares, 5);
  assert.equal(one.weightedExecutableMarkoutPerShare, '0.020000');
  assert.equal(one.weightedMidMarkoutPerShare, '0.030000');
  assert.equal(result.horizons['5000'].weightedExecutableMarkoutPerShare, '0.010000');
  assert.equal(result.horizons['30000'].weightedExecutableMarkoutPerShare, '-0.010000');
  assert.equal(result.assumptions.feesPerShare, '0.000000');
  assert.equal(result.assumptions.realizedPortfolioPnl, false);
  assert.equal(result.assumptions.profitabilityEstablished, false);
  assert.equal(JSON.stringify(result).includes('7001'), false);
  assert.doesNotMatch(JSON.stringify(result), /"(?:orderId|marketId|fillId|accountId)"/);
});

test('a book with source time before the horizon cannot leak forward into the markout', () => {
  const tracker = trackerFor();
  tracker.observeBook(validBook({receivedMs: base + 11_000, sourceMs: base + 10_999, bid: '0.800000', ask: '0.820000'}));
  tracker.observeBook(validBook({receivedMs: base + 12_000, sourceMs: base + 10_999, bid: '0.800000', ask: '0.820000', seq: 2}));
  assert.equal(tracker.finish().horizons['1000'].observedLaterBooks, 0);
});

test('continuity audit finds a causal next book beyond 30 seconds without calling it a 30s mark', () => {
  const tracker = trackerFor();
  tracker.observeBook(validBook({receivedMs: base + 55_000, sourceMs: base + 52_000,
    bid: '0.500000', ask: '0.510000'}));
  const result = tracker.finish();
  assert.deepEqual(result.bookContinuity.firstLaterBookReceiptLagMs,
    {count: 1, medianMs: 45_000, p90Ms: 45_000, maxMs: 45_000});
  assert.deepEqual(result.bookContinuity.firstLaterBookSourceLagMs,
    {count: 1, medianMs: 42_000, p90Ms: 42_000, maxMs: 42_000});
  assert.equal(result.bookContinuity.laterBookReceiptLagBuckets.thirtySecondsToFiveMinutes, 1);
  assert.equal(result.bookCoverageNearFill.fillsWithNoBookReceiptInWindow, 1);
  assert.equal(result.horizons['30000'].observedLaterBooks, 0);
  assert.equal(result.horizons['30000'].weightedExecutableMarkoutPerShare, null);
});

test('near-fill coverage separates same-market and other-market receipt activity', () => {
  const tracker = trackerFor();
  tracker.observeBook(validBook({receivedMs: base - 20_000, sourceMs: base - 20_050}));
  tracker.observeBook(validBook({receivedMs: base + 10_100, sourceMs: base + 10_050}));
  tracker.observeBook(validBook({marketId: '390', exchangeId: '1081', receivedMs: base - 20_000,
    sourceMs: base - 20_050}));
  tracker.observeBook(validBook({marketId: '390', exchangeId: '1081', receivedMs: base + 40_000,
    sourceMs: base + 39_950}));
  tracker.observeBook(validBook({marketId: '390', exchangeId: '1081', receivedMs: base + 40_001,
    sourceMs: base + 39_950}));
  const result = tracker.finish();
  assert.equal(result.bookCoverageNearFill.windowMsEachSide, 30_000);
  assert.equal(result.bookCoverageNearFill.fillsWithSameMarketBookReceipt, 1);
  assert.equal(result.bookCoverageNearFill.fillsWithOtherMarketBookReceipt, 1);
  assert.equal(result.bookCoverageNearFill.fillsWithOtherMarketButNoSameMarketBookReceipt, 0);
  assert.deepEqual(result.bookCoverageNearFill.receiptsPerFillWindow, {sameMarket: 2, otherMarket: 2});
  assert.deepEqual(result.bookCoverageNearFill.receiptDirectionCounts, {
    sameMarketBeforeFill: 1, sameMarketAfterFill: 1, otherMarketBeforeFill: 1, otherMarketAfterFill: 1,
  });
  assert.equal(JSON.stringify(result).includes('7001'), false);
  assert.doesNotMatch(JSON.stringify(result), /"(?:orderId|marketId|fillId|accountId)"/);
});

test('other-market capture in the window is not mistaken for same-market fill coverage', () => {
  const tracker = trackerFor();
  tracker.observeBook(validBook({marketId: '390', exchangeId: '1081', receivedMs: base + 10_000,
    sourceMs: base + 9_950}));
  const coverage = tracker.finish().bookCoverageNearFill;
  assert.equal(coverage.fillsWithSameMarketBookReceipt, 0);
  assert.equal(coverage.fillsWithOtherMarketBookReceipt, 1);
  assert.equal(coverage.fillsWithOtherMarketButNoSameMarketBookReceipt, 1);
  assert.deepEqual(coverage.receiptsPerFillWindow, {sameMarket: 0, otherMarket: 1});
});

test('book received after fill but sourced at or before the fill is not a later causal quote', () => {
  const tracker = trackerFor();
  tracker.observeBook(validBook({receivedMs: base + 11_000, sourceMs: base + 10_000,
    bid: '0.800000', ask: '0.820000'}));
  const result = tracker.finish();
  assert.equal(result.bookContinuity.fillsWithAnyLaterCausalBook, 0);
  assert.equal(result.bookContinuity.fillsWithNoLaterCausalBook, 1);
  assert.equal(result.bookContinuity.laterBookReceiptLagBuckets.missing, 1);
});

test('ACK response timing labels fills completed before the HTTP response without calling them stale orders', () => {
  const acks = verifiedAcks([{...ackRow('7001'), receivedAt: iso(base + 11_000)}]);
  const result = trackerFor({acknowledgements: acks}).finish();
  assert.equal(result.fillVsAckResponse.fillsWithAckReceiptTimestamp, 1);
  assert.equal(result.fillVsAckResponse.buckets.fillBeforeOrAtAckReceipt, 1);
  assert.equal(result.fillVsAckResponse.lagMs.negativeMeansFillPrecededAckResponse.medianMs, 1_000);
  assert.equal(result.fillVsAckResponse.lagMs.positiveMeansFillFollowedAckResponse.count, 0);
});

test('public-book journal verifies session scope, every hash link, timestamp and depth constraints', () => {
  const initial = createBookPublicationState(universe), verifier = createPublicBookJournalVerifier(universe);
  const start = {schemaVersion: 1, kind: 'session_start', sequence: 1, previousHash: '', sessionId: 'book-test',
    tournamentId: tournament, scopeHash: initial.scopeHash, at: iso(base), payload: {scope: initial.scope}};
  start.hash = payloadHash(start);
  verifier.observe(start);
  const book = validBook({receivedMs: base + 500, sourceMs: base + 490});
  const record = bookRecord(verifier, 2, start.hash, base + 600, 'book', {book});
  assert.equal(verifier.observe(record), book);
  assert.equal(verifier.stats().books, 1);
  assert.throws(() => verifier.observe({...record, sequence: 3}), /SOURCE_CHAIN/);
  const badVerifier = createPublicBookJournalVerifier(universe);
  badVerifier.observe(start);
  const crossed = bookRecord(badVerifier, 2, start.hash, base + 600, 'book', {book: {...book, asks: [{price: '0.500000', quantity: 20}]}});
  assert.throws(() => badVerifier.observe(crossed), /SOURCE_BOOK_CROSSED/);
});

test('duplicate official fills are counted once and missing books remain censored', () => {
  const fill = {id: 9001, orderId: 7001, marketId: '386', exchangeId: '1075', side: 'yes', quantity: 10, price: 0.5, at: iso(base + 10_000)};
  const tracker = trackerFor({fills: [fill, fill]});
  const result = tracker.finish();
  assert.equal(result.coverage.duplicateFillIds, 1);
  assert.equal(result.horizons['1000'].observedLaterBooks, 0);
  assert.equal(result.horizons['1000'].unmarkedShares, 10);
  assert.equal(result.horizons['1000'].weightedExecutableMarkoutPerShare, null);
  assert.equal(result.coverage.officialFillPageHasMore, true);
});

test('numeric API prices tolerate binary representation noise but reject off-tick fills', () => {
  const noisy = {id: 9002, orderId: 7001, marketId: '386', exchangeId: '1075', side: 'yes', quantity: 1,
    price: 0.30000000000000004, at: iso(base + 10_000)};
  const offTick = {...noisy, id: 9003, price: 0.302};
  const tracker = trackerFor({fills: [noisy, offTick]});
  const result = tracker.finish();
  assert.equal(result.coverage.matchedBaalFillRows, 1);
  assert.equal(result.coverage.excluded.invalidFill, 1);
});

