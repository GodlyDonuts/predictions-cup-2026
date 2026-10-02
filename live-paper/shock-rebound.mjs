import {createHash} from 'node:crypto';
import {amount, money, price, TICK, SCALE, instant, sequence} from '../paper/fixed.mjs';
import {instantNs, timestamp} from '../feed/contract.mjs';

// Research candidate only: this module has no executor, fills, portfolio or I/O.
// References describe a recovery hypothesis, never an estimate of fair value.
export const SHOCK_REBOUND_DEFAULTS = Object.freeze({
  lookbackMs: 1800000, minimumSamples: 8, minimumSpanMs: 300000,
  shockWindowMs: 120000, maxAnchorAgeMs: 240000, trendWindowMs: 900000,
  minimumTrendSpanMs: 300000, maxDataAgeMs: 15000, maxObservationGapMs: 120000,
  maxReboundWaitMs: 120000, minConfirmationDelayMs: 1000, cooldownMs: 60000,
  minimumShock: '0.020000', maximumShock: '0.100000', minimumRoom: '0.010000',
  maximumEffectiveSpread: '0.020000', minimumPrice: '0.050000', maximumPrice: '0.950000',
  volatilityFloor: 0.0025, minimumShockZ: 2.5, minimumTrendPerMinute: -0.0025,
  slippageTicks: 1, confirmationTicks: 2, orderShares: 10, maxEntryCost: '25.000000',
  depthFractionBps: 100, feeBps: 0,
});
const canonical = x => Array.isArray(x) ? '[' + x.map(canonical).join(',') + ']' : x && typeof x === 'object'
  ? '{' + Object.keys(x).sort().map(k => JSON.stringify(k) + ':' + canonical(x[k])).join(',') + '}' : JSON.stringify(x);
