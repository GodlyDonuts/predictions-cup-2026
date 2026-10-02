import {digest, validateConfig, initialState, normalizeBook, invalidate, assertState, report, sampleEquity} from './model.mjs';
import {historicalReference} from './history.mjs';
import {dipSignal, validateDipPolicy} from './dip-shadow.mjs';
import {amount, money, TICK, SCALE} from '../paper/fixed.mjs';
import {instantNs} from '../feed/contract.mjs';

export const LIMIT_ENTRY_MARKETS = Object.freeze(['377', '354', '277', '305']);
export const LIMIT_ENTRY_EXECUTION = Object.freeze({name: 'price_protected_observed_marketable_v1', feeBps: 0,
  slippageTicks: 1, depthFractionBps: 100, orderShares: 10, maxMarketCost: '25.000000', maxExposure: '100.000000',
  cumulativeBuyBudget: '100.000000', maxOpenMarkets: 3, latencyMs: 1000, entryLifetimeMs: 30000,
  maximumQuoteAgeMs: 20000, accountContextMaxAgeMs: 180000, takeProfit: '0.010000', stopLoss: '0.030000',
  maximumHoldMs: 3600000, cooldownMs: 60000,
  admission: 'unchanged_native_padded_price_gate; hybrid_adds_conditional_one_tick_lower_limit',
  execution: 'later_independent_marketable_asks; adverse_tick_capped_at_frozen_actual_order_limit',
  evidence: 'counterfactual_executable_opportunity_at_later_receipt_not_proof_of_resting_fill_or_queue_priority'});
const names = ['immediate_dip', 'price_protected_hybrid'], clone = structuredClone;
const hex = x => typeof x === 'string' && /^[a-f0-9]{64}$/.test(x);
const clock = x => typeof x === 'string' && Number.isFinite(Date.parse(x)) && new Date(x).toISOString() === x;
const need = (ok, message) => { if (!ok) throw Error(message); };
const count = (object, key) => { object[key] = (object[key] || 0) + 1; };
const iso = ms => new Date(ms).toISOString();
const ledger = v => v.wallet.strategies.mean_reversion;
const spent = st => st.fills.filter(f => f.action === 'buy').reduce((n, f) => n + amount(f.price) * BigInt(f.quantity), 0n);

function checkBank(bank) {
  need(bank?.schemaVersion === 1 && bank.name === 'limit-entry-experiment-v1' && Number.isSafeInteger(bank.sourceRecords)
    && bank.sourceRecords > 0 && hex(bank.sourceHash) && clock(bank.cutoff), 'Invalid limit-entry bank');
  validateConfig(bank.config); validateDipPolicy(bank.policy);
  need(digest(bank.execution) === digest(LIMIT_ENTRY_EXECUTION) && digest(bank.config.marketIds) === digest(LIMIT_ENTRY_MARKETS)
    && bank.config.feeBps === 0 && bank.config.slippageTicks === 1 && bank.config.depthFractionBps === 100
    && bank.config.orderShares === 10 && bank.config.latencyMs === 1000 && bank.config.maxDataAgeMs <= 20000
    && bank.config.maxExposure === '100.000000' && amount(bank.config.cashReserve) === amount(bank.config.initialCash) - 100000000n,
  'Changed limit-entry execution contract');
  need(bank.universe?.tournamentId === bank.config.tournamentId && Array.isArray(bank.universe.exchanges), 'Invalid limit-entry universe');
  need(new Set(bank.universe.exchanges.map(x => x.exchangeId)).size === bank.universe.exchanges.length, 'Duplicate exchange');
  for (const marketId of LIMIT_ENTRY_MARKETS) {
    const matches = bank.universe.exchanges.filter(x => x.marketId === marketId && x.option === 'YES');
    need(matches.length === 1 && /^[1-9]\d*$/.test(matches[0].exchangeId), 'Missing scoped comparison market');
  }
  need(bank.codeIdentity && Object.keys(bank.codeIdentity).length > 0 && Object.values(bank.codeIdentity).every(hex), 'Missing code identity');
}

