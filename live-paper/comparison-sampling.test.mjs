import test from 'node:test';
import assert from 'node:assert/strict';
import {createSchedulerState, recordSchedulerAttempts} from './book-scheduler.mjs';
import {planComparisonSampling, recordComparisonAttempt, COMPARISON_SAMPLING} from './comparison-sampling.mjs';
const panel = ['377', '354', '277', '305'], tid = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const ids = [...panel, '369', '381', '382'];
const scope = {universe: {tournamentId: tid, exchanges: ids.map((marketId, i) => ({marketId, exchangeId: String(1000 + i), option: 'YES'}))}, coreMarketIds: panel};
const zero = Date.parse('2026-10-02T14:00:00.000Z'), stamp = ms => new Date(zero + ms).toISOString();
const sessionId = '11111111-1111-4111-8111-111111111111', restarted = '22222222-2222-4222-8222-222222222222';
function read(marketId, reason = 'comparison') { const s = scope.universe.exchanges.find(x => x.marketId === marketId); return {...s, path: panel.includes(marketId) ? 'sourceSnapshots' : 'supplementalDepth', reason}; }
function base(state = createSchedulerState(scope), comparison = panel.slice(0, 2)) {
  return {state, reads: [...comparison.map(id => read(id)), read('381', 'discovery'), read('382', 'discovery')], entryMarketIds: [], managedMarketIds: []};
}
const plan = (b, ms = 0, session = sessionId) => planComparisonSampling(b, scope, {now: zero + ms, sessionId: session});
function reserve(p, marketId, ms, state = p.state) { return recordComparisonAttempt(state, scope, {marketId, at: stamp(ms), planId: p.measurement.planId}); }
function reservePlan(p, ms = 1) { let state = p.state; for (const [i, id] of p.measurement.reservationMarketIds.entries()) state = reserve(p, id, ms + i, state); return state; }

test('the two existing comparison slots repeat a pair once and rotate without growing the read budget', () => {
  const b = base(), original = structuredClone(b), first = plan(b);
  assert.deepEqual(b, original); assert.deepEqual(first.reads, b.reads); assert.equal(first.measurement.requestsAdded, 0);
  assert.equal(first.state.comparisonSampling.totalAttempts, 0);
  const afterFirst = reservePlan(first), second = plan(base(afterFirst, panel.slice(2)), 35000);
  assert.deepEqual(second.reads.slice(0, 2).map(x => x.marketId), panel.slice(0, 2));
  assert.deepEqual(second.measurement.remainingAttempts, {'377': 1, '354': 1});
  const done = reservePlan(second, 35001); assert.equal(done.comparisonSampling.lastPhase.reason, 'completed');
  assert.throws(() => reserve(second, second.measurement.reservationMarketIds[0], 35003, done), /Duplicate/);
  const third = plan(base(done), 70000); assert.deepEqual(third.measurement.marketIds, panel.slice(2));
  assert.equal(third.reads.length, 4); assert.deepEqual(third.entryMarketIds, b.entryMarketIds);
});

test('one immutable plan reserves each member at most once, and foreign plan IDs are rejected', () => {
  const p = plan(base()), state = reserve(p, '377', 1);
  assert.throws(() => reserve(p, '377', 2, state), /Duplicate/);
  assert.throws(() => recordComparisonAttempt(state, scope, {marketId: '354', at: stamp(2), planId: 'f'.repeat(64)}), /plan mismatch/);
  assert.deepEqual(recordComparisonAttempt(state, scope, {marketId: '381', at: stamp(2), planId: p.measurement.planId}), state);
});

test('partial cycles finish all first-pass members before permitting any second-pass reservation', () => {
  const p = plan(base()), onlyFirst = reserve(p, '377', 1);
  const next = plan(base(onlyFirst), 35000);
  assert.deepEqual(next.measurement.reservationMarketIds, ['354']);
  const firstPass = reserve(next, '354', 35001), secondPass = plan(base(firstPass), 70000);
  assert.deepEqual(new Set(secondPass.measurement.reservationMarketIds), new Set(['377', '354']));
  assert.deepEqual(secondPass.measurement.remainingAttempts, {'377': 1, '354': 1});
});

test('planning and skipped cycles consume none; failed or STOP-interrupted pre-request reservations stay consumed', () => {
  const first = plan(base()), pending = plan(base(first.state, panel.slice(2)), 30000);
  assert.equal(pending.state.comparisonSampling.totalAttempts, 0);
  const failedRead = reserve(pending, '377', 30001), serialized = JSON.parse(JSON.stringify(failedRead));
  const resumed = plan(base(serialized), 35000);
  assert.equal(resumed.state.comparisonSampling.totalAttempts, 1); assert.deepEqual(resumed.measurement.reservationMarketIds, ['354']);
});

