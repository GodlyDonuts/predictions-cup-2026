import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {digest} from './model.mjs';
import {amount} from '../paper/fixed.mjs';
import {createEntryComparison, emptyEntryComparisonState, advanceEntryComparison, entryComparisonReport} from './entry-comparison.mjs';
const config = JSON.parse(fs.readFileSync(new URL('./config.json', import.meta.url)));
const policy = JSON.parse(fs.readFileSync(new URL('./dip-policy.json', import.meta.url)));
const base = Date.parse('2026-10-02T10:00:00.000Z'), stamp = ms => new Date(base + ms).toISOString();
const specs = [{marketId: '377', exchangeId: '1066'}, {marketId: '354', exchangeId: '1043'}, {marketId: '277', exchangeId: '966'}, {marketId: '305', exchangeId: '994'}];
const universe = {tournamentId: config.tournamentId, exchanges: specs.map(x => ({...x, option: 'YES'}))};
function quote(ms, sequence, marketId = '377', bid = .565, ask = .570, quantity = 10000, options = {}) {
  const spec = specs.find(x => x.marketId === marketId);
  return {endpoint: '/markets/' + marketId + '/orderbook?tournamentId=' + config.tournamentId + '&depth=20',
    receivedAt: stamp(ms), requestStartedAt: stamp(ms - 100), data: {contexts: [{type: 'tournament', tournament: {id: config.tournamentId},
      orderbook: {exchanges: [{exchangeId: spec.exchangeId, option: 'YES', asOf: {sequence, at: stamp(ms - 200)},
        bids: [{price: bid, quantity}], asks: [{price: ask, quantity}]}]}}]}, ...options};
}
function history(marketId = '377', ms = -1000, center = .60) {
  return {marketId, exchangeId: specs.find(x => x.marketId === marketId).exchangeId, tournamentId: config.tournamentId,
    receivedAt: stamp(ms), requestStartedAt: stamp(ms - 100), coverage: {complete: true},
    candles: Array.from({length: 8}, (_, i) => ({time: stamp(-36 * 60000 + i * 5 * 60000), vwap: center}))};
}
function setup({prefixBooks = [], allHistories = false} = {}) {
  const events = [{ms: -700000, event: {type: 'started'}}, ...prefixBooks.map(b => ({ms: Date.parse(b.receivedAt) - base, event: cycleEvent([b])})),
    ...(allHistories ? specs : [specs[0]]).map((x, i) => ({ms: -1000 + i, event: {type: 'market_history', history: history(x.marketId, -1000 + i)}}))];
  let previous = ''; const records = events.map(({ms, event}, i) => { const body = {sequence: i + 1, previous, at: stamp(ms), event: {clockAt: stamp(ms), ...event}, state: {fixture: true}};
    const r = {...body, hash: digest(body)}; previous = r.hash; return r; });
  const {bank, state: initial} = createEntryComparison({sourceRecords: records.length, sourceHash: 'a'.repeat(64), cutoff: stamp(0), config, universe, identities: {}, policy, codeIdentity: {fixture: 'b'.repeat(64)}});
  let state = records.reduce((s, r) => advanceEntryComparison(s, r, bank), initial);
  return {bank, records, get state() { return state; }, set state(value) { state = value; },
    push(ms, event) { const body = {sequence: records.length + 1, previous: records.at(-1).hash, at: stamp(ms), event: {clockAt: stamp(ms), ...event}, state: {fixture: true}};
      const r = {...body, hash: digest(body)}; records.push(r); state = advanceEntryComparison(state, r, bank); return state; },
    book(ms, seq, ...args) { const b = quote(ms, seq, ...args); return this.push(ms + 1, cycleEvent([b])); }};
}
function cycleEvent(books, extra = {}) { return {type: 'cycle', sourceSnapshots: books, supplementalDepth: [], accountExclusions: [], exclusionObservedAt: books[0]?.requestStartedAt || stamp(1), ...extra}; }
const wallet = (f, name = 'historical_dip') => f.state.strategies[name].wallet.strategies.mean_reversion;

test('frozen prefix warms references without trades; dip uses its genuine center once and fills only a subsequent request', () => {
  const f = setup({prefixBooks: [quote(-5000, 1)]}); assert.equal(wallet(f).fills.length, 0); assert.equal(f.state.strategies.historical_dip.uniqueObservations, 0);
  f.book(1000, 2); assert.equal(wallet(f).fills.length, 0); assert.equal(wallet(f).pending['1066:YES'].limit, '0.575000');
  assert.equal(f.state.strategies.historical_dip.decisions[1].reference, .6); assert.ok(f.state.strategies.historical_dip.decisions[1].nativeSignal.admitted);
  f.book(1500, 3); assert.equal(wallet(f).fills.length, 0);
  f.book(2200, 4); assert.equal(wallet(f).fills.length, 1); assert.equal(wallet(f).fills[0].price, '0.575000'); assert.equal(wallet(f).fills[0].quantity, 10);
  assert.equal(wallet(f, 'shock_rebound').fills.length, 0);
  const r = entryComparisonReport(f.state, f.bank, stamp(3000)); assert.equal(r.strategies.historical_dip.cumulativeBuyCost, '5.750000');
  assert.equal(r.strategies.historical_dip.recentFillAttribution[0].sourceHash, f.records.at(-1).hash); assert.equal(r.competitionOrders, 0);
});

