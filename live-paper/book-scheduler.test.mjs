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

const allocationAt = '2026-10-02T10:00:00.000Z';
const clockAfter = (milliseconds, at = allocationAt) => new Date(Date.parse(at) + milliseconds).toISOString();
function allocation(s, patch = {}) {
  return {schemaVersion: 1, at: allocationAt, markets: s.universe.exchanges.map(({marketId}) => ({marketId, referenceReady: false,
    validUntil: null, receivedAt: null, identityKnown: false, quotePlausible: false, opportunityGapMicros: null, ...patch}))};
}
const hint = (a, marketId, patch) => Object.assign(a.markets.find(row => row.marketId === marketId), patch);

test('due allocation preserves official, managed and candidate priority with every fourth global reservation', () => {
  const s = scope(), historyAllocation = allocation(s, {identityKnown: true, quotePlausible: true, opportunityGapMicros: 10000});
  hint(historyAllocation, '7', {opportunityGapMicros: -5000});
  const inputs = {priorityManagedMarketIds: ['9'], heldMarketIds: ['1'], candidateMarketIds: ['8'], historyAllocation};
  let state = createSchedulerState(s); const selected = [];
  for (let n = 0; n < 4; n++) {
    const plan = planHistoryReads(state, s, inputs), read = plan.reads[0]; selected.push(read.marketId);
    assert.deepEqual(plan.activeMarketIds.slice(0, 3), ['9', '1', '8']); assert.ok(plan.activeMarketIds.length <= 8);
    assert.equal(read.reservedDiscovery, n === 3); assert.equal(read.priority, n === 3 ? 4 : n);
    state = JSON.parse(JSON.stringify(recordSchedulerAttempts(state, s, {historyMarketIds: [read.marketId], attemptedAt: historyAllocation.at})));
  }
  assert.deepEqual(selected, ['9', '1', '8', '10']);
});

test('fresh active references spend no history slot until the ten-minute expiry boundary', () => {
  const s = scope(), historyAllocation = allocation(s, {referenceReady: true, receivedAt: clockAfter(-60000), validUntil: clockAfter(600001), identityKnown: true});
  const inputs = {heldMarketIds: ['1'], candidateMarketIds: ['2'], usableMarketIds: ['1', '2'], historyAllocation}, state = createSchedulerState(s);
  let plan = planHistoryReads(state, s, inputs); assert.deepEqual(plan.reads, []); assert.equal(plan.dueMarketCount, 0);
  historyAllocation.at = clockAfter(1); plan = planHistoryReads(state, s, inputs);
  assert.deepEqual(ids(plan), ['1']); assert.equal(plan.reads[0].reason, 'due_managed_history');
  hint(historyAllocation, '1', {validUntil: null}); assert.deepEqual(ids(planHistoryReads(state, s, inputs)), ['1'], 'No expiry proof must not mean indefinitely fresh');
});

test('successful but unusable receipts wait ten minutes and successful ready receipts stop refreshes', () => {
  const s = scope(), historyAllocation = allocation(s, {referenceReady: true, receivedAt: clockAfter(-1000), validUntil: clockAfter(3600000)});
  hint(historyAllocation, '1', {referenceReady: false, receivedAt: clockAfter(-599999), validUntil: null, identityKnown: true});
  const state = createSchedulerState(s), inputs = {heldMarketIds: ['1'], historyAllocation};
  let plan = planHistoryReads(state, s, inputs); assert.deepEqual(plan.reads, []); assert.deepEqual(plan.receiptCooldownMarketIds, ['1']);
  historyAllocation.at = clockAfter(1); assert.deepEqual(ids(planHistoryReads(state, s, inputs)), ['1']);
  const attempted = recordSchedulerAttempts(state, s, {historyMarketIds: ['1'], attemptedAt: historyAllocation.at});
  hint(historyAllocation, '1', {referenceReady: true, receivedAt: historyAllocation.at, validUntil: clockAfter(3600001)});
  plan = planHistoryReads(attempted, s, inputs); assert.deepEqual(plan.reads, []); assert.equal(plan.dueMarketCount, 0);
});

