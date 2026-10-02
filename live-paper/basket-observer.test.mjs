import test from 'node:test';
import assert from 'node:assert/strict';
import {createBasketObserver, observeBasket, invalidateBasket, basketReport} from './basket-observer.mjs';
import {timestamp} from '../feed/contract.mjs';
const tournamentId = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const legs = [{marketId: '382', exchangeId: '1071', predicateHash: 'a'.repeat(64)}, {marketId: '381', exchangeId: '1070', predicateHash: 'b'.repeat(64)}];
const zero = Date.parse('2026-10-02T10:00:00.000Z'), at = seconds => new Date(zero + seconds * 1000).toISOString();
const create = options => createBasketObserver({tournamentId, legs, sourceHash: 'c'.repeat(64), cutoff: at(0), ...options});
function book(index, seconds, seq = 1, bids, overrides = {}) {
  const leg = legs[index], sourceAt = timestamp(overrides.sourceAt ?? at(seconds));
  bids ??= [{price: index === 0 ? '0.200000' : '0.835000', quantity: 10000}];
  const ask = (Number(bids[0].price) + .005).toFixed(6);
  return {tournamentId, marketId: leg.marketId, exchangeId: leg.exchangeId, instrument: leg.exchangeId + ':YES',
    requestStartedAt: at(seconds - .1), receivedAt: at(seconds), sourceAt, sequence: String(seq), versionKey: seq + ':' + sourceAt,
    freshnessPolicy: 'authoritative_receipt_v1', bids, asks: [{price: ask, quantity: 10000}], ...overrides};
}
function pair(options) {const {bank, state} = create(options), first = observeBasket(state, book(0, 1), bank); return {...observeBasket(first.state, book(1, 2), bank), bank};}
test('NO complements, two adverse ticks, one-percent depth and total budget use exact integer costs', () => {
  const {observation: o} = pair(); assert.equal(o.status, 'apparent_positive_basket');
  assert.equal(o.basket.quantity, 25); assert.equal(o.basket.bestUnitCostAfterCushion, '0.975000');
  assert.equal(o.basket.totalCost, '24.375000'); assert.deepEqual(o.basket.legCosts, ['20.125000', '4.250000']);
  assert.equal(o.basket.conditionalMargin, '0.625000'); assert.equal(o.basket.stopReason, 'budget_cap');
  assert.deepEqual(o.basket.unmatchedLegRisk.worstCaseUnmatchedCostByLeg, o.basket.legCosts);
  assert.equal(o.eligibleForExecution, false); assert.equal(o.fills, null); assert.equal(o.pnl, null);
});
test('walk both ladders once, expose marginal depletion and stop before nonpositive deeper quantity', () => {
  const {bank, state} = create(); const a = book(0, 1, 1, [{price: '0.200000', quantity: 200}, {price: '0.190000', quantity: 200}]);
  const b = book(1, 2, 1, [{price: '0.835000', quantity: 300}, {price: '0.820000', quantity: 1000}]);
  const out = observeBasket(observeBasket(state, a, bank).state, b, bank).observation.basket;
  assert.equal(out.quantity, 3); assert.deepEqual(out.segments.map(x => x.quantity), [2, 1]);
  assert.deepEqual(out.segments.map(x => x.marginalUnitCost), ['0.975000', '0.985000']);
  assert.equal(out.totalCost, '2.935000'); assert.equal(out.conditionalMargin, '0.065000');
  assert.equal(out.stopReason, 'nonpositive_marginal_margin'); assert.deepEqual(out.displayedDepthUnits, ['4', '13']);
});
test('thin or mismatched leg depth limits measured basket; no fractional shares or fabricated fills', () => {
  const {bank, state} = create(); let s = observeBasket(state, book(0, 1, 1, [{price: '0.200000', quantity: 99}]), bank).state;
  assert.equal(observeBasket(s, book(1, 2), bank).observation.basket.quantity, 0);
  s = observeBasket(state, book(0, 1, 1, [{price: '0.200000', quantity: 150}]), bank).state;
  const out = observeBasket(s, book(1, 2), bank).observation; assert.equal(out.basket.quantity, 1);
  assert.equal(out.basket.stopReason, 'displayed_depth_depleted'); assert.equal(out.fills, null);
});
test('no proof remains an unverified metadata lead, including every apparent positive', () => {
  const r = pair(); assert.equal(r.observation.classification, 'unverified_metadata_lead');
  assert.equal(r.observation.verifiedFloor, null); assert.equal(r.observation.unconditionalFloor, null);
  assert.equal(basketReport(r.state, r.bank).eligibleForExecution, false);
});
test('verified proof binds exact members and evidence before cutoff, and covers normal resolution only', () => {
  const settlementProof = {status: 'verified', tournamentId, legs, relationship: {id: '11111111-2222-3333-4444-555555555555', version: 1, status: 'active', type: 'mutually_exclusive', isExhaustive: false,
    members: legs.map(({marketId, exchangeId}) => ({marketId, exchangeId, outcome: 'YES', role: 'member'}))}, minimumCombinedNoPayout: '1.000000', normalResolutionOnly: true, refundFloorVerified: false, evidenceHash: 'd'.repeat(64), verifiedAt: at(-1)};
  const r = pair({settlementProof}); assert.equal(r.observation.classification, 'verified_normal_resolution_floor');
  assert.equal(r.observation.verifiedFloor, '1.000000'); assert.equal(r.observation.unconditionalFloor, null); assert.equal(r.observation.eligibleForExecution, false);
  assert.throws(() => create({settlementProof: {...settlementProof, refundFloorVerified: true}}), /settlement proof/);
  assert.throws(() => create({settlementProof: {...settlementProof, verifiedAt: at(1)}}), /settlement proof/);
  const foreign = structuredClone(settlementProof); foreign.relationship.members[0].exchangeId = '999';
  assert.throws(() => create({settlementProof: foreign}), /member mismatch/);
});
test('foreign identity, malformed ladders and unsafe policy widening fail closed', () => {
  const {bank, state} = create();
  for (const change of [{tournamentId: '11111111-2222-3333-4444-555555555555'}, {exchangeId: '999'}, {instrument: '1071:NO'}, {freshnessPolicy: 'event_age_v1'}])
    assert.throws(() => observeBasket(state, {...book(0, 1), ...change}, bank), /Foreign/);
  assert.throws(() => observeBasket(state, book(0, 1, 1, [{price: '0.200000', quantity: 100}, {price: '0.205000', quantity: 100}]), bank), /Unsorted/);
  assert.throws(() => create({legs: [legs[0], legs[0]]}), /identity/);
  for (const overrides of [{depthFractionBps: 101}, {totalBudget: '25.000001'}, {maxSourceClockSkewMs: 2001}, {maxDataAgeMs: 20001}, {adverseTicksPerLeg: 0}])
    assert.throws(() => create({policy: overrides}));
});
test('fresh authoritative receipts tolerate an old quiet source, reject future source and slow/reversed clocks', () => {
  const {bank, state} = create(); const quiet = book(0, 1, 1, undefined, {sourceAt: timestamp(at(-10000)), versionKey: '1:' + timestamp(at(-10000))});
  assert.equal(observeBasket(state, quiet, bank).observation.status, 'waiting_for_pair');
  assert.doesNotThrow(() => observeBasket(state, book(0, 1, 1, undefined, {sourceAt: timestamp(at(1.130)), versionKey: '1:' + timestamp(at(1.130))}), bank));
  assert.throws(() => observeBasket(state, book(0, 1, 1, undefined, {sourceAt: timestamp(at(3.001)), versionKey: '1:' + timestamp(at(3.001))}), bank), /clocks/);
  assert.throws(() => observeBasket(state, book(0, 30, 1, undefined, {requestStartedAt: at(0)}), bank), /clocks/);
  assert.throws(() => observeBasket(state, book(0, 1, 1, undefined, {requestStartedAt: at(2)}), bank), /clocks/);
  const s = observeBasket(state, book(0, 2, 2), bank).state;
  assert.throws(() => observeBasket(s, book(0, 1, 2), bank), /clocks/);
  assert.throws(() => observeBasket(s, book(0, 3, 1), bank), /rollback/);
});
test('stale cross-leg receipts and requests at/before prospective cutoff cannot create positives', () => {
  const {bank, state} = create(); let s = observeBasket(state, book(0, 1), bank).state;
  const stale = observeBasket(s, book(1, 21.001), bank); assert.equal(stale.observation.status, 'stale_pair'); assert.equal(stale.state.positivePersistenceCount, 0);
  s = observeBasket(state, book(0, 0), bank).state;
  assert.equal(observeBasket(s, book(1, 1), bank).observation.status, 'before_cutoff');
});
test('persistence needs both independent versions and requests after the previous observation plus latency', () => {
  const r = pair(); assert.equal(r.state.positivePersistenceCount, 1);
  let s = observeBasket(r.state, book(0, 4, 2), r.bank).state; assert.equal(s.positivePersistenceCount, 1);
  const independent = observeBasket(s, book(1, 5, 2), r.bank); assert.equal(independent.observation.independentPair, true);
  assert.equal(independent.observation.status, 'independent_positive_persistence'); assert.equal(independent.state.positivePersistenceCount, 2);
  s = observeBasket(r.state, book(0, 3, 2, undefined, {requestStartedAt: at(2.5)}), r.bank).state;
  assert.equal(observeBasket(s, book(1, 4, 2), r.bank).observation.independentPair, false);
});
test('unchanged snapshots cannot replenish liquidity or count persistence; same-version mutation is invalid', () => {
  const r = pair(); let s = r.state;
  for (const [index, seconds, original] of [[0, 4, 1], [1, 5, 2], [0, 6, 1]]) {
    const q = {...book(index, original), receivedAt: at(seconds), requestStartedAt: at(seconds - .1)};
    const out = observeBasket(s, q, r.bank); s = out.state; assert.equal(out.observation.status, 'repeated_pair'); assert.equal(out.observation.independentPair, false);
  }
  assert.equal(s.counts.cofreshPairs, 1); assert.equal(s.counts.apparentPositivePairs, 1); assert.equal(s.positivePersistenceCount, 1);
  const changed = {...book(1, 2), receivedAt: at(7), requestStartedAt: at(6.9), bids: [{price: '0.835000', quantity: 9999}]};
  assert.throws(() => observeBasket(s, changed, r.bank), /unchanged basket version/);
});
test('invalidation clears the pair and preserves rollback guards; replay is deterministic and inputs immutable', () => {
  const r = pair(), before = structuredClone(r.state), invalid = invalidateBasket(r.state, '382', at(3), r.bank);
  assert.deepEqual(r.state, before); assert.equal(invalid.positivePersistenceCount, 0); assert.equal(invalid.books['1071'], undefined);
  assert.throws(() => observeBasket(invalid, book(0, 4, 0), r.bank), /rollback/);
  assert.equal(observeBasket(invalid, book(0, 4, 2, undefined, {requestStartedAt: at(2.9)}), r.bank).observation.status, 'request_before_invalidation');
  const checkpoint = JSON.parse(JSON.stringify(invalid)), a = observeBasket(checkpoint, book(0, 5, 2), r.bank), b = observeBasket(invalid, book(0, 5, 2), r.bank);
  assert.deepEqual(a, b); assert.deepEqual(checkpoint, invalid); assert.equal(basketReport(a.state, r.bank).fills, null);
  assert.throws(() => observeBasket(a.state, book(1, 6, 2), {...r.bank, sourceHash: 'e'.repeat(64)}), /bank changed/);
});