export function createLimitEntry({sourceRecords, sourceHash, cutoff, config, universe, identities = {}, policy, codeIdentity}) {
  const c = {...clone(config), marketIds: [...LIMIT_ENTRY_MARKETS], feeBps: 0, slippageTicks: 1, depthFractionBps: 100,
    orderShares: 10, maxPositionShares: 50, latencyMs: 1000, maxExposure: '100.000000',
    cashReserve: money(amount(config.initialCash) - 100000000n), stopLoss: '0.030000', exitAfterMs: 3600000};
  need(amount(c.initialCash) > 100000000n, 'Insufficient comparison cash');
  const bank = {schemaVersion: 1, name: 'limit-entry-experiment-v1', sourceRecords, sourceHash, cutoff, config: c,
    universe: clone(universe), identities: clone(identities), policy: clone(policy), codeIdentity: clone(codeIdentity), execution: clone(LIMIT_ENTRY_EXECUTION)};
  checkBank(bank); return {bank, state: emptyLimitEntryState(bank)};
}

export function emptyLimitEntryState(bank) {
  checkBank(bank);
  return {schemaVersion: 1, bankHash: digest(bank), sourceSequence: 0, lastRecordHash: '', lastSourceAt: null, lastAvailableAt: null,
    histories: {}, bookHistory: {}, seen: {}, invalidatedAt: {}, counters: {developmentBooks: 0, prospectiveBooks: 0,
      independentBooks: 0, invalidBooks: 0, staleAtDecisionBooks: 0, skippedBooks: 0, collectorBarriers: 0, accountOutages: 0},
    strategies: Object.fromEntries(names.map(name => { const wallet = initialState(bank.config, bank.name + ':' + name, bank.cutoff);
      delete wallet.strategies.buy_hold_benchmark;
      return [name, {wallet, uniqueObservations: 0, actualNativeAdmissions: 0, conditionalLimitAdmissions: 0,
        gateCounts: {}, executionGateCounts: {}, contaminationCounts: {}, lastSignals: {}, decisions: {}, outcomes: {},
        exitObligations: {}, cooldowns: {}}]; }))};
}

function finish(v, id, at, reason) {
  const st = ledger(v), p = st.pending[id]; if (!p) return;
  const o = v.outcomes[p.id]; o.status = reason; o.resolvedAt = at; o.unfilledQuantity = p.quantity;
  if (p.action === 'buy' && reason === 'expired') {
    o.censoring = o.independentInWindowReceipts === 0 ? 'no_independent_in_window_quote' : 'observed_without_full_execution';
    count(v.executionGateCounts, o.independentInWindowReceipts === 0 ? 'entry_expired_unobserved' : 'entry_expired_observed');
  }
  if (reason !== 'filled') st.expiredIntents++;
  delete st.pending[id];
}

function revoke(s, bank, marketId, at, reason) {
  for (const id of LIMIT_ENTRY_MARKETS.filter(x => marketId === null || marketId === x)) {
    s.invalidatedAt[id] = at;
    for (const v of Object.values(s.strategies)) {
      invalidate(v.wallet, id, at, reason);
      for (const [instrument, pending] of Object.entries(ledger(v).pending))
        if (pending.action === 'buy' && pending.marketId === id) finish(v, instrument, at, 'invalidated_' + reason);
    }
  }
}

