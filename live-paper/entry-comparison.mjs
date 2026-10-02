import {digest, validateConfig, initialState, normalizeBook, invalidate, stepDip, report, sampleEquity, assertState} from './model.mjs';
import {historicalReference} from './history.mjs';
import {dipSignal, validateDipPolicy} from './dip-shadow.mjs';
import {createShockRebound, advanceShockRebound, invalidateShockRebound} from './shock-rebound.mjs';
import {amount, money, TICK, SCALE} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';

export const ENTRY_COMPARISON_MARKETS = Object.freeze(['377', '354', '277', '305']);
export const ENTRY_COMPARISON_EXECUTION = Object.freeze({name: 'common_next_independent_book_v1', feeBps: 0,
  slippageTicks: 1, depthFractionBps: 100, orderShares: 10, maxMarketCost: '25.000000', maxExposure: '100.000000',
  cumulativeBuyBudget: '100.000000', maxOpenMarkets: 3, latencyMs: 1000, entryLifetimeMs: 60000,
  accountContextMaxAgeMs: 180000,
  duplicateEdgeCheckRoundoffTolerance: 4 * Number.EPSILON,
  takeProfit: '0.010000', stopLoss: '0.030000', maximumHoldMs: 3600000, cooldownMs: 60000,
  dipReferenceConvention: 'historical_center_minus_padded_ask_once',
  shockReferenceConvention: 'historical_bid_minus_exit_tick_minus_padded_ask',
  liquidityAssumption: 'aggregate_public_depth_may_include_own_orders; separate_hypothetical_wallets'});
const variants = ['historical_dip', 'shock_rebound'];
const clone = structuredClone, hex = x => typeof x === 'string' && /^[a-f0-9]{64}$/.test(x);
const canonical = x => typeof x === 'string' && Number.isFinite(Date.parse(x)) && new Date(x).toISOString() === x;
const need = (ok, code) => { if (!ok) throw Error(code); };
const count = (o, key) => { o[key] = (o[key] || 0) + 1; };

function checkBank(bank) {
  need(bank?.schemaVersion === 1 && bank.name === 'entry-quality-comparison-v1' && Number.isSafeInteger(bank.sourceRecords)
    && bank.sourceRecords > 0 && hex(bank.sourceHash) && canonical(bank.cutoff), 'Invalid comparison bank');
  validateConfig(bank.config); validateDipPolicy(bank.policy);
  need(digest(bank.sharedExecution) === digest(ENTRY_COMPARISON_EXECUTION)
    && digest(bank.config.marketIds) === digest(ENTRY_COMPARISON_MARKETS)
    && bank.config.feeBps === 0 && bank.config.slippageTicks === 1 && bank.config.depthFractionBps === 100
    && bank.config.orderShares === 10 && bank.config.latencyMs === 1000 && bank.config.stopLoss === '0.030000'
    && bank.config.maxExposure === '100.000000' && bank.config.maxDataAgeMs <= 20000
    && amount(bank.config.cashReserve) === amount(bank.config.initialCash) - 100000000n,
  'Changed comparison execution contract');
  need(bank.universe?.tournamentId === bank.config.tournamentId && Array.isArray(bank.universe.exchanges), 'Invalid comparison universe');
  for (const marketId of ENTRY_COMPARISON_MARKETS) {
    const matches = bank.universe.exchanges.filter(x => x.marketId === marketId && x.option === 'YES');
    need(matches.length === 1 && /^[1-9]\d*$/.test(matches[0].exchangeId), 'Missing common scoped market');
  }
  need(new Set(bank.universe.exchanges.map(x => x.exchangeId)).size === bank.universe.exchanges.length, 'Duplicate comparison exchange');
  need(bank.codeIdentity && typeof bank.codeIdentity === 'object' && Object.keys(bank.codeIdentity).length > 0
    && Object.values(bank.codeIdentity).every(hex), 'Missing frozen comparison code identity');
}

