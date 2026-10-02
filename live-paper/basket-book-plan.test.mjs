import test from 'node:test';
import assert from 'node:assert/strict';
import {createSchedulerState, planBookReads, recordSchedulerAttempts} from './book-scheduler.mjs';
import {planBasketBooks} from './basket-book-plan.mjs';

const now = Date.parse('2026-10-02T11:00:00.000Z'), stamp = offset => new Date(now + offset).toISOString();
const scope = {universe: {tournamentId: 'fixture', exchanges: Array.from({length: 12}, (_, n) => ({marketId: String(n + 1), exchangeId: String(n + 101), option: 'YES'}))}, coreMarketIds: ['1', '2', '3', '4']};
const ids = plan => plan.reads.map(read => read.marketId);
const candidate = (a, b, priceGapMicros = 10000, receivedAt = stamp(-1000)) => ({tournamentId: 'fixture', marketIds: [a, b], exchangeIds: [String(Number(a) + 100), String(Number(b) + 100)], receivedAt, priceGapMicros});
const base = (inputs = {}, state = createSchedulerState(scope)) => planBookReads(state, scope, inputs);
const overlay = (plan, candidates, extra = {}) => planBasketBooks(plan, scope, {candidates, cycle: 0, now, ...extra});

test('same-response fresh pair receives both depth reads within four slots without creating entry eligibility', () => {
  const plan = base(), pair = candidate('7', '8'), before = JSON.stringify({plan, pair}), result = overlay(plan, [pair]);
  assert.deepEqual(ids(result), ['7', '8', '1', '2']); assert.equal(result.reads.length, 4);
  assert.equal(result.basketMeasurement.reason, 'paired_depth_measurement'); assert.equal(result.basketMeasurement.receivedAt, pair.receivedAt);
  assert.deepEqual(result.basketMeasurement.marketIds, ['7', '8']); assert.deepEqual(result.entryMarketIds, []);
  assert.deepEqual(result.state, plan.state); assert.deepEqual(result.managedMarketIds, plan.managedMarketIds);
  assert.equal(JSON.stringify({plan, pair}), before); assert.notEqual(result.reads, plan.reads);
  assert.ok(result.reads.slice(0, 2).every(read => read.path === 'supplementalDepth' && read.reason === 'basket_measurement'));
});

test('official risk, paper holdings and selected candidates keep their reads and order before measurements', () => {
  const plan = base({priorityManagedMarketIds: ['9'], heldMarketIds: ['1'], candidateMarketIds: ['8']}), protectedReads = plan.reads.slice(0, 3);
  const result = overlay(plan, [candidate('8', '7')]);
  assert.deepEqual(result.reads.slice(0, 3), protectedReads); assert.deepEqual(ids(result), ['9', '1', '8', '7']);
  assert.deepEqual(result.entryMarketIds, ['8']); assert.equal(result.reads[2].reason, 'eligible_candidate');
  assert.equal(result.reads[1].path, 'sourceSnapshots'); assert.equal(result.reads[3].path, 'supplementalDepth');
});

test('insufficient spare capacity preserves the original plan; a lower-ranked fitting pair may use one free slot', () => {
  const plan = base({priorityManagedMarketIds: ['9'], heldMarketIds: ['1'], candidateMarketIds: ['8']});
  let result = overlay(plan, [candidate('7', '10', 50000)]);
  assert.deepEqual(result.reads, plan.reads); assert.equal(result.basketMeasurement.reason, 'protected_capacity');
  result = overlay(plan, [candidate('7', '10', 50000), candidate('8', '7', 10000)]);
  assert.deepEqual(result.basketMeasurement.marketIds, ['7', '8']); assert.deepEqual(ids(result), ['9', '1', '8', '7']);
  const full = base({priorityManagedMarketIds: ['9', '10', '11'], heldMarketIds: ['1']});
  assert.deepEqual(overlay(full, [candidate('7', '8')]).reads, full.reads);
});

test('an already-present leg is read once and existing core/supplemental provenance is retained', () => {
  const plan = base(), result = overlay(plan, [candidate('2', '7')]);
  assert.deepEqual(ids(result), ['2', '7', '1', '5']); assert.equal(new Set(ids(result)).size, 4);
  assert.deepEqual(result.reads[0], plan.reads.find(read => read.marketId === '2'));
  assert.equal(result.reads[0].path, 'sourceSnapshots');
  const core = overlay(plan, [candidate('3', '7')]); assert.equal(core.reads[0].path, 'sourceSnapshots');
  const both = overlay(plan, [candidate('1', '2')]); assert.deepEqual(both.reads, plan.reads);
});

