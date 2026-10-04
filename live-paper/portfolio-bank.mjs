import fs from 'node:fs';
import {randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {trainBank} from './ml-bank.mjs';
import {verifySource, advanceShadow} from './ml-shadow-v2.mjs';
import {digest, initialState, report, invalidate, validateConfig} from './model.mjs';
import {amount, money} from '../paper/fixed.mjs';
import {scopedSnapshots} from './depth-scope.mjs';
import {ROUND_TRIP} from './roundtrip-target.mjs';
import {ISOLATED} from './history-policy.mjs';
import {PAPER_EDGE_MARGINS} from './execution-metrics.mjs';
import {PORTFOLIO_PLAN, validateCandidatePlan} from './candidate-plan.mjs';

export const SELECTION_POLICY = Object.freeze({
  version: PORTFOLIO_PLAN, fitFraction: 0.6, selectionFraction: 0.2,
  validationBlocks: 3, minimumCompletedTrades: 20, minimumPositiveBlocks: 2,
});
const clock = r => r.event?.clockAt || r.at;

function source(raw, config) {
  const records = verifySource(raw);
  if (!records) throw Error('Complete immutable capture required');
  // Reject future-dated observations concealed inside an earlier journal event.
  for (const r of records) if (r.event.type === 'cycle') {
    if (!Array.isArray(r.event.accountExclusions)) throw Error('Missing account exclusions');
    for (const snap of scopedSnapshots(r.event, config)) {
      const at = snap.skipped ? snap.at || clock(r) : snap.receivedAt;
      if (!Number.isFinite(Date.parse(at)) || Date.parse(at) > Date.parse(clock(r))) throw Error('Observation exceeds event clock');
      if (!snap.skipped && (!Number.isFinite(Date.parse(snap.requestStartedAt))
          || Date.parse(snap.requestStartedAt) > Date.parse(at))) throw Error('Invalid request chronology');
    }
  }
  return {records, lines: raw.slice(0, -1).split('\n')};
}
function prefix(s, n, includeRaw = false) {
  const records = s.records.slice(0, n);
  const raw = includeRaw ? s.lines.slice(0, n).join('\n') + '\n' : undefined;
  Object.defineProperty(records, 'prefixHashes', {value: s.records.prefixHashes.slice(0, n)});
  return {records, raw, sourceHash: s.records.prefixHashes[n - 1], sourceRecords: n, trainingCaptureThrough: clock(records.at(-1))};
}
function cutAfter(records, at) {
  const n = records.findIndex(r => Date.parse(clock(r)) > Date.parse(at));
  return n < 0 ? records.length : n;
}
function policyOptions(options) {
  const p = {...SELECTION_POLICY, ...options};
  if (Object.keys(p).some(k => !Object.keys(SELECTION_POLICY).includes(k)) || p.version !== PORTFOLIO_PLAN
      || !Number.isFinite(p.fitFraction) || p.fitFraction <= 0 || p.fitFraction >= 1
      || !Number.isFinite(p.selectionFraction) || p.selectionFraction <= 0 || p.fitFraction + p.selectionFraction >= 1
      || !Number.isSafeInteger(p.validationBlocks) || p.validationBlocks < 2 || p.validationBlocks > 10
      || !Number.isSafeInteger(p.minimumCompletedTrades) || p.minimumCompletedTrades < 1
      || !Number.isSafeInteger(p.minimumPositiveBlocks) || p.minimumPositiveBlocks < 1 || p.minimumPositiveBlocks > p.validationBlocks) throw Error('Invalid selection policy');
  return p;
}
export function chronologicalPartitions(records, options = {}) {
  const p = policyOptions(options), cycles = records.filter(r => r.event.type === 'cycle');
  if (cycles.length < 20) throw Error('Insufficient cycles for three chronological windows');
  const fitAt = clock(cycles[Math.floor(cycles.length * p.fitFraction) - 1]);
  const selectedAt = clock(cycles[Math.floor(cycles.length * (p.fitFraction + p.selectionFraction)) - 1]);
  const fitRecords = cutAfter(records, fitAt), selectionRecords = cutAfter(records, selectedAt);
  if (!(fitRecords > 0 && fitRecords < selectionRecords && selectionRecords < records.length)) throw Error('Chronological windows overlap or are empty');
  return {fitAt, selectedAt, fitRecords, selectionRecords, finalAt: clock(records.at(-1))};
}

// Missing or stale liquidation depth cannot make an unfinished trade disappear.
// A cash-only floor charges the full remaining basis when its exit is unknown.
export function portfolioPoint(wallet, config, at) {
  if (typeof at !== 'string' || !Number.isFinite(Date.parse(at))) throw Error('Invalid portfolio valuation clock');
  const w = structuredClone(wallet);
  for (const b of Object.values(w.books)) {
    const age = Date.parse(at) - Date.parse(b.receivedAt);
    if (!Number.isFinite(age) || age < 0 || age > config.maxDataAgeMs) invalidate(w, b.marketId, at, 'selection_stale_book');
  }
  const r = report(w, config, at).strategies.mean_reversion;
  const equity = r.estimatedLiquidationEquity ?? r.cash;
  return {at, conservativePnl: money(amount(equity) - amount(config.initialCash)),
    liquidationPnl: r.liquidationPnl, valuation: r.liquidationPnl === null ? 'cash_only_inventory_zero' : 'depth_checked_liquidation_estimate',
    cash: r.cash, realizedPnl: r.realizedPnl, fees: r.fees, turnover: r.turnover,
    completedTrades: r.closedTrades, fillExecutions: r.fills, intents: r.intents,
    expiredIntents: r.expiredIntents, pendingIntents: r.pending.length,
    openShares: r.positions.reduce((n, x) => n + x.quantity, 0),
    unsoldShares: r.positions.reduce((n, x) => n + x.unsoldShares, 0)};
}

function replayCandidates(s, config, modelBank, startCount, endCounts) {
  const start = prefix(s, startCount);
  const bank = {...modelBank, config, configHash: digest(config),
    sourceHash: start.sourceHash, sourceRecords: start.sourceRecords,
    trainingCaptureThrough: start.trainingCaptureThrough,
    requireInstrumentCoverage: true, trainingScope: 'all-markets-v1',
    forecastLifecycle: 'revoke-on-invalidation-v2'};
  validateCandidatePlan(bank);
  let state = {bankHash: digest(bank), sourceSequence: 0, sourceHash: null, wallets: {}, histories: {}, predictions: {}};
  const points = new Map();
  for (const end of endCounts) {
    const view = prefix(s, end);
    state = advanceShadow(state, view.records, bank);
    for (const model of bank.selected) {
      const margins = bank.candidatePlan?.candidates.filter(x => x.modelId === model.id).map(x => x.margin) ?? PAPER_EDGE_MARGINS;
      for (const margin of margins) {
        const id = model.id + '-edge' + margin;
        const w = state.wallets[id] ?? initialState(config, id, start.trainingCaptureThrough);
        const list = points.get(id) ?? [];
        list.push(portfolioPoint(w, config, view.trainingCaptureThrough)); points.set(id, list);
      }
    }
  }
  return points;
}
function blockEnds(s, startCount, endCount, blocks) {
  const cycles = s.records.slice(startCount, endCount).filter(r => r.event.type === 'cycle');
  if (cycles.length < blocks * 2) throw Error('Insufficient validation block coverage');
  const ends = Array.from({length: blocks}, (_, i) => i === blocks - 1 ? endCount
    : Math.min(endCount, cutAfter(s.records, clock(cycles[Math.floor(cycles.length * (i + 1) / blocks) - 1]))));
  if (new Set(ends).size !== blocks || ends[0] <= startCount) throw Error('Validation block clocks overlap');
  return ends;
}

export function rankPortfolios(scorecards, options = {}) {
  const policy = policyOptions(options);
  if (!Array.isArray(scorecards) || new Set(scorecards.map(s => s.id)).size !== scorecards.length) throw Error('Invalid portfolio scorecards');
  const results = scorecards.map(s => {
    if (!s || typeof s.id !== 'string' || !s.id) throw Error('Invalid portfolio identifier');
    if (!Array.isArray(s.points) || s.points.length !== policy.validationBlocks) throw Error('Missing sequential score checkpoints');
    for (const p of s.points) {
      if (!Number.isSafeInteger(p.completedTrades) || p.completedTrades < 0) throw Error('Invalid completed trade count');
      amount(p.conservativePnl);
      if (p.liquidationPnl !== null && amount(p.liquidationPnl) !== amount(p.conservativePnl)) throw Error('Inconsistent liquidation checkpoint');
    }
    let previous = 0n;
    const blockPnl = s.points.map(p => {const value = amount(p.conservativePnl), change = value - previous; previous = value; return money(change);});
    const last = s.points.at(-1), reasons = [];
    if (last.completedTrades < policy.minimumCompletedTrades) reasons.push('insufficient_completed_trades');
    if (amount(last.conservativePnl) <= 0n) reasons.push('does_not_beat_no_trade');
    if (blockPnl.some(v => amount(v) < 0n)) reasons.push('negative_validation_block');
    if (blockPnl.filter(v => amount(v) > 0n).length < policy.minimumPositiveBlocks) reasons.push('insufficient_positive_blocks');
    if (last.liquidationPnl === null) reasons.push('unresolved_terminal_liquidation');
    return {...s, blockPnl, eligible: reasons.length === 0, rejectionReasons: reasons};
  });
  results.sort((a, b) => {
    if (a.eligible !== b.eligible) return a.eligible ? -1 : 1;
    const x = amount(a.points.at(-1).conservativePnl), y = amount(b.points.at(-1).conservativePnl);
    return x === y ? (a.id < b.id ? -1 : a.id > b.id ? 1 : 0) : x > y ? -1 : 1;
  });
  return {winner: results.find(x => x.eligible) ?? null, results};
}

export function trainPortfolioBank(raw, config, {policy: options = {}, family = 'ridge', breadthContext = false} = {}) {
  validateConfig(config);
  const policy = policyOptions(options), s = source(raw, config), cuts = chronologicalPartitions(s.records, policy);
  const development = prefix(s, cuts.selectionRecords, true);
  // Fit on labels matured before fitAt. Every later observation, including the
  // entire holdout, is excluded from fitting. Selection compares ALL variants.
  const fitted = trainBank(development.raw, config, {family, breadthContext,
    historyPolicy: ISOLATED, targetPolicy: ROUND_TRIP, developmentSplitAt: cuts.fitAt,
    requireInstrumentCoverage: false, trainingScope: 'all-markets-v1'});
  const validationBank = {...fitted, selected: fitted.models};
  const checkpoints = blockEnds(s, cuts.fitRecords, cuts.selectionRecords, policy.validationBlocks);
  const paths = replayCandidates(s, config, validationBank, cuts.fitRecords, checkpoints);
  const cards = fitted.models.flatMap(model => PAPER_EDGE_MARGINS.map(margin => ({
    id: model.id + '-edge' + margin, modelId: model.id, margin, rmse: model.rmse,
    points: paths.get(model.id + '-edge' + margin),
  })));
  const ranked = rankPortfolios(cards, policy), winner = ranked.winner;
  const selected = winner ? [fitted.models.find(m => m.id === winner.modelId)] : [];
  const candidatePlan = {version: PORTFOLIO_PLAN, candidates: winner ? [{modelId: winner.modelId, margin: winner.margin}] : []};
  // Nothing below this line can choose another candidate or refit its weights.
  const selectionHash = digest({policy, candidatePlan, selected});
  let holdout;
  if (winner) {
    const paths = replayCandidates(s, config, {...fitted, selected, candidatePlan}, cuts.selectionRecords, [s.records.length]);
    const result = paths.get(winner.id)[0];
    const assessment = result.liquidationPnl === null ? 'unresolved_terminal_inventory'
      : amount(result.conservativePnl) <= 0n ? 'did_not_beat_no_trade'
      : result.completedTrades < policy.minimumCompletedTrades ? 'positive_but_insufficient_completed_trades'
      : 'positive_simulation_requires_prospective_validation';
    holdout = {...result, status: 'reported_without_reselection', assessment, selectedCandidateId: winner.id};
  } else holdout = {status: 'no_trade_selected', selectedCandidateId: null,
    assessment: 'no_trade_selected', conservativePnl: '0.000000', realizedPnl: '0.000000', completedTrades: 0,
    openShares: 0, pendingIntents: 0, at: cuts.finalAt};
  const full = prefix(s, s.records.length);
  const bank = {...fitted, createdAt: new Date().toISOString(), selected, candidatePlan,
    sourceHash: full.sourceHash, sourceRecords: full.sourceRecords, trainingCaptureThrough: full.trainingCaptureThrough,
    forecastLifecycle: 'revoke-on-invalidation-v2', requireInstrumentCoverage: true,
    selection: 'Validation-only sequential portfolio selection; the no-trade baseline may win. Holdout is never used for reselection.',
    portfolioSelection: {version: PORTFOLIO_PLAN, policy, windows: cuts, fitSourceHash: prefix(s, cuts.fitRecords).sourceHash,
      developmentSourceHash: development.sourceHash, selectionHash,
      decision: winner ? 'candidate_for_prospective_paper' : 'no_trade',
      testedModelMarginPairs: cards.length, validation: ranked.results, holdout,
      noTradeBaseline: {pnl: '0.000000'}, productionActivation: false, scalingEvaluated: false},
    limitations: [...fitted.limitations,
      'Single chronological train/validation/holdout split, not multiple independent walk-forward folds.',
      'Validation searches up to 384 correlated model/margin pairs; passing heuristic gates is not statistical significance.',
      'Validation checkpoints use one continuing wallet. They are not separate independent trades, nor a full drawdown trace.',
      'Holdout starts flat with fresh warmup. Remaining positions stay charged; missing liquidation uses a cash-only floor.',
      'Snapshot-based hypothetical fills and liquidation are not verified exchange executions or settlement payouts.',
      'Only configured market depth is evaluated; no new network collection, capital scaling, or live execution.',
      'Weights are not refitted on validation or holdout. Prospective processing starts after the complete frozen capture.']};
  validateCandidatePlan(bank);
  return bank;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const values = new Map(), flags = new Set(['--journal', '--config', '--out', '--family']);
  for (let i = 2; i < process.argv.length; i += 2) {
    const k = process.argv[i], v = process.argv[i + 1];
    if (!flags.has(k) || values.has(k) || !v || v.startsWith('--')) throw Error('Invalid portfolio training arguments');
    values.set(k, v);
  }
  for (const k of ['--journal', '--config', '--out']) if (!values.has(k)) throw Error('Required ' + k);
  const out = values.get('--out');
  if (fs.existsSync(out)) throw Error('Refusing to overwrite a frozen portfolio bank');
  const bank = trainPortfolioBank(fs.readFileSync(values.get('--journal'), 'utf8'), JSON.parse(fs.readFileSync(values.get('--config'), 'utf8')), {family: values.get('--family') ?? 'ridge'});
  const tmp = out + '.' + randomUUID() + '.tmp';
  try {fs.writeFileSync(tmp, JSON.stringify(bank, null, 2), {flag: 'wx', mode: 0o600}); fs.linkSync(tmp, out);}
  finally {if (fs.existsSync(tmp)) fs.unlinkSync(tmp);}
  console.log(JSON.stringify({decision: bank.portfolioSelection.decision, candidatePlan: bank.candidatePlan,
    modelMarginPairs: bank.portfolioSelection.testedModelMarginPairs,
    holdout: bank.portfolioSelection.holdout, sourceHash: bank.sourceHash,
    paperOnly: true, profitabilityEstablished: false, networkRequests: 0}));
}