function checkedBook(s, snap, id, bank, availableAt) {
  need(snap.endpoint === '/markets/' + id + '/orderbook?tournamentId=' + bank.config.tournamentId + '&depth=20'
    && clock(snap.receivedAt) && clock(snap.requestStartedAt) && Date.parse(snap.receivedAt) <= Date.parse(availableAt), 'Invalid book scope/receipt');
  const b = normalizeBook(snap.data, id, bank.config.tournamentId, snap.receivedAt, snap.requestStartedAt,
    bank.config.maxDataAgeMs, new Set(), 'authoritative_receipt_v1');
  need(b.exchangeId === bank.universe.exchanges.find(x => x.marketId === id && x.option === 'YES').exchangeId, 'Foreign exchange');
  const old = s.seen[id], content = digest({bids: b.bids, asks: b.asks});
  if (old) {
    need(Date.parse(b.requestStartedAt) >= Date.parse(old.requestStartedAt) && Date.parse(b.receivedAt) >= Date.parse(old.receivedAt)
      && BigInt(b.sequence) >= BigInt(old.sequence) && instantNs(b.sourceAt) >= instantNs(old.sourceAt), 'Book rollback');
    if (b.sequence === old.sequence) need(b.versionKey === old.versionKey && content === old.content, 'Same sequence changed identity/content');
  }
  if (s.invalidatedAt[id]) need(Date.parse(b.requestStartedAt) > Date.parse(s.invalidatedAt[id]), 'Pre-invalidation book');
  const independent = !old || b.versionKey !== old.versionKey && instantNs(b.sourceAt) > instantNs(old.sourceAt);
  s.seen[id] = {sequence: b.sequence, sourceAt: b.sourceAt, versionKey: b.versionKey, content,
    requestStartedAt: b.requestStartedAt, receivedAt: b.receivedAt};
  return {b, independent};
}

function enqueue(v, b, record, at, action, quantity, limit, reason, reference, signal, contamination, bank) {
  const st = ledger(v), id = ++st.intents;
  const pending = {id, marketId: b.marketId, instrument: b.instrument, action, quantity, limit: money(limit), reason,
    createdAt: at, expiresAt: action === 'buy' ? iso(Date.parse(at) + bank.execution.entryLifetimeMs) : null,
    sourceVersion: b.versionKey};
  st.pending[b.instrument] = pending;
  v.decisions[id] = {...clone(pending), quantity, sourceSequence: record.sequence, sourceHash: record.hash,
    quoteReceivedAt: b.receivedAt, requestStartedAt: b.requestStartedAt, reference: clone(reference ?? null),
    admission: clone(signal ?? null), contamination};
  v.outcomes[id] = {status: 'open', filledQuantity: 0, unfilledQuantity: quantity,
    independentInWindowReceipts: 0, marketableReceipts: 0, firstEligibleReceiptAt: null, lastEligibleReceiptAt: null};
}