const hash = x => createHash('sha256').update(canonical(x)).digest('hex');
const need = (ok, message) => { if (!ok) throw Error(message); };
const median = a => { const b = [...a].sort((x, y) => x - y); const i = Math.floor(b.length / 2); return b.length % 2 ? b[i] : (b[i - 1] + b[i]) / 2; };
function config(input) {
  need(input && Object.keys(input).every(k => Object.hasOwn(SHOCK_REBOUND_DEFAULTS, k)), 'Unknown shock config');
  const c = {...SHOCK_REBOUND_DEFAULTS, ...input};
  for (const k of Object.keys(c).filter(k => k.endsWith('Ms')).concat(['minimumSamples', 'confirmationTicks', 'orderShares', 'depthFractionBps']))
    need(Number.isSafeInteger(c[k]) && c[k] > 0, 'Invalid shock config: ' + k);
  need(Number.isSafeInteger(c.slippageTicks) && c.slippageTicks >= 1 && c.slippageTicks <= 5 && c.feeBps === 0, 'Unsupported costs');
  need(c.depthFractionBps <= 100 && c.orderShares <= 100 && c.minimumSamples >= 8, 'Unsafe sizing or history');
  need(c.lookbackMs >= c.trendWindowMs && c.trendWindowMs >= c.minimumTrendSpanMs + c.shockWindowMs
    && c.maxAnchorAgeMs >= c.shockWindowMs && c.minimumSpanMs >= c.shockWindowMs
    && c.maxReboundWaitMs >= c.minConfirmationDelayMs, 'Inconsistent time windows');
  for (const k of ['minimumShock', 'maximumShock', 'minimumRoom', 'maximumEffectiveSpread', 'minimumPrice', 'maximumPrice', 'maxEntryCost'])
    need(amount(c[k]) > 0n, 'Invalid amount: ' + k);
  need(amount(c.minimumShock) < amount(c.maximumShock) && amount(c.maximumShock) < SCALE
    && price(c.minimumPrice) < price(c.maximumPrice) && amount(c.maxEntryCost) <= amount('25.000000'), 'Invalid price bounds');
  need(Number.isFinite(c.volatilityFloor) && c.volatilityFloor > 0 && Number.isFinite(c.minimumShockZ) && c.minimumShockZ > 0
    && Number.isFinite(c.minimumTrendPerMinute) && c.minimumTrendPerMinute < 0, 'Invalid statistical limits');
  return c;
}
export function createShockRebound({marketId, exchangeId, sourceHash, cutoff, config: overrides = {}}) {
  need(/^[1-9]\d*$/.test(String(marketId)) && /^[1-9]\d*$/.test(String(exchangeId)), 'Invalid identity');
  need(/^[a-f0-9]{64}$/.test(sourceHash), 'A frozen source SHA256 is required'); instant(cutoff);
  const c = config(overrides), bank = {schemaVersion: 1, name: 'causal-shock-rebound-v1', mode: 'research_paper_only',
    marketId: String(marketId), exchangeId: String(exchangeId), sourceHash, cutoff, config: c, configHash: hash(c)};
  const state = {schemaVersion: 1, bankHash: hash(bank), history: [], armed: null, last: null, cooldownUntil: null, invalidatedAt: null};
  return {bank, state};
}
function checkState(s, bank) {
  need(bank?.name === 'causal-shock-rebound-v1' && bank.mode === 'research_paper_only'
    && bank.configHash === hash(config(bank.config)) && s?.schemaVersion === 1 && s.bankHash === hash(bank), 'Frozen bank changed');
  need(Array.isArray(s.history), 'Invalid candidate state');
}
function checkedBook(b, bank, s) {
  const c = bank.config;
  need(b?.marketId === bank.marketId && b.exchangeId === bank.exchangeId && b.instrument === bank.exchangeId + ':YES', 'Book identity mismatch');
  const at = instant(b.receivedAt), requestAt = instant(b.requestStartedAt), source = instantNs(b.sourceAt);
  sequence(b.sequence);
  need(b.versionKey === b.sequence + ':' + timestamp(b.sourceAt), 'Book version mismatch');
  need(requestAt <= at && at - requestAt <= c.maxDataAgeMs && source <= BigInt(at) * 1000000n, 'Noncausal or stale response');
  if (s.last) {
    need(at >= s.last.at && requestAt >= s.last.requestAt, 'Receipt rollback');
    need(BigInt(b.sequence) >= BigInt(s.last.sequence) && source >= BigInt(s.last.sourceNs), 'Source rollback');
  }
  for (const [side, direction] of [['bids', -1], ['asks', 1]]) {
    need(Array.isArray(b[side]) && b[side].length > 0, 'Missing executable depth');
    let previous;
    for (const l of b[side]) {
      const p = price(l.price); need(Number.isSafeInteger(l.quantity) && l.quantity > 0, 'Invalid depth');
      if (previous !== undefined) need(direction < 0 ? p < previous : p > previous, 'Unsorted depth'); previous = p;
    }
  }
  const bid = price(b.bids[0].price), ask = price(b.asks[0].price);
  need(bid < ask, 'Crossed or locked book');
  const bookHash = hash({bids: b.bids, asks: b.asks});
  if (s.last?.versionKey === b.versionKey) need(bookHash === s.last.bookHash, 'Changed depth under unchanged version');
  return {at, requestAt, sourceNs: String(source), bid, ask, bookHash};
}
function features(history, q, c) {
  const prior = history.filter(h => h.at <= q.requestAt && h.at >= q.at - c.lookbackMs);
  if (prior.length < c.minimumSamples || prior.at(-1).at - prior[0].at < c.minimumSpanMs) return {ready: false, reason: 'insufficient_history'};
  if (q.at - prior.at(-1).at > c.maxObservationGapMs) return {ready: false, reason: 'observation_gap'};
  const anchor = prior.filter(h => h.at <= q.at - c.shockWindowMs).at(-1);
  if (!anchor || q.at - anchor.at > c.maxAnchorAgeMs) return {ready: false, reason: 'missing_shock_anchor'};
  const background = prior.filter(h => h.at <= q.at - c.shockWindowMs && h.at >= q.at - c.trendWindowMs);
  if (background.length < 3 || background.at(-1).at - background[0].at < c.minimumTrendSpanMs) return {ready: false, reason: 'insufficient_trend_history'};
  const xs = background.map(h => (h.at - background[0].at) / 60000), ys = background.map(h => h.mid);
  const xbar = xs.reduce((a, b) => a + b, 0) / xs.length, ybar = ys.reduce((a, b) => a + b, 0) / ys.length;
  const trendPerMinute = xs.reduce((a, x, i) => a + (x - xbar) * (ys[i] - ybar), 0) / xs.reduce((a, x) => a + (x - xbar) ** 2, 0);
  const returns = prior.slice(1).flatMap((h, i) => h.at > prior[i].at ? [(h.mid - prior[i].mid) / Math.sqrt((h.at - prior[i].at) / 60000)] : []);
  const center = median(returns), volatility = Math.max(c.volatilityFloor, 1.4826 * median(returns.map(x => Math.abs(x - center))));
  const drop = anchor.mid - Number(q.bid + q.ask) / (2 * Number(SCALE)), elapsedMinutes = (q.at - anchor.at) / 60000;
  return {ready: true, drop, shockZ: drop / (volatility * Math.sqrt(elapsedMinutes)), elapsedMinutes, volatility, trendPerMinute,
    anchorMid: money(BigInt(Math.round(anchor.mid * Number(SCALE)))),
    // Conservative executable BID reference, rounded down to the micro unit.
    referenceBid: money(BigInt(Math.floor(median(background.map(h => Number(amount(h.bid)))))))};
}
export function invalidateShockRebound(input, bank, at) {
  checkState(input, bank); const t = instant(at); need(!input.last || t >= input.last.at, 'Invalidation rollback');
  const s = structuredClone(input); s.history = []; s.armed = null; s.invalidatedAt = at; return s;
}
export function advanceShockRebound(input, book, bank) {
  checkState(input, bank); const q = checkedBook(book, bank, input), c = bank.config, s = structuredClone(input);
  const finish = (reason, extra = {}, signal = null) => ({state: s, signal, diagnostic: {reason, at: book.receivedAt, ...extra}});
  if (s.invalidatedAt && q.requestAt <= instant(s.invalidatedAt)) return finish('request_before_invalidation');
  const repeated = s.last?.versionKey === book.versionKey;
  const independent = !s.last || BigInt(q.sourceNs) > BigInt(s.last.sourceNs);
  s.last = {at: q.at, requestAt: q.requestAt, sourceNs: q.sourceNs, sequence: book.sequence, versionKey: book.versionKey, bookHash: q.bookHash};
  if (s.armed) {
    // Keep the ORIGINAL shock anchor through every trough and confirmation.
    // Check before expiry or branch selection, so a delayed collapse cannot hide
    // behind a new trough, a bounce, or an expired hypothesis. Twice-midpoint
    // arithmetic preserves the exact maximumShock boundary without float error.
    const totalDropTwice = 2n * amount(s.armed.anchorMid) - q.bid - q.ask;
    if (totalDropTwice > 2n * amount(c.maximumShock)) {
      s.armed = null; s.history = []; s.invalidatedAt = book.receivedAt;
      return finish('maximum_shock_breached', {totalShock: money(totalDropTwice / 2n), requiresBackgroundRebuild: true});
    }
  }
  // An unchanged receipt advances the observation clock, but creates no new sample or liquidity.
  if (s.armed && q.at - s.armed.at > c.maxReboundWaitMs) s.armed = null;
  if (repeated || !independent) return finish('no_independent_quote');
  if (s.history.length && q.at - s.history.at(-1).at > c.maxObservationGapMs) { s.history = []; s.armed = null; }
  const f = features(s.history, q, c);
  // Append only after computing features: no current quote is in its own reference.
  s.history.push({at: q.at, mid: Number(q.bid + q.ask) / (2 * Number(SCALE)), bid: money(q.bid), versionKey: book.versionKey});
  s.history = s.history.filter(h => h.at >= q.at - c.lookbackMs).slice(-2048);
  if (q.requestAt <= instant(bank.cutoff) || q.at <= instant(bank.cutoff)) return finish('development_prefix');
  if (s.cooldownUntil && q.at < instant(s.cooldownUntil)) return finish('cooldown');
  if (s.armed) {
    if (q.bid < amount(s.armed.troughBid)) {
      s.armed.troughBid = money(q.bid); s.armed.troughAt = q.at; s.armed.troughSourceNs = q.sourceNs; s.armed.troughVersion = book.versionKey;
      return finish('new_trough');
    }
    if (q.requestAt < s.armed.troughAt + c.minConfirmationDelayMs || BigInt(q.sourceNs) <= BigInt(s.armed.troughSourceNs)
      || book.versionKey === s.armed.troughVersion) return finish('confirmation_not_causal');
    if (q.bid < amount(s.armed.troughBid) + BigInt(c.confirmationTicks) * TICK) return finish('awaiting_bid_recovery');
    if (!f.ready || f.trendPerMinute < c.minimumTrendPerMinute) { s.armed = null; return finish('trend_veto', f); }
    const slip = BigInt(c.slippageTicks) * TICK, entry = q.ask + slip, exit = q.bid - slip;
    const spread = entry - exit, referenceExit = amount(s.armed.referenceBid) - slip, room = referenceExit - entry;
    if (entry > price(c.maximumPrice) || entry < price(c.minimumPrice) || exit < TICK) return finish('price_bounds');
    // Integer micro amounts include one adverse cushion on EACH side; equality is allowed.
    if (spread > amount(c.maximumEffectiveSpread)) return finish('effective_spread', {effectiveSpread: money(spread)});
    if (room < amount(c.minimumRoom)) return finish('insufficient_recovery_room', {room: money(room)});
    const quantity = Number([BigInt(c.orderShares), amount(c.maxEntryCost) / entry,
      BigInt(book.asks[0].quantity) * BigInt(c.depthFractionBps) / 10000n,
      BigInt(book.bids[0].quantity) * BigInt(c.depthFractionBps) / 10000n].reduce((a, b) => a < b ? a : b));
    if (!quantity) return finish('insufficient_depth');
    const signal = {kind: 'research_entry_proposal', strategy: bank.name, bankHash: s.bankHash, sourceHash: bank.sourceHash,
      configHash: bank.configHash, cutoff: bank.cutoff, marketId: bank.marketId, exchangeId: bank.exchangeId, side: 'yes', action: 'buy',
      quantity, maximumEntryPrice: money(entry), observedExitBidAfterCushion: money(exit), effectiveSpread: money(spread),
      recoveryReferenceBid: s.armed.referenceBid, recoveryRoom: money(room), at: book.receivedAt, sourceVersion: book.versionKey,
      shockAt: new Date(s.armed.at).toISOString(), troughAt: new Date(s.armed.troughAt).toISOString(),
      troughBid: s.armed.troughBid, troughSourceVersion: s.armed.troughVersion,
      featuresAtShock: s.armed.features, profitabilityEstablished: false};
    signal.signalId = hash(signal); s.armed = null; s.cooldownUntil = new Date(q.at + c.cooldownMs).toISOString();
    return finish('confirmed_rebound', f, signal);
  }
  if (!f.ready) return finish(f.reason);
  if (f.trendPerMinute < c.minimumTrendPerMinute) return finish('trend_veto', f);
  // Float operations are restricted to statistical features; all executable gates use fixed money.
  const dropMicros = BigInt(Math.round(f.drop * Number(SCALE)));
  if (dropMicros < amount(c.minimumShock) || dropMicros > amount(c.maximumShock) || f.shockZ < c.minimumShockZ) return finish('shock_gate', f);
  s.armed = {at: q.at, anchorMid: f.anchorMid, troughAt: q.at, troughBid: money(q.bid), troughSourceNs: q.sourceNs,
    troughVersion: book.versionKey, referenceBid: f.referenceBid, features: f};
  return finish('shock_armed', f);
}
