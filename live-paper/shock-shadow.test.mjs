import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createShock, advanceShock, runShock, shockReport} from './shock-shadow.mjs';
import {verifySource} from './ml-shadow.mjs';
import {digest} from './model.mjs';
import {Journal} from './runtime.mjs';

const zero = Date.parse('2026-10-02T10:00:00Z'), at = s => new Date(zero + s * 1000).toISOString();
const base = JSON.parse(fs.readFileSync(new URL('./config.json', import.meta.url)));
const config = {...base, marketIds: ['377'], feeBps: 0, maxExposure: '100.000000', cashReserve: '99900.000000'};
const universe = {tournamentId: config.tournamentId, exchanges: [{marketId: '377', exchangeId: '1066', option: 'YES'}]};
function cycle(t, seq, bid = .595, ask = .600, quantity = 10000, extra = {}) {
  return {type: 'cycle', clockAt: at(t), accountExclusions: [], sourceSnapshots: [{endpoint: '/markets/377/orderbook?tournamentId=' + config.tournamentId,
    receivedAt: at(t), requestStartedAt: at(t - .1), data: {contexts: [{type: 'tournament', tournament: {id: config.tournamentId}, orderbook: {exchanges: [{exchangeId: '1066', option: 'YES',
      asOf: {at: at(t), sequence: seq}, bids: [{price: bid, quantity}], asks: [{price: ask, quantity}]}]}}]}}], ...extra};
}
function source(events) {
  let previous = '';
  return events.map((event, i) => { const body = {sequence: i + 1, previous, at: event.clockAt, event}; previous = digest(body); return JSON.stringify({...body, hash: previous}) + '\n'; }).join('');
}
const prefix = () => [...Array.from({length: 10}, (_, i) => cycle(-600 + i * 60, i + 1)), {type: 'cutoff', clockAt: at(0)}];
function advance(s, events, bank) { return advanceShock(s, verifySource(source(events)), bank); }