export function createEntryComparison({sourceRecords, sourceHash, cutoff, config, universe, identities = {}, policy, codeIdentity}) {
  const c = {...clone(config), marketIds: [...ENTRY_COMPARISON_MARKETS], feeBps: 0, slippageTicks: 1, depthFractionBps: 100,
    orderShares: 10, maxPositionShares: 50, latencyMs: 1000, maxExposure: '100.000000',
    cashReserve: money(amount(config.initialCash) - 100000000n), stopLoss: '0.030000', exitAfterMs: 3600000};
  need(amount(c.initialCash) > 100000000n, 'Insufficient comparison initial cash');
  const bank = {schemaVersion: 1, name: 'entry-quality-comparison-v1', sourceRecords, sourceHash, cutoff,
    config: c, universe: clone(universe), identities: clone(identities), policy: clone(policy), codeIdentity: clone(codeIdentity),
    sharedExecution: clone(ENTRY_COMPARISON_EXECUTION)};
  checkBank(bank); return {bank, state: emptyEntryComparisonState(bank)};
}

export function emptyEntryComparisonState(bank) {
  checkBank(bank);
  const shocks = Object.fromEntries(ENTRY_COMPARISON_MARKETS.map(marketId => {
    const spec = bank.universe.exchanges.find(x => x.marketId === marketId && x.option === 'YES');
    return [marketId, createShockRebound({...spec, sourceHash: bank.sourceHash, cutoff: bank.cutoff,
      config: {maxDataAgeMs: bank.config.maxDataAgeMs}})];
  }));
  return {schemaVersion: 1, bankHash: digest(bank), sourceSequence: 0, lastRecordHash: '', lastSourceAt: null,
    histories: {}, bookHistory: {}, seen: {}, invalidatedAt: {}, shocks, proposals: {},
    counters: {developmentBooks: 0, prospectiveBooks: 0, independentBooks: 0, contaminatedBooks: 0,
      unknownContaminationBooks: 0, skippedBooks: 0, invalidBooks: 0, accountOutages: 0},
    strategies: Object.fromEntries(variants.map(name => [name, {wallet: initialState(bank.config, bank.name + ':' + name, bank.cutoff),
      uniqueObservations: 0, acceptedSignals: 0, gateCounts: {}, executionGateCounts: {}, contaminationCounts: {},
      lastSignals: {}, cooldowns: {}, decisions: {}, fillAttribution: []}]))};
}

function revoke(s, bank, marketId, at, reason) {
  for (const id of ENTRY_COMPARISON_MARKETS.filter(x => marketId === null || marketId === x)) {
    const shock = s.shocks[id], clock = new Date(Math.max(Date.parse(at), shock.state.last?.at || 0)).toISOString();
    shock.state = invalidateShockRebound(shock.state, shock.bank, clock); delete s.proposals[id]; s.invalidatedAt[id] = clock;
    for (const v of Object.values(s.strategies)) {
      invalidate(v.wallet, id, clock, reason);
      for (const [key, p] of Object.entries(v.wallet.strategies.mean_reversion.pending)) {
        if (p.action === 'buy' && v.wallet.books[key]?.marketId === id) { delete v.wallet.strategies.mean_reversion.pending[key]; v.wallet.strategies.mean_reversion.expiredIntents++; }
      }
    }
  }
}

function checkedPublicBook(s, snap, marketId, bank, at) {
  const c = bank.config, spec = bank.universe.exchanges.find(x => x.marketId === marketId && x.option === 'YES');
  need(snap.endpoint === '/markets/' + marketId + '/orderbook?tournamentId=' + c.tournamentId + '&depth=20', 'Foreign book endpoint');
  need(canonical(snap.receivedAt) && canonical(snap.requestStartedAt) && Date.parse(snap.receivedAt) <= Date.parse(at), 'Invalid public receipt');
  // Holdings are metadata, never a reason to delete otherwise valid public prices.
  const b = normalizeBook(snap.data, marketId, c.tournamentId, snap.receivedAt, snap.requestStartedAt, c.maxDataAgeMs, new Set(), 'authoritative_receipt_v1');
  need(b.exchangeId === spec.exchangeId, 'Foreign public exchange');
  const old = s.seen[marketId], content = digest({bids: b.bids, asks: b.asks});
  if (old) {
    need(Date.parse(b.requestStartedAt) >= Date.parse(old.requestStartedAt) && Date.parse(b.receivedAt) >= Date.parse(old.receivedAt)
      && BigInt(b.sequence) >= BigInt(old.sequence) && instantNs(b.sourceAt) >= instantNs(old.sourceAt), 'Public book rollback');
    if (b.sequence === old.sequence) need(b.versionKey === old.versionKey && content === old.content, 'Changed book under same sequence');
  }
  if (s.invalidatedAt[marketId]) need(Date.parse(b.requestStartedAt) > Date.parse(s.invalidatedAt[marketId]), 'Pre-invalidation public book');
  const independent = !old || (b.versionKey !== old.versionKey && instantNs(b.sourceAt) > instantNs(old.sourceAt));
  s.seen[marketId] = {requestStartedAt: b.requestStartedAt, receivedAt: b.receivedAt, sequence: b.sequence, sourceAt: b.sourceAt, versionKey: b.versionKey, content};
  delete s.invalidatedAt[marketId]; return {book: b, independent};
}

