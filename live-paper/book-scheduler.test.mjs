import test from 'node:test';
import assert from 'node:assert/strict';
import {createSchedulerState, validateSchedulerState, planBookReads, planHistoryReads, recordSchedulerAttempts} from './book-scheduler.mjs';
import {CompetitionController} from '../execution/controller.mjs';

const scope = (count = 12) => ({universe: {tournamentId: 'fixture-tournament', exchanges: Array.from({length: count}, (_, index) => ({marketId: String(index + 1), exchangeId: String(index + 101), option: 'YES'}))}, coreMarketIds: ['1', '2', '3', '4']});
const ids = plan => plan.reads.map(read => read.marketId);

// Exercise the actual controller selection method with read-only metadata mocks.
async function selectControllerBooks(plan, s, ownedMarketIds = []) {
  const metadataReads = [], controller = {
    bootstrapped: true, state: {owned: Object.fromEntries(ownedMarketIds.map(id => [id, {marketId: id}]))}, universe: s.universe, markets: {},
    assertLedgers() {}, canOperate() { return true; }, setAccount() {}, active() {}, async cashRefresh() {}, publish() {},
    transport: {async market(id) { metadataReads.push(id); return {receivedAt: new Date().toISOString(), data: {title: 'Fixture', status: 'open', contexts: [{status: 'open', settledWith: null}]}}; }},
  };
  await CompetitionController.prototype.beforeBooks.call(controller, {positions: [], orders: [], ordersAt: new Date().toISOString(), candidateMarketIds: plan.entryMarketIds});
  return {selected: controller.selected, metadataReads};
}

test('one frozen book plan supplies controller candidates when the raw first candidate is deferred', async () => {
  const s = scope(), raw = {heldMarketIds: ['1', '2'], candidateMarketIds: ['5', '6']};
  const state = recordSchedulerAttempts(createSchedulerState(s), s, {bookMarketIds: ['5']});
  const planned = planBookReads(state, s, raw);
  assert.deepEqual(planned.entryMarketIds, ['6']);
  assert.ok(!ids(planned).includes(raw.candidateMarketIds[0]), 'Original raw-first selection had no scheduled book');
  const controller = await selectControllerBooks(planned, s);
  assert.deepEqual(controller.selected, ['6']);
  assert.deepEqual(controller.metadataReads, ['6']);
  assert.ok(controller.selected.every(id => ids(planned).includes(id)));
  raw.candidateMarketIds.reverse();
  assert.deepEqual(controller.selected, planned.entryMarketIds, 'Later shadow input changes cannot change the retained cycle plan');
});

test('actual obligations receive books before any number of shadow holdings and paper slots remain fair', async () => {
  const s = scope(), official = ['9', '10', '11'], shadow = ['1', '2', '3', '4', '5', '6', '7', '8'];
  const inputs = {priorityManagedMarketIds: official, heldMarketIds: [...shadow, ...official], candidateMarketIds: ['12']};
  let state = recordSchedulerAttempts(createSchedulerState(s), s, {bookMarketIds: official});
  const seenShadow = new Set();
  for (let cycle = 0; cycle < shadow.length; cycle++) {
    const planned = planBookReads(state, s, inputs);
    assert.equal(planned.reads.length, 4);
    assert.deepEqual(new Set(ids(planned).slice(0, 3)), new Set(official));
    assert.equal(planned.reads[3].reason, 'held_or_pending');
    assert.deepEqual(planned.entryMarketIds, []);
    const controller = await selectControllerBooks(planned, s, official);
    assert.ok(controller.selected.every(id => ids(planned).includes(id)));
    seenShadow.add(planned.reads[3].marketId);
    state = JSON.parse(JSON.stringify(recordSchedulerAttempts(state, s, {bookMarketIds: ids(planned)})));
  }
  assert.deepEqual(seenShadow, new Set(shadow));
});

test('priority obligations preserve partial-attempt fairness, capacity and history membership', () => {
  const s = scope(), inputs = {priorityManagedMarketIds: ['9', '10', '11'], heldMarketIds: ['1', '2', '3', '4', '5', '6', '7', '8', '9'], candidateMarketIds: ['12']};
  const state = recordSchedulerAttempts(createSchedulerState(s), s, {bookMarketIds: ['9']});
  const planned = planBookReads(JSON.parse(JSON.stringify(state)), s, inputs);
  assert.deepEqual(ids(planned).slice(0, 3), ['10', '11', '9']);
  assert.equal(new Set(ids(planned)).size, 4);
  assert.equal(planned.availableCapacity, 0);
  assert.deepEqual(planHistoryReads(state, s, inputs).activeMarketIds.slice(0, 3), ['9', '10', '11']);
  assert.throws(() => planBookReads(state, s, {priorityManagedMarketIds: ['999']}), /foreign/);
});

