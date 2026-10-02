import test from 'node:test';
import assert from 'node:assert/strict';
import {createSchedulerState, planBookReads, recordSchedulerAttempts} from './book-scheduler.mjs';
import {planBasketBooks} from './basket-book-plan.mjs';
import {timestamp} from '../feed/contract.mjs';
import {createBasketObserver, observeBasket} from './basket-observer.mjs';

const now = Date.parse('2026-10-02T11:00:00.000Z'), stamp = offset => new Date(now + offset).toISOString();
const scope = {universe: {tournamentId: 'fixture', exchanges: Array.from({length: 12}, (_, n) => ({marketId: String(n + 1), exchangeId: String(n + 101), option: 'YES'}))}, coreMarketIds: ['1', '2', '3', '4']};
const ids = plan => plan.reads.map(read => read.marketId);
const candidate = (a, b, priceGapMicros = 10000, receivedAt = stamp(-1000)) => ({tournamentId: 'fixture', marketIds: [a, b], exchangeIds: [String(Number(a) + 100), String(Number(b) + 100)], receivedAt, priceGapMicros});
const base = (inputs = {}, state = createSchedulerState(scope)) => planBookReads(state, scope, inputs);
const overlay = (plan, candidates, extra = {}) => planBasketBooks(plan, scope, {candidates, cycle: 0, now, ...extra});
const followup = (a = '7', b = '8', offset = -30000) => ({...candidate(a, b), observerBankHash: 'a'.repeat(64),
  pairId: 'b'.repeat(64), sourceSequence: 123, sourceHash: 'c'.repeat(64), cutoff: stamp(-3600000),
  sourceMeasurement: {reason: 'paired_depth_measurement', cycle: 0, every: 4, marketIds: [a, b],
    exchangeIds: [String(Number(a) + 100), String(Number(b) + 100)], receivedAt: stamp(offset - 10000)},
  observation: {pairId: 'b'.repeat(64), at: stamp(offset), status: 'apparent_positive_basket', apparentPositive: true, newPairEvidence: true,
    independentPair: false, positivePersistenceCount: 1, eligibleForExecution: false, fills: null, pnl: null,
    pairReceipts: [stamp(offset - 3220), stamp(offset)],
    pairVersions: ['10:' + timestamp(stamp(offset - 3090)), '11:' + timestamp(stamp(offset + 130))],
    basket: {quantity: 5, totalCost: '4.975000', conditionalMargin: '0.025000', marginalUnitCost: '0.995000', depthFractionBps: 100}}});

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

test('actual positive depth gets one bounded follow-up even when later bulk no longer screens positive', () => {
  const plan = base(), anchor = followup(), before = JSON.stringify({plan, anchor});
  const result = overlay(plan, [], {followups: [anchor]});
  assert.deepEqual(ids(result), ['7', '8', '1', '2']); assert.deepEqual(result.entryMarketIds, []);
  assert.equal(result.basketMeasurement.reason, 'paired_depth_followup');
  assert.equal(result.basketMeasurement.receivedAt, anchor.observation.at);
  assert.equal(result.basketMeasurement.priceGapMicros, 5000);
  assert.equal(result.basketMeasurement.persistSchedulerStateBeforeReads, true);
  const claim = Object.values(result.state.basketFollowups.claims)[0];
  assert.equal(claim.sourceSequence, 123); assert.equal(claim.sourceHash, 'c'.repeat(64));
  assert.deepEqual(claim.pairVersions, anchor.observation.pairVersions); assert.equal(claim.claimedAt, stamp(0));
  assert.equal(result.state.bookAttempts, plan.state.bookAttempts); assert.equal(JSON.stringify({plan, anchor}), before);
  // An attractive changed bulk quote cannot replace or rewrite the anchor.
  const competing = overlay(plan, [candidate('9', '10', 50000)], {followups: [anchor]});
  assert.deepEqual(competing.basketMeasurement.marketIds, ['7', '8']);
  assert.deepEqual(competing.basketMeasurement.followup, result.basketMeasurement.followup);
});

test('persisted claim prevents repeat after restart, including a crash before either read or a failed first leg', () => {
  const anchor = followup(), selected = overlay(base(), [], {followups: [anchor]});
  const persisted = JSON.parse(JSON.stringify(selected.state));
  const restarted = overlay(base({}, persisted), [], {followups: [anchor], cycle: 4});
  assert.equal(restarted.basketMeasurement.reason, 'no_fresh_positive_pair'); assert.deepEqual(restarted.state, persisted);
  const attempted = recordSchedulerAttempts(persisted, scope, {bookMarketIds: ['7']});
  assert.deepEqual(attempted.basketFollowups, persisted.basketFollowups);
  const afterFailure = overlay(base({}, JSON.parse(JSON.stringify(attempted))), [], {followups: [anchor], cycle: 8});
  assert.equal(afterFailure.basketMeasurement.reason, 'no_fresh_positive_pair');
  const repackagedHeartbeat = {...anchor, sourceSequence: 200, sourceHash: 'd'.repeat(64)};
  assert.equal(overlay(base({}, persisted), [], {followups: [repackagedHeartbeat], cycle: 12}).basketMeasurement.reason, 'no_fresh_positive_pair');
});