test('failed history attempts need eight intervening attempts through restarts; book attempts do not count', () => {
  const s = scope(20), inputs = {priorityManagedMarketIds: ['1'], historyAllocation: allocation(s)};
  let state = recordSchedulerAttempts(createSchedulerState(s), s, {historyMarketIds: ['1'], bookMarketIds: ['2', '3', '4'], attemptedAt: allocationAt});
  for (let n = 0; n < 8; n++) {
    const plan = planHistoryReads(JSON.parse(JSON.stringify(state)), s, inputs);
    assert.equal(plan.attemptCooldownMarketIds.includes('1'), true); assert.notEqual(plan.reads[0].marketId, '1');
    state = recordSchedulerAttempts(state, s, {historyMarketIds: ids(plan), attemptedAt: allocationAt});
  }
  const plan = planHistoryReads(JSON.parse(JSON.stringify(state)), s, inputs);
  assert.equal(plan.attemptCooldownMarketIds.includes('1'), false); assert.deepEqual(ids(plan), ['1']);
});

test('known identities with plausible near opportunities outrank missing low-value references and ties stay fair', () => {
  const s = scope(), historyAllocation = allocation(s);
  hint(historyAllocation, '1', {identityKnown: true, quotePlausible: false, opportunityGapMicros: -10000});
  hint(historyAllocation, '2', {identityKnown: true, quotePlausible: true, opportunityGapMicros: 40000});
  hint(historyAllocation, '3', {identityKnown: true, quotePlausible: true, opportunityGapMicros: 5000});
  hint(historyAllocation, '4', {identityKnown: true, quotePlausible: true, opportunityGapMicros: 5000});
  hint(historyAllocation, '5', {identityKnown: false, quotePlausible: true, opportunityGapMicros: -10000});
  const state = createSchedulerState(s), inputs = {historyAllocation};
  let plan = planHistoryReads(state, s, inputs); assert.deepEqual(ids(plan), ['3']); assert.equal(plan.reads[0].opportunityGapMicros, 5000);
  const next = recordSchedulerAttempts(state, s, {historyMarketIds: ids(plan), attemptedAt: allocationAt}); plan = planHistoryReads(next, s, inputs); assert.deepEqual(ids(plan), ['4']);
});

test('reserved oldest-attempt global discovery eventually reaches unknown identities despite stronger known opportunities', () => {
  const s = scope(20), historyAllocation = allocation(s);
  for (const row of historyAllocation.markets) if (Number(row.marketId) > 12) Object.assign(row, {identityKnown: true, quotePlausible: true, opportunityGapMicros: Number(row.marketId)});
  let state = createSchedulerState(s); const seenUnknown = new Set(), last = {};
  for (let n = 0; n < 80; n++) {
    const plan = planHistoryReads(state, s, {historyAllocation}), read = plan.reads[0];
    assert.equal(plan.reads.length, 1); assert.equal(read.reservedDiscovery, (n + 1) % 4 === 0);
    if (last[read.marketId] !== undefined) assert.ok(n - last[read.marketId] >= 9);
    last[read.marketId] = n;
    if (Number(read.marketId) <= 12) seenUnknown.add(read.marketId);
    state = JSON.parse(JSON.stringify(recordSchedulerAttempts(state, s, {historyMarketIds: ids(plan), attemptedAt: allocationAt})));
  }
  assert.equal(seenUnknown.size, 12);
});

test('two-slot allocation remains immutable, unique and scoped while only actual attempts advance the ledger', () => {
  const s = scope(), historyAllocation = allocation(s, {identityKnown: true}), state = recordSchedulerAttempts(createSchedulerState(s), s, {historyMarketIds: ['1', '2', '3'], attemptedAt: allocationAt}), inputs = {historyAllocation, excludedMarketIds: ['4']};
  const before = JSON.stringify({state, inputs}), bookBefore = planBookReads(state, s, inputs), plan = planHistoryReads(state, s, inputs, {budget: 2});
  assert.deepEqual(ids(plan), ['5', '6']); assert.equal(plan.reads[0].reservedDiscovery, true); assert.equal(plan.reads[1].reservedDiscovery, false);
  assert.equal(new Set(ids(plan)).size, 2); assert.deepEqual(plan.state, state); assert.equal(JSON.stringify({state, inputs}), before);
  assert.deepEqual(planBookReads(state, s, inputs), bookBefore);
  const partial = recordSchedulerAttempts(state, s, {historyMarketIds: ['5'], attemptedAt: allocationAt}); assert.equal(partial.historyLastAttempt['6'], undefined);
  assert.deepEqual(ids(planHistoryReads(partial, s, inputs)), ['6']); assert.throws(() => planHistoryReads(state, s, inputs, {budget: 3}), /budget/);
});

