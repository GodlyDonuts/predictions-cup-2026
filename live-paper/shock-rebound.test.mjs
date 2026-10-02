import test from 'node:test';
import assert from 'node:assert/strict';
import {createShockRebound, advanceShockRebound, invalidateShockRebound} from './shock-rebound.mjs';
import {timestamp} from '../feed/contract.mjs';

const zero = Date.parse('2026-10-02T10:00:00.000Z');
const at = seconds => new Date(zero + seconds * 1000).toISOString();
function quote(seconds, seq, bid = '0.595000', ask = '0.600000', options = {}) {
  const sourceAt = timestamp(options.sourceAt ?? at(seconds));
  return {marketId: '377', exchangeId: '1066', instrument: '1066:YES', receivedAt: at(seconds),
    requestStartedAt: at(seconds - .1), sourceAt, sequence: String(seq), versionKey: seq + ':' + sourceAt,
    bids: [{price: bid, quantity: 10000}], asks: [{price: ask, quantity: 10000}], ...options};
}
function warm(overrides = {}, step = 60, count = 10, prices) {
  const created = createShockRebound({marketId: '377', exchangeId: '1066', sourceHash: 'a'.repeat(64), cutoff: at(0), config: overrides});
  let state = created.state;
  for (let i = count; i > 0; i--) state = advanceShockRebound(state, quote(-i * step, count - i + 1, ...(prices?.(count - i) ?? [])), created.bank).state;
  return {...created, state};
}
const shock = (s, bank) => advanceShockRebound(s, quote(30, 20, '0.545000', '0.550000'), bank);
test('a drop alone cannot enter; a later causal bid rebound proposes conservative executable terms', () => {
  const {state, bank} = warm(); const original = structuredClone(state);
  const armed = shock(state, bank); assert.equal(armed.signal, null); assert.equal(armed.diagnostic.reason, 'shock_armed');
  assert.deepEqual(state, original);
  const r = advanceShockRebound(armed.state, quote(32, 21, '0.555000', '0.560000'), bank);
  assert.equal(r.diagnostic.reason, 'confirmed_rebound'); assert.equal(r.signal.maximumEntryPrice, '0.565000');
  assert.equal(r.signal.observedExitBidAfterCushion, '0.550000'); assert.equal(r.signal.quantity, 10);
  assert.equal(r.signal.troughBid, '0.545000'); assert.equal(r.signal.troughSourceVersion, '20:' + timestamp(at(30)));
  assert.equal(r.signal.profitabilityEstablished, false); assert.equal(r.signal.sourceHash, bank.sourceHash);
  assert.equal(advanceShockRebound(r.state, quote(33, 22, '0.555000', '0.560000'), bank).signal, null);
});
test('exact two-cent effective spread is admitted; a wider executable spread is refused', () => {
  const {state, bank} = warm(), armed = shock(state, bank).state;
  const equal = advanceShockRebound(armed, quote(32, 21, '0.555000', '0.565000'), bank);
  assert.equal(equal.signal.effectiveSpread, '0.020000');
  const wide = advanceShockRebound(armed, quote(32, 21, '0.555000', '0.570000'), bank);
  assert.equal(wide.signal, null); assert.equal(wide.diagnostic.reason, 'effective_spread');
});
test('repeated source versions neither confirm nor create additional history and quietly expire', () => {
  const {state, bank} = warm(), armed = shock(state, bank).state;
  const same = {...quote(30, 20, '0.545000', '0.550000'), receivedAt: at(32), requestStartedAt: at(31.9)};
  const repeated = advanceShockRebound(armed, same, bank);
  assert.equal(repeated.signal, null); assert.equal(repeated.state.history.length, armed.history.length);
  const expired = advanceShockRebound(repeated.state, {...same, receivedAt: at(151), requestStartedAt: at(150.9)}, bank);
  assert.equal(expired.state.armed, null);
  assert.throws(() => advanceShockRebound(armed, {...same, bids: [{price: '0.545000', quantity: 9999}]}, bank), /unchanged version/);
});
test('pre-trough request cannot confirm, and a lower trough requires a new later request', () => {
  const {state, bank} = warm(), armed = shock(state, bank).state;
  const early = advanceShockRebound(armed, quote(32, 21, '0.555000', '0.560000', {requestStartedAt: at(29.9)}), bank);
  assert.equal(early.signal, null); assert.equal(early.diagnostic.reason, 'confirmation_not_causal');
  const lower = advanceShockRebound(armed, quote(32, 21, '0.535000', '0.540000'), bank);
  assert.equal(lower.diagnostic.reason, 'new_trough');
  assert.equal(advanceShockRebound(lower.state, quote(32.5, 22, '0.550000', '0.555000'), bank).signal, null);
  const recovered = advanceShockRebound(lower.state, quote(34, 22, '0.550000', '0.555000'), bank).signal;
  assert.ok(recovered); assert.equal(recovered.troughBid, '0.535000');
  assert.equal(recovered.troughSourceVersion, '21:' + timestamp(at(32)));
});
test('cutoff, lagged reference, frozen source/config and identity prevent lookahead or scope substitution', () => {
  const {state, bank} = warm();
  const laterBank = createShockRebound({marketId: '377', exchangeId: '1066', sourceHash: 'a'.repeat(64), cutoff: at(100)}).bank;
  assert.throws(() => shock(state, laterBank), /bank changed/);
  assert.throws(() => shock(state, {...bank, sourceHash: 'b'.repeat(64)}), /bank changed/);
  assert.throws(() => shock(state, {...bank, config: {...bank.config, minimumShock: '0.010000'}}), /bank changed/);
  assert.throws(() => advanceShockRebound(state, {...quote(30, 20), exchangeId: '1067'}, bank), /identity/);
  const prefix = advanceShockRebound(state, quote(-1, 20, '0.545000', '0.550000'), bank);
  assert.equal(prefix.signal, null); assert.equal(prefix.state.armed, null);
  const armed = shock(state, bank); assert.equal(armed.state.armed.referenceBid, '0.595000');
  assert.equal(armed.state.history.at(-1).bid, '0.545000');
});
test('invalid clocks, source rollback, stale requests and missing liquidity fail closed', () => {
  const {state, bank} = warm();
  assert.throws(() => advanceShockRebound(state, quote(30, 20, undefined, undefined, {sourceAt: at(31), versionKey: '20:' + timestamp(at(31))}), bank), /Noncausal/);
  assert.throws(() => advanceShockRebound(state, quote(30, 1), bank), /rollback/);
  assert.throws(() => advanceShockRebound(state, quote(30, 20, undefined, undefined, {requestStartedAt: at(0)}), bank), /stale/);
  const armed = shock(state, bank).state;
  const shallow = advanceShockRebound(armed, quote(32, 21, '0.555000', '0.560000', {bids: [{price: '0.555000', quantity: 99}]}), bank);
  assert.equal(shallow.diagnostic.reason, 'insufficient_depth'); assert.equal(shallow.signal, null);
});
test('invalidation and observation gaps reset the hypothesis; recovery requires rebuilding history', () => {
  const {state, bank} = warm(), armed = shock(state, bank).state;
  const invalid = invalidateShockRebound(armed, bank, at(31)); assert.equal(invalid.armed, null); assert.equal(invalid.history.length, 0);
  const inFlight = advanceShockRebound(invalid, quote(32, 21, '0.555000', '0.560000', {requestStartedAt: at(30.9)}), bank);
  assert.equal(inFlight.diagnostic.reason, 'request_before_invalidation');
  assert.equal(advanceShockRebound(invalid, quote(33, 22, '0.555000', '0.560000'), bank).signal, null);
  const gap = advanceShockRebound(armed, quote(151, 21, '0.555000', '0.560000'), bank);
  assert.equal(gap.signal, null); assert.equal(gap.state.armed, null); assert.equal(gap.state.history.length, 1);
});
test('trend uses elapsed minutes and vetoes persistent declines under different sampling cadences', () => {
  const prices = increment => i => { const bid = .695 - i * increment; return [bid.toFixed(6), (bid + .005).toFixed(6)]; };
  for (const [step, count, increment] of [[60, 10, .005], [30, 20, .0025]]) {
    // Half-tick fixture would be invalid; every other fast observation retains the same valid tick.
    const p = increment === .0025 ? i => prices(.005)(Math.floor(i / 2)) : prices(increment);
    const {state, bank} = warm({}, step, count, p), r = shock(state, bank);
    assert.equal(r.signal, null); assert.equal(r.diagnostic.reason, 'trend_veto');
    assert.ok(r.diagnostic.trendPerMinute < -.004 && r.diagnostic.trendPerMinute > -.006);
  }
});
test('replay prefixes are deterministic and appending future quotes cannot alter a prior proposal', () => {
  const {state, bank} = warm();
  const events = [quote(30, 20, '0.545000', '0.550000'), quote(32, 21, '0.555000', '0.560000')];
  const replay = list => { let s = state; return list.map(b => { const r = advanceShockRebound(s, b, bank); s = r.state; return r; }); };
  const prefix = replay(events), extended = replay([...events, quote(40, 22, '0.605000', '0.610000')]);
  assert.deepEqual(prefix, extended.slice(0, 2));
  assert.equal(prefix[1].signal.signalId, replay(events)[1].signal.signalId);
});
test('quote recovery without sufficient executable room remains a rejected hypothesis', () => {
  const {state, bank} = warm(), armed = shock(state, bank).state;
  const r = advanceShockRebound(armed, quote(32, 21, '0.585000', '0.590000'), bank);
  assert.equal(r.signal, null); assert.equal(r.diagnostic.reason, 'insufficient_recovery_room');
});
test('collapse after arming disarms permanently until causal background is rebuilt', () => {
  const {state, bank} = warm(), armed = shock(state, bank).state;
  assert.equal(armed.armed.anchorMid, '0.597500');
  const collapse = advanceShockRebound(armed, quote(32, 21, '0.095000', '0.100000'), bank);
  assert.equal(collapse.signal, null); assert.equal(collapse.diagnostic.reason, 'maximum_shock_breached');
  assert.equal(collapse.diagnostic.totalShock, '0.500000');
  assert.equal(collapse.state.armed, null); assert.equal(collapse.state.history.length, 0);
  const bounce = advanceShockRebound(collapse.state, quote(34, 22, '0.105000', '0.110000'), bank);
  assert.equal(bounce.signal, null); assert.equal(bounce.state.armed, null);
  assert.equal(bounce.diagnostic.reason, 'insufficient_history');
  // Eight new samples without the required elapsed span cannot silently rearm.
  let s = bounce.state;
  for (let i = 0; i < 8; i++) {
    const r = advanceShockRebound(s, quote(36 + 2 * i, 23 + i, '0.105000', '0.110000'), bank);
    assert.equal(r.signal, null); assert.equal(r.state.armed, null); s = r.state;
  }
  // A fully NEW observed background can establish a distinct later hypothesis.
  for (let i = 1; i <= 15; i++) s = advanceShockRebound(s, quote(60 * i, 40 + i, '0.295000', '0.300000'), bank).state;
  const later = advanceShockRebound(s, quote(930, 100, '0.245000', '0.250000'), bank);
  assert.equal(later.diagnostic.reason, 'shock_armed'); assert.equal(later.state.armed.anchorMid, '0.297500');
  assert.ok(advanceShockRebound(later.state, quote(932, 101, '0.255000', '0.260000'), bank).signal);
});
test('maximum total shock includes exact boundary, delayed expiry and confirmation branch', () => {
  const {state, bank} = warm(), armed = shock(state, bank).state;
  const boundary = advanceShockRebound(armed, quote(32, 21, '0.495000', '0.500000'), bank);
  assert.equal(boundary.diagnostic.reason, 'new_trough');
  assert.ok(advanceShockRebound(boundary.state, quote(34, 22, '0.505000', '0.510000'), bank).signal);
  const expiredCollapse = advanceShockRebound(armed, quote(151, 21, '0.095000', '0.100000'), bank);
  assert.equal(expiredCollapse.diagnostic.reason, 'maximum_shock_breached');
  // Defensive replay fixture: even an existing excessively low trough must not
  // allow the confirmation branch to emit. The original anchor remains fixed.
  const oldTrough = {...armed, armed: {...armed.armed, troughBid: '0.095000', troughAt: zero + 31000}};
  const bounce = advanceShockRebound(oldTrough, quote(34, 22, '0.105000', '0.110000'), bank);
  assert.equal(bounce.signal, null); assert.equal(bounce.diagnostic.reason, 'maximum_shock_breached');
});