test('follow-ups use the existing cadence and protect all risk, pending and entry reads without consuming deferred anchors', () => {
  const anchor = followup(), normal = base();
  const deferred = overlay(normal, [], {followups: [anchor], cycle: 1});
  assert.equal(deferred.basketMeasurement.reason, 'cadence_reserved'); assert.equal(deferred.state.basketFollowups, undefined);
  const crowded = base({priorityManagedMarketIds: ['9'], pendingMarketIds: ['1'], candidateMarketIds: ['8']});
  const protectedReads = crowded.reads.slice(0, 3);
  const blocked = overlay(crowded, [], {followups: [followup('7', '10')]});
  assert.deepEqual(blocked.reads, crowded.reads); assert.equal(blocked.state.basketFollowups, undefined);
  assert.equal(blocked.basketMeasurement.reason, 'protected_capacity');
  const overlap = overlay(crowded, [], {followups: [anchor]});
  assert.deepEqual(overlap.reads.slice(0, 3), protectedReads); assert.deepEqual(ids(overlap), ['9', '1', '8', '7']);
  assert.deepEqual(overlap.entryMarketIds, ['8']); assert.equal(overlap.reads.length, 4);
  const excluded = overlay(normal, [], {followups: [anchor], excludedMarketIds: ['7']});
  assert.deepEqual(excluded.reads, normal.reads); assert.equal(excluded.state.basketFollowups, undefined);
});

test('a follow-up result never recursively schedules another follow-up', () => {
  for (const changes of [{status: 'independent_positive_persistence', independentPair: true, positivePersistenceCount: 2},
    {status: 'repeated_pair', newPairEvidence: false}, {status: 'no_positive_sized_basket', apparentPositive: false}]) {
    const anchor = followup(); Object.assign(anchor.observation, changes);
    const result = overlay(base(), [], {followups: [anchor]});
    assert.equal(result.basketMeasurement.reason, 'no_fresh_positive_pair'); assert.equal(result.state.basketFollowups, undefined);
  }
});

test('actual observer stale-leg reset cannot disguise a follow-up result as a new initiating observation', () => {
  const tournamentId = 'bda92870-621e-47b0-bc3c-3602c5c26f55', actualScope = {...scope, universe: {...scope.universe, tournamentId}};
  const {bank, state: initial} = createBasketObserver({tournamentId, legs: [{marketId: '7', exchangeId: '107', predicateHash: '1'.repeat(64)},
    {marketId: '8', exchangeId: '108', predicateHash: '2'.repeat(64)}], sourceHash: '3'.repeat(64), cutoff: stamp(-3600000)});
  const book = (marketId, offset, sequence) => ({tournamentId, marketId, exchangeId: String(Number(marketId) + 100),
    instrument: String(Number(marketId) + 100) + ':YES', freshnessPolicy: 'authoritative_receipt_v1',
    receivedAt: stamp(offset), requestStartedAt: stamp(offset - 100), sourceAt: timestamp(stamp(offset + 130)),
    sequence: String(sequence), versionKey: sequence + ':' + timestamp(stamp(offset + 130)),
    bids: [{price: marketId === '7' ? '0.835000' : '0.180000', quantity: 1000}],
    asks: [{price: marketId === '7' ? '0.840000' : '0.185000', quantity: 1000}]});
  let s = observeBasket(initial, book('7', -33220, 1), bank).state;
  const first = observeBasket(s, book('8', -30000, 1), bank); s = first.state;
  const anchor = {...followup(), tournamentId, observation: {pairId: 'b'.repeat(64), ...first.observation}};
  const firstPlan = planBasketBooks(planBookReads(createSchedulerState(actualScope), actualScope), actualScope,
    {cycle: 0, now: now - 25000, followups: [anchor]});
  assert.equal(firstPlan.basketMeasurement.reason, 'paired_depth_followup');
  const stale = observeBasket(s, book('7', -6000, 2), bank); assert.equal(stale.observation.status, 'stale_pair');
  const again = observeBasket(stale.state, book('8', -2800, 2), bank);
  assert.equal(again.observation.status, 'apparent_positive_basket'); assert.equal(again.observation.positivePersistenceCount, 1);
  assert.equal(again.observation.independentPair, false);
  const derived = {...anchor, sourceSequence: 124, sourceHash: 'd'.repeat(64), sourceMeasurement: firstPlan.basketMeasurement,
    observation: {pairId: anchor.pairId, ...again.observation}};
  const repeat = planBasketBooks(planBookReads(JSON.parse(JSON.stringify(firstPlan.state)), actualScope), actualScope,
    {cycle: 4, now, followups: [derived]});
  assert.equal(repeat.basketMeasurement.reason, 'no_fresh_positive_pair');
  assert.deepEqual(repeat.state.basketFollowups, firstPlan.state.basketFollowups);
});

