import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {digest} from './model.mjs';
import {amount} from '../paper/fixed.mjs';
import {createLimitEntry, emptyLimitEntryState, advanceLimitEntry, limitEntryReport} from './limit-entry.mjs';

const config = JSON.parse(fs.readFileSync(new URL('./config.json', import.meta.url))),
  policy = JSON.parse(fs.readFileSync(new URL('./dip-policy.json', import.meta.url))),
  base = Date.parse('2026-10-02T10:00:00.000Z'), stamp = ms => new Date(base + ms).toISOString(),
  specs = [{marketId: '377', exchangeId: '1066'}, {marketId: '354', exchangeId: '1043'}, {marketId: '277', exchangeId: '966'}, {marketId: '305', exchangeId: '994'}],
  universe = {tournamentId: config.tournamentId, exchanges: specs.map(x => ({...x, option: 'YES'}))};
function quote(ms, sequence, {marketId = '377', bid = .565, ask = .570, quantity = 10000, bids, asks, ...rest} = {}) {
  const spec = specs.find(x => x.marketId === marketId);
  return {endpoint: '/markets/' + marketId + '/orderbook?tournamentId=' + config.tournamentId + '&depth=20',
    receivedAt: stamp(ms), requestStartedAt: stamp(ms - 100), data: {contexts: [{type: 'tournament', tournament: {id: config.tournamentId},
      orderbook: {exchanges: [{exchangeId: spec.exchangeId, option: 'YES', asOf: {sequence, at: stamp(ms - 200)},
        bids: bids || [{price: bid, quantity}], asks: asks || [{price: ask, quantity}]}]}}]}, ...rest};
}
function history(marketId = '377', ms = -1000, center = .6) {
  return {marketId, exchangeId: specs.find(x => x.marketId === marketId).exchangeId, tournamentId: config.tournamentId,
    receivedAt: stamp(ms), requestStartedAt: stamp(ms - 100), coverage: {complete: true},
    candles: Array.from({length: 8}, (_, i) => ({time: stamp(-36 * 60000 + i * 5 * 60000), vwap: center}))};
}
function cycle(books, extra = {}) { return {type: 'cycle', sourceSnapshots: books, supplementalDepth: [], accountExclusions: [],
  exclusionObservedAt: books[0]?.requestStartedAt || stamp(1), ...extra}; }
function setup({prefixBooks = [], allHistories = false} = {}) {
  const records = [], availability = [];
  function record(ms, event) { const body = {sequence: records.length + 1, previous: records.at(-1)?.hash || '', at: stamp(ms),
    event: {clockAt: stamp(ms), ...event}, state: {fixture: true}}; const r = {...body, hash: digest(body)}; records.push(r); return r; }
  record(-700000, {type: 'started'});
  for (const b of prefixBooks) record(Date.parse(b.receivedAt) - base + 1, cycle([b]));
  for (const [i, spec] of (allHistories ? specs : [specs[0]]).entries()) record(-1000 + i, {type: 'market_history', history: history(spec.marketId, -1000 + i)});
  const created = createLimitEntry({sourceRecords: records.length, sourceHash: 'a'.repeat(64), cutoff: stamp(0), config,
    universe, identities: {}, policy, codeIdentity: {fixture: 'b'.repeat(64)}});
  let state = created.state;
  for (const r of records) { availability.push(stamp(0)); state = advanceLimitEntry(state, r, created.bank, {availableAt: stamp(0)}); }
  return {bank: created.bank, records, availability, get state() { return state; }, set state(value) { state = value; },
    push(ms, event, hostMs = ms) { const r = record(ms, event); availability.push(stamp(hostMs)); state = advanceLimitEntry(state, r, created.bank, {availableAt: stamp(hostMs)}); return state; },
    book(ms, sequence, options = {}, hostMs = ms + 1) { return this.push(ms + 1, cycle([quote(ms, sequence, options)]), hostMs); }};
}
const variant = (f, name = 'immediate_dip') => f.state.strategies[name], wallet = (f, name) => variant(f, name).wallet.strategies.mean_reversion;
const pending = (f, name) => wallet(f, name).pending['1066:YES'];

test('prefix seeds history only; both arms use the same immediate actual price cap and later quote evidence', () => {
  const f = setup({prefixBooks: [quote(-5000, 1)]}); assert.equal(wallet(f).fills.length, 0);
  f.book(1000, 2); assert.equal(pending(f).limit, '0.570000'); assert.equal(pending(f).createdAt, stamp(1001));
  assert.equal(pending(f, 'price_protected_hybrid').limit, pending(f).limit);
  f.book(2000, 3); assert.equal(wallet(f).fills.length, 0); // request started before decision + latency
  f.book(2200, 4); assert.equal(wallet(f).fills[0].price, '0.570000');
  assert.deepEqual(wallet(f).fills, wallet(f, 'price_protected_hybrid').fills);
  assert.equal(wallet(f).fills[0].at, stamp(2200)); assert.equal(wallet(f).fills[0].fee, '0.000000');
});

