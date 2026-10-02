import {readVerifiedSource} from './journal-lines.mjs';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {verifySource} from './ml-shadow.mjs';
import {digest, initialState, normalizeBook, invalidate, stepDip, report, sampleEquity, assertState, validateConfig} from './model.mjs';
import {amount, money, TICK} from '../paper/fixed.mjs';
import {createShockRebound, advanceShockRebound, invalidateShockRebound} from './shock-rebound.mjs';
import {atomicJson, Journal, lock} from './runtime.mjs';

const workspace = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const clone = structuredClone;
export function shockCodeIdentity() {
  return Object.fromEntries(['live-paper/shock-shadow.mjs', 'live-paper/shock-rebound.mjs', 'live-paper/model.mjs', 'live-paper/ml-shadow.mjs', 'live-paper/runtime.mjs', 'live-paper/network-budget.mjs', 'paper/fixed.mjs', 'feed/contract.mjs'].map(f =>
    [f, createHash('sha256').update(fs.readFileSync(path.join(workspace, f))).digest('hex')]));
}
function checkBank(bank) {
  validateConfig(bank.config);
  if (bank.name !== 'shock-rebound-shadow-v1' || bank.cohortId !== 'shock-rebound-' + bank.cutoff || bank.config.feeBps !== 0 || bank.config.marketIds.length > 4
    || bank.policy.maxOpenMarkets !== 3 || bank.policy.cumulativeBuyBudget !== '100.000000'
    || amount(bank.config.maxExposure) > amount('100.000000') || bank.config.slippageTicks !== 1
    || bank.config.depthFractionBps > 100 || bank.config.orderShares > 10) throw Error('Invalid frozen shock cohort');
}
export function createShock(raw, config, universe, codeIdentity = shockCodeIdentity(), cutoffAt) {
  const records = verifySource(raw);
  if (!records?.length) throw Error('Complete source required');
  const c = clone(config); validateConfig(c);
  const sourceAt = records.at(-1).event.clockAt || records.at(-1).at, cutoff = cutoffAt || sourceAt, sourceHash = records.prefixHashes.at(-1);
  if (!Number.isFinite(Date.parse(cutoff)) || Date.parse(cutoff) < Date.parse(sourceAt)) throw Error('Cutoff precedes frozen source');
  const markets = c.marketIds.map(marketId => {
    const matches = universe.exchanges.filter(x => x.marketId === marketId && x.option === 'YES');
    if (matches.length !== 1 || universe.tournamentId !== c.tournamentId) throw Error('Shock universe scope mismatch');
    return createShockRebound({...matches[0], sourceHash, cutoff, config: {maxDataAgeMs: c.maxDataAgeMs}});
  });
  const bank = {schemaVersion: 1, name: 'shock-rebound-shadow-v1', cohortId: 'shock-rebound-' + cutoff, config: c, cutoff, sourceHash, sourceRecords: records.length,
    codeIdentity: clone(codeIdentity), markets: markets.map(x => x.bank), policy: {maxOpenMarkets: 3, cumulativeBuyBudget: '100.000000',
      entryLifetimeMs: 60000, takeProfit: '0.010000', maximumHoldMs: 600000, maxMarketCost: '25.000000'}};
  checkBank(bank);
  const state = {schemaVersion: 1, bankHash: digest(bank), sourceSequence: 0, lastRecordHash: null,
    candidates: Object.fromEntries(markets.map(x => [x.bank.marketId, x.state])), proposals: {}, signals: {}, diagnosticCounts: {},
    wallet: initialState(c, bank.cohortId, cutoff)};
  return {bank, state: advanceShock(state, records, bank)};
}
function revoke(s, bank, marketId, at, reason) {
  invalidate(s.wallet, marketId, at, reason);
  for (const m of bank.markets) if (marketId === null || m.marketId === marketId) {
    const latest = s.candidates[m.marketId].last?.at || 0;
    s.candidates[m.marketId] = invalidateShockRebound(s.candidates[m.marketId], m, new Date(Math.max(Date.parse(at), latest)).toISOString());
    delete s.proposals[m.marketId];
    const id = m.exchangeId + ':YES', st = s.wallet.strategies.mean_reversion;
    if (st.pending[id]?.action === 'buy') { delete st.pending[id]; st.expiredIntents++; }
    s.signals[m.marketId] = {reason, at};
    s.diagnosticCounts[reason] = (s.diagnosticCounts[reason] || 0) + 1;
  }
}
export function advanceShock(input, records, bank) {
  checkBank(bank);
  if (input.bankHash !== digest(bank) || records?.prefixHashes?.[bank.sourceRecords - 1] !== bank.sourceHash
    || input.sourceSequence > records.length || (input.sourceSequence && records[input.sourceSequence - 1]?.hash !== input.lastRecordHash)) throw Error('Shock source or bank changed');
  const s = clone(input), c = bank.config, p = bank.policy; assertState(s.wallet, c);
  for (const r of records) {
    if (r.sequence <= s.sourceSequence) continue;
    const e = r.event, at = e.clockAt || r.at, prospective = r.sequence > bank.sourceRecords;
    s.sourceSequence = r.sequence; s.lastRecordHash = r.hash;
    if (e.type === 'account_read_outage') { revoke(s, bank, null, at, 'account_outage'); continue; }
    if (e.type !== 'cycle') continue;
    const snapshots = [...(e.sourceSnapshots || []), ...(e.supplementalDepth || [])].sort((a,b) => Date.parse(a.receivedAt || a.at || at) - Date.parse(b.receivedAt || b.at || at));
    for (const snap of snapshots) {
      const marketId = snap.marketId || /^\/markets\/(\d+)\/orderbook\?/.exec(snap.endpoint || '')?.[1], m = bank.markets.find(x => x.marketId === marketId);
      if (!m) continue;
      if (snap.skipped) { revoke(s, bank, marketId, snap.at || at, snap.skipped); continue; }
      let b, out;
      try {
        if (!Array.isArray(e.accountExclusions)) throw Error('Missing reconciled exclusions');
        b = normalizeBook(snap.data, marketId, c.tournamentId, snap.receivedAt, snap.requestStartedAt, c.maxDataAgeMs, new Set(e.accountExclusions), 'authoritative_receipt_v1');
        if (Date.parse(b.receivedAt) > Date.parse(at)) throw Error('Future book');
        out = advanceShockRebound(s.candidates[marketId], b, m);
      } catch { revoke(s, bank, marketId, at, 'invalid_or_excluded_book'); continue; }
      s.candidates[marketId] = out.state; s.signals[marketId] = out.diagnostic;
      s.diagnosticCounts[out.diagnostic.reason] = (s.diagnosticCounts[out.diagnostic.reason] || 0) + 1;
      if (!prospective || Date.parse(b.requestStartedAt) <= Date.parse(bank.cutoff)) continue;
      if (out.diagnostic.reason === 'request_before_invalidation') continue;
      const independent = out.diagnostic.reason !== 'no_independent_quote';
      if (!independent) delete s.proposals[marketId];
      if (out.signal) s.proposals[marketId] = out.signal;
      const proposal = s.proposals[marketId], st = s.wallet.strategies.mean_reversion, id = b.instrument;
      // Confirmation does not license buying a later collapse. Preserve the
      // original shock anchor and apply its cap through the delayed fill.
      const totalDropTwice = proposal ? 2n * amount(proposal.featuresAtShock.anchorMid) - amount(b.bids[0].price) - amount(b.asks[0].price) : 0n;
      if (proposal && (amount(b.bids[0].price) < amount(proposal.troughBid) || totalDropTwice > 2n * amount(m.config.maximumShock)
        || ['maximum_shock_breached', 'observation_gap', 'insufficient_history', 'trend_veto', 'missing_shock_anchor', 'insufficient_trend_history'].includes(out.diagnostic.reason))) {
        revoke(s, bank, marketId, b.receivedAt, 'proposal_risk_revoked'); continue;
      }
      const used = st.fills.filter(f => f.action === 'buy').reduce((sum, f) => sum + amount(f.price) * BigInt(f.quantity), 0n);
      const remaining = amount(p.cumulativeBuyBudget) - used;
      const occupied = new Set([...Object.keys(st.positions), ...Object.keys(st.pending)]);
      const ask = amount(b.asks[0].price) + TICK, bid = amount(b.bids[0].price) - TICK;
      const quantityCap = proposal ? Math.min(proposal.quantity, Math.max(0, Number(remaining / amount(proposal.maximumEntryPrice)))) : 0;
      const eligible = Boolean(proposal && Date.parse(b.receivedAt) - Date.parse(proposal.at) <= p.entryLifetimeMs
        && ask <= amount(proposal.maximumEntryPrice) && ask - bid <= amount(m.config.maximumEffectiveSpread)
        && amount(proposal.recoveryReferenceBid) - TICK - ask >= amount(m.config.minimumRoom)
        && (occupied.has(id) || occupied.size < p.maxOpenMarkets) && quantityCap > 0);
      if (st.pending[id]?.action === 'buy' && proposal && quantityCap > 0) st.pending[id].quantity = Math.min(st.pending[id].quantity, quantityCap);
      // A fresh receipt may age an owned position into a timeout even if its
      // engine quote did not change. Evaluate that exit while withholding all
      // same-version liquidity from the hypothetical fill engine.
      const priorConsumed = st.consumedVersions[id];
      if (!independent) st.consumedVersions[id] = b.versionKey;
      s.wallet = stepDip(s.wallet, b, c, {reference: proposal ? Number(amount(proposal.recoveryReferenceBid) - TICK) / 1e6 : null,
        allowEntry: eligible, minimumEdge: Number(amount(m.config.minimumRoom)) / 1e6, takeProfit: p.takeProfit,
        horizonMs: p.maximumHoldMs, maxMarketCost: p.maxMarketCost, executionPolicy: 'responsive_v2'});
      const next = s.wallet.strategies.mean_reversion;
      if (!independent) { if (priorConsumed === undefined) delete next.consumedVersions[id]; else next.consumedVersions[id] = priorConsumed; }
      if (next.pending[id]?.action === 'buy' && proposal) {
        next.pending[id].quantity = Math.min(next.pending[id].quantity, quantityCap);
        next.pending[id].reason = 'causal_shock_rebound'; next.pending[id].signalId = proposal.signalId;
      }
      if (next.positions[id] || !eligible) delete s.proposals[marketId];
    }
    if (prospective) sampleEquity(s.wallet, c, at);
  }
  assertState(s.wallet, c); return s;
}
export function shockReport(s, bank, at = new Date().toISOString()) {
  const r = report(s.wallet, bank.config, at), st = s.wallet.strategies.mean_reversion;
  const names = {'377': 'Alaska Senate', '354': 'Florida Senate', '277': 'North Carolina Senate', '305': 'Michigan Governor'};
  const label = id => (names[s.wallet.books[id]?.marketId] || 'Market ' + s.wallet.books[id]?.marketId) + ' Democratic YES';
  const strategy = r.strategies.mean_reversion;
  strategy.pending = Object.entries(st.pending).map(([instrument, p]) => ({...p, instrument, label: label(instrument)}));
  strategy.positions = strategy.positions.map(p => ({...p, label: label(p.instrument)}));
  strategy.recentFills = strategy.recentFills.map(f => ({...f, label: label(f.instrument)}));
  return {mode: 'prospective_shock_rebound_paper', cohortId: bank.cohortId, updatedAt: at, bankHash: s.bankHash, cutoff: bank.cutoff, sourceSequence: s.sourceSequence,
    feeBps: 0, universeSize: bank.markets.length, historiesAvailable: Object.values(s.candidates).filter(v => v.history.length > 0).length,
    sourceHash: s.lastRecordHash, frozenSourceHash: bank.sourceHash, codeIdentity: bank.codeIdentity, marketIds: bank.config.marketIds,
    observations: r.observations, signals: s.signals, diagnosticCounts: s.diagnosticCounts, historyCounts: Object.fromEntries(Object.entries(s.candidates).map(([id, v]) => [id, v.history.length])),
    strategy, cumulativeBuyCost: money(st.fills.filter(f => f.action === 'buy').reduce((v,f) => v + amount(f.price) * BigInt(f.quantity), 0n)),
    policy: bank.policy, assumptions: {...r.assumptions, feeBps: 0, feeStatus: 'zero_fee_scenario'}, networkRequests: 0, competitionOrders: 0, profitabilityEstablished: false};
}
export async function runShock({dir, initialize = false, once = false, workspaceDir = workspace, now = Date.now} = {}) {
  const stopped = () => ['STOP', 'SHOCK_STOP'].some(f => fs.existsSync(path.join(dir, f))) || fs.existsSync(path.join(workspaceDir, 'STOP'));
  if (stopped()) throw Error('STOP exists');
  const release = lock(path.join(dir, 'shock-shadow.lock'));
  let bank, state;
  try {
    const bankFile = path.join(dir, 'shock-bank.json'), journalFile = path.join(dir, 'shock-journal.jsonl'), journal = new Journal(journalFile);
    if (initialize) {
      if (fs.existsSync(bankFile) || journal.sequence) throw Error('Frozen shock cohort already exists');
      const base = JSON.parse(fs.readFileSync(path.join(dir, 'dip-responsive-bank.json'), 'utf8'));
      const c = {...base.config, marketIds: ['377','354','277','305'], feeBps: 0, orderShares: 10, maxPositionShares: 50,
        maxExposure: '100.000000', cashReserve: money(amount(base.config.initialCash) - amount('100.000000')), exitAfterMs: 600000};
      const raw = fs.readFileSync(path.join(dir, 'journal.jsonl'), 'utf8');
      ({bank, state} = createShock(raw, c, base.universe, shockCodeIdentity(), new Date(now()).toISOString()));
      fs.writeFileSync(bankFile, JSON.stringify(bank, null, 2), {flag: 'wx', mode: 0o600});
      journal.commit(state, {type: 'cohort_frozen', sourceSequence: state.sourceSequence, sourceHash: bank.sourceHash, cutoff: bank.cutoff});
    } else {
      bank = JSON.parse(fs.readFileSync(bankFile, 'utf8')); state = journal.last?.state;
      if (!state) throw Error('Shock journal missing; explicit recovery required');
    }
    if (digest(bank.codeIdentity) !== digest(shockCodeIdentity())) throw Error('Frozen shock code changed; new cohort required');
    let partials = 0;
    const publish = status => { atomicJson(path.join(dir, 'shock-state.json'), state); atomicJson(path.join(dir, 'shock-stats.json'), {...shockReport(state, bank), status}); };
    try {
      do {
        if (stopped()) break;
        const records = readVerifiedSource(path.join(dir,'journal.jsonl'));
        if (!records) { if (++partials > 3) throw Error('Incomplete source'); publish('awaiting_capture'); }
        else {
          partials = 0; const next = advanceShock(state, records, bank);
          if (next.sourceSequence !== state.sourceSequence) journal.commit(next, {type: 'source_advanced', sourceSequence: next.sourceSequence, sourceHash: next.lastRecordHash});
          state = next; publish('running');
        }
        if (once) break;
        for (let i = 0; i < 75 && !stopped(); i++) await new Promise(r => setTimeout(r, 200));
      } while (!stopped());
      if (stopped()) publish('stopped');
    } catch (error) { publish('failed'); throw error; }
    return {bank, state};
  } finally { release(); }
}
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const dir = process.env.SIG_PAPER_DIR || path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime');
  try { await runShock({dir, initialize: process.argv.includes('--init'), once: process.argv.includes('--once')}); }
  catch (error) { process.stderr.write(error.message + '\n'); process.exitCode = 1; }
}
