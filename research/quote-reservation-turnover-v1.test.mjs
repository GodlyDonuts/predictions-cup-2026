import test from 'node:test';
import assert from 'node:assert/strict';
import {payloadHash} from '../execution/ledger.mjs';
import {analyzeQuoteReservationTurnoverV1} from './quote-reservation-turnover-v1.mjs';

const epoch = Date.parse('2026-10-13T00:00:00.000Z');
const at = n => new Date(epoch + n).toISOString();

function bank() {
  return {schemaVersion: 1, executionAuthority: false, bankHash: 'a'.repeat(64), cohort: 'fixture',
    cutoff: at(0), policy: {participationBps: 100, wallet: {fillModel: 'strict_tape_through',
      latencyMs: 3_000, lifetimeMs: 120_000, initialCash: '1000.000000', maximumCommitted: '100.000000'}}};
}

function quote(id, overrides = {}) {
  const book = {marketId: '258', exchangeId: '947', versionKey: `1:${at(0)}`,
    requestStartedAt: at(0), receivedAt: at(100), sourceAt: at(0),
    bids: [{price: '0.400000', quantity: 100}], asks: [{price: '0.410000', quantity: 200}],
    originalSourceBookHash: 'b'.repeat(64), sourceBinding: {recordHash: 'c'.repeat(64), rawBookHash: 'd'.repeat(64),
      continuityProven: false, observedDepth: {bids: 12, asks: 9}}};
  return {id, marketId: '258', exchangeId: '947', side: 'YES', action: 'buy', quantity: 10, remaining: 10,
    limit: '0.400000', createdAt: at(1_000), activationAt: epoch + 2_000, expiresAt: epoch + 5_500,
    cancelEffectiveAt: epoch + 4_500, closedAt: null, decisionVersion: book.versionKey,
    decisionBookHash: payloadHash(book), queueDecisionBook: book,
    queueStudy: {models: [{initialDisplayedAhead: 100, queuePositionProven: false, modeledQuantity: 0}]},
    projection: undefined, ...overrides};
}

function state(b, quotes, fills = [], patch = {}) {
  return {bankHash: b.bankHash, wallet: {participationCounts: {prints: 4_080, eligiblePrints: 0, grantedShares: 0},
    wallet: {lastAt: at(10_000), cash: '1000.000000', realized: '0.000000', positions: {}, quotes, fills}, ...patch}};
}

test('expired or modeled-cancelled buy quotes keep reserving cash until wallet closure', () => {
  const b = bank(), q = quote(1), report = analyzeQuoteReservationTurnoverV1({bank: b, state: state(b, [q])});
  assert.equal(report.wallet.reservedCash, '4.000000');
  assert.equal(report.quoteLifecycle.unresolvedExpiredBuyQuotes, 1);
  assert.equal(report.quoteLifecycle.unresolvedAfterModeledCancel, 1);
  assert.equal(report.quoteLifecycle.reservedCashAfterModeledCancel, '4.000000');
  assert.equal(report.quoteLifecycle.reservationCapitalDollarHours, '0.010');
  assert.equal(report.quoteLifecycle.cashReservationDollarHoursAfterEffectiveCancelOrExpiry, '0.006');
  assert.equal(report.quoteLifecycle.retirementBlockers.missing_projection, 1);
  assert.equal(report.fillsAndMarkouts.paperFillEvents, 0);
  assert.equal(report.fillsAndMarkouts.markouts[0].n, 0);
  assert.equal(report.interpretation.profitabilityEstablished, false);
});

test('uses local fill knowledge time for reserve release and source time for fill age', () => {
  const b = bank(), q = quote(2, {remaining: 2});
  const fill = {quoteId: 2, marketId: q.marketId, exchangeId: q.exchangeId, side: q.side, action: q.action,
    quantity: 8, price: q.limit, at: at(5_800), actualFill: false,
    source: {createdAt: at(4_000), knownAt: at(5_600), strictTradeThrough: true,
      queuePositionProven: false, tapeParticipationBps: 100}};
  const afterFill = {marketId: '258', exchangeId: '947', versionKey: `2:${at(7_000)}`,
    requestStartedAt: at(7_000), receivedAt: at(7_100), sourceAt: at(6_950),
    bids: [{price: '0.410000', quantity: 10}], asks: [{price: '0.420000', quantity: 20}]};
  const report = analyzeQuoteReservationTurnoverV1({bank: b, state: state(b, [q], [fill]), bookReceipts: [afterFill]});
  assert.equal(report.wallet.paperFillEvents, 1);
  assert.equal(report.wallet.actualFillEvents, 0);
  assert.equal(report.wallet.paperBuyFillNotional, '3.200000');
  assert.equal(report.wallet.reservedCash, '0.800000');
  assert.equal(report.quoteLifecycle.reservationCapitalDollarHours, '0.006');
  assert.equal(report.quoteLifecycle.cashReservationDollarHoursAfterEffectiveCancelOrExpiry, '0.002');
  assert.equal(report.fillsAndMarkouts.fillSourceAgeMs.median, 2_000);
  assert.equal(report.fillsAndMarkouts.fillKnowledgeDelayMs.median, 1_800);
  assert.equal(report.fillsAndMarkouts.markouts[0].n, 1);
  assert.equal(report.fillsAndMarkouts.markouts[0].meanPerShare, '0.010000');
  assert.equal(report.fillsAndMarkouts.markouts[0].depthComplete, 1);
});

test('uses side-correct YES-book levels and refuses a modified decision receipt', () => {
  const b = bank(), q = quote(3, {side: 'NO', limit: '0.590000'});
  const yesBook = {...q.queueDecisionBook, bids: [{price: '0.400000', quantity: 7}],
    asks: [{price: '0.410000', quantity: 13}]};
  q.queueDecisionBook = yesBook;
  q.decisionBookHash = payloadHash(yesBook);
  q.queueStudy.models[0].initialDisplayedAhead = 13;
  const report = analyzeQuoteReservationTurnoverV1({bank: b, state: state(b, [q])});
  assert.equal(report.priceAndDepth.topOfBookSameOutcomeBidShares.median, 13);
  assert.equal(report.priceAndDepth.topOfBookSameOutcomeAskShares.median, 7);
  assert.equal(report.priceAndDepth.quotesWithHashBoundDecisionBook, 1);
  q.decisionBookHash = 'f'.repeat(64);
  assert.throws(() => analyzeQuoteReservationTurnoverV1({bank: b, state: state(b, [q])}), /QUOTE_TURNOVER_BOOK_HASH/);
});

test('rejects platform fills from a paper-only analysis', () => {
  const b = bank(), q = quote(4);
  const fill = {quoteId: 4, marketId: q.marketId, exchangeId: q.exchangeId, side: q.side, action: q.action,
    quantity: 1, price: q.limit, at: at(4_000), actualFill: true,
    source: {createdAt: at(3_000), knownAt: at(3_500), strictTradeThrough: true, queuePositionProven: false}};
  assert.throws(() => analyzeQuoteReservationTurnoverV1({bank: b, state: state(b, [q], [fill])}), /QUOTE_TURNOVER_NON_PAPER_FILL/);
});

