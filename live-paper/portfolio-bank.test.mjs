import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {digest, initialState, stepForecast} from './model.mjs';
import {trainBank} from './ml-bank.mjs';
import {verifySource, advanceShadow, requireSourceMetadata} from './ml-shadow-v2.mjs';
import {advanceShadow as originalShadow} from './ml-shadow.mjs';
import {trainPortfolioBank, chronologicalPartitions, portfolioPoint, rankPortfolios} from './portfolio-bank.mjs';
import {PORTFOLIO_PLAN, modelMargins, validateCandidatePlan} from './candidate-plan.mjs';

const c = {...JSON.parse(fs.readFileSync(new URL('./config.json', import.meta.url))), marketIds: ['377'], feeBps: 0};
const epoch = Date.parse('2026-10-01T16:00:00Z'), at = n => new Date(epoch + n * 30000).toISOString();
function capture(n = 300, {flat = false, shiftFrom = Infinity, invalidFrom = Infinity, futureAt = -1, whitespace = false} = {}) {
  let previous = '';
  return Array.from({length: n}, (_, i) => {
    const low = i % 12 < 3, bid = flat ? .4 : i >= shiftFrom ? .25 + (i % 12) * .005 : low ? .35 : .6;
    const t = at(i), sourceAt = i === futureAt ? at(i + 100) : t;
    const snap = i >= invalidFrom ? {marketId: '377', skipped: 'test_outage', at: t} : {
      endpoint: '/markets/377/orderbook?tournamentId=' + c.tournamentId, requestStartedAt: sourceAt, receivedAt: sourceAt,
      data: {contexts: [{type: 'tournament', tournament: {id: c.tournamentId}, orderbook: {exchanges: [{
        exchangeId: '9', option: 'YES', asOf: {at: sourceAt, sequence: i},
        bids: [{price: Number(bid.toFixed(3)), quantity: 10000}], asks: [{price: Number((bid + .01).toFixed(3)), quantity: 10000}],
      }]}}]},
    };
    const body = {sequence: i + 1, previous, at: t, state: {}, event: {type: 'cycle', clockAt: t, accountExclusions: [], sourceSnapshots: [snap]}};
    const hash = digest(body); previous = hash;
    const text = JSON.stringify({...body, hash}); return whitespace ? '  ' + text + '  ' : text;
  }).join('\n') + '\n';
}
function book(n, bid = '.4', ask = '.41', bidQuantity = 10000) {
  return {marketId: '377', exchangeId: '9', instrument: '9:YES', sequence: String(n), sourceAt: at(n),
    receivedAt: at(n), requestStartedAt: at(n), versionKey: n + ':' + at(n),
    bids: [{price: '0' + bid, quantity: bidQuantity}], asks: [{price: '0' + ask, quantity: 10000}]};
}
const model = (id = 'synthetic') => ({id, horizon: 60, weights: [.08], means: [0], scales: [1], rmse: .1});
const fakeBank = () => ({config: c, targetPolicy: 'delayed_fifo_roundtrip_net_v1', forecastLifecycle: 'revoke-on-invalidation-v2', selected: [model()], trainingCaptureThrough: at(0)});
const empty = bank => ({bankHash: digest(bank), sourceSequence: 0, sourceHash: null, wallets: {}, histories: {}, predictions: {}});
const point = (pnl, trades = 20, extra = {}) => ({conservativePnl: pnl, liquidationPnl: pnl, completedTrades: trades, ...extra});

// These traces are synthetic policy tests, not observed competition returns.
test('portfolio selection prefers actual sequential net results over lower prediction error', () => {
  const r = rankPortfolios([
    {id: 'accurate-loser', rmse: .001, points: [point('-0.1'), point('-0.2'), point('-0.3')]},
    {id: 'less-accurate-winner', rmse: .1, points: [point('0.1'), point('0.2'), point('0.3')]},
  ]);
  assert.equal(r.winner.id, 'less-accurate-winner');
  assert.ok(r.results.find(x => x.id === 'accurate-loser').rejectionReasons.includes('does_not_beat_no_trade'));
});

