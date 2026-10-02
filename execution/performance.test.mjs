import test from 'node:test';
import assert from 'node:assert/strict';
import {measureExecutionPerformance} from './performance.mjs';
import {normalizeBook} from '../live-paper/model.mjs';

const TID = 'bda92870-621e-47b0-bc3c-3602c5c26f55', NOW = Date.parse('2026-10-02T12:00:00.000Z');
const at = delta => new Date(NOW + delta).toISOString();
const fill = (changes = {}) => ({id: 1, orderId: 100, marketId: '369', exchangeId: '1058', action: 'buy', quantity: 16, price: '0.245000', filledAt: at(-5000), ...changes});
const holding = (quantity = 16, cost = '3.920000', lots = [{quantity, cost}]) => ({marketId: '369', quantity, cost, lots});
function book(changes = {}) {
  const raw = {contexts: [{type: 'tournament', tournament: {id: TID}, orderbook: {exchanges: [{exchangeId: '1058', option: 'YES',
    asOf: {sequence: 12, at: at(-1000)}, bids: [{price: 0.235, quantity: 100}], asks: [{price: 0.240, quantity: 100}]}]}}]};
  return {...normalizeBook(raw, '369', TID, at(-500), at(-1500), 20000, new Set(), 'authoritative_receipt_v1'), tournamentId: TID, ...changes};
}
const input = changes => ({tournamentId: TID, positions: {'1058': holding()}, fills: [fill()], books: [book()], now: NOW, ...changes});

test('first official fill marks actual spread loss exactly, separately from midpoint and realized P&L', () => {
  const data = input(), before = JSON.stringify(data), result = measureExecutionPerformance(data);
  assert.equal(result.realizedPnl, '0.000000'); assert.equal(result.ownedCost, '3.920000');
  assert.equal(result.observedLiquidationValue, '3.760000'); assert.equal(result.observedLiquidationTotalPnl, '-0.160000');
  assert.equal(result.midpointValue, '3.800000'); assert.equal(result.midpointTotalPnl, '-0.120000');
  assert.equal(result.positions[0].midpointPrice, '0.237500'); assert.equal(result.addedFees, '0.000000');
  assert.equal(result.executionGuaranteed, false); assert.equal(result.coverage.unsoldQuantity, 0);
  assert.equal(result.positions[0].book.sourceAt, book().sourceAt);
  assert.equal(result.positions[0].lastOwnFillAt, '2026-10-02T11:59:55.000000000Z');
  assert.equal(result.asOf, at(0)); assert.equal(JSON.stringify(data), before);
  assert.ok(Object.isFrozen(result.positions[0].book)); assert.throws(() => { result.positions[0].quantity = 1; }, TypeError);
});

test('liquidation estimate walks full displayed bid depth instead of using best bid for all shares', () => {
  const result = measureExecutionPerformance(input({books: [book({bids: [{price: '0.240000', quantity: 4}, {price: '0.235000', quantity: 12}], asks: [{price: '0.245000', quantity: 100}]})]}));
  assert.equal(result.observedLiquidationValue, '3.780000'); assert.equal(result.observedLiquidationTotalPnl, '-0.140000');
  assert.equal(result.positions[0].availableBidQuantity, 16); assert.equal(result.positions[0].observedLiquidationQuantity, 16);
  assert.equal(result.coverage.liquidationComplete, true);
});

test('insufficient depth exposes partial observed value and unsold quantity without inventing a fill or full P&L', () => {
  const result = measureExecutionPerformance(input({books: [book({bids: [{price: '0.235000', quantity: 5}]})]}));
  assert.equal(result.observedLiquidationValue, null); assert.equal(result.observedLiquidationTotalPnl, null);
  assert.equal(result.observedLiquidationUnrealizedPnl, null); assert.equal(result.midpointTotalPnl, '-0.120000');
  assert.equal(result.positions[0].observedAvailableDepthValue, '1.175000'); assert.equal(result.positions[0].observedLiquidationQuantity, 5);
  assert.equal(result.positions[0].unsoldQuantity, 11); assert.equal(result.coverage.unsoldQuantity, 11);
  assert.equal(result.positions[0].reasonCode, 'INSUFFICIENT_BID_DEPTH'); assert.equal(result.coverage.midpointComplete, true);
});

test('missing, stale, future, slow, pre-fill and unscoped evidence leaves totals unknown and all shares unsold', () => {
  const cases = [[], [book({receivedAt: at(-20001), requestStartedAt: at(-21000)})], [book({receivedAt: at(1)})],
    [book({requestStartedAt: at(1)})], [book({requestStartedAt: at(-21000)})], [book({receivedAt: at(-6000), requestStartedAt: at(-7000)})],
    [book({tournamentId: undefined})], [book({tournamentId: 'foreign'})], [book({marketId: '377'})], [book({exchangeId: '1066'})],
    [book({freshnessPolicy: 'event_age_v1'})], [book({invalidated: true})], [book({valid: false})],
    [book({sourceAt: '2026-10-02T12:00:00.000000001Z', versionKey: '12:2026-10-02T12:00:00.000000001Z'})]];
  for (const books of cases) {
    const result = measureExecutionPerformance(input({books})); assert.equal(result.observedLiquidationTotalPnl, null);
    assert.equal(result.midpointTotalPnl, null); assert.equal(result.coverage.unsoldQuantity, 16); assert.equal(result.realizedPnl, '0.000000');
    assert.equal(result.positions[0].observedAvailableDepthValue, null);
  }
});