// This is an observed counterfactual taker opportunity. A resting order's queue
// position, earlier execution, and cancellation-driven depth changes are unknown.
function fill(v, b, independent, record, bank) {
  const st = ledger(v), p = st.pending[b.instrument]; if (!p || !independent || st.consumedVersions[b.instrument] === b.versionKey
    || b.versionKey === p.sourceVersion || Date.parse(b.requestStartedAt) < Date.parse(p.createdAt) + bank.execution.latencyMs) return;
  if (p.action === 'buy' && Date.parse(b.receivedAt) >= Date.parse(p.expiresAt)) return;
  const outcome = v.outcomes[p.id]; outcome.independentInWindowReceipts++;
  outcome.firstEligibleReceiptAt ??= b.receivedAt; outcome.lastEligibleReceiptAt = b.receivedAt;
  const limit = amount(p.limit), rows = p.action === 'buy' ? b.asks : b.bids;
  if (p.action === 'buy' ? amount(rows[0].price) > limit : amount(rows[0].price) < limit) return;
  outcome.marketableReceipts++;
  let total = 0;
  for (const level of rows) {
    const raw = amount(level.price);
    if (p.action === 'buy' ? raw > limit : raw < limit) break;
    const price = p.action === 'buy' ? (raw + TICK > limit ? limit : raw + TICK) : (raw - TICK < limit ? limit : raw - TICK);
    if (price < TICK || price > SCALE - TICK) break;
    let quantity = Math.min(p.quantity, Math.floor(level.quantity / 100));
    if (p.action === 'buy') {
      const exposure = Object.values(st.positions).reduce((n, pos) => n + amount(pos.cost), 0n);
      quantity = Math.min(quantity, bank.config.maxPositionShares - (st.positions[b.instrument]?.quantity || 0),
        Number((amount(st.cash) - amount(bank.config.cashReserve)) / price),
        Number((amount(bank.execution.maxExposure) - exposure) / price),
        Number((amount(bank.execution.cumulativeBuyBudget) - spent(st)) / price));
    } else quantity = Math.min(quantity, st.positions[b.instrument]?.quantity || 0);
    if (quantity < 1) continue;
    const notional = price * BigInt(quantity);
    if (p.action === 'buy') {
      const old = st.positions[b.instrument];
      st.positions[b.instrument] = {quantity: (old?.quantity || 0) + quantity, cost: money(amount(old?.cost || '0') + notional),
        enteredAt: old?.enteredAt || b.receivedAt, lots: [...(old?.lots || []), {quantity, cost: money(notional)}]};
      st.cash = money(amount(st.cash) - notional);
    } else {
      const pos = st.positions[b.instrument]; let left = quantity, basis = 0n;
      while (left) { const lot = pos.lots[0], q = Math.min(left, lot.quantity), cost = q === lot.quantity ? amount(lot.cost) : amount(lot.cost) * BigInt(q) / BigInt(lot.quantity);
        lot.quantity -= q; lot.cost = money(amount(lot.cost) - cost); basis += cost; left -= q; if (!lot.quantity) pos.lots.shift(); }
      pos.quantity -= quantity; pos.cost = money(amount(pos.cost) - basis);
      st.cash = money(amount(st.cash) + notional); st.realized = money(amount(st.realized) + notional - basis);
      if (!pos.quantity) { delete st.positions[b.instrument]; delete v.exitObligations[b.instrument];
        st.closedTrades++; v.cooldowns[b.marketId] = iso(Date.parse(b.receivedAt) + bank.execution.cooldownMs); }
    }
    st.turnover = money(amount(st.turnover) + notional);
    st.fills.push({intentId: p.id, instrument: b.instrument, marketId: b.marketId, action: p.action, quantity, price: money(price),
      fee: '0.000000', at: b.receivedAt, evidenceAvailableAt: record.availableAt, requestStartedAt: b.requestStartedAt,
      sourceAt: b.sourceAt, sourceVersion: b.versionKey, sourceSequence: record.sequence, sourceHash: record.hash,
      decisionType: v.decisions[p.id].reason, model: 'counterfactual_later_marketable_quote_not_resting_execution', depthFractionBps: 100, slippageTicks: 1});
    p.quantity -= quantity; total += quantity; outcome.filledQuantity += quantity; outcome.unfilledQuantity = p.quantity;
    if (!p.quantity) break;
  }
  if (total) { st.consumedVersions[b.instrument] = b.versionKey; outcome.status = p.quantity ? 'partially_filled' : 'filled';
    if (!p.quantity) finish(v, b.instrument, b.receivedAt, 'filled'); }
}