function runWallet(s, bank, name, b, independent, contamination, gate, record) {
  const v = s.strategies[name], c = bank.config, p = bank.sharedExecution, id = b.instrument;
  const st = v.wallet.strategies.mean_reversion, held = Boolean(st.positions[id]), fillsBefore = st.fills.length;
  const used = st.fills.filter(f => f.action === 'buy').reduce((n, f) => n + amount(f.price) * BigInt(f.quantity), 0n);
  const ask = amount(b.asks[0].price) + TICK;
  const cap = Math.max(0, Math.min(10, gate.quantity ?? 10, Number((amount(p.cumulativeBuyBudget) - used) / ask),
    Number(amount(p.maxMarketCost) / ask), Math.floor(b.asks[0].quantity / 100), Math.floor(b.bids[0].quantity / 100)));
  const occupied = new Set([...Object.keys(st.positions), ...Object.keys(st.pending)]);
  let reason = !gate.admitted ? 'native_signal_rejected' : !independent ? 'no_independent_quote'
    : !cap ? 'depth_or_budget' : !occupied.has(id) && occupied.size >= p.maxOpenMarkets ? 'maximum_open_markets'
    : Date.parse(v.cooldowns[b.marketId] || '') > Date.parse(b.receivedAt) ? 'cooldown' : 'allowed';
  if (st.pending[id]?.action === 'buy') {
    if (Date.parse(b.receivedAt) - Date.parse(st.pending[id].createdAt) > p.entryLifetimeMs) {
      delete st.pending[id]; st.expiredIntents++; reason = 'entry_expired';
    } else st.pending[id].quantity = Math.min(st.pending[id].quantity, cap);
  }
  if (independent) { count(v.executionGateCounts, reason); count(v.contaminationCounts, contamination); }
  const priorConsumed = st.consumedVersions[id];
  if (!independent) st.consumedVersions[id] = b.versionKey;
  // Native admission above remains authoritative (shock uses exact micro money).
  // The legacy executor repeats the edge subtraction in binary floats. Allow
  // only roundoff in that duplicate check, never a new native gate admission.
  v.wallet = stepDip(v.wallet, b, c, {reference: gate.reference, allowEntry: reason === 'allowed',
    minimumEdge: gate.minimumEdge - p.duplicateEdgeCheckRoundoffTolerance,
    takeProfit: p.takeProfit, horizonMs: p.maximumHoldMs, maxMarketCost: p.maxMarketCost, executionPolicy: 'responsive_v2'});
  const next = v.wallet.strategies.mean_reversion;
  if (!independent) { if (priorConsumed === undefined) delete next.consumedVersions[id]; else next.consumedVersions[id] = priorConsumed; }
  if (next.pending[id]?.action === 'buy') {
    next.pending[id].quantity = Math.min(next.pending[id].quantity, cap);
    next.pending[id].reason = name; next.pending[id].signalSourceSequence ??= record.sequence;
    next.pending[id].signalSourceHash ??= record.hash; next.pending[id].signalVersion ??= b.versionKey;
  }
  const intent = next.pending[id];
  if (intent && !v.decisions[intent.id]) v.decisions[intent.id] = {intentId: intent.id, action: intent.action,
    reason: intent.reason, quantity: intent.quantity, limit: intent.limit, createdAt: intent.createdAt,
    sourceSequence: record.sequence, sourceHash: record.hash, sourceVersion: b.versionKey,
    receivedAt: b.receivedAt, requestStartedAt: b.requestStartedAt, contamination,
    nativeSignal: clone(gate.diagnostic), nativeSignalHash: digest(gate.diagnostic),
    reference: gate.reference, minimumEdge: gate.minimumEdge};
  for (let i = fillsBefore; i < next.fills.length; i++) v.fillAttribution.push({fillIndex: i, sourceSequence: record.sequence,
    sourceHash: record.hash, receivedAt: b.receivedAt, sourceVersion: b.versionKey, contamination, intentId: next.fills[i].intentId,
    model: 'hypothetical_public_book_not_official_execution'});
  if (held && !next.positions[id]) v.cooldowns[b.marketId] = new Date(Date.parse(b.receivedAt) + p.cooldownMs).toISOString();
  if (name === 'shock_rebound' && (next.positions[id] || reason !== 'allowed')) delete s.proposals[b.marketId];
  need(next.fills.filter(f => f.action === 'buy').reduce((n, f) => n + amount(f.price) * BigInt(f.quantity), 0n) <= amount(p.cumulativeBuyBudget), 'Comparison cumulative budget exceeded');
  assertState(v.wallet, c);
}