test('frozen prefix seeds observations but never fills; rebound fills only a later fresh snapshot and respects proposal depth', () => {
  const events = prefix(), {bank, state} = createShock(source(events), config, universe);
  assert.equal(state.wallet.observations, 0); assert.equal(state.candidates['377'].history.length, 10);
  events.push(cycle(30, 20, .545, .550), cycle(32, 21, .555, .560, 300));
  let s = advance(state, events, bank), st = s.wallet.strategies.mean_reversion;
  assert.equal(st.fills.length, 0); assert.equal(st.pending['1066:YES'].quantity, 3);
  events.push(cycle(34, 22, .555, .560)); s = advance(s, events, bank); st = s.wallet.strategies.mean_reversion;
  assert.equal(st.fills.length, 1); assert.equal(st.fills[0].quantity, 3); assert.equal(st.fills[0].price, '0.565000'); assert.equal(st.fills[0].fee, '0.000000');
  events.push(cycle(40, 23, .590, .595)); s = advance(s, events, bank); assert.equal(s.wallet.strategies.mean_reversion.fills.length, 1);
  events.push(cycle(42, 24, .590, .595)); s = advance(s, events, bank); st = s.wallet.strategies.mean_reversion;
  assert.equal(st.fills.length, 2); assert.equal(st.fills[1].price, '0.585000'); assert.equal(st.positions['1066:YES'], undefined);
  assert.equal(shockReport(s, bank, at(42)).cumulativeBuyCost, '1.695000'); assert.equal(st.realized, '0.060000');
  assert.deepEqual(advance(s, events, bank), s);
});
test('original maximum entry price remains binding after confirmation and outage revokes pending buys', () => {
  const events = prefix(), {bank, state} = createShock(source(events), config, universe);
  events.push(cycle(30, 20, .545, .550), cycle(32, 21, .555, .560)); let s = advance(state, events, bank);
  events.push(cycle(34, 22, .560, .565)); s = advance(s, events, bank);
  assert.equal(s.wallet.strategies.mean_reversion.fills.length, 0); assert.deepEqual(s.wallet.strategies.mean_reversion.pending, {});
  const fresh = prefix(); fresh.push(cycle(30, 20, .545, .550), cycle(32, 21, .555, .560)); s = advance(state, fresh, bank);
  fresh.push({type: 'account_read_outage', clockAt: at(33)}, cycle(34, 22, .555, .560)); s = advance(s, fresh, bank);
  assert.equal(s.wallet.strategies.mean_reversion.fills.length, 0); assert.deepEqual(s.proposals, {});
});
test('later thin depth produces partial fills and stale held inventory remains unsold', () => {
  const events = prefix(), {bank, state} = createShock(source(events), config, universe);
  events.push(cycle(30, 20, .545, .550), cycle(32, 21, .555, .560), cycle(34, 22, .555, .560, 300));
  let s = advance(state, events, bank); assert.equal(s.wallet.strategies.mean_reversion.positions['1066:YES'].quantity, 3);
  events.push(cycle(40, 23, .590, .595), cycle(42, 24, .590, .595, 200)); s = advance(s, events, bank);
  assert.equal(s.wallet.strategies.mean_reversion.positions['1066:YES'].quantity, 1);
  const stale = shockReport(s, bank, at(400)).strategy;
  assert.equal(stale.estimatedLiquidationEquity, null); assert.equal(stale.positions[0].unsoldShares, 1);
  events.push(cycle(44, 25, .590, .595, 100)); s = advance(s, events, bank);
  assert.equal(s.wallet.strategies.mean_reversion.positions['1066:YES'], undefined);
  assert.equal(s.wallet.strategies.mean_reversion.realized, '0.060000');
});
test('confirmed proposal cannot fill a later collapse or unchanged quote version', () => {
  const events = prefix(), {bank, state} = createShock(source(events), config, universe);
  events.push(cycle(30, 20, .545, .550), cycle(32, 21, .555, .560)); const confirmed = advance(state, events, bank);
  const collapse = advance(confirmed, [...events, cycle(34, 22, .095, .100)], bank);
  assert.equal(collapse.wallet.strategies.mean_reversion.fills.length, 0); assert.deepEqual(collapse.wallet.strategies.mean_reversion.pending, {});
  assert.equal(collapse.signals['377'].reason, 'proposal_risk_revoked'); assert.equal(collapse.candidates['377'].history.length, 0);
  const failedRebound = advance(confirmed, [...events, cycle(34, 22, .535, .540)], bank);
  assert.equal(failedRebound.wallet.strategies.mean_reversion.fills.length, 0); assert.deepEqual(failedRebound.wallet.strategies.mean_reversion.pending, {});
  assert.equal(failedRebound.signals['377'].reason, 'proposal_risk_revoked'); assert.equal(failedRebound.candidates['377'].history.length, 0);
  const repeated = cycle(34, 21, .555, .560); repeated.sourceSnapshots[0].data.contexts[0].orderbook.exchanges[0].asOf.at = at(32);
  const same = advance(confirmed, [...events, repeated], bank); assert.equal(same.wallet.strategies.mean_reversion.fills.length, 0);
  assert.equal(same.signals['377'].reason, 'no_independent_quote');
});
test('fresh unchanged receipts trigger held timeout without inventing liquidity until a later independent quote', () => {
  const events = prefix(), {bank, state} = createShock(source(events), config, universe);
  events.push(cycle(30, 20, .545, .550), cycle(32, 21, .555, .560), cycle(34, 22, .555, .560)); let s = advance(state, events, bank);
  const quiet = t => { const e = cycle(t, 22, .555, .560); e.sourceSnapshots[0].data.contexts[0].orderbook.exchanges[0].asOf.at = at(34); return e; };
  events.push(quiet(700)); s = advance(s, events, bank);
  assert.equal(s.wallet.strategies.mean_reversion.pending['1066:YES'].reason, 'dip_timeout'); assert.equal(s.wallet.strategies.mean_reversion.fills.length, 1);
  events.push(quiet(702)); s = advance(s, events, bank); assert.equal(s.wallet.strategies.mean_reversion.fills.length, 1);
  events.push(cycle(704, 23, .555, .560)); s = advance(s, events, bank);
  assert.equal(s.wallet.strategies.mean_reversion.fills.length, 2); assert.equal(s.wallet.strategies.mean_reversion.positions['1066:YES'], undefined);
});
test('cumulative purchase budget does not reset after profitable exits', () => {
  const events = prefix(), {bank, state} = createShock(source(events), config, universe);
  let s = state, seq = 20;
  for (let episode = 0; episode < 22; episode++) {
    const t = episode * 1200;
    if (episode) for (let k = t - 600; k < t; k += 60) events.push(cycle(k, seq++));
    events.push(cycle(t + 30, seq++, .545, .550), cycle(t + 32, seq++, .555, .560), cycle(t + 34, seq++, .555, .560), cycle(t + 40, seq++, .590, .595), cycle(t + 42, seq++, .590, .595));
    s = advance(s, events, bank);
  }
  const r = shockReport(s, bank, at(26000)); assert.ok(Number(r.cumulativeBuyCost) <= 100); assert.ok(Number(r.cumulativeBuyCost) > 99);
  assert.deepEqual(s.wallet.strategies.mean_reversion.positions, {}); assert.ok(Number(r.strategy.realizedPnl) > 0);
});
test('source-prefix tampering, config changes, wrong exchange and account exclusions fail closed', () => {
  const events = prefix(), {bank, state} = createShock(source(events), config, universe);
  const changed = structuredClone(events); changed[0].clockAt = at(-601);
  assert.throws(() => advance(state, changed, bank), /source or bank changed/);
  assert.throws(() => advance(state, events, {...bank, cutoff: at(1)}), /source or bank changed/);
  events.push(cycle(30, 20, .545, .550), cycle(32, 21, .555, .560, 10000, {accountExclusions: ['1066']}));
  const s = advance(state, events, bank); assert.equal(s.signals['377'].reason, 'invalid_or_excluded_book'); assert.deepEqual(s.wallet.strategies.mean_reversion.pending, {});
  const wrong = prefix(); const foreign = cycle(30, 20, .545, .550); foreign.sourceSnapshots[0].data.contexts[0].orderbook.exchanges[0].exchangeId = '1067'; wrong.push(foreign);
  assert.equal(advance(state, wrong, bank).signals['377'].reason, 'invalid_or_excluded_book');
});
test('worker replays own durable journal, ignores edited state projection, preserves collector bytes and refuses reinitialization', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'shock-shadow-test-'));
  const allUniverse = {...universe, exchanges: [...universe.exchanges, {marketId: '354', exchangeId: '1043', option: 'YES'}, {marketId: '277', exchangeId: '966', option: 'YES'}, {marketId: '305', exchangeId: '994', option: 'YES'}]};
  try {
    const raw = source(prefix()); fs.writeFileSync(path.join(dir, 'journal.jsonl'), raw);
    fs.writeFileSync(path.join(dir, 'dip-responsive-bank.json'), JSON.stringify({config: {...config, initialCash: '100000.000000'}, universe: allUniverse}));
    const first = await runShock({dir, initialize: true, once: true, workspaceDir: dir, now: () => zero + 1000});
    assert.equal(first.bank.cutoff, at(1));
    fs.writeFileSync(path.join(dir, 'shock-state.json'), '{}');
    const second = await runShock({dir, once: true, workspaceDir: dir}); assert.deepEqual(second.state, first.state);
    assert.equal(new Journal(path.join(dir, 'shock-journal.jsonl')).sequence, 1); assert.equal(fs.readFileSync(path.join(dir, 'journal.jsonl'), 'utf8'), raw);
    await assert.rejects(runShock({dir, initialize: true, once: true, workspaceDir: dir}), /already exists/);
    fs.writeFileSync(path.join(dir, 'SHOCK_STOP'), 'test'); await assert.rejects(runShock({dir, once: true, workspaceDir: dir}), /STOP/);
    assert.equal(fs.existsSync(path.join(dir, 'shock-shadow.lock')), false);
  } finally { fs.rmSync(dir, {recursive: true, force: true}); }
});