function run(v, name, b, independent, record, bank, reference, contamination, {evidence = true, decide = true} = {}) {
  const st = ledger(v), at = record.availableAt, id = b.instrument, bid = amount(b.bids[0].price), ask = amount(b.asks[0].price),
    current = dipSignal(reference, Number(ask + TICK) / 1e6, Number(bid - TICK) / 1e6, bank.policy),
    parkedPrice = ask - TICK, parked = parkedPrice >= TICK ? dipSignal(reference, Number(parkedPrice + TICK) / 1e6, Number(bid - TICK) / 1e6, bank.policy) : {admitted: false};
  if (evidence) { v.wallet.books[id] = clone(b); delete v.wallet.invalidated?.[id]; v.wallet.lastAt = at; v.wallet.observations++; }
  if (evidence && independent) { v.uniqueObservations++; if (current.admitted) v.actualNativeAdmissions++;
    if (!current.admitted && parked.admitted) v.conditionalLimitAdmissions++;
    count(v.gateCounts, current.reason); count(v.contaminationCounts, contamination); }
  v.lastSignals[b.marketId] = {at, quoteReceivedAt: b.receivedAt, currentNative: clone(current), conditionalLimit: clone(parked), proposedParkedLimit: money(parkedPrice), contamination};
  const heldAtStart = Boolean(st.positions[id]), pendingAtStart = Boolean(st.pending[id]);
  // Committed orders retain adverse outcomes: never re-run native gates as a
  // veto on an otherwise causal marketable crossing of their frozen price.
  if (evidence) fill(v, b, independent, record, bank);
  if (!decide) return;
  const pos = st.positions[id], paddedBid = bid - TICK;
  if (pos) {
    const average = amount(pos.cost) / BigInt(pos.quantity);
    if (!v.exitObligations[id]) {
      const reason = average - paddedBid >= amount(bank.execution.stopLoss) ? 'stop_loss'
        : Date.parse(at) - Date.parse(pos.enteredAt) >= bank.execution.maximumHoldMs ? 'holding_timeout' : null;
      if (reason) v.exitObligations[id] = {reason, triggeredAt: at};
    }
    const risk = v.exitObligations[id], pending = st.pending[id], exitLimit = paddedBid < TICK ? TICK : paddedBid;
    if (risk && (!pending || pending.action === 'buy' || pending.reason !== risk.reason || exitLimit < amount(pending.limit))) {
      if (pending) finish(v, id, at, 'risk_replaced');
      enqueue(v, b, record, at, 'sell', pos.quantity, exitLimit, risk.reason, null, null, contamination, bank);
    } else if (!pending && paddedBid - average >= amount(bank.execution.takeProfit))
      enqueue(v, b, record, at, 'sell', pos.quantity, exitLimit, 'take_profit', null, null, contamination, bank);
    return;
  }
  if (heldAtStart || pendingAtStart || st.pending[id] || !independent) return;
  const decisionType = current.admitted ? 'immediate' : name === 'price_protected_hybrid' && parked.admitted ? 'conditional_park' : null;
  if (!decisionType) return;
  if (Date.parse(v.cooldowns[b.marketId] || '') > Date.parse(at)) { count(v.executionGateCounts, 'cooldown'); return; }
  const occupied = new Set([...Object.keys(st.positions), ...Object.keys(st.pending)]);
  if (occupied.size >= bank.execution.maxOpenMarkets) { count(v.executionGateCounts, 'maximum_open_markets'); return; }
  const limit = decisionType === 'immediate' ? ask : parkedPrice,
    reserved = Object.values(st.pending).filter(p => p.action === 'buy').reduce((n, p) => n + amount(p.limit) * BigInt(p.quantity), 0n),
    exposure = Object.values(st.positions).reduce((n, p) => n + amount(p.cost), 0n);
  const quantity = Math.max(0, Math.min(10, Math.floor(b.bids[0].quantity / 100), Math.floor(b.asks[0].quantity / 100),
    Number(amount(bank.execution.maxMarketCost) / limit), Number((amount(bank.execution.cumulativeBuyBudget) - spent(st) - reserved) / limit),
    Number((amount(bank.execution.maxExposure) - exposure - reserved) / limit), Number((amount(st.cash) - amount(bank.config.cashReserve) - reserved) / limit)));
  if (!quantity) { count(v.executionGateCounts, 'depth_or_budget'); return; }
  enqueue(v, b, record, at, 'buy', quantity, limit, decisionType, reference, decisionType === 'immediate' ? current : parked, contamination, bank);
  count(v.executionGateCounts, decisionType + '_order_created');
}