export function advanceEntryComparison(input, record, bank) {
  checkBank(bank); need(input.bankHash === digest(bank), 'Comparison state bank mismatch');
  const {hash, ...body} = record, at = record.event?.clockAt || record.at;
  need(record.sequence === input.sourceSequence + 1 && record.previous === input.lastRecordHash && hex(hash) && digest(body) === hash,
    'Comparison source continuity failure');
  need(canonical(at) && (!input.lastSourceAt || Date.parse(at) >= Date.parse(input.lastSourceAt)), 'Comparison source clock rollback');
  if (record.sequence <= bank.sourceRecords) need(Date.parse(at) <= Date.parse(bank.cutoff), 'Source prefix beyond cutoff');
  const s = clone(input), e = record.event, prospective = record.sequence > bank.sourceRecords && Date.parse(at) > Date.parse(bank.cutoff);
  s.sourceSequence = record.sequence; s.lastRecordHash = hash; s.lastSourceAt = at;
  if (e.type === 'market_history') {
    const h = e.history, spec = bank.universe.exchanges.find(x => x.marketId === h?.marketId && ENTRY_COMPARISON_MARKETS.includes(x.marketId));
    if (spec) {
      need(h.exchangeId === spec.exchangeId && h.tournamentId === bank.config.tournamentId && canonical(h.receivedAt) && canonical(h.requestStartedAt)
        && Date.parse(h.requestStartedAt) <= Date.parse(h.receivedAt) && Date.parse(h.receivedAt) - Date.parse(h.requestStartedAt) <= 20000
        && Date.parse(h.receivedAt) <= Date.parse(at) && Array.isArray(h.candles) && h.candles.length <= 500, 'Invalid comparison history');
      s.histories[h.exchangeId] = clone(h);
    }
    return s;
  }
  if (e.type === 'account_read_outage') { if (prospective) s.counters.accountOutages++; revoke(s, bank, null, at, 'account_outage'); return s; }
  if (['stopped', 'started', 'session_start', 'session_stop'].includes(e.type)) {
    if (prospective) count(s.counters, 'collectorBarriers'); revoke(s, bank, null, at, 'collector_barrier'); return s;
  }
  if (e.type !== 'cycle') return s;
  const acceptedFresh = new Map();
  const snapshots = [...(e.sourceSnapshots || []), ...(e.supplementalDepth || [])].map((snap, index) => ({snap, index,
    at: snap.skipped ? snap.at : snap.receivedAt})).sort((a, b) => (canonical(a.at) ? Date.parse(a.at) : Infinity) - (canonical(b.at) ? Date.parse(b.at) : Infinity) || a.index - b.index);
  for (const {snap} of snapshots) {
    const marketId = snap.marketId || /^\/markets\/([1-9]\d*)\/orderbook\?/.exec(snap.endpoint || '')?.[1];
    if (!ENTRY_COMPARISON_MARKETS.includes(marketId)) continue;
    if (snap.skipped) {
      // Collector exclusion diagnostics sometimes accompany the captured valid
      // public response. They do not invalidate the research public book.
      const accepted = acceptedFresh.get(marketId);
      if (snap.skipped === 'Exchange has real account holdings or open orders' && accepted
        && Array.isArray(e.accountExclusions) && e.accountExclusions.includes(accepted.exchangeId)
        && canonical(snap.at) && Date.parse(snap.at) >= Date.parse(accepted.receivedAt) && Date.parse(snap.at) <= Date.parse(at)) {
        if (prospective) count(s.counters, 'exclusionDiagnosticsRetained'); continue;
      }
      if (prospective) s.counters.skippedBooks++;
      revoke(s, bank, marketId, canonical(snap.at) && Date.parse(snap.at) <= Date.parse(at) ? snap.at : at, 'read_skipped'); continue;
    }
    let b, independent, shock;
    try {
      ({book: b, independent} = checkedPublicBook(s, snap, marketId, bank, at));
      shock = advanceShockRebound(s.shocks[marketId].state, b, s.shocks[marketId].bank);
    } catch { if (prospective) s.counters.invalidBooks++; revoke(s, bank, marketId, at, 'invalid_public_book'); continue; }
    acceptedFresh.set(marketId, b);
    s.shocks[marketId].state = shock.state;
    const knownContext = Array.isArray(e.accountExclusions) && e.accountExclusions.every(x => typeof x === 'string')
      && canonical(e.exclusionObservedAt) && Date.parse(e.exclusionObservedAt) <= Date.parse(b.requestStartedAt);
    const freshContext = knownContext && Date.parse(b.requestStartedAt) - Date.parse(e.exclusionObservedAt) <= bank.sharedExecution.accountContextMaxAgeMs;
    const contamination = !freshContext ? 'unknown_account_context' : e.accountExclusions.includes(b.exchangeId) ? 'account_exposure_or_orders' : 'no_reported_account_exposure';
    const decision = prospective && Date.parse(b.requestStartedAt) > Date.parse(bank.cutoff);
    if (!decision) s.counters.developmentBooks++;
    else {
      s.counters.prospectiveBooks++; if (independent) s.counters.independentBooks++;
      if (contamination === 'account_exposure_or_orders') s.counters.contaminatedBooks++;
      if (contamination === 'unknown_account_context') s.counters.unknownContaminationBooks++;
      const c = bank.config, reference = historicalReference(s.histories[b.exchangeId], s.bookHistory[b.exchangeId], b.requestStartedAt, bank.policy);
      const dip = dipSignal(reference, Number(amount(b.asks[0].price) + TICK) / 1e6, Number(amount(b.bids[0].price) - TICK) / 1e6, bank.policy);
      if (shock.signal) s.proposals[marketId] = clone(shock.signal);
      let proposal = s.proposals[marketId];
      const riskRevoked = proposal && (amount(b.bids[0].price) < amount(proposal.troughBid)
        || 2n * amount(proposal.featuresAtShock.anchorMid) - amount(b.bids[0].price) - amount(b.asks[0].price) > 2n * amount(s.shocks[marketId].bank.config.maximumShock)
        || ['maximum_shock_breached', 'insufficient_history', 'trend_veto', 'missing_shock_anchor', 'insufficient_trend_history'].includes(shock.diagnostic.reason));
      if (riskRevoked) {
        s.shocks[marketId].state = invalidateShockRebound(s.shocks[marketId].state, s.shocks[marketId].bank, b.receivedAt);
        shock.diagnostic = {reason: 'proposal_risk_revoked', at: b.receivedAt};
      }
      if (proposal && (riskRevoked || Date.parse(b.receivedAt) - Date.parse(proposal.at) > bank.sharedExecution.entryLifetimeMs)) {
        delete s.proposals[marketId]; proposal = undefined;
      }
      const sc = s.shocks[marketId].bank.config, ask = amount(b.asks[0].price) + TICK, bid = amount(b.bids[0].price) - TICK;
      const shockAllowed = Boolean(proposal && ask <= amount(proposal.maximumEntryPrice)
        && ask - bid <= amount(sc.maximumEffectiveSpread) && amount(proposal.recoveryReferenceBid) - TICK - ask >= amount(sc.minimumRoom));
      const gates = {historical_dip: {admitted: dip.admitted, reference: reference.ready ? reference.center : null,
        minimumEdge: bank.policy.minimumDip, reason: dip.reason, diagnostic: dip},
      shock_rebound: {admitted: shockAllowed, reference: proposal ? Number(amount(proposal.recoveryReferenceBid) - TICK) / 1e6 : null,
        minimumEdge: Number(amount(sc.minimumRoom)) / 1e6, quantity: proposal?.quantity, reason: shock.diagnostic.reason, diagnostic: shock.diagnostic}};
      for (const name of variants) {
        const v = s.strategies[name], gate = gates[name];
        if (independent) { v.uniqueObservations++; count(v.gateCounts, gate.reason); if (name === 'historical_dip' ? gate.admitted : Boolean(shock.signal)) v.acceptedSignals++; }
        v.lastSignals[marketId] = {at: b.receivedAt, sourceVersion: b.versionKey, contamination, admitted: gate.admitted, ...clone(gate.diagnostic)};
        runWallet(s, bank, name, b, independent, contamination, gate, record);
      }
    }
    if (independent) {
      const list = s.bookHistory[b.exchangeId] || [], mid = Number(amount(b.bids[0].price) + amount(b.asks[0].price)) / (2 * Number(SCALE));
      s.bookHistory[b.exchangeId] = [...list, {at: b.sourceAt, receivedAt: b.receivedAt, mid, versionKey: b.versionKey}]
        .filter(x => Date.parse(x.at) >= Date.parse(b.receivedAt) - bank.policy.lookbackMs).slice(-512);
    }
  }
  if (prospective) for (const v of Object.values(s.strategies)) sampleEquity(v.wallet, bank.config, at);
  return s;
}