test('no-trade beats weak evidence, one-block windfalls, terminal censored inventory and negative returns', () => {
  for (const points of [
    [point('0.1', 1), point('0.2', 2), point('0.3', 3)],
    [point('5'), point('4'), point('6')],
    [point('0'), point('0'), point('6')],
    [point('0.1'), point('0.2'), point('0.3', 20, {liquidationPnl: null})],
    [point('-0.1'), point('-0.2'), point('-0.3')],
  ]) assert.equal(rankPortfolios([{id: 'bad', points}]).winner, null);
  assert.equal(rankPortfolios([]).winner, null);
  assert.throws(() => rankPortfolios([{id: 'bad', points: []}]), /checkpoints/);
});

test('unknown liquidation charges remaining cost rather than cherry-picking realized profit', () => {
  let w = initialState(c, 'inventory', at(0));
  const policy = {netEdge: .1, horizonMs: 300000, minimumEdge: .005};
  w = stepForecast(w, book(1), c, policy); w = stepForecast(w, book(2), c, policy);
  const bytes = JSON.stringify(w);
  const fresh = portfolioPoint(w, c, at(2)), stale = portfolioPoint(w, c, at(4));
  assert.equal(fresh.openShares, 10); assert.equal(fresh.valuation, 'depth_checked_liquidation_estimate');
  assert.equal(stale.liquidationPnl, null); assert.equal(stale.conservativePnl, '-4.150000');
  assert.equal(stale.realizedPnl, '0.000000'); assert.equal(stale.unsoldShares, 10);
  assert.equal(JSON.stringify(w), bytes, 'reporting cannot alter state');
});

test('thin terminal books never invent a full-position exit', () => {
  let w = initialState(c, 'thin', at(0)); const p = {netEdge: .1, horizonMs: 300000, minimumEdge: .005};
  w = stepForecast(w, book(1), c, p); w = stepForecast(w, book(2), c, p);
  w = stepForecast(w, book(3, '.4', '.41', 100), c, p);
  const result = portfolioPoint(w, c, at(3));
  assert.equal(result.liquidationPnl, null); assert.equal(result.unsoldShares, 9);
  assert.equal(result.conservativePnl, '-4.150000');
});

test('fixed candidate margins reduce both production paper consumers to the chosen wallet', () => {
  const records = verifySource(capture(40, {flat: true}));
  for (const run of [advanceShadow, originalShadow]) {
    const legacy = fakeBank(), old = run(empty(legacy), records, legacy, {allowUnboundFixture: true});
    assert.equal(Object.keys(old.wallets).length, 3);
    const bank = {...legacy, candidatePlan: {version: PORTFOLIO_PLAN, candidates: [{modelId: 'synthetic', margin: .01}]}};
    const next = run(empty(bank), records, bank, {allowUnboundFixture: true});
    assert.deepEqual(Object.keys(next.wallets), ['synthetic-edge0.01']);
    assert.deepEqual(next.wallets['synthetic-edge0.01'], old.wallets['synthetic-edge0.01']);
    assert.deepEqual(run(next, records, bank, {allowUnboundFixture: true}), next, 'resume is idempotent');
  }
});

test('unknown plans, unsupported margins and mismatched models fail closed; legacy banks stay intact', () => {
  const b = fakeBank(); assert.deepEqual(modelMargins(b, b.selected[0]), [.005, .01, .015]);
  for (const plan of [null, {version: 'other', candidates: []}, {version: PORTFOLIO_PLAN, candidates: []},
    {version: PORTFOLIO_PLAN, candidates: [{modelId: 'other', margin: .005}]},
    {version: PORTFOLIO_PLAN, candidates: [{modelId: 'synthetic', margin: .0001}]}]) {
    assert.throws(() => validateCandidatePlan({...b, candidatePlan: plan}), /candidate/);
  }
  const flat = {...b, selected: [], candidatePlan: {version: PORTFOLIO_PLAN, candidates: []}};
  validateCandidatePlan(flat);
  const state = advanceShadow(empty(flat), verifySource(capture(30)), flat, {allowUnboundFixture: true});
  assert.deepEqual(state.wallets, {});
});