export function advanceLimitEntry(input, record, bank, {availableAt} = {}) {
  checkBank(bank); need(input.bankHash === digest(bank), 'Limit-entry bank mismatch');
  const {hash, ...body} = record, eventAt = record.event?.clockAt || record.at;
  need(record.sequence === input.sourceSequence + 1 && record.previous === input.lastRecordHash && hex(hash) && digest(body) === hash, 'Source continuity failure');
  need(clock(record.at) && clock(eventAt), 'Invalid source clock');
  const sourceAt = iso(Math.max(Date.parse(record.at), Date.parse(eventAt))), prefix = record.sequence <= bank.sourceRecords;
  need(!input.lastSourceAt || Date.parse(sourceAt) >= Date.parse(input.lastSourceAt), 'Source clock rollback');
  if (prefix) need(Date.parse(sourceAt) <= Date.parse(bank.cutoff), 'Prefix beyond cutoff');
  need(prefix && availableAt === undefined || clock(availableAt), 'Prospective source requires host availability clock');
  const at = iso(Math.max(Date.parse(sourceAt), Date.parse(availableAt ?? bank.cutoff)));
  need(!input.lastAvailableAt || Date.parse(at) >= Date.parse(input.lastAvailableAt), 'Host availability clock rollback');
  const s = clone(input), e = record.event, prospective = !prefix && Date.parse(sourceAt) > Date.parse(bank.cutoff);
  s.sourceSequence = record.sequence; s.lastRecordHash = hash; s.lastSourceAt = sourceAt; s.lastAvailableAt = at;
  if (e.type === 'market_history') {
    const h = e.history, spec = bank.universe.exchanges.find(x => LIMIT_ENTRY_MARKETS.includes(x.marketId) && x.marketId === h?.marketId);
    if (spec) { need(h.exchangeId === spec.exchangeId && h.tournamentId === bank.config.tournamentId && clock(h.receivedAt) && clock(h.requestStartedAt)
      && Date.parse(h.requestStartedAt) <= Date.parse(h.receivedAt) && Date.parse(h.receivedAt) - Date.parse(h.requestStartedAt) <= 20000
      && Date.parse(h.receivedAt) <= Date.parse(sourceAt) && Array.isArray(h.candles) && h.candles.length <= 500, 'Invalid historical evidence'); s.histories[h.exchangeId] = clone(h); }
  } else if (e.type === 'account_read_outage') { if (prospective) s.counters.accountOutages++; revoke(s, bank, null, sourceAt, 'account_outage'); }
  else if (['started', 'stopped', 'session_start', 'session_stop'].includes(e.type)) { if (prospective) s.counters.collectorBarriers++; revoke(s, bank, null, sourceAt, 'collector_barrier'); }
  else if (e.type === 'cycle') {
    const accepted = new Map(), decisions = new Map(), snapshots = [...(e.sourceSnapshots || []), ...(e.supplementalDepth || [])]
      .map((snap, index) => ({snap, index, at: snap.skipped ? snap.at : snap.receivedAt}))
      .sort((a, b) => (clock(a.at) ? Date.parse(a.at) : Infinity) - (clock(b.at) ? Date.parse(b.at) : Infinity) || a.index - b.index);
    for (const {snap} of snapshots) {
      const id = snap.marketId || /^\/markets\/([1-9]\d*)\/orderbook\?/.exec(snap.endpoint || '')?.[1];
      if (!LIMIT_ENTRY_MARKETS.includes(id)) continue;
      if (snap.skipped) {
        const prior = accepted.get(id);
        if (snap.skipped === 'Exchange has real account holdings or open orders' && prior && e.accountExclusions?.includes(prior.exchangeId)
          && clock(snap.at) && Date.parse(snap.at) >= Date.parse(prior.receivedAt) && Date.parse(snap.at) <= Date.parse(sourceAt)) continue;
        if (prospective) s.counters.skippedBooks++; decisions.delete(id); revoke(s, bank, id, sourceAt, 'read_skipped'); continue;
      }
      let b, independent;
      try { ({b, independent} = checkedBook(s, snap, id, bank, sourceAt)); }
      catch { if (prospective) s.counters.invalidBooks++; decisions.delete(id); revoke(s, bank, id, sourceAt, 'invalid_public_book'); continue; }
      accepted.set(id, b);
      const decision = prospective && Date.parse(b.requestStartedAt) > Date.parse(bank.cutoff), fresh = Date.parse(at) - Date.parse(b.receivedAt) <= bank.execution.maximumQuoteAgeMs;
      if (!decision) s.counters.developmentBooks++;
      else if (!fresh) {
        s.counters.staleAtDecisionBooks++;
        // Late host consumption can establish a prior order's in-window
        // opportunity. It cannot authorize a backdated new entry or exit.
        for (const v of Object.values(s.strategies)) {
          v.wallet.books[b.instrument] = clone(b); delete v.wallet.invalidated?.[b.instrument];
          v.wallet.lastAt = at; v.wallet.observations++;
          fill(v, b, independent, {...record, availableAt: at, sourceAvailableAt: sourceAt}, bank);
        }
      }
      else {
        s.counters.prospectiveBooks++; if (independent) s.counters.independentBooks++;
        const reference = historicalReference(s.histories[b.exchangeId], s.bookHistory[b.exchangeId], b.requestStartedAt, bank.policy),
          contextAge = Date.parse(b.requestStartedAt) - Date.parse(e.exclusionObservedAt || ''),
          known = Array.isArray(e.accountExclusions) && e.accountExclusions.every(x => typeof x === 'string') && clock(e.exclusionObservedAt)
            && contextAge >= 0 && contextAge <= bank.execution.accountContextMaxAgeMs,
          contamination = !known ? 'unknown_account_context' : e.accountExclusions.includes(b.exchangeId) ? 'account_exposure_or_orders' : 'no_reported_account_exposure';
        for (const name of names) run(s.strategies[name], name, b, independent, {...record, availableAt: at, sourceAvailableAt: sourceAt}, bank, reference, contamination, {decide: false});
        decisions.set(id, {b, independent, reference, contamination});
      }
      if (independent) s.bookHistory[b.exchangeId] = [...(s.bookHistory[b.exchangeId] || []), {at: b.sourceAt, receivedAt: b.receivedAt,
        mid: Number(amount(b.bids[0].price) + amount(b.asks[0].price)) / 2e6, versionKey: b.versionKey}]
        .filter(x => Date.parse(x.at) >= Date.parse(b.receivedAt) - bank.policy.lookbackMs).slice(-512);
    }
    // All receipts predate the published cycle's availability. Resolve prior
    // orders across the entire capture before a new cancellation or risk action;
    // use only each market's latest valid quote for the newly available decision.
    for (const {b, independent, reference, contamination} of decisions.values())
      for (const name of names) run(s.strategies[name], name, b, independent, {...record, availableAt: at, sourceAvailableAt: sourceAt},
        bank, reference, contamination, {evidence: false});
  }
  if (prospective) for (const v of Object.values(s.strategies)) {
    for (const [id, pending] of Object.entries(ledger(v).pending))
      // Host catch-up time cannot censor receipt evidence still ahead in the
      // verified source. Resolve expiry only once its source watermark passes.
      if (e.type === 'cycle' && pending.action === 'buy' && Date.parse(sourceAt) >= Date.parse(pending.expiresAt)) finish(v, id, at, 'expired');
    // Marking ages strictly at maxDataAgeMs; collector cadence is no extension.
    sampleEquity(v.wallet, {...bank.config, cycleIntervalMs: 0}, at);
    assertState(v.wallet, bank.config);
    need(spent(ledger(v)) <= amount(bank.execution.cumulativeBuyBudget), 'Cumulative buy budget exceeded');
  }
  return s;
}