test('default cadence permits only zero-based cycles divisible by four and does not advance attempt state', () => {
  const plan = base(), pair = candidate('7', '8');
  for (let cycle = 0; cycle < 9; cycle++) {
    const result = overlay(plan, [pair], {cycle}); assert.deepEqual(result.state, plan.state);
    if (cycle % 4) { assert.deepEqual(result.reads, plan.reads); assert.equal(result.basketMeasurement.reason, 'cadence_reserved'); }
    else assert.deepEqual(result.basketMeasurement.marketIds, ['7', '8']);
  }
  assert.equal(plan.state.bookAttempts, 0);
});

test('highest positive adjusted gap wins and equal gaps rotate using persisted actual book attempts', () => {
  const pairs = [candidate('7', '8'), candidate('9', '10')], original = base();
  assert.deepEqual(overlay(original, [pairs[0], candidate('9', '10', 20000)]).basketMeasurement.marketIds, ['9', '10']);
  const first = overlay(original, [...pairs].reverse()); assert.deepEqual(first.basketMeasurement.marketIds, ['7', '8']);
  const next = JSON.parse(JSON.stringify(recordSchedulerAttempts(original.state, scope, {bookMarketIds: ids(first)})));
  const second = overlay(base({}, next), pairs, {cycle: 4}); assert.deepEqual(second.basketMeasurement.marketIds, ['9', '10']);
  const reversed = overlay(original, [candidate('8', '7'), pairs[0]]); assert.deepEqual(reversed.basketMeasurement.marketIds, ['7', '8']);
  assert.equal(new Set(ids(reversed)).size, 4);
});

test('missing, stale, future, malformed and nonpositive hints cannot replace the existing books', () => {
  const plan = base(), invalid = [null, {}, [null], [candidate('7', '8', 0)], [candidate('7', '8', -1)],
    [candidate('7', '8', 1.5)], [candidate('7', '8', Infinity)], [candidate('7', '8', 1000001)],
    [candidate('7', '8', 10000, stamp(-120001))], [candidate('7', '8', 10000, stamp(1))],
    [candidate('7', '8', 10000, null)], [candidate('7', '8', 10000, 'invalid')]];
  for (const hints of invalid) assert.deepEqual(overlay(plan, hints).reads, plan.reads);
  assert.equal(overlay(plan, [candidate('7', '8', 10000, stamp(-120000))]).basketMeasurement.reason, 'paired_depth_measurement');
  assert.deepEqual(overlay(plan, [candidate('7', '8')], {excludedMarketIds: ['8']}).reads, plan.reads);
});

test('foreign scope, duplicate legs and inconsistent base plans fail closed', () => {
  const plan = base();
  for (const pair of [{...candidate('7', '8'), tournamentId: 'foreign'}, candidate('7', '7'), candidate('7', '99'),
    {...candidate('7', '8'), exchangeIds: ['107', '107']}, {...candidate('7', '8'), exchangeIds: ['107', '999']},
    {...candidate('7', '8'), marketIds: [7, '8']}]) assert.throws(() => overlay(plan, [pair]), /basket/);
  assert.throws(() => overlay(plan, [], {excludedMarketIds: ['999']}), /exclusions/);
  assert.throws(() => overlay({...plan, reads: [...plan.reads, plan.reads[0]]}, []), /budget/);
  const wrongPath = structuredClone(plan); wrongPath.reads[0].path = 'supplementalDepth'; assert.throws(() => overlay(wrongPath, []), /scope/);
  assert.throws(() => overlay(plan, [], {now: NaN}), /clock/); assert.throws(() => overlay(plan, [], {cycle: -1}), /cadence/);
  assert.throws(() => planBasketBooks(plan, scope, {cycle: 0, now}, {maxAgeMs: 120001}), /clock/);
});

test('unknown future priority reasons are preserved instead of silently becoming replaceable', () => {
  const plan = base(); plan.reads = plan.reads.map(read => ({...read, reason: 'new_risk_reason'}));
  const result = overlay(plan, [candidate('7', '8')]); assert.deepEqual(result.reads, plan.reads); assert.equal(result.basketMeasurement.reason, 'protected_capacity');
});