test('held core and supplemental positions precede candidates and preserve replay paths', () => {
  const s = scope(), state = createSchedulerState(s);
  const plan = planBookReads(state, s, {heldMarketIds: ['1'], pendingMarketIds: ['5'], candidateMarketIds: ['6', '7']});
  assert.deepEqual(ids(plan).slice(0, 3), ['1', '5', '6']);
  assert.equal(plan.reads[0].path, 'sourceSnapshots');
  assert.equal(plan.reads[1].path, 'supplementalDepth');
  assert.deepEqual(plan.entryMarketIds, ['6']);
  assert.deepEqual(plan.deferredCandidateMarketIds, ['7']);
  assert.equal(plan.reads.length, 4);
  assert.deepEqual(plan.state, state);
});

test('planning is immutable; only attempts move persisted fairness through restarts', () => {
  const s = scope(), original = createSchedulerState(s), inputs = {heldMarketIds: ['1', '5'], candidateMarketIds: ['6', '7', '8']};
  const planned = planBookReads(original, s, inputs);
  assert.deepEqual(planBookReads(original, s, inputs), planned);
  const attempted = recordSchedulerAttempts(original, s, {bookMarketIds: [planned.reads[0].marketId]});
  assert.equal(original.bookAttempts, 0);
  assert.equal(attempted.bookAttempts, 1);
  assert.equal(attempted.bookLastAttempt['5'], undefined);
  const restored = JSON.parse(JSON.stringify(attempted));
  assert.deepEqual(planBookReads(restored, s, inputs), planBookReads(attempted, s, inputs));
  assert.equal(planBookReads(restored, s, inputs).reads[0].marketId, '5');
});

test('equally eligible signals rotate fairly through available managed capacity', () => {
  const s = scope(), inputs = {heldMarketIds: ['1', '2'], candidateMarketIds: ['5', '6', '7', '8']};
  let state = createSchedulerState(s);
  const selected = [];
  for (let cycle = 0; cycle < 4; cycle++) {
    const plan = planBookReads(state, s, inputs);
    assert.equal(plan.entryMarketIds.length, 1);
    selected.push(...plan.entryMarketIds);
    state = JSON.parse(JSON.stringify(recordSchedulerAttempts(state, s, {bookMarketIds: ids(plan)})));
  }
  assert.deepEqual(new Set(selected), new Set(inputs.candidateMarketIds));
});

test('default comparison and discovery rotation reaches the full scope despite repeated restarts', () => {
  const s = scope(20);
  let state = createSchedulerState(s);
  const seen = new Set();
  for (let cycle = 0; cycle < 8; cycle++) {
    const plan = planBookReads(state, s);
    assert.equal(plan.reads.filter(read => read.path === 'sourceSnapshots').length, 2);
    ids(plan).forEach(value => seen.add(value));
    state = JSON.parse(JSON.stringify(recordSchedulerAttempts(state, s, {bookMarketIds: ids(plan)})));
  }
  assert.equal(seen.size, 20);
});

test('union of held/pending prevents duplicate reads and caps new entry capacity', () => {
  const s = scope(), state = createSchedulerState(s);
  const plan = planBookReads(state, s, {heldMarketIds: ['1', '2'], pendingMarketIds: ['2', '5'], candidateMarketIds: ['1', '2', '5', '6', '7']});
  assert.equal(plan.availableCapacity, 0);
  assert.deepEqual(plan.entryMarketIds, []);
  assert.equal(new Set(ids(plan)).size, plan.reads.length);
  assert.deepEqual(ids(plan).slice(0, 3), ['1', '2', '5']);
  assert.throws(() => planBookReads(state, s, {}, {budget: 5}), /budget/);
  assert.throws(() => planBookReads(state, s, {}, {managedCapacity: 4}), /headroom/);
});