test('anchors obey causal latency and bounded age; unverified bulk values cannot masquerade as depth observations', () => {
  for (const offset of [-1000, -500, 1000, -600001]) {
    const result = overlay(base(), [], {followups: [followup('7', '8', offset)]});
    assert.equal(result.basketMeasurement.reason, 'no_fresh_positive_pair'); assert.equal(result.state.basketFollowups, undefined);
  }
  assert.equal(overlay(base(), [], {followups: [followup('7', '8', -1001)]}).basketMeasurement.reason, 'paired_depth_followup');
  assert.equal(overlay(base(), [], {followups: [followup('7', '8', -600000)]}).basketMeasurement.reason, 'paired_depth_followup');
  assert.throws(() => overlay(base(), [], {followups: [candidate('7', '8')]}), /provenance/);
  const bad = followup(); bad.observation.basket.totalCost = '4.900000';
  assert.throws(() => overlay(base(), [], {followups: [bad]}), /economics/);
  const skew = followup(); skew.observation.pairReceipts[0] = stamp(-60000);
  assert.throws(() => overlay(base(), [], {followups: [skew]}), /clocks/);
  const futureSource = followup(); futureSource.observation.pairVersions[1] = '11:' + timestamp(stamp(-27999));
  assert.throws(() => overlay(base(), [], {followups: [futureSource]}), /version/);
});

test('follow-up provenance, persisted identity and scoped legs fail closed on conflicts', () => {
  const anchor = followup(), selected = overlay(base(), [], {followups: [anchor]});
  assert.throws(() => overlay(base({}, selected.state), [], {followups: [{...anchor, sourceHash: 'd'.repeat(64)}]}), /Conflicting/);
  const changed = followup(); changed.observation.pairVersions[0] = '12:' + timestamp(stamp(-33090));
  assert.throws(() => overlay(base({}, selected.state), [], {followups: [changed]}), /Conflicting/);
  const corrupt = structuredClone(selected.state); Object.values(corrupt.basketFollowups.claims)[0].pairVersions[0] = 'changed';
  assert.throws(() => overlay(base({}, corrupt), []), /identity/);
  assert.throws(() => overlay(base(), [], {followups: [{...anchor, tournamentId: 'foreign'}]}), /provenance/);
  assert.throws(() => overlay(base(), [], {followups: [{...anchor, exchangeIds: ['107', '999']}]}), /scope/);
  assert.throws(() => overlay(base(), [], {followups: [{...anchor, pairId: 'd'.repeat(64)}]}), /observation/);
  assert.throws(() => overlay(base(), [], {followups: [anchor, {...anchor, sourceHash: 'd'.repeat(64)}]}), /Conflicting/);
  assert.equal(overlay(base(), [], {followups: [{...anchor, sourceMeasurement: undefined}]}).basketMeasurement.reason, 'no_fresh_positive_pair');
  const wrongOrigin = {...anchor, sourceMeasurement: {...anchor.sourceMeasurement, marketIds: ['9', '10'], exchangeIds: ['109', '110']}};
  assert.throws(() => overlay(base(), [], {followups: [wrongOrigin]}), /origin/);
  assert.throws(() => planBasketBooks(base(), scope, {cycle: 0, now, followups: [anchor]}, {followupMaxAgeMs: 600001}), /clock/);
  assert.throws(() => planBasketBooks(base(), scope, {cycle: 0, now, followups: [anchor]}, {followupLatencyMs: 999}), /clock/);
});

test('the overlay never increases a caller-provided smaller book budget', () => {
  const two = planBookReads(createSchedulerState(scope), scope, {}, {budget: 2, managedCapacity: 1});
  const result = overlay(two, [], {followups: [followup()]});
  assert.deepEqual(ids(result), ['7', '8']); assert.equal(result.reads.length, 2);
  const protectedTwo = planBookReads(createSchedulerState(scope), scope, {priorityManagedMarketIds: ['9']}, {budget: 2, managedCapacity: 1});
  assert.deepEqual(overlay(protectedTwo, [], {followups: [followup()]}).reads, protectedTwo.reads);
  assert.equal(overlay(two, [candidate('7', '8')]).reads.length, 2);
});