test('allocation rejects foreign, duplicate, incomplete and malformed hints without changing the scheduler', () => {
  const s = scope(), state = createSchedulerState(s), valid = allocation(s), changes = [
    a => a.markets.pop(), a => a.markets[0].marketId = '999', a => a.markets[0].marketId = '2', a => a.markets[0].marketId = 1,
    a => a.at = '2026-10-02T10:00:00Z', a => a.at = 'invalid', a => a.markets[0].receivedAt = clockAfter(1),
    a => a.markets[0].validUntil = 'not-a-clock', a => a.markets[0].referenceReady = 1, a => a.markets[0].identityKnown = null,
    a => a.markets[0].quotePlausible = 'true', a => a.markets[0].opportunityGapMicros = NaN,
    a => a.markets[0].opportunityGapMicros = 1000001, a => a.markets[0].opportunityGapMicros = -1000001,
    a => a.markets[0].opportunityGapMicros = .5, a => delete a.markets[0].receivedAt, a => a.markets[0].unexpected = true,
  ];
  for (const change of changes) { const altered = structuredClone(valid); change(altered); assert.throws(() => planHistoryReads(state, s, {historyAllocation: altered}), /history allocation/); }
  assert.equal(state.historyAttempts, 0); assert.deepEqual(planHistoryReads(state, s), planHistoryReads(state, s, {historyAllocation: undefined}));
});

test('a small pool of failed reads retries after ten wall-clock minutes without requiring impossible extra attempts', () => {
  const s = scope(), historyAllocation = allocation(s, {referenceReady: true, receivedAt: clockAfter(-1000), validUntil: clockAfter(3600000)});
  for (const id of ['1', '2']) hint(historyAllocation, id, {referenceReady: false, receivedAt: null, validUntil: null});
  let state = createSchedulerState(s);
  for (const expected of ['1', '2']) {
    const plan = planHistoryReads(state, s, {historyAllocation}); assert.deepEqual(ids(plan), [expected]);
    state = JSON.parse(JSON.stringify(recordSchedulerAttempts(state, s, {historyMarketIds: ids(plan), attemptedAt: historyAllocation.at})));
  }
  assert.equal(state.historyAttempts, 2); assert.deepEqual(planHistoryReads(state, s, {historyAllocation}).reads, []);
  historyAllocation.at = clockAfter(599999); assert.deepEqual(planHistoryReads(state, s, {historyAllocation}).reads, []);
  historyAllocation.at = clockAfter(600000); const plan = planHistoryReads(JSON.parse(JSON.stringify(state)), s, {historyAllocation});
  assert.deepEqual(ids(plan), ['1']); assert.equal(state.historyAttempts, 2); assert.equal(state.schemaVersion, 1);
});

test('legacy untimed attempts get one retry and then acquire the persisted cooldown without inventing a receipt', () => {
  const s = scope(), historyAllocation = allocation(s, {referenceReady: true, receivedAt: clockAfter(-1000), validUntil: clockAfter(3600000)});
  hint(historyAllocation, '1', {referenceReady: false, receivedAt: null, validUntil: null});
  const legacy = recordSchedulerAttempts(createSchedulerState(s), s, {historyMarketIds: ['1']});
  assert.deepEqual(ids(planHistoryReads(legacy, s, {historyAllocation})), ['1']);
  const timed = recordSchedulerAttempts(legacy, s, {historyMarketIds: ['1'], attemptedAt: allocationAt});
  assert.equal(timed.historyLastAttemptAt['1'], allocationAt); assert.equal(historyAllocation.markets[0].receivedAt, null);
  assert.deepEqual(planHistoryReads(JSON.parse(JSON.stringify(timed)), s, {historyAllocation}).reads, []);
  assert.deepEqual(validateSchedulerState(timed, s), timed); assert.equal(legacy.historyLastAttemptAt, undefined);
  assert.throws(() => recordSchedulerAttempts(legacy, s, {historyMarketIds: ['1'], attemptedAt: '2026-10-02T10:00:00Z'}), /clock/);
  for (const clocks of [null, [], {'999': allocationAt}, {'2': allocationAt}, {'1': 'invalid'}]) assert.throws(() => validateSchedulerState({...legacy, historyLastAttemptAt: clocks}, s), /clock ledger/);
  assert.throws(() => planHistoryReads({...timed, historyLastAttemptAt: {'1': clockAfter(1)}}, s, {historyAllocation}), /precedes attempt/);
  assert.equal(recordSchedulerAttempts(timed, s, {historyMarketIds: ['1']}).historyLastAttemptAt['1'], undefined, 'An untimed legacy caller cannot retain a previous attempt clock as the current one');
});