test('over-capacity holdings stay fairly monitored and admit no new market', () => {
  const s = scope(), inputs = {heldMarketIds: ['1', '2', '3', '4', '5', '6'], candidateMarketIds: ['7']};
  let state = createSchedulerState(s);
  const seen = new Set();
  for (let cycle = 0; cycle < 2; cycle++) {
    const plan = planBookReads(state, s, inputs);
    assert.equal(plan.overCapacity, true);
    assert.deepEqual(plan.entryMarketIds, []);
    assert.equal(plan.reads.length, 4);
    assert.ok(plan.reads.every(read => read.reason === 'held_or_pending'));
    ids(plan).forEach(value => seen.add(value));
    state = recordSchedulerAttempts(state, s, {bookMarketIds: ids(plan)});
  }
  assert.equal(seen.size, 6);
});

test('excluded holdings retain their capacity reservation and are reported unserviceable', () => {
  const s = scope(), state = createSchedulerState(s);
  const plan = planBookReads(state, s, {heldMarketIds: ['1', '2', '3'], excludedMarketIds: ['1'], candidateMarketIds: ['5']});
  assert.ok(!ids(plan).includes('1'));
  assert.deepEqual(plan.unserviceableManagedMarketIds, ['1']);
  assert.deepEqual(plan.entryMarketIds, []);
});

test('foreign/malformed proposals and altered scope fail closed', () => {
  const s = scope(), state = createSchedulerState(s);
  for (const value of [['999'], [5], ['0'], ['5', 'evil'], '5']) assert.throws(() => planBookReads(state, s, {candidateMarketIds: value}), /foreign/);
  assert.throws(() => recordSchedulerAttempts(state, s, {bookMarketIds: ['999']}), /foreign/);
  assert.throws(() => recordSchedulerAttempts(state, s, {bookMarketIds: ['1', '1']}), /Duplicate/);
  for (const change of [value => value.universe.tournamentId = 'foreign', value => value.universe.exchanges[0].exchangeId = '555', value => value.coreMarketIds = ['2']]) {
    const altered = structuredClone(s); change(altered);
    assert.throws(() => validateSchedulerState(state, altered), /scope/);
  }
  const ambiguous = structuredClone(s); ambiguous.universe.exchanges[1].marketId = '1';
  assert.throws(() => createSchedulerState(ambiguous), /scope/);
  assert.throws(() => validateSchedulerState({...state, bookCursor: -1}, s), /cursor/);
  assert.throws(() => validateSchedulerState({...state, bookLastAttempt: {'999': 1}, bookAttempts: 1}, s), /ledger/);
});

test('history retains a small usable set while reserving periodic discovery', () => {
  const s = scope(237), inputs = {heldMarketIds: ['1'], pendingMarketIds: ['2'], candidateMarketIds: ['3'], usableMarketIds: s.universe.exchanges.map(exchange => exchange.marketId)};
  let state = createSchedulerState(s);
  const refreshes = new Map(), discoveries = new Set();
  for (let cycle = 0; cycle < 24; cycle++) {
    const plan = planHistoryReads(state, s, inputs);
    assert.equal(plan.activeMarketIds.length, 8);
    assert.deepEqual(new Set(plan.activeMarketIds), new Set(['1', '2', '3', '4', '5', '6', '7', '8']));
    const read = plan.reads[0];
    if (read.reason === 'active_history_refresh') refreshes.set(read.marketId, (refreshes.get(read.marketId) || 0) + 1);
    else discoveries.add(read.marketId);
    state = JSON.parse(JSON.stringify(recordSchedulerAttempts(state, s, {historyMarketIds: [read.marketId]})));
  }
  assert.equal(discoveries.size, 6);
  assert.equal(refreshes.size, 8);
  assert.ok([...refreshes.values()].every(count => count >= 2));
});

test('history can seed discovery without usable data and does not advance unattempted slots', () => {
  const s = scope(), state = createSchedulerState(s);
  const plan = planHistoryReads(state, s, {}, {budget: 2});
  assert.deepEqual(ids(plan), ['1', '2']);
  assert.deepEqual(plan.state, state);
  const next = recordSchedulerAttempts(state, s, {historyMarketIds: ['1']});
  assert.equal(next.historyAttempts, 1);
  assert.equal(planHistoryReads(next, s).reads[0].marketId, '2');
  assert.throws(() => planHistoryReads(state, s, {}, {activeSetLimit: 237}), /budget/);
});