test('hybrid conditionally parks below a nonqualifying ask; bid touch alone never fills', () => {
  const f = setup(); f.book(1000, 1, {bid: .570, ask: .580});
  assert.equal(pending(f), undefined); assert.equal(pending(f, 'price_protected_hybrid').limit, '0.575000');
  assert.equal(variant(f, 'price_protected_hybrid').actualNativeAdmissions, 0);
  assert.equal(variant(f, 'price_protected_hybrid').conditionalLimitAdmissions, 1);
  f.book(3000, 2, {bid: .575, ask: .580}); assert.equal(wallet(f, 'price_protected_hybrid').fills.length, 0);
  f.book(5000, 3, {bid: .570, ask: .575});
  assert.equal(wallet(f, 'price_protected_hybrid').fills[0].price, '0.575000');
  assert.equal(wallet(f).fills.length, 0); assert.equal(pending(f).reason, 'immediate');
});

test('committed orders retain adverse crossings even after native discount and spread gates fail', () => {
  const f = setup(); f.book(1000, 1, {bid: .570, ask: .580});
  f.book(3000, 2, {bid: .400, ask: .450});
  const st = wallet(f, 'price_protected_hybrid'); assert.equal(st.fills[0].price, '0.455000');
  assert.equal(variant(f, 'price_protected_hybrid').lastSignals['377'].currentNative.admitted, false);
  assert.equal(st.positions['1066:YES'].quantity, 10); assert.equal(st.pending['1066:YES'].reason, 'stop_loss');
});

test('entry price never moves upward and source reference is immutable', () => {
  const f = setup(); f.book(1000, 1, {bid: .570, ask: .580});
  const first = structuredClone(variant(f, 'price_protected_hybrid').decisions[1]);
  f.book(3000, 2, {bid: .580, ask: .585});
  assert.equal(pending(f, 'price_protected_hybrid').limit, '0.575000');
  assert.deepEqual(variant(f, 'price_protected_hybrid').decisions[1], first);
});

test('expiry is strict; missing later observation and observed nonmarketability have separate outcomes', () => {
  const f = setup(); f.book(1000, 1); f.book(31001, 2); assert.equal(wallet(f).fills.length, 0);
  assert.equal(variant(f).outcomes[1].censoring, 'no_independent_in_window_quote');
  const g = setup(); g.book(1000, 1); g.book(3000, 2, {bid: .570, ask: .575}); g.push(32000, cycle([]));
  assert.equal(wallet(g).fills.length, 0); assert.equal(variant(g).outcomes[1].censoring, 'observed_without_full_execution');
  assert.equal(limitEntryReport(g.state, g.bank, stamp(33000)).strategies.immediate_dip.entryOutcomes.expiredObserved, 1);
});

test('late host consumption can reconcile an in-window receipt, never create a backdated fresh decision', () => {
  const f = setup(); f.book(1000, 1); f.book(3000, 2, {}, 60000);
  assert.equal(wallet(f).fills[0].at, stamp(3000)); assert.equal(wallet(f).fills[0].evidenceAvailableAt, stamp(60000));
  assert.equal(f.state.counters.staleAtDecisionBooks, 1);
  const g = setup(); g.book(1000, 1, {}, 60000); assert.equal(wallet(g).intents, 0);
  const h = setup(); h.book(1000, 1, {}, 15000); assert.equal(pending(h).createdAt, stamp(15000));
  h.book(16000, 2, {}, 16001); assert.equal(wallet(h).fills.length, 0);
  h.book(17000, 3); assert.equal(wallet(h).fills.length, 1);
});

test('intermediate backlog records cannot expire an order before its captured in-window receipt is replayed', () => {
  const f = setup(); f.book(1000, 1); f.push(2000, {type: 'diagnostic'}, 60000);
  assert.ok(pending(f)); assert.equal(limitEntryReport(f.state, f.bank, stamp(60000)).strategies.immediate_dip.pending[0].clockExpiredAwaitingSource, true);
  f.book(3000, 2, {}, 60000); assert.equal(wallet(f).fills[0].at, stamp(3000));
  assert.equal(variant(f).outcomes[1].status, 'filled');
});

test('noncycle publication beyond expiry cannot erase later-published in-window receipt evidence', () => {
  const f = setup(); f.book(1000, 1); f.push(40000, {type: 'diagnostic'}, 60000);
  assert.ok(pending(f)); f.push(41000, cycle([quote(3000, 2)]), 60000);
  assert.equal(wallet(f).fills[0].at, stamp(3000)); assert.equal(variant(f).outcomes[1].status, 'filled');
});

