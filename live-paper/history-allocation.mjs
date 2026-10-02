import {digest} from './model.mjs';
import {historicalReference} from './history.mjs';

const canonical = value => typeof value === 'string' && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;
const identityKnown = value => typeof value?.state === 'string' && value.state.trim().length > 0 && typeof value.office === 'string' && value.office.trim().length > 0;

function supportExpiry(history, books, reference, policy, now) {
  const candles = reference.source === 'closed_trade_candle_vwap_median';
  const points = candles
    ? history.candles.filter(c => Date.parse(c.time) + 60000 <= now && Date.parse(c.time) >= now - policy.lookbackMs).map(c => ({windowAt: Date.parse(c.time), observedAt: Date.parse(c.time) + 60000}))
    : (books || []).filter(b => Date.parse(b.at) < now && Date.parse(b.receivedAt) <= now && Date.parse(b.at) >= now - policy.lookbackMs).map(b => ({windowAt: Date.parse(b.at), observedAt: Date.parse(b.at)}));
  points.sort((a, b) => a.windowAt - b.windowAt);
  const last = Date.parse(reference.lastAt), spanSupport = points.filter(p => p.observedAt <= last - policy.minimumSpanMs).at(-1);
  if (points.length < policy.minimumSamples || !spanSupport) throw Error('History allocation support mismatch');
  // The current reference can lose its minimum sample count or time span before
  // the latest candle becomes stale, when older support rolls out of lookback.
  return Math.min(points[points.length - policy.minimumSamples].windowAt, spanSupport.windowAt) + policy.lookbackMs;
}

// Read priority only. Bulk quotes have no source clock or executable depth;
// these hints cannot authorize orders or change any frozen strategy threshold.
export function historyAllocation(sources, universe, coverage, now = Date.now()) {
  if (!Number.isSafeInteger(now) || !Array.isArray(universe?.exchanges)) throw Error('Invalid history allocation scope/clock');
  const at = new Date(now).toISOString();
  for (const {state, bank} of sources) {
    if (state.bankHash !== digest(bank) || bank.universe.hash !== universe.hash || bank.config.tournamentId !== universe.tournamentId) throw Error('History allocation source identity mismatch');
  }
  const scopedCoverage = coverage?.universeHash === universe.hash && coverage.tournamentId === universe.tournamentId;
  const markets = universe.exchanges.map(spec => {
    const quote = scopedCoverage ? coverage.quotes?.[spec.exchangeId] : null;
    const quoteFresh = quote?.marketId === spec.marketId && quote.exchangeId === spec.exchangeId && quote.option === 'YES'
      && canonical(quote.receivedAt) && Date.parse(quote.receivedAt) <= now && now - Date.parse(quote.receivedAt) <= 600000
      && Number.isFinite(quote.bestAsk) && Number.isFinite(quote.bestBid) && quote.bestBid >= 0 && quote.bestAsk <= 1
      && quote.bestAsk > quote.bestBid && quote.crossed !== true;
    let known = false, receivedAt = null, best = null, plausible = false;
    for (const {state, bank, healthy} of sources) {
      known ||= identityKnown(bank.identities?.[spec.marketId]);
      const h = state.histories?.[spec.exchangeId], p = bank.policy;
      if (h && (h.exchangeId !== spec.exchangeId || h.marketId !== spec.marketId || h.tournamentId !== universe.tournamentId)) throw Error('History allocation capture scope mismatch');
      if (canonical(h?.receivedAt) && Date.parse(h.receivedAt) <= now && (!receivedAt || Date.parse(h.receivedAt) > Date.parse(receivedAt))) receivedAt = h.receivedAt;
      const cushion = bank.config.slippageTicks * .005;
      const plausibleHere = quoteFresh && quote.bestAsk + cushion >= p.minimumPrice && quote.bestAsk + cushion <= p.maximumPrice
        && quote.bestAsk - quote.bestBid + 2 * cushion <= p.maximumSpread + 1e-12;
      plausible ||= plausibleHere;
      if (!healthy) continue;
      const ref = historicalReference(h, state.bookHistory?.[spec.exchangeId], at, p);
      if (!ref.ready) continue;
      let expiry = Math.min(Date.parse(ref.lastAt) + p.lastHistoricalObservationAgeMs,
        supportExpiry(h, state.bookHistory?.[spec.exchangeId], ref, p, now));
      if (ref.source === 'closed_trade_candle_vwap_median') expiry = Math.min(expiry, Date.parse(h.receivedAt) + p.historyCacheAgeMs);
      if (!Number.isSafeInteger(expiry)) throw Error('Invalid history allocation expiry');
      const required = Math.max(p.minimumDip, ref.center * p.minimumRelativeDip, p.minimumZ * ref.robustDeviation);
      // This rounded gap ranks reads, never entry acceptance or accounting.
      const gap = quoteFresh ? Math.max(-1000000, Math.min(1000000, Math.ceil((required - (ref.center - quote.bestAsk - cushion)) * 1e6))) : null;
      const candidate = {validUntil: new Date(expiry).toISOString(), opportunityGapMicros: gap};
      if (!best || expiry > Date.parse(best.validUntil)) best = candidate;
    }
    return {marketId: spec.marketId, referenceReady: best !== null, validUntil: best?.validUntil ?? null, receivedAt,
      identityKnown: known, quotePlausible: plausible, opportunityGapMicros: best?.opportunityGapMicros ?? null};
  });
  return {schemaVersion: 1, at, markets};
}
