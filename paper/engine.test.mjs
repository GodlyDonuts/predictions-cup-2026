import test from 'node:test';
import assert from 'node:assert/strict';
import {PaperEngine, replay} from './engine.mjs';
import {amount, money, price, fee} from './fixed.mjs';
import {validateEvent, validateConfig} from './schema.mjs';
import {config, event, snapshot, order, trade, timestamp, level} from './test-helpers.mjs';

const run = (events, cfg = config()) => replay(events, cfg);
test('fixed point round-trips beyond Number range, negatives, tick and fee rounding', () => {
  assert.equal(money(amount('999999999999999999.123456')), '999999999999999999.123456');
  assert.equal(money(amount('-0.123')), '-0.123000'); assert.equal(price('0.995'), 995_000n);
  for (const x of [0.4, '1e-3', '0.1234567', '01.0', 'NaN', 'Infinity']) assert.throws(() => amount(x));
  for (const x of ['0', '1', '0.004', '0.006']) assert.throws(() => price(x));
  assert.equal(fee(5000n, 1, {feeBps: 1, feePerShare: '0'}), 1n);
});
test('snapshot touch, shrinking depth, and mark changes never imply fills', () => {
  const r = run([snapshot(), order(), snapshot(2, {sequence: '2', bids: [level('0.400', 1)]}),
    event('mark', 3, {instrument: 'synthetic:YES', source: 'last', price: '0.400'})]);
  assert.equal(r.fills.length, 0); assert.equal(r.orders[0].queueAhead, '10'); assert.equal(r.cash, '100.000000');
});
test('trade volume first consumes external queue, then partially fills', () => {
  const r = run([snapshot(), order(), trade(2, {quantity: 12})]);
  assert.equal(r.fills[0].quantity, 2); assert.equal(r.orders[0].remaining, 3); assert.equal(r.cash, '99.200000');
  assert.equal(r.reservedCash, '1.200000'); assert.equal(r.remainingCostBasis, '0.800000');
});
test('same-price own orders are FIFO and cannot multiply a single trade volume', () => {
  const r = run([snapshot(), order(1), order(2), trade(3, {quantity: 16}), trade(4, {quantity: 4})]);
  assert.deepEqual(r.fills.map(x => [x.orderId, x.quantity]), [['o-1', 5], ['o-2', 1], ['o-2', 4]]);
  assert.equal(r.cash, '96.000000'); assert.equal(r.reservedCash, '0.000000');
});
test('disappearing depth cannot let later FIFO orders overtake or multiply print volume', () => {
  const r = run([snapshot(0, {bids: [level('0.400', 10)]}), order(1, {quantity: 10}),
    snapshot(2, {sequence: '2', bids: []}), order(3, {quantity: 10}), order(4, {quantity: 10}),
    trade(5, {bookSequence: '2', quantity: 15}), trade(6, {bookSequence: '2', quantity: 10})]);
  assert.deepEqual(r.fills.map(x => [x.orderId, x.quantity]), [['o-1', 5], ['o-1', 5], ['o-3', 5]]);
  assert.equal(r.fills.filter(x => x.eventId === 'trade-6').reduce((n, x) => n + x.quantity, 0), 10);
});
test('pending-cancel own paper quotes cannot be crossed', () => {
  const r = run([snapshot(0, {bids: []}), order(), trade(2, {quantity: 5}), order(3, {limitPrice: '0.500'}),
    event('cancel', 4, {orderId: 'o-3'}), order(5, {action: 'sell', limitPrice: '0.450'})]);
  assert.equal(r.orders[2].reason, 'crosses_own_paper_order');
});
test('trade-depleted zero-depth level cannot reject a nonmarketable order', () => {
  const r = run([snapshot(0, {bids: [level('0.400', 1)]}), order(), trade(2, {quantity: 6}),
    order(3, {action: 'sell', limitPrice: '0.400'})]);
  assert.equal(r.orders[1].status, 'open');
});
test('own historical bid depth is never counted as executable liquidation liquidity', () => {
  const r = run([snapshot(0, {bids: []}), order(), trade(2, {quantity: 5}),
    snapshot(3, {sequence: '3', bids: [level('0.450', 5, 4)]})]);
  assert.equal(r.positions[0].executable.soldQuantity, 1);
  assert.equal(r.positions[0].executable.unsoldQuantity, 4);
  assert.equal(r.estimatedExecutableEquity, null); assert.equal(r.positions[0].executable.fullPositionValue, null);
});
test('known own aggressor volume never creates a self-fill', () => {
  const r = run([snapshot(), order(), trade(2, {externalAggressor: false, quantity: 100})]);
  assert.equal(r.fills.length, 0); assert.equal(r.metrics.ownAggressorExcluded, 1);
});
test('a worse/better price print is not evidence of volume at our limit', () => {
  const r = run([snapshot(), order(), trade(2, {price: '0.395', quantity: 100}), trade(3, {price: '0.405', quantity: 100})]);
  assert.equal(r.fills.length, 0);
});
test('latency prevents pre-arrival and ambiguous same-time fills', () => {
  const r = run([snapshot(0, {bids: []}), order(), trade(20, {quantity: 5}), trade(51, {quantity: 5}), trade(52, {quantity: 3})], config({orderLatencyMs: 50}));
  assert.equal(r.fills.length, 1); assert.equal(r.fills[0].quantity, 3); assert.equal(r.fills[0].eventId, 'trade-52');
});
test('cancel/fill race fills before modeled acknowledgement and stops after', () => {
  const r = run([snapshot(0, {bids: []}), order(), event('cancel', 2, {orderId: 'o-1'}),
    trade(20, {quantity: 2}), trade(52, {quantity: 9}), trade(60, {quantity: 9})]);
  assert.equal(r.fills.length, 1); assert.equal(r.orders[0].status, 'cancelled'); assert.equal(r.orders[0].remaining, 3);
  assert.equal(r.reservedCash, '0.000000');
});
test('expiry is exclusive and evaluated before same-time trade', () => {
  const r = run([snapshot(0, {bids: []}), order(1, {expiresAt: timestamp(3)}), trade(2, {quantity: 2}), trade(3, {quantity: 3})]);
  assert.equal(r.orders[0].status, 'expired'); assert.equal(r.fills.length, 1);
});
test('expiry while placement is pending precludes activation', () => {
  const r = run([snapshot(), order(1, {expiresAt: timestamp(10)}), event('clock', 100)], config({orderLatencyMs: 50}));
  assert.equal(r.orders[0].status, 'expired'); assert.equal(r.fills.length, 0);
});
test('late pre-cancel print fails closed rather than hiding ambiguous execution', () => {
  const engine = new PaperEngine(config());
  [snapshot(0, {bids: []}), order(), event('cancel', 2, {orderId: 'o-1'}), event('clock', 60)].forEach(e => engine.apply(e));
  assert.throws(() => engine.apply(trade(61, {occurredAt: timestamp(20), quantity: 2})), /Ambiguous late/);
  assert.throws(() => engine.report(), /failed closed/); assert.throws(() => engine.apply(event('clock', 70)), /failed closed/);
});
test('stale evidence cannot bypass ambiguous late fill race detection', () => {
  assert.throws(() => run([snapshot(0, {bids: []}), order(), event('cancel', 2, {orderId: 'o-1'}),
    trade(100, {occurredAt: timestamp(20)})], config({maxDataAgeMs: 10})), /Ambiguous late/);
});
test('book expiry and stale source/receipt time block new paper orders', () => {
  assert.equal(run([snapshot(0, {nextExpiryAt: timestamp(10)}), order(10)]).orders[0].reason, 'book_not_fresh_at_submission');
  assert.equal(run([snapshot(), order(11)], config({maxDataAgeMs: 10})).orders[0].reason, 'book_not_fresh_at_submission');
  assert.equal(run([snapshot(100, {occurredAt: timestamp(0)}), order(101)], config({maxDataAgeMs: 10})).orders[0].reason, 'book_not_fresh_at_submission');
});
test('activation checks freshness again after latency', () => {
  const r = run([snapshot(), order(), event('clock', 100)], config({orderLatencyMs: 50, maxDataAgeMs: 10}));
  assert.equal(r.orders[0].reason, 'book_not_fresh_at_activation');
});
test('gaps block fills and decisions until a newer authoritative snapshot', () => {
  const r = run([snapshot(0, {bids: []}), order(), event('gap', 2, {instrument: 'synthetic:YES', reason: 'disconnect'}),
    trade(3), snapshot(4, {sequence: '4', authoritative: false}), order(5),
    snapshot(6, {sequence: '6', bids: []}), trade(7, {bookSequence: '6', quantity: 2})]);
  assert.equal(r.fills.length, 1); assert.equal(r.fills[0].eventId, 'trade-7');
  assert.equal(r.orders[1].reason, 'book_not_fresh_at_submission'); assert.equal(r.metrics.gaps, 1);
});
test('trade/book version mismatch requires authoritative resync', () => {
  const r = run([snapshot(), order(), trade(2, {bookSequence: '0'}), trade(3)]);
  assert.equal(r.fills.length, 0); assert.equal(r.metrics.gaps, 1);
});
test('in-flight authoritative snapshot sourced before gap cannot resurrect an old book', () => {
  const r = run([snapshot(0, {bids: []}), order(), event('gap', 5, {instrument: 'synthetic:YES', reason: 'disconnect'}),
    snapshot(6, {occurredAt: timestamp(2), sequence: '2', bids: []}),
    trade(7, {sequence: '3', bookSequence: '2', quantity: 5})]);
  assert.equal(r.fills.length, 0); assert.equal(r.cash, '100.000000');
  assert.ok(r.audit.some(x => x.reason === 'snapshot_before_recovery_barrier'));
});
test('mismatched trade establishes sequence and source-time floors independent from applied book', () => {
  const r = run([snapshot(0, {bids: []}), order(), trade(5, {sequence: '5', bookSequence: '0', quantity: 5}),
    snapshot(6, {occurredAt: timestamp(2), sequence: '2', bids: []}),
    snapshot(7, {sequence: '4', bids: []}), trade(8, {sequence: '3', bookSequence: '4', quantity: 5}),
    snapshot(9, {sequence: '5', bids: []}), trade(10, {sequence: '6', bookSequence: '5', quantity: 2})]);
  assert.equal(r.fills.length, 1); assert.equal(r.fills[0].eventId, 'trade-10'); assert.equal(r.fills[0].quantity, 2);
});
test('gap before first book and boundary-equal resync remain blocked until later authoritative source', () => {
  const r = run([event('gap', 5, {instrument: 'synthetic:YES', reason: 'initial disconnect'}),
    snapshot(6, {occurredAt: timestamp(5), sequence: '3', bids: []}), order(7),
    snapshot(8, {sequence: '4', authoritative: false, bids: []}), order(9),
    snapshot(10, {sequence: '4', bids: []}), order(11), trade(12, {sequence: '5', bookSequence: '4', quantity: 2})]);
  assert.equal(r.orders[0].reason, 'book_not_fresh_at_submission'); assert.equal(r.orders[1].reason, 'book_not_fresh_at_submission');
  assert.equal(r.fills.length, 1); assert.equal(r.fills[0].orderId, 'o-11');
});
test('observed higher versions during invalid state cannot be rolled back by later resync', () => {
  const r = run([snapshot(0, {bids: []}), order(), event('gap', 3, {instrument: 'synthetic:YES', reason: 'disconnect'}),
    trade(4, {sequence: '20', quantity: 5}), snapshot(5, {sequence: '10', bids: []}),
    snapshot(6, {sequence: '21', occurredAt: timestamp(2), bids: []}), snapshot(7, {sequence: '20', bids: []}),
    snapshot(8, {sequence: '21', bids: []}), trade(9, {sequence: '22', bookSequence: '21', quantity: 2})]);
  assert.equal(r.fills.length, 1); assert.equal(r.fills[0].eventId, 'trade-9');
});
test('repeated gaps raise recovery time floor without lowering observed sequence', () => {
  const r = run([snapshot(0, {bids: []}), order(), event('gap', 3, {instrument: 'synthetic:YES', reason: 'first gap'}),
    snapshot(4, {sequence: '20', occurredAt: timestamp(2), bids: []}),
    event('gap', 6, {instrument: 'synthetic:YES', reason: 'second gap'}),
    snapshot(7, {sequence: '20', occurredAt: timestamp(5), bids: []}), order(8),
    snapshot(9, {sequence: '19', bids: []}), order(10),
    snapshot(11, {sequence: '20', bids: []}), trade(12, {sequence: '21', bookSequence: '20', quantity: 2})]);
  assert.equal(r.orders[1].reason, 'book_not_fresh_at_submission'); assert.equal(r.orders[2].reason, 'book_not_fresh_at_submission');
  assert.equal(r.fills.length, 1); assert.equal(r.fills[0].eventId, 'trade-12');
});
test('replenishment is conservatively moved ahead, cancellation is never inferred', () => {
  const r = run([snapshot(), order(), trade(2, {quantity: 5}), snapshot(3, {sequence: '3', bids: [level('0.400', 20)]}), trade(4, {bookSequence: '3', quantity: 19})]);
  assert.equal(r.fills.length, 0); assert.equal(r.orders[0].queueAhead, '1');
});
test('identical duplicate events are idempotent; conflicting event IDs halt', () => {
  const s = snapshot(); const t = trade(2, {quantity: 12}); const r = run([s, order(), t, t]);
  assert.equal(r.fills.length, 1); assert.equal(r.metrics.duplicates, 1);
  assert.throws(() => run([s, {...s, bids: []}]), /Conflicting duplicate/);
});
test('older sequence ignored; duplicate snapshot version does not refresh its age', () => {
  const r = run([snapshot(), snapshot(2, {eventId: 'older', sequence: '0'}),
    snapshot(20, {occurredAt: timestamp(0)}), order(21)], config({maxDataAgeMs: 10}));
  assert.equal(r.metrics.staleVersions, 2); assert.equal(r.orders[0].reason, 'book_not_fresh_at_submission');
  assert.throws(() => run([snapshot(), snapshot(1, {occurredAt: timestamp(0), bids: []})]), /Conflicting snapshot/);
});
test('backward availability chronology and occurrence timestamps rejected', () => {
  assert.throws(() => run([snapshot(2), order(1)]), /chronological/);
  assert.throws(() => run([snapshot(1, {occurredAt: timestamp(2)})]), /after receipt/);
  const r = run([snapshot(), order(), trade(10, {quantity: 1}), trade(11, {occurredAt: timestamp(5), quantity: 100})]);
  assert.equal(r.metrics.gaps, 1); assert.equal(r.fills.length, 0);
});
test('stale and same/older trade versions cannot be reused', () => {
  const r = run([snapshot(), order(), trade(2, {quantity: 12}), trade(3, {sequence: '2', occurredAt: timestamp(2), quantity: 12}), trade(30)], config({maxDataAgeMs: 10}));
  assert.equal(r.fills.length, 1); assert.equal(r.metrics.staleVersions, 1); assert.equal(r.metrics.staleEvidence, 1);
});
test('same trade sequence with newer timestamp cannot consume more evidence', () => {
  assert.throws(() => run([snapshot(0, {bids: []}), order(), trade(2, {quantity: 2}), trade(3, {sequence: '2', quantity: 2})]), /Conflicting trade evidence/);
});
test('known duplicate evidence after cancel and own-aggressor late prints are not ambiguous new fills', () => {
  const r = run([snapshot(0, {bids: []}), order(), trade(2, {quantity: 2}), event('cancel', 3, {orderId: 'o-1'}),
    trade(60, {sequence: '2', occurredAt: timestamp(2), quantity: 2}),
    trade(61, {sequence: '3', occurredAt: timestamp(3), quantity: 2, externalAggressor: false})]);
  assert.equal(r.fills.length, 1); assert.equal(r.orders[0].status, 'cancelled');
});
test('higher snapshot sequence with backwards occurrence requires resync', () => {
  const r = run([snapshot(10), snapshot(11, {sequence: '2', occurredAt: timestamp(5)}), order(12)]);
  assert.equal(r.metrics.gaps, 1); assert.equal(r.orders[0].reason, 'book_not_fresh_at_submission');
});
test('reservations prevent overspend across active and pending buys', () => {
  const r = run([snapshot(), order(1, {quantity: 5}), order(2, {quantity: 1})], config({initialCash: '2', orderLatencyMs: 100}));
  assert.equal(r.orders[1].reason, 'insufficient_unreserved_cash'); assert.equal(r.reservedCash, '2.000000');
});
test('per-share conservative fee reserve remains solvent through partial fills', () => {
  const cfg = config({initialCash: '0.015003', costs: {feeBps: 1, feePerShare: '0', assumption: 'unverified_scenario'}});
  const r = run([snapshot(0, {bids: []}), order(1, {quantity: 3, limitPrice: '0.005'}),
    trade(2, {price: '0.005', quantity: 1}), trade(3, {price: '0.005', quantity: 1}), trade(4, {price: '0.005', quantity: 1})], cfg);
  assert.equal(r.cash, '0.000000'); assert.equal(r.totalFees, '0.000003'); assert.equal(r.remainingCostBasis, '0.015003');
});
test('position limit includes pending buy quantity', () => {
  const r = run([snapshot(), order(), order(2)], config({maxPositionShares: 8}));
  assert.equal(r.orders[1].reason, 'inventory_limit');
});
test('long-only sells reserve held inventory and reject shorting', () => {
  const r = run([snapshot(0, {bids: []}), order(), trade(2, {quantity: 5}), order(3, {action: 'sell', quantity: 4, limitPrice: '0.600'}),
    order(4, {action: 'sell', quantity: 2, limitPrice: '0.600'})]);
  assert.equal(r.orders[2].reason, 'insufficient_unreserved_inventory');
});
test('marketable passive orders are rejected instead of fabricating taker/post-only support', () => {
  const r = run([snapshot(), order(1, {limitPrice: '0.600'})]); assert.equal(r.orders[0].reason, 'marketable_order_unsupported');
});
test('FIFO cost basis conserves micros; fees affect realized and unrealized separately', () => {
  const cfg = config({costs: {feeBps: 1, feePerShare: '0', assumption: 'unverified_scenario'}});
  const r = run([snapshot(0, {bids: [], asks: []}), order(1, {quantity: 3, limitPrice: '0.005'}),
    trade(2, {quantity: 3, price: '0.005'}), order(3, {action: 'sell', quantity: 3, limitPrice: '0.010'}),
    trade(4, {aggressor: 'buy', price: '0.010', quantity: 1}), trade(5, {aggressor: 'buy', price: '0.010', quantity: 2})], cfg);
  assert.equal(r.remainingCostBasis, '0.000000'); assert.equal(r.realizedPnl, '0.014995');
  assert.equal(r.cash, '100.014995'); assert.equal(r.totalFees, '0.000005');
});
test('reference, last, tournament and depth valuation stay separate', () => {
  const r = run([snapshot(0, {bids: []}), order(), trade(2, {quantity: 5}), snapshot(3, {sequence: '3', bids: [level('0.450', 2), level('0.440', 3)]}),
    event('mark', 4, {instrument: 'synthetic:YES', source: 'reference', price: '0.5'}),
    event('mark', 5, {instrument: 'synthetic:YES', source: 'tournament', price: '0.99'}),
    event('mark', 6, {instrument: 'synthetic:YES', source: 'last', price: '0.9'})]);
  assert.equal(r.realizedPnl, '0.000000'); assert.equal(r.referenceUnrealizedPnl, '0.500000');
  assert.equal(r.referenceEquity, '100.500000'); assert.equal(r.estimatedExecutableEquity, '100.220000');
  assert.equal(r.positions[0].marks.tournament.value, '4.950000'); assert.equal(r.metrics.drawdownComplete, false);
});
test('stale marks/depth produce null full valuation rather than fabricated zero or best bid exit', () => {
  const r = run([snapshot(0, {bids: []}), order(), trade(2, {quantity: 5}),
    event('mark', 3, {instrument: 'synthetic:YES', source: 'reference', price: '0.5'}), event('clock', 20)], config({maxDataAgeMs: 10}));
  assert.equal(r.referenceEquity, null); assert.equal(r.estimatedExecutableEquity, null); assert.equal(r.positions[0].executable.unsoldQuantity, 5);
});
test('extreme fee scenario discloses liquidation funding shortfall instead of promising executable equity', () => {
  const cfg = config({initialCash: '0.115', costs: {feeBps: 0, feePerShare: '0.100', assumption: 'unverified_scenario'}});
  const r = run([snapshot(0, {bids: []}), order(1, {quantity: 1, limitPrice: '0.010'}), trade(2, {quantity: 1, price: '0.010'}),
    snapshot(3, {sequence: '3', bids: [level('0.005', 1)]})], cfg);
  assert.equal(r.depthComplete, true); assert.equal(r.liquidationFeeFundingShortfall, '0.090000');
  assert.equal(r.executableComplete, false); assert.equal(r.estimatedExecutableEquity, null);
});
test('same event list deterministically reproduces report and does not mutate source', () => {
  const events = [snapshot(), order(), trade(2, {quantity: 12})]; const copy = structuredClone(events);
  assert.deepEqual(run(events), run(events)); assert.deepEqual(events, copy);
});
test('schema rejects numeric money, unknown own depth, no evidence, bad times and invalid config', () => {
  for (const e of [snapshot(0, {bids: [{price: '0.4', quantity: 1}]}), trade(2, {evidence: 'touch'}),
    trade(2, {externalAggressor: undefined}), order(1, {limitPrice: 0.4}), order(1, {quantity: 1.2}),
    snapshot(0, {bids: [level('0.600', 2)]}), snapshot(0, {bids: [level('0.400', 1, 2)]}),
    snapshot(0, {receivedAt: '2026-02-30T00:00:00.000Z'}), snapshot(0, {sequence: 1})]) assert.throws(() => validateEvent(e));
  assert.throws(() => validateConfig(config({costs: {feeBps: 0, feePerShare: '0', assumption: 'verified'}})));
});
test('unknown cancel and reused order IDs halt instead of creating duplicate intents', () => {
  assert.throws(() => run([event('cancel', 1, {orderId: 'unknown'})]), /unknown order/);
  assert.throws(() => run([snapshot(), order(), order(2, {orderId: 'o-1'})]), /already used/);
});
test('no-trade baseline remains exactly constant with zero requests and fees', () => {
  const r = run([snapshot(), event('clock', 100)]);
  assert.equal(r.cash, '100.000000'); assert.equal(r.realizedPnl, '0.000000'); assert.equal(r.metrics.simulatedWriteRequests, 0);
  assert.equal(r.metrics.fillRatio, null); assert.deepEqual(r.noTradeBaseline, {cash: '100.000000', pnl: '0.000000'});
});