test('all in-window cycle receipts resolve before expiry or a newly available risk cancellation', () => {
  const f = setup(); f.book(1000, 1);
  f.push(33000, cycle([quote(29000, 2, {bid: .570, ask: .575}), quote(30000, 3)]));
  assert.equal(wallet(f).fills[0].at, stamp(30000)); assert.equal(variant(f).outcomes[1].status, 'filled');
  const g = setup(); g.book(1000, 1);
  g.push(10000, cycle([quote(3000, 2, {bid: .400, ask: .450, quantity: 300}), quote(5000, 3, {bid: .390, ask: .400, quantity: 700})]));
  assert.equal(wallet(g).positions['1066:YES'].quantity, 10); assert.equal(wallet(g).fills.length, 2);
  assert.equal(pending(g).action, 'sell'); assert.equal(pending(g).createdAt, stamp(10000));
});

test('only latest same-market quote may initiate a newly available decision', () => {
  const f = setup(); f.push(5000, cycle([quote(1000, 1), quote(3000, 2, {bid: .610, ask: .615})]));
  assert.equal(wallet(f).intents, 0); assert.equal(wallet(f, 'price_protected_hybrid').intents, 0);
});

test('a later invalid or skipped receipt cannot resurrect an earlier decision from the same cycle', () => {
  const f = setup(), bad = quote(3000, 2); bad.data.contexts[0].orderbook.exchanges[0].exchangeId = '999';
  f.push(5000, cycle([quote(1000, 1), bad])); assert.equal(wallet(f).intents, 0); assert.equal(f.state.counters.invalidBooks, 1);
  const g = setup(); g.push(5000, cycle([quote(1000, 1)], {supplementalDepth: [{marketId: '377', skipped: 'read_failed', at: stamp(3000)}]}));
  assert.equal(wallet(g).intents, 0); g.book(7000, 2); assert.ok(pending(g));
});

test('independent versions, stable same-sequence content, and request barriers prevent reused liquidity', () => {
  const f = setup(); f.book(1000, 1); const repeated = {...quote(1000, 1), receivedAt: stamp(3000), requestStartedAt: stamp(2900)};
  f.push(3001, cycle([repeated])); assert.equal(wallet(f).fills.length, 0);
  f.book(5000, 1); assert.equal(f.state.counters.invalidBooks, 1); assert.equal(pending(f), undefined);
  f.book(7000, 2); f.book(9000, 3, {quantity: 300}); assert.equal(wallet(f).positions['1066:YES'].quantity, 3);
  const again = {...quote(9000, 3, {quantity: 300}), receivedAt: stamp(11000), requestStartedAt: stamp(10900)};
  f.push(11001, cycle([again])); assert.equal(wallet(f).positions['1066:YES'].quantity, 3);
});

test('eligible depth is walked per level, price caps bind, and partial orders keep exact residuals', () => {
  const f = setup(); f.book(1000, 1);
  f.book(3000, 2, {bid: .550, asks: [{price: .560, quantity: 200}, {price: .565, quantity: 300}, {price: .570, quantity: 100}, {price: .575, quantity: 10000}]});
  const st = wallet(f); assert.equal(st.positions['1066:YES'].quantity, 6); assert.equal(pending(f).quantity, 4);
  assert.deepEqual(st.fills.map(x => [x.quantity, x.price]), [[2, '0.565000'], [3, '0.570000'], [1, '0.570000']]);
  assert.equal(st.positions['1066:YES'].cost, '3.410000');
  f.book(5000, 3); assert.equal(st.fills.length, 3); // old immutable input remains unchanged
  assert.equal(wallet(f).positions['1066:YES'].quantity, 10); assert.equal(pending(f), undefined);
});

test('risk exits latch across price gaps, partial depth and rebound, with no same-book sell', () => {
  const f = setup(); f.book(1000, 1); f.book(3000, 2); f.book(5000, 3, {bid: .530, ask: .535});
  assert.equal(pending(f).reason, 'stop_loss'); assert.equal(wallet(f).fills.length, 1);
  f.book(7000, 4, {bid: .500, ask: .505}); assert.equal(wallet(f).fills.length, 1); assert.equal(pending(f).limit, '0.495000');
  f.book(9000, 5, {bid: .500, ask: .505, quantity: 300}); assert.equal(wallet(f).positions['1066:YES'].quantity, 7);
  assert.equal(wallet(f).closedTrades, 0);
  assert.ok(variant(f).exitObligations['1066:YES']);
  f.book(11000, 6, {bid: .550, ask: .555}); assert.equal(wallet(f).positions['1066:YES'], undefined);
  assert.equal(variant(f).exitObligations['1066:YES'], undefined); assert.ok(amount(wallet(f).realized) < 0n);
  assert.equal(wallet(f).closedTrades, 1);
});

