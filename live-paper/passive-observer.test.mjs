import test from 'node:test';
import assert from 'node:assert/strict';
import {createPassiveObserver, observePassive, passiveReport, replayPassive} from './passive-observer.mjs';
import {timestamp} from '../feed/contract.mjs';
const start = Date.parse('2026-10-02T12:00:00.000Z'), at = ms => new Date(start + ms).toISOString();
const make = config => createPassiveObserver({marketId: '377', exchangeId: '1066', sourceHash: 'a'.repeat(64), cutoff: at(0),
  config: {quoteEveryMs: 600000, horizonsMs: [2000, 4000], markoutLatenessMs: 1000, ...config}});
const continuity = (revision, patch = {}) => ({epoch: 'epoch_1', revision, previousRevision: revision - 1, ready: true, continuous: true, ...patch});
function book(index, ms, revision, bid = '0.400000', ask = '0.420000', options = {}) {
  const sourceAt = timestamp(at(ms));
  return {index, kind: 'book', receivedAt: at(ms), continuity: continuity(revision, index === 1 ? {recovered: true} : {}),
    book: {marketId: '377', exchangeId: '1066', instrument: '1066:YES', receivedAt: at(ms), requestStartedAt: at(ms),
      sourceAt, sequence: String(revision * 10), versionKey: revision * 10 + ':' + sourceAt,
      bids: [{price: bid, quantity: 1000}], asks: [{price: ask, quantity: 1000}]}, ...options};
}
function trade(index, ms, revision, id = 'trade_1', price = '0.400000', quantity = 3, options = {}) {
  return {index, kind: 'trade', receivedAt: at(ms), continuity: continuity(revision), trade: {id, sequence: String(revision * 10),
    marketId: '377', exchangeId: '1066', tournamentId: 'bda92870-621e-47b0-bc3c-3602c5c26f55', sourceAt: timestamp(at(ms)), price, quantity}, ...options};
}
function step(x, e) { const out = observePassive(x.state, e, x.bank); return {...x, ...out}; }
test('prospective distinct interior hypotheses use fixed ticks and never imply a placement or fill', () => {
  let x = make(); x = step(x, book(1, -1000, 1)); assert.equal(x.state.pairs, 0);
  x = step(x, book(2, 100, 2, '0.400000', '0.410000')); assert.equal(x.state.pairs, 0);
  x = step(x, book(3, 200, 3, '0.400000', '0.415000'));
  assert.equal(x.state.pairs, 1); assert.deepEqual(x.state.quotes.map(q => q.price), ['0.405000', '0.410000']);
  assert.deepEqual(passiveReport(x.state, x.bank).execution, {ordersSubmitted: 0, simulatedFills: null, partialFillQuantity: null, fillProbability: null, pnl: null});
});
test('quotes require accepted recovery; lifecycle readiness cannot be inferred from a snapshot', () => {
  let x = make(); x = step(x, book(1, 100, 1, undefined, undefined, {continuity: continuity(1)}));
  assert.equal(x.state.pairs, 0); assert.equal(x.state.ready, false);
  x = step(x, book(2, 200, 2, undefined, undefined, {continuity: continuity(2, {recovered: true})}));
  assert.equal(x.state.pairs, 1);
});
test('latency and a later independent nonmarketable book precede any trade evidence', () => {
  let x = step(make(), book(1, 100, 1));
  x = step(x, trade(2, 200, 2)); assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '0');
  x = step(x, book(3, 700, 3)); assert.equal(x.state.quotes[0].activationStatus, 'observed_nonmarketable');
  x = step(x, trade(4, 700, 3, 'same_batch', '0.400000', 100));
  assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '0');
  x = step(x, trade(5, 800, 4, 'later', '0.400000', 3));
  assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '3');
  assert.equal(x.events[0].executionClaim, 'unverified'); assert.equal(x.events[0].queueConsumed, false);
  assert.equal(passiveReport(x.state, x.bank).execution.partialFillQuantity, null);
});
test('opposite book touch or cross and a trade at the quote price do not count as trade-through', () => {
  let x = step(make(), book(1, 100, 1)); x = step(x, book(2, 700, 2));
  x = step(x, book(3, 800, 3, '0.395000', '0.405000'));
  assert.equal(x.state.quotes[0].firstOppositeQuoteTouchOrCrossAt, at(800));
  assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '0');
  x = step(x, trade(4, 900, 4, 'touch', '0.405000', 1000));
  assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '0');
  assert.equal(x.state.counts.qualifiedTradeTouches, 1);
});
test('visible same or better queue high-water never falls with cancellations or partial-sized trades', () => {
  let x = step(make(), book(1, 100, 1));
  x = step(x, book(2, 700, 2, '0.410000', '0.425000'));
  assert.equal(x.state.quotes[0].visibleQueueAheadHighWater, '1000');
  x = step(x, trade(3, 800, 3, 'partial_size', '0.400000', 3));
  assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '3');
  x = step(x, book(4, 900, 4));
  assert.equal(x.state.quotes[0].visibleSameOrBetterNow, '0');
  assert.equal(x.state.quotes[0].visibleQueueAheadHighWater, '1000');
});
test('activation observed marketable closes that side without fabricated fill evidence', () => {
  let x = step(make(), book(1, 100, 1)); x = step(x, book(2, 700, 2, '0.390000', '0.400000'));
  assert.equal(x.state.quotes[0].activationStatus, 'marketable_at_first_followup');
  x = step(x, trade(3, 800, 3)); assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '0');
});
test('markouts are signed unexecuted-quote price observations, with late horizons censored', () => {
  let x = step(make(), book(1, 100, 1)); x = step(x, book(2, 700, 2)); x = step(x, book(3, 2100, 3, '0.395000', '0.415000'));
  const mark = x.state.quotes[0].markouts['2000']; assert.equal(mark.status, 'observed_unexecuted_quote_markout');
  assert.equal(mark.signedOppositeExecutableMoveAfterCushion, '-0.015000'); assert.equal(mark.latenessMs, 0);
  x = step(x, {index: 4, kind: 'clock', receivedAt: at(5200), continuity: continuity(3)});
  assert.equal(x.state.quotes[0].markouts['4000'].status, 'missing_fresh_followup');
});
test('gap, disconnect and reconnect permanently censor earlier evidence and markouts', () => {
  let x = step(make(), book(1, 100, 1)); x = step(x, book(2, 700, 2));
  x = step(x, trade(3, 800, 4, 'gap', '0.400000', 100));
  assert.equal(x.state.ready, false); assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '0');
  assert.equal(x.state.quotes[0].markouts['2000'].status, 'censored_continuity');
  x = step(x, book(4, 2100, 1, undefined, undefined, {continuity: continuity(1, {epoch: 'epoch_2', recovered: true}),
    book: {...book(4, 2100, 5).book}}));
  assert.equal(x.state.quotes[0].closedReason, 'delivery_gap');
  assert.equal(x.state.quotes[0].markouts['2000'].status, 'censored_continuity');
});
test('stable trade IDs deduplicate volume and reject conflicting re-delivery', () => {
  let x = step(make(), book(1, 100, 1)); x = step(x, book(2, 700, 2));
  const t = trade(3, 800, 3); x = step(x, t);
  x = step(x, {...t, index: 4, receivedAt: at(900)});
  assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '3'); assert.equal(x.state.counts.duplicateTrades, 1);
  assert.throws(() => step(x, {...t, index: 5, receivedAt: at(1000), trade: {...t.trade, quantity: 4}}), /Conflicting duplicate/);
});
test('clock, version, scope and frozen bank violations fail closed without mutating input', () => {
  let x = step(make(), book(1, 100, 1)); const before = structuredClone(x.state);
  assert.throws(() => step(x, book(2, 99, 2)), /Receipt rollback/);
  assert.throws(() => step(x, book(3, 700, 2)), /Missing or reordered/);
  assert.throws(() => step(x, {...book(2, 700, 2), book: {...book(2, 700, 2).book, exchangeId: '99'}}), /scope mismatch/);
  assert.throws(() => step(x, {...book(2, 700, 2), book: {...book(2, 700, 2).book, requestStartedAt: at(701)}}), /request clock/);
  assert.throws(() => observePassive(x.state, book(2, 700, 2), {...x.bank, cutoff: at(1)}), /bank changed/);
  assert.deepEqual(x.state, before);
});
test('chained sanitized records replay deterministically and never include unknown fields', () => {
  const initial = make(), e = {...book(1, 100, 1), secret: 'do-not-retain'};
  const a = step(initial, e), b = step(initial, a.record.event);
  assert.deepEqual(a.state, b.state); assert.deepEqual(a.record, b.record);
  assert.ok(!JSON.stringify(a.record).includes('do-not-retain'));
  const duplicate = observePassive(a.state, e, a.bank); assert.equal(duplicate.record, null); assert.deepEqual(duplicate.events, []);
  const future = step(a, book(2, 700, 2)); assert.equal(future.record.previousHash, a.record.hash);
  assert.equal(a.state.quotes[0].activationStatus, 'awaiting_independent_book');
  assert.deepEqual(replayPassive([a.record, future.record], a.bank), future.state);
  assert.throws(() => replayPassive([a.record, {...future.record, previousHash: '0'.repeat(64)}], a.bank), /journal hash/);
  assert.throws(() => replayPassive([a.record, {...future.record, event: {...future.record.event, receivedAt: at(800)}}], a.bank));
});
test('trade-through cannot use stale displayed depth even when a caller reports continuity', () => {
  let x = step(make(), book(1, 100, 1)); x = step(x, book(2, 700, 2));
  x = step(x, trade(3, 20701, 3, 'late_trade', '0.400000', 1000));
  assert.equal(x.state.ready, false); assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '0');
  assert.equal(x.state.quotes[0].closedReason, 'stale_book');
});
test('same source version cannot activate or label a quote; source skew is bounded', () => {
  let x = step(make(), book(1, 100, 1));
  const unchanged = book(2, 700, 2); unchanged.book = {...x.state.lastBook, receivedAt: at(700), requestStartedAt: at(700)};
  x = step(x, unchanged); assert.equal(x.state.quotes[0].activationStatus, 'awaiting_independent_book');
  const repeated = book(3, 2100, 3); repeated.book = {...x.state.lastBook, receivedAt: at(2100), requestStartedAt: at(2100)};
  x = step(x, repeated); assert.equal(x.state.quotes[0].markouts['2000'].status, 'pending');
  const future = book(4, 2200, 4), source = timestamp(at(4201));
  future.book.sourceAt = source; future.book.versionKey = '40:' + source;
  assert.throws(() => step(x, future), /source clock/);
});
test('merged overlapping delivery revisions retain continuity; a forward hole invalidates', () => {
  let x = step(make(), book(1, 100, 1));
  x = step(x, book(2, 700, 3, undefined, undefined, {continuity: continuity(3, {previousRevision: 1})}));
  assert.equal(x.state.quotes[0].activationStatus, 'observed_nonmarketable');
  x = step(x, trade(3, 800, 5, 'overlapping_delivery', '0.400000', 3,
    {continuity: continuity(5, {previousRevision: 2})}));
  assert.equal(x.state.ready, true); assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '3');
  assert.equal(x.state.quotes[0].closedAt, null);
  const sameRevisionNewBook = book(4, 900, 5, undefined, undefined, {continuity: continuity(5, {previousRevision: 2})});
  x = step(x, sameRevisionNewBook); assert.equal(x.state.ready, true);
  x = step(x, trade(5, 1000, 7, 'actual_gap', '0.400000', 100,
    {continuity: continuity(7, {previousRevision: 6})}));
  assert.equal(x.state.ready, false); assert.equal(x.state.quotes[0].qualifiedTradeThroughQuantity, '3');
});