test('public books on account-held markets remain observed with contamination labels', () => {
  const f = setup();
  for (const [ms, seq] of [[1000, 1], [3000, 2]]) f.push(ms + 1, cycleEvent([quote(ms, seq)], {accountExclusions: ['1066']}));
  assert.equal(wallet(f).fills.length, 1); assert.equal(f.state.counters.contaminatedBooks, 2);
  assert.equal(f.state.strategies.historical_dip.fillAttribution[0].contamination, 'account_exposure_or_orders');
  f.push(4001, cycleEvent([quote(4000, 3)], {accountExclusions: undefined}));
  assert.equal(f.state.counters.unknownContaminationBooks, 1); assert.equal(wallet(f).positions['1066:YES'].quantity, 10);
});

test('duplicate versions never fill or recount signal samples; conflicting same-sequence timestamp is invalid', () => {
  const f = setup(), first = quote(1000, 1); f.push(1001, cycleEvent([first]));
  const repeated = {...structuredClone(first), receivedAt: stamp(3000), requestStartedAt: stamp(2900)};
  f.push(3001, cycleEvent([repeated])); assert.equal(wallet(f).fills.length, 0); assert.equal(f.state.strategies.historical_dip.uniqueObservations, 1);
  f.book(5000, 1); assert.equal(f.state.counters.invalidBooks, 1); assert.equal(wallet(f).pending['1066:YES'], undefined);
  f.book(7000, 2); f.book(9000, 3); assert.equal(wallet(f).fills.length, 1);
});

test('source prefix replay is deterministic, immutable and refuses forged source or bank identity', () => {
  const f = setup(); const before = structuredClone(f.state); f.book(1000, 1); f.book(3000, 2);
  assert.equal(before.sourceSequence, f.bank.sourceRecords);
  const replay = f.records.reduce((s, r) => advanceEntryComparison(s, r, f.bank), emptyEntryComparisonState(f.bank));
  assert.deepEqual(replay, f.state);
  assert.throws(() => advanceEntryComparison(f.state, {...f.records.at(-1), sequence: 99}, f.bank), /continuity/);
  assert.throws(() => entryComparisonReport(f.state, {...f.bank, cutoff: stamp(1)}, stamp(5000)), /report/);
});

test('existing shock requires drop then later recovery then later fill; both wallets share actual fill assumptions', () => {
  const prefixBooks = Array.from({length: 10}, (_, i) => quote(-600000 + i * 60000, i + 1, '377', .595, .600));
  const f = setup({prefixBooks});
  f.book(30000, 20, '377', .545, .550); assert.equal(wallet(f, 'shock_rebound').intents, 0);
  f.book(32000, 21, '377', .555, .560); assert.equal(wallet(f, 'shock_rebound').fills.length, 0); assert.ok(wallet(f, 'shock_rebound').pending['1066:YES']);
  f.book(34000, 22, '377', .555, .560); const fills = wallet(f, 'shock_rebound').fills;
  assert.equal(fills.length, 1); assert.equal(fills[0].price, '0.565000'); assert.equal(fills[0].fee, '0.000000'); assert.equal(fills[0].depthFractionBps, 100);
  assert.equal(f.state.strategies.shock_rebound.acceptedSignals, 1);
});

test('exact native shock room boundary cannot be rejected by the executor repeating it in binary floats', () => {
  const prefixBooks = Array.from({length: 10}, (_, i) => quote(-600000 + i * 60000, i + 1, '377', .575, .580));
  const f = setup({prefixBooks}); f.book(30000, 20, '377', .540, .545); f.book(32000, 21, '377', .550, .555);
  assert.equal(f.state.strategies.shock_rebound.acceptedSignals, 1);
  assert.ok(wallet(f, 'shock_rebound').pending['1066:YES']);
  f.book(34000, 22, '377', .550, .555); assert.equal(wallet(f, 'shock_rebound').fills[0].price, '0.560000');
});

test('thin entry depth cancels eligibility, and three occupied markets prevent a fourth', () => {
  const f = setup({allHistories: true});
  f.book(1000, 1, '377', .565, .570, 99); assert.equal(wallet(f).intents, 0);
  for (const [i, marketId] of specs.map(x => x.marketId).entries()) { f.book(3000 + i * 4000, 2, marketId); f.book(5000 + i * 4000, 3, marketId); }
  assert.equal(Object.keys(wallet(f).positions).length, 3); assert.equal(wallet(f).positions['994:YES'], undefined);
});