test('partitions keep equal-time records together and retain a disjoint final window', () => {
  const records = verifySource(capture(100)), cuts = chronologicalPartitions(records);
  assert.equal(cuts.fitRecords, 60); assert.equal(cuts.selectionRecords, 80);
  assert.equal(cuts.fitAt, at(59)); assert.equal(cuts.selectedAt, at(79));
  assert.throws(() => chronologicalPartitions(records, {fitFraction: .9, selectionFraction: .2}), /policy/);
  assert.throws(() => chronologicalPartitions(records, {minimumCompletedTrades: 0}), /policy/);
  const tied = records.map(r => ({...r, event: {...r.event, clockAt: at(1)}}));
  assert.throws(() => chronologicalPartitions(tied), /overlap/);
});

test('explicit fit cutoff purges labels and later validation prices cannot change fitted weights', () => {
  const a = trainBank(capture(180), c, {developmentSplitAt: at(100)});
  const b = trainBank(capture(180, {shiftFrom: 101}), c, {developmentSplitAt: at(100)});
  for (const m of a.models) {
    const n = b.models.find(x => x.id === m.id); assert.ok(n);
    assert.deepEqual(n.weights, m.weights); assert.deepEqual(n.means, m.means);
    assert.deepEqual(n.scales, m.scales); assert.ok(Date.parse(m.maxTrainingLabelAt) < Date.parse(at(100)));
  }
  assert.throws(() => trainBank(capture(180), c, {developmentSplitAt: at(0)}), /split/);
});

let trained;
test('full portfolio bank trains all candidates, freezes one margin and binds exact source bytes', () => {
  const raw = capture(300, {whitespace: true}), before = digest(c);
  trained = trainPortfolioBank(raw, c, {policy: {minimumCompletedTrades: 1}});
  assert.equal(digest(c), before);
  assert.equal(trained.modelsTrained, 128);
  assert.equal(trained.portfolioSelection.testedModelMarginPairs, 384);
  assert.equal(trained.portfolioSelection.decision, 'candidate_for_prospective_paper');
  assert.equal(trained.selected.length, 1); assert.equal(trained.candidatePlan.candidates.length, 1);
  assert.equal(trained.portfolioSelection.holdout.status, 'reported_without_reselection');
  requireSourceMetadata(trained);
  assert.equal(trained.trainingCaptureThrough, at(299));
  const current = advanceShadow(empty(trained), verifySource(raw), trained);
  assert.deepEqual(current.wallets, {}, 'training, validation and test are not prospective results');
  const future = advanceShadow(current, verifySource(capture(320, {whitespace: true})), trained);
  assert.equal(Object.keys(future.wallets).length, 1);
});

test('holdout changes never alter selection, the chosen margin or fitted weights', () => {
  const changed = trainPortfolioBank(capture(300, {shiftFrom: 240, whitespace: true}), c, {policy: {minimumCompletedTrades: 1}});
  assert.ok(trained);
  assert.deepEqual(changed.candidatePlan, trained.candidatePlan);
  assert.deepEqual(changed.selected, trained.selected);
  assert.deepEqual(changed.portfolioSelection.validation, trained.portfolioSelection.validation);
  assert.equal(changed.portfolioSelection.selectionHash, trained.portfolioSelection.selectionHash);
  assert.notEqual(changed.sourceHash, trained.sourceHash);
  assert.notDeepEqual(changed.portfolioSelection.holdout, trained.portfolioSelection.holdout);
});

test('flat losing market yields an explicit no-trade bank instead of forcing a best-of-bad strategy', () => {
  const b = trainPortfolioBank(capture(240, {flat: true}), c);
  assert.equal(b.portfolioSelection.decision, 'no_trade'); assert.deepEqual(b.selected, []);
  assert.equal(b.portfolioSelection.holdout.conservativePnl, '0.000000');
  requireSourceMetadata(b);
});

