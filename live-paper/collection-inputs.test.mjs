import test from 'node:test';
import assert from 'node:assert/strict';
import {shockCollectionRisk} from './collection-inputs.mjs';
import {digest} from './model.mjs';
import {createSchedulerState, planBookReads} from './book-scheduler.mjs';

function fixture() {
  const universe = {tournamentId: 'fixture', exchanges: Array.from({length: 8}, (_, i) => ({marketId: String(i + 1), exchangeId: String(i + 101), option: 'YES'}))};
  const bank = {name: 'shock-rebound-shadow-v1', config: {tournamentId: universe.tournamentId}, markets: structuredClone(universe.exchanges.slice(0, 4))};
  const state = {bankHash: digest(bank), wallet: {books: Object.fromEntries(bank.markets.map(m => [m.exchangeId + ':YES', {...m}])), strategies: {mean_reversion: {positions: {'101:YES': {}}, pending: {'102:YES': {}}}}}};
  return {universe, bank, state};
}
test('shock contributes verified held/pending risk only and preserves official priority within four books', () => {
  const {universe, bank, state} = fixture(), risk = shockCollectionRisk(state, bank, universe);
  assert.deepEqual(risk, {heldMarketIds: ['1'], pendingMarketIds: ['2']});
  const scope = {universe, coreMarketIds: ['1', '2', '3', '4']};
  const plan = planBookReads(createSchedulerState(scope), scope, {...risk, priorityManagedMarketIds: ['8']});
  assert.equal(plan.reads.length, 4); assert.equal(plan.reads[0].marketId, '8'); assert.deepEqual(plan.entryMarketIds, []);
});
test('shock bank, tournament and every market/exchange mapping must match before scheduling', () => {
  for (const mutate of [f => f.state.bankHash = 'foreign', f => f.bank.name = 'other', f => f.bank.config.tournamentId = 'other', f => f.bank.markets[0].exchangeId = '999', f => f.bank.markets.push({...f.bank.markets[0]})]) {
    const f = fixture(); mutate(f); if (f.state.bankHash !== 'foreign') f.state.bankHash = digest(f.bank);
    assert.throws(() => shockCollectionRisk(f.state, f.bank, f.universe), /identity|scope/);
  }
  for (const mutate of [f => delete f.state.wallet.books['101:YES'], f => f.state.wallet.books['101:YES'].exchangeId = '999', f => f.state.wallet.books['101:YES'].marketId = '8']) {
    const f = fixture(); mutate(f); assert.throws(() => shockCollectionRisk(f.state, f.bank, f.universe), /Unmapped/);
  }
});