test('outages and collector stop revoke buys while inventory remains; quiet book ages into a causal timeout', () => {
  const f = setup(); f.book(1000, 1); f.push(1500, {type: 'account_read_outage'});
  assert.equal(wallet(f).pending['1066:YES'], undefined);
  f.book(3000, 2); f.book(5000, 3); assert.equal(wallet(f).positions['1066:YES'].quantity, 10);
  f.push(6000, {type: 'stopped'}); assert.equal(wallet(f).positions['1066:YES'].quantity, 10);
  const old = quote(5000, 3), repeated = {...old, receivedAt: stamp(3606000), requestStartedAt: stamp(3605900)};
  f.push(3606001, cycleEvent([repeated])); assert.equal(wallet(f).fills.length, 1); assert.equal(wallet(f).pending['1066:YES'].reason, 'dip_timeout');
  f.book(3608000, 4, '377', .565, .570, 400);
  assert.equal(wallet(f).positions['1066:YES'].quantity, 6); assert.equal(wallet(f).fills.at(-1).quantity, 4);
  assert.equal(wallet(f).pending['1066:YES'].action, 'sell');
});

test('expired next-book entry is labeled and does not manufacture a historical fill', () => {
  const f = setup(); f.book(1000, 1); f.book(66000, 2);
  assert.equal(wallet(f).fills.length, 0); assert.equal(f.state.strategies.historical_dip.executionGateCounts.entry_expired, 1);
});

test('only a scoped exclusion diagnostic accompanying an accepted same-cycle book may be ignored', () => {
  const f = setup(), b = quote(1000, 1);
  f.push(1002, cycleEvent([b], {accountExclusions: ['1066'], supplementalDepth: [{marketId: '377', skipped: 'Exchange has real account holdings or open orders', at: stamp(1001)}]}));
  assert.ok(wallet(f).pending['1066:YES']); assert.equal(f.state.counters.exclusionDiagnosticsRetained, 1);
  f.push(2001, cycleEvent([], {accountExclusions: ['1066'], supplementalDepth: [{marketId: '377', skipped: 'Exchange has real account holdings or open orders', at: stamp(2000)}]}));
  assert.equal(wallet(f).pending['1066:YES'], undefined); assert.equal(f.state.counters.skippedBooks, 1);
  const g = setup(); g.push(1002, cycleEvent([b], {supplementalDepth: [{marketId: '377', skipped: 'Exchange has real account holdings or open orders', at: stamp(1001)}]}));
  assert.equal(wallet(g).pending['1066:YES'], undefined);
});

test('failed shock rebound cancels delayed entry and requires a rebuilt causal background', () => {
  const prefixBooks = Array.from({length: 10}, (_, i) => quote(-600000 + i * 60000, i + 1, '377', .595, .600));
  const f = setup({prefixBooks}); f.book(30000, 20, '377', .545, .550); f.book(32000, 21, '377', .555, .560);
  assert.ok(wallet(f, 'shock_rebound').pending['1066:YES']);
  f.book(34000, 22, '377', .535, .540);
  assert.equal(wallet(f, 'shock_rebound').fills.length, 0); assert.equal(wallet(f, 'shock_rebound').pending['1066:YES'], undefined);
  assert.equal(f.state.shocks['377'].state.history.length, 0);
  assert.equal(f.state.strategies.shock_rebound.gateCounts.proposal_risk_revoked, 1);
  f.book(36000, 23, '377', .555, .560); assert.equal(wallet(f, 'shock_rebound').fills.length, 0);
});

test('stale account context stays explicit uncertainty instead of becoming an unexposed label', () => {
  const f = setup(); f.push(200001, cycleEvent([quote(200000, 1)], {exclusionObservedAt: stamp(0)}));
  assert.equal(f.state.counters.unknownContaminationBooks, 1);
  assert.equal(f.state.strategies.historical_dip.lastSignals['377'].contamination, 'unknown_account_context');
});

test('cumulative buy budget never recycles realized proceeds; final affordable entry is quantity-limited', () => {
  const f = setup();
  for (let i = 0; i < 21; i++) {
    const ms = 1000 + i * 70000, seq = 1 + i * 4;
    f.book(ms, seq); f.book(ms + 2000, seq + 1);
    f.book(ms + 4000, seq + 2, '377', .590, .595); f.book(ms + 6000, seq + 3, '377', .590, .595);
    const spent = wallet(f).fills.filter(x => x.action === 'buy').reduce((n, x) => n + amount(x.price) * BigInt(x.quantity), 0n);
    assert.ok(spent <= 100000000n);
  }
  const buys = wallet(f).fills.filter(x => x.action === 'buy');
  assert.equal(buys.length, 18); assert.equal(buys.at(-1).quantity, 3);
  assert.equal(entryComparisonReport(f.state, f.bank, stamp(1500000)).strategies.historical_dip.cumulativeBuyCost, '99.475000');
  assert.ok(amount(wallet(f).realized) > 0n); assert.equal(Object.keys(wallet(f).positions).length, 0);
  for (const fill of wallet(f).fills) assert.ok(f.state.strategies.historical_dip.decisions[fill.intentId]);
});