test('valid receipt exactly at the last fill or age bound remains usable; quiet older source clocks are disclosed', () => {
  const last = measureExecutionPerformance(input({books: [book({receivedAt: at(-5000), requestStartedAt: at(-5500), sourceAt: '2026-10-01T12:00:00.000000000Z', versionKey: '12:2026-10-01T12:00:00.000000000Z'})]}));
  assert.equal(last.observedLiquidationTotalPnl, '-0.160000'); assert.equal(last.positions[0].book.sourceEventAgeMs, 86400000);
  const boundary = measureExecutionPerformance(input({fills: [fill({filledAt: at(-21000)})], books: [book({receivedAt: at(-20000), requestStartedAt: at(-21000), sourceAt: '2026-10-02T11:59:40.000000000Z', versionKey: '12:2026-10-02T11:59:40.000000000Z'})]}));
  assert.equal(boundary.observedLiquidationTotalPnl, '-0.160000');
});

test('invalid integer quantity, tick, ordering, duplicate versions and crossed depth never produce marks', () => {
  const invalid = [book({bids: []}), book({asks: []}), book({bids: [{price: '0.236000', quantity: 100}]}),
    book({bids: [{price: '0.235000', quantity: 1.5}]}), book({bids: [{price: '0.235000', quantity: 0}]}),
    book({bids: [{price: '0.235000', quantity: 100}, {price: '0.240000', quantity: 100}]}),
    book({bids: [{price: '0.235000', quantity: 100}, {price: '0.235000', quantity: 100}]}),
    book({bids: [{price: '0.235000', quantity: Number.MAX_SAFE_INTEGER}, {price: '0.230000', quantity: 1}]}),
    book({asks: [{price: '0.235000', quantity: 100}]}), book({sequence: '012'}), book({versionKey: 'wrong'}), book({instrument: '1058:NO'})];
  for (const b of invalid) assert.equal(measureExecutionPerformance(input({books: [b]})).observedLiquidationTotalPnl, null);
  const duplicate = measureExecutionPerformance(input({books: [book(), book()]}));
  assert.equal(duplicate.positions[0].reasonCode, 'DUPLICATE_BOOK'); assert.equal(duplicate.observedLiquidationTotalPnl, null);
});

test('partial sells and multiple entry prices conserve FIFO basis and signed realized P&L exactly', () => {
  const fills = [fill({quantity: 10, price: '0.200000', filledAt: at(-9000)}),
    fill({id: 2, orderId: 101, quantity: 10, price: '0.300000', filledAt: at(-8000)}),
    fill({id: 3, orderId: 102, action: 'sell', quantity: 12, price: '0.250000', filledAt: at(-7000)})];
  const result = measureExecutionPerformance(input({fills, positions: {'1058': holding(8, '2.400000')}}));
  assert.equal(result.realizedPnl, '0.400000'); assert.equal(result.ownedCost, '2.400000');
  assert.equal(result.observedLiquidationValue, '1.880000'); assert.equal(result.observedLiquidationUnrealizedPnl, '-0.520000');
  assert.equal(result.observedLiquidationTotalPnl, '-0.120000'); assert.equal(result.midpointTotalPnl, '-0.100000');
  const loss = measureExecutionPerformance(input({fills: [fill(), fill({id: 2, orderId: 101, action: 'sell', price: '0.235000', filledAt: at(-1000)})], positions: {}, books: []}));
  assert.equal(loss.realizedPnl, '-0.160000'); assert.equal(loss.observedLiquidationValue, '0.000000');
  assert.equal(loss.observedLiquidationTotalPnl, '-0.160000'); assert.equal(loss.coverage.liquidationComplete, true);
});

test('one uncovered position nulls portfolio marks; unrelated external books do not create positions', () => {
  const second = fill({id: 2, orderId: 101, marketId: '377', exchangeId: '1066', quantity: 4, price: '0.500000'});
  const result = measureExecutionPerformance(input({fills: [fill(), second], positions: {'1058': holding(), '1066': {marketId: '377', quantity: 4, cost: '2.000000', lots: [{quantity: 4, cost: '2.000000'}]}}}));
  assert.equal(result.observedLiquidationTotalPnl, null); assert.equal(result.midpointTotalPnl, null);
  assert.equal(result.coverage.ownedQuantity, 20); assert.equal(result.coverage.observedLiquidationQuantity, 16); assert.equal(result.coverage.unsoldQuantity, 4);
  const unrelated = measureExecutionPerformance(input({books: [book(), book({marketId: '999', exchangeId: '999', instrument: '999:YES'})]}));
  assert.equal(unrelated.positions.length, 1); assert.equal(unrelated.observedLiquidationTotalPnl, '-0.160000');
});

test('ownership, cashflow, duplicate fills, scope and future-fill inputs must reconcile before any valuation', () => {
  for (const changed of [input({fills: []}), input({positions: {}}), input({positions: {'1058': holding(16, '4.000000')}}),
    input({fills: [fill(), fill()]}), input({fills: [fill({action: 'sell'})]}), input({fills: [fill({filledAt: at(1)})]}),
    input({fills: [fill({side: 'no'})]}), input({fills: [fill({tournamentId: 'foreign'})]}),
    input({fills: [fill({quantity: 16.5})]}), input({positions: {'1058': {...holding(), marketId: '377'}}})]) assert.throws(() => measureExecutionPerformance(changed));
  assert.throws(() => measureExecutionPerformance(input({maxBookAgeMs: 20001})), /FRESHNESS/);
  assert.throws(() => measureExecutionPerformance(input({now: NaN})), /CLOCK/);
});