export function entryComparisonReport(state, bank, at) {
  checkBank(bank); need(state.bankHash === digest(bank) && canonical(at) && (!state.lastSourceAt || Date.parse(at) >= Date.parse(state.lastSourceAt)), 'Invalid comparison report');
  const strategies = {};
  for (const name of variants) {
    const v = state.strategies[name], raw = v.wallet.strategies.mean_reversion, metrics = report(v.wallet, bank.config, at).strategies.mean_reversion;
    const label = instrument => { const marketId = v.wallet.books[instrument]?.marketId; return bank.identities[marketId]?.title || 'Market ' + marketId + ' YES'; };
    strategies[name] = {...metrics, positions: metrics.positions.map(p => ({...p, label: label(p.instrument)})),
      recentFills: metrics.recentFills.map(f => ({...f, label: label(f.instrument)})),
      pending: Object.entries(raw.pending).map(([instrument, intent]) => ({instrument, label: label(instrument), ...clone(intent)})),
      cumulativeBuyCost: money(raw.fills.filter(f => f.action === 'buy').reduce((n, f) => n + amount(f.price) * BigInt(f.quantity), 0n)),
      uniqueObservations: v.uniqueObservations, acceptedSignals: v.acceptedSignals, gateCounts: clone(v.gateCounts),
      executionGateCounts: clone(v.executionGateCounts), contaminationCounts: clone(v.contaminationCounts), lastSignals: clone(v.lastSignals),
      recentFillAttribution: clone(v.fillAttribution.slice(-20))};
  }
  return {schemaVersion: 1, mode: 'prospective_common_feed_entry_comparison_paper', cohortId: bank.name + ':' + bank.cutoff,
    updatedAt: at, bankHash: state.bankHash, cutoff: bank.cutoff, sourceSequence: state.sourceSequence, sourceHash: state.lastRecordHash,
    marketIds: [...ENTRY_COMPARISON_MARKETS], counters: clone(state.counters), sharedExecution: clone(bank.sharedExecution), strategies,
    networkRequests: 0, competitionOrders: 0, profitabilityEstablished: false,
    assumptions: ['Separate hypothetical wallets reuse the same public observation; their capacity is not additive.',
      'Exposure/open-order contamination and unknown context are labeled, not erased; public depth may contain own orders.',
      'One adverse tick per executed side, zero added fees, one percent displayed depth, causal independent next-book fills.',
      'Native dip and shock gates differ; execution costs, capacity, holding horizon and exit policy are shared.',
      'Only post-cutoff decisions/fills count; sparse sampling cannot establish unobserved price crossings.']};
}