export function limitEntryReport(state, bank, at) {
  checkBank(bank); need(state.bankHash === digest(bank) && clock(at) && (!state.lastAvailableAt || Date.parse(at) >= Date.parse(state.lastAvailableAt)), 'Invalid limit-entry report');
  const strategies = Object.fromEntries(names.map(name => {
    const v = state.strategies[name], st = ledger(v), r = report(v.wallet, {...bank.config, cycleIntervalMs: 0}, at).strategies.mean_reversion,
      label = instrument => { const id = v.wallet.books[instrument]?.marketId; return bank.identities[id]?.title || 'Market ' + id + ' YES'; },
      outcomes = Object.values(v.outcomes), orders = Object.values(v.decisions).filter(x => x.action === 'buy');
    return [name, {...r, positions: r.positions.map(p => ({...p, label: label(p.instrument)})),
      recentFills: r.recentFills.map(f => ({...f, label: label(f.instrument)})),
      pending: Object.entries(st.pending).map(([instrument, p]) => ({...clone(p), instrument, remaining: p.quantity, label: label(instrument),
        clockExpiredAwaitingSource: p.action === 'buy' && Date.parse(at) >= Date.parse(p.expiresAt)})),
      cumulativeBuyCost: money(spent(st)), uniqueObservations: v.uniqueObservations, actualNativeAdmissions: v.actualNativeAdmissions,
      conditionalLimitAdmissions: v.conditionalLimitAdmissions, gateCounts: clone(v.gateCounts), executionGateCounts: clone(v.executionGateCounts),
      contaminationCounts: clone(v.contaminationCounts), lastSignals: clone(v.lastSignals), exitObligations: clone(v.exitObligations),
      entryOutcomes: {orders: orders.length, immediate: orders.filter(x => x.reason === 'immediate').length,
        conditionalPark: orders.filter(x => x.reason === 'conditional_park').length,
        expiredUnobserved: outcomes.filter(x => x.status === 'expired' && x.censoring === 'no_independent_in_window_quote').length,
        expiredObserved: outcomes.filter(x => x.status === 'expired' && x.censoring === 'observed_without_full_execution').length,
        unresolved: Object.values(st.pending).filter(x => x.action === 'buy').length,
        partiallyFilledOpen: outcomes.filter(x => x.status === 'partially_filled').length},
      recentDecisions: Object.values(v.decisions).slice(-20).map(x => ({...clone(x), outcome: clone(v.outcomes[x.id])}))}];
  }));
  return {schemaVersion: 1, mode: 'prospective_price_protected_limit_entry_paper', cohortId: bank.name + ':' + bank.cutoff,
    updatedAt: at, cutoff: bank.cutoff, bankHash: state.bankHash, sourceSequence: state.sourceSequence, sourceHash: state.lastRecordHash,
    marketIds: [...LIMIT_ENTRY_MARKETS], execution: clone(bank.execution), counters: clone(state.counters), strategies,
    networkRequests: 0, competitionOrders: 0, profitabilityEstablished: false,
    assumptions: ['Full policy comparison: hybrid adds conditional-price opportunities; it is not an isolated execution-effect estimate.',
      'Both arms submit the same frozen actual ask limit when the current padded native dip gate admits.',
      'Hybrid otherwise tests a one-tick-lower actual limit with the original padded-price gates; committed orders are not filtered by later outcomes.',
      'A later independently requested marketable quote supports a counterfactual fill at that receipt only, not resting execution or queue priority.',
      'Thirty-second entries require request after decision availability plus one second and receipt strictly before expiry.',
      'One percent per displayed eligible level, one adverse tick capped at the actual order limit, zero added fees; wallets reuse public liquidity independently.',
      'Account contamination remains labeled; quote touch, cancellations, missing observations and expired quotes are not fills.']};
}