test('incomplete, altered and future-dated journals are rejected without fitting', () => {
  const raw = capture(240);
  assert.throws(() => trainPortfolioBank(raw.slice(0, -1), c), /Complete/);
  assert.throws(() => trainPortfolioBank(raw.replace('10000', '10001'), c), /integrity/);
  assert.throws(() => trainPortfolioBank(capture(240, {futureAt: 200}), c), /event clock/);
});

test('CLI rejects overwrites and unknown flags before opening training data', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'baal-portfolio-'));
  try {
    const out = path.join(dir, 'bank.json'); fs.writeFileSync(out, 'keep');
    const entry = fileURLToPath(new URL('./portfolio-bank.mjs', import.meta.url));
    const r = spawnSync(process.execPath, [entry, '--journal', 'missing', '--config', 'missing', '--out', out], {encoding: 'utf8'});
    assert.notEqual(r.status, 0); assert.match(r.stderr, /Refusing to overwrite/);
    assert.equal(fs.readFileSync(out, 'utf8'), 'keep');
    const bad = spawnSync(process.execPath, [entry, '--live', 'true'], {encoding: 'utf8'});
    assert.notEqual(bad.status, 0); assert.match(bad.stderr, /Invalid/);
  } finally {fs.rmSync(dir, {recursive: true, force: true});}
});


test('holdout inventory censored by later invalidation remains a charged open position', () => {
  const b = trainPortfolioBank(capture(300, {invalidFrom: 254}), c, {policy: {minimumCompletedTrades: 1}});
  assert.equal(b.portfolioSelection.decision, 'candidate_for_prospective_paper');
  const result = b.portfolioSelection.holdout;
  assert.ok(result.openShares > 0);
  assert.equal(result.liquidationPnl, null);
  assert.equal(result.valuation, 'cash_only_inventory_zero');
  assert.ok(Number(result.conservativePnl) < Number(result.realizedPnl));
  assert.deepEqual(b.candidatePlan, trained.candidatePlan);
});

test('malformed scorecards cannot pass evidence gates via undefined or NaN counts', () => {
  for (const completedTrades of [undefined, NaN, -1, 0.5]) {
    const p = point('0.1', completedTrades); p.completedTrades = completedTrades;
    assert.throws(() => rankPortfolios([{id: 'malformed', points: [p, p, p]}]), /trade count/);
  }
  const p = point('0.1', 20, {liquidationPnl: '0.2'});
  assert.throws(() => rankPortfolios([{id: 'inconsistent', points: [p, p, p]}]), /checkpoint/);
});

test('new CLI publishes a source-bound no-trade bank atomically with no stray temporary file', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'baal-portfolio-cli-'));
  try {
    const journal = path.join(dir, 'source.jsonl'), config = path.join(dir, 'config.json'), out = path.join(dir, 'ml-bank.json');
    fs.writeFileSync(journal, capture(240, {flat: true})); fs.writeFileSync(config, JSON.stringify(c));
    const entry = fileURLToPath(new URL('./portfolio-bank.mjs', import.meta.url));
    const result = spawnSync(process.execPath, [entry, '--journal', journal, '--config', config, '--out', out], {encoding: 'utf8', timeout: 30000});
    assert.equal(result.status, 0, result.stderr);
    const summary = JSON.parse(result.stdout), bank = JSON.parse(fs.readFileSync(out));
    assert.equal(summary.decision, 'no_trade'); assert.equal(summary.networkRequests, 0); assert.equal(summary.paperOnly, true);
    assert.equal(bank.portfolioSelection.policy.minimumCompletedTrades, 20);
    assert.equal(bank.portfolioSelection.policy.minimumPositiveBlocks, 2);
    assert.equal(bank.portfolioSelection.productionActivation, false); assert.equal(bank.portfolioSelection.scalingEvaluated, false);
    requireSourceMetadata(bank);
    assert.deepEqual(advanceShadow(empty(bank), verifySource(capture(260, {flat: true})), bank).wallets, {});
    assert.ok(!fs.readdirSync(dir).some(x => x.endsWith('.tmp')));
  } finally {fs.rmSync(dir, {recursive: true, force: true});}
});
