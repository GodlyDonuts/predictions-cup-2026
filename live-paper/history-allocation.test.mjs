import test from 'node:test';
import assert from 'node:assert/strict';
import {digest} from './model.mjs';
import {historyAllocation} from './history-allocation.mjs';

const now = Date.parse('2026-10-02T10:00:00.000Z'), stamp = offset => new Date(now + offset).toISOString();
function fixture() {
  const universe = {hash: 'universe', tournamentId: 'fixture', exchanges: [{marketId: '1', exchangeId: '101', option: 'YES'}, {marketId: '2', exchangeId: '102', option: 'YES'}]};
  const bank = {universe, config: {tournamentId: 'fixture', slippageTicks: 1}, identities: {'1': {state: 'CA', office: 'Senate'}},
    policy: {minimumPrice: .05, maximumPrice: .95, maximumSpread: .02, minimumDip: .02, minimumRelativeDip: .03, minimumZ: 1.5,
      lookbackMs: 21600000, historyCacheAgeMs: 3600000, minimumSamples: 8, minimumSpanMs: 1800000, lastHistoricalObservationAgeMs: 2700000}};
  const history = {marketId: '1', exchangeId: '101', tournamentId: 'fixture', receivedAt: stamp(-120000), coverage: {complete: true},
    candles: Array.from({length: 8}, (_, i) => ({time: stamp(-2400000 + i * 300000), vwap: .6}))};
  const state = {bankHash: digest(bank), histories: {'101': history}, bookHistory: {}};
  const coverage = {universeHash: universe.hash, tournamentId: universe.tournamentId, quotes: {'101': {marketId: '1', exchangeId: '101', option: 'YES', receivedAt: stamp(-1000), bestAsk: .585, bestBid: .580, crossed: false}}};
  return {universe, bank, state, coverage, sources: [{state, bank, healthy: true}]};
}
test('history deadline uses both cache TTL and last traded candle, not HTTP success alone', () => {
  const f = fixture(), copy = JSON.stringify(f), row = historyAllocation(f.sources, f.universe, f.coverage, now).markets[0];
  assert.equal(row.referenceReady, true); assert.equal(row.validUntil, stamp(2460000));
  assert.equal(row.identityKnown, true); assert.equal(row.quotePlausible, true);
  assert.ok(row.opportunityGapMicros >= 9999 && row.opportunityGapMicros <= 10001);
  assert.equal(JSON.stringify(f), copy);
  f.state.histories['101'].receivedAt = stamp(-3500000);
  assert.equal(historyAllocation(f.sources, f.universe, f.coverage, now).markets[0].validUntil, stamp(100000));
  f.state.histories['101'].candles.forEach(c => c.time = new Date(Date.parse(c.time) - 2700000).toISOString());
  f.state.histories['101'].receivedAt = stamp(-1000);
  const stale = historyAllocation(f.sources, f.universe, f.coverage, now).markets[0];
  assert.equal(stale.referenceReady, false); assert.equal(stale.validUntil, null); assert.equal(stale.receivedAt, stamp(-1000));
});
test('missing history and unhealthy worker preserve static identity without inventing a reference', () => {
  const f = fixture(); f.sources[0].healthy = false;
  const rows = historyAllocation(f.sources, f.universe, f.coverage, now).markets;
  assert.equal(rows[0].identityKnown, true); assert.equal(rows[0].referenceReady, false);
  assert.equal(rows[0].opportunityGapMicros, null); assert.equal(rows[1].identityKnown, false); assert.equal(rows[1].receivedAt, null);
});
test('sparse references become due before their oldest required support leaves lookback', () => {
  const f = fixture();
  f.state.histories['101'].candles[0].time = stamp(-f.bank.policy.lookbackMs + 120000);
  const row = historyAllocation(f.sources, f.universe, f.coverage, now).markets[0];
  assert.equal(row.referenceReady, true); assert.equal(row.validUntil, stamp(120000));
});
test('stale, future, foreign, crossed and costly bulk quotes cannot gain opportunity priority', () => {
  for (const change of [f => f.coverage.tournamentId = 'foreign', f => f.coverage.universeHash = 'foreign',
    f => f.coverage.quotes['101'].receivedAt = stamp(-600001), f => f.coverage.quotes['101'].receivedAt = stamp(1),
    f => f.coverage.quotes['101'].marketId = '2', f => f.coverage.quotes['101'].crossed = true,
    f => f.coverage.quotes['101'].bestBid = .56]) {
    const f = fixture(); change(f); const row = historyAllocation(f.sources, f.universe, f.coverage, now).markets[0];
    assert.equal(row.quotePlausible, false); assert.equal(row.referenceReady, true);
  }
});
test('bank and historical scope mutations are rejected and book fallback has its own deadline', () => {
  let f = fixture(); f.state.bankHash = 'changed'; assert.throws(() => historyAllocation(f.sources, f.universe, f.coverage, now), /identity/);
  f = fixture(); f.state.histories['101'].marketId = '2'; assert.throws(() => historyAllocation(f.sources, f.universe, f.coverage, now), /scope/);
  f = fixture(); delete f.state.histories['101'];
  f.state.bookHistory['101'] = Array.from({length: 8}, (_, i) => ({at: stamp(-2400000 + i * 300000), receivedAt: stamp(-2399000 + i * 300000), mid: .6}));
  const row = historyAllocation(f.sources, f.universe, f.coverage, now).markets[0];
  assert.equal(row.referenceReady, true); assert.equal(row.receivedAt, null); assert.equal(row.validUntil, stamp(2400000));
});