test('protected risk, candidates, basket and unknown reads are unchanged, including their ordering', () => {
  const p = plan(base()), state = reservePlan(p);
  for (const reason of ['priority_held_or_pending', 'held_or_pending', 'eligible_candidate', 'basket_measurement', 'new_unknown_reason']) {
    const protectedRead = {...read('377', reason), custom: {retain: true}};
    const b = {state, reads: [protectedRead, read('277'), read('381', 'basket_measurement'), read('382', 'basket_measurement')], entryMarketIds: ['377']};
    const out = plan(b, 35000); assert.deepEqual(out.reads[0], protectedRead); assert.deepEqual(out.reads.slice(2), b.reads.slice(2));
    assert.equal(out.reads[1].marketId, '354'); assert.equal(new Set(out.reads.map(x => x.marketId)).size, 4);
    assert.deepEqual(out.measurement.reservationMarketIds, ['377', '354']); assert.deepEqual(out.entryMarketIds, ['377']);
  }
});

test('an overlay with no comparison slots cannot overwrite basket reads or claim absent members', () => {
  const first = plan(base()), carried = reservePlan(first);
  const b = {state: carried, reads: [read('369', 'priority_held_or_pending'), read('381', 'basket_measurement'), read('382', 'basket_measurement')], entryMarketIds: []};
  const out = plan(b, 30000); assert.deepEqual(out.reads, b.reads); assert.deepEqual(out.measurement.reservationMarketIds, []);
  assert.equal(out.state.comparisonSampling.totalAttempts, 2);
  const empty = plan({...b, state: createSchedulerState(scope)}, 30000); assert.equal(empty.measurement.reason, 'insufficient_comparison_slots');
});

test('a basket leg reused from a comparison read remains protected even when its reason was preserved', () => {
  const first = plan(base()), carried = reservePlan(first);
  const b = {state: carried, reads: [read('277'), read('381', 'basket_measurement'), read('305'), read('382', 'discovery')],
    entryMarketIds: [], basketMeasurement: {reason: 'paired_depth_measurement', marketIds: ['277', '381'], exchangeIds: ['1002', '1005']}};
  const out = plan(b, 35000);
  assert.deepEqual(out.reads[0], b.reads[0]); assert.deepEqual(out.reads[1], b.reads[1]);
  assert.deepEqual(out.basketMeasurement, b.basketMeasurement); assert.equal(out.reads[2].marketId, '377');
  assert.deepEqual(out.measurement.reservationMarketIds, ['377']); assert.equal(out.reads.length, 4);
});

test('expired carry aborts rather than retrying forever and rotates fairly at the exact deadline', () => {
  const first = plan(base()), partial = reserve(first, '377', 1);
  const out = plan(base(partial), COMPARISON_SAMPLING.carryMs);
  assert.equal(out.measurement.expiredCarry, true); assert.equal(out.state.comparisonSampling.lastPhase.reason, 'expired');
  assert.deepEqual(out.measurement.marketIds, panel.slice(2)); assert.equal(out.state.comparisonSampling.totalAttempts, 1);
  const expiredReservation = reserve(first, '354', COMPARISON_SAMPLING.carryMs, partial);
  assert.equal(expiredReservation.comparisonSampling.lastPhase.reason, 'expired'); assert.equal(expiredReservation.comparisonSampling.totalAttempts, 1);
  assert.throws(() => reserve(first, '354', COMPARISON_SAMPLING.carryMs + 1, expiredReservation), /Duplicate/);
});

test('new collector session abandons carry and preserves fair cursor; same-session JSON resume keeps it', () => {
  const first = plan(base()), state = reserve(first, '377', 1);
  const same = plan(base(JSON.parse(JSON.stringify(state))), 30000); assert.equal(same.measurement.abandonedOnRestart, false); assert.deepEqual(same.measurement.marketIds, panel.slice(0, 2));
  const next = plan(base(JSON.parse(JSON.stringify(state))), 30000, restarted);
  assert.equal(next.measurement.abandonedOnRestart, true); assert.equal(next.state.comparisonSampling.lastPhase.reason, 'collector_restart');
  assert.deepEqual(next.measurement.marketIds, panel.slice(2)); assert.equal(next.state.comparisonSampling.totalAttempts, 1);
});

test('book-attempt accounting preserves reserved phase state through persistence and does not consume it twice', () => {
  const p = plan(base()); let state = reserve(p, '377', 1);
  state = recordSchedulerAttempts(state, scope, {bookMarketIds: ['377'], attemptedAt: stamp(1)});
  assert.equal(state.bookAttempts, 1); assert.equal(state.comparisonSampling.totalAttempts, 1);
  state = reserve(p, '354', 2, JSON.parse(JSON.stringify(state)));
  assert.equal(state.comparisonSampling.totalAttempts, 2); assert.equal(state.bookAttempts, 1);
});

test('tampered counters, reservation identity, scope and clocks are rejected', () => {
  const p = plan(base());
  for (const mutate of [x => x.comparisonSampling.remainingAttempts['377'] = 0,
    x => x.comparisonSampling.lastPlan.marketIds = ['305'], x => x.comparisonSampling.scopeHash = 'x',
    x => x.comparisonSampling.selectedMarketIds = ['369', '354']]) {
    const s = structuredClone(p.state); mutate(s); assert.throws(() => plan(base(s), 30000));
  }
  const state = reserve(p, '377', 10); assert.throws(() => plan(base(state), 9), /rollback/);
  assert.throws(() => planComparisonSampling(base(), {...scope, coreMarketIds: ['377', '354']}, {now: zero, sessionId}));
});