test('selling one position across multiple levels counts one losing roundtrip', () => {
  const f = setup(); f.book(1000, 1); f.book(3000, 2); f.book(5000, 3, {bid: .530, ask: .535});
  f.book(7000, 4, {ask: .555, bids: [{price: .550, quantity: 300}, {price: .540, quantity: 300}, {price: .530, quantity: 400}]});
  assert.equal(wallet(f).fills.filter(x => x.action === 'sell').length, 3);
  assert.equal(wallet(f).closedTrades, 1); assert.equal(wallet(f).realized, '-0.360000');
  assert.equal(limitEntryReport(f.state, f.bank, stamp(7002)).strategies.immediate_dip.closedTrades, 1);
});

test('quiet authoritative receipt triggers holding timeout but cannot reuse depth; outage retains inventory', () => {
  const f = setup(); f.book(1000, 1); f.book(3000, 2); f.push(5000, {type: 'account_read_outage'});
  assert.equal(wallet(f).positions['1066:YES'].quantity, 10);
  const repeated = {...quote(3000, 2), receivedAt: stamp(3604000), requestStartedAt: stamp(3603900)};
  f.push(3604001, cycle([repeated])); assert.equal(pending(f).reason, 'holding_timeout'); assert.equal(wallet(f).fills.length, 1);
  f.book(3606000, 3, {quantity: 400}); assert.equal(wallet(f).positions['1066:YES'].quantity, 6);
  const r = limitEntryReport(f.state, f.bank, stamp(3630000)); assert.equal(r.strategies.immediate_dip.liquidationPnl, null);
  assert.equal(r.strategies.immediate_dip.positions[0].unsoldShares, 6);
});

test('three positions/entries cap includes parked orders, and thin evidence creates no order', () => {
  const f = setup({allHistories: true}); f.book(1000, 1, {quantity: 99}); assert.equal(wallet(f).intents, 0);
  for (const [i, spec] of specs.entries()) f.book(3000 + 2000 * i, i + 2, {marketId: spec.marketId, bid: .570, ask: .580});
  assert.equal(Object.keys(wallet(f, 'price_protected_hybrid').pending).length, 3);
  assert.equal(wallet(f, 'price_protected_hybrid').pending['994:YES'], undefined);
});

test('public account exposure remains labeled; unpaired exclusion invalidates orders, not positions', () => {
  const f = setup(), b = quote(1000, 1);
  f.push(1002, cycle([b], {accountExclusions: ['1066'], supplementalDepth: [{marketId: '377', skipped: 'Exchange has real account holdings or open orders', at: stamp(1001)}]}));
  assert.ok(pending(f)); assert.equal(variant(f).contaminationCounts.account_exposure_or_orders, 1);
  f.push(2000, cycle([], {accountExclusions: ['1066'], supplementalDepth: [{marketId: '377', skipped: 'Exchange has real account holdings or open orders', at: stamp(1999)}]}));
  assert.equal(pending(f), undefined);
});

test('exact cash and cumulative purchases do not recycle proceeds after profitable exits', () => {
  const f = setup(); let ms = 1000, sequence = 0;
  for (let i = 0; i < 20; i++) {
    f.push(ms, {type: 'market_history', history: history('377', ms - 1)});
    f.book(ms + 1000, ++sequence); f.book(ms + 3000, ++sequence);
    f.book(ms + 5000, ++sequence, {bid: .590, ask: .595}); f.book(ms + 7000, ++sequence, {bid: .590, ask: .595}); ms += 70000;
  }
  const st = wallet(f), cost = st.fills.filter(x => x.action === 'buy').reduce((n, f) => n + amount(f.price) * BigInt(f.quantity), 0n);
  assert.equal(cost, 99750000n); assert.ok(amount(st.realized) > 0n);
  assert.equal(st.positions['1066:YES'], undefined); assert.equal(pending(f), undefined);
});

test('replay restores exact availability and state; tampered lineage, clock and banks fail closed', () => {
  const f = setup(); f.book(1000, 1); f.book(3000, 2, {}, 60000);
  const replay = f.records.reduce((s, r, i) => advanceLimitEntry(s, r, f.bank, {availableAt: f.availability[i]}), emptyLimitEntryState(f.bank));
  assert.deepEqual(replay, f.state);
  assert.throws(() => advanceLimitEntry(f.state, {...f.records.at(-1), hash: '0'.repeat(64)}, f.bank, {availableAt: stamp(60000)}), /continuity/);
  assert.throws(() => limitEntryReport(f.state, {...f.bank, cutoff: stamp(1)}, stamp(61000)), /report/);
  const g = setup(), body = {sequence: g.records.length + 1, previous: g.records.at(-1).hash, at: stamp(1001), event: {type: 'heartbeat'}};
  assert.throws(() => advanceLimitEntry(g.state, {...body, hash: digest(body)}, g.bank), /availability/);
});
