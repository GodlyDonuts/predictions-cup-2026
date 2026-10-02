import {validateSchedulerState} from './book-scheduler.mjs';

const canonicalClock = value => typeof value === 'string' && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;
const replaceable = new Set(['comparison', 'discovery']);

// This is a read-only measurement overlay. A positive supplied gap is a reason
// to inspect both books, never proof of a hedge, signal eligibility, or profit.
// The caller constructs each candidate from one scoped bulk response; receivedAt
// is that shared receipt, not a merge of independently timed leg quotes.
export function planBasketBooks(basePlan, scope, {candidates = [], excludedMarketIds = [], cycle, now} = {}, {every = 4, maxAgeMs = 120000} = {}) {
  if (!Number.isSafeInteger(cycle) || cycle < 0 || !Number.isSafeInteger(now) || now < 0 || now > 8640000000000000
    || !Number.isSafeInteger(every) || every < 1 || !Number.isSafeInteger(maxAgeMs) || maxAgeMs < 1 || maxAgeMs > 120000) throw Error('Invalid basket measurement cadence/clock');
  const state = validateSchedulerState(basePlan?.state, scope), markets = new Map(scope.universe.exchanges.map(spec => [spec.marketId, spec])), core = new Set(scope.coreMarketIds);
  if (!Array.isArray(basePlan.reads) || basePlan.reads.length > 4) throw Error('Invalid basket base book budget');
  const existing = new Map();
  for (const read of basePlan.reads) {
    const spec = markets.get(read?.marketId);
    if (!spec || spec.exchangeId !== read.exchangeId || existing.has(read.marketId) || read.path !== (core.has(read.marketId) ? 'sourceSnapshots' : 'supplementalDepth')) throw Error('Invalid or foreign basket base book scope');
    existing.set(read.marketId, read);
  }
  if (!Array.isArray(excludedMarketIds) || excludedMarketIds.some(marketId => !markets.has(marketId))) throw Error('Invalid or foreign basket exclusions');
  const excluded = new Set(excludedMarketIds), output = structuredClone(basePlan);
  const finish = (reason, selected) => ({...output, basketMeasurement: {reason, cycle, every, ...(selected ? {
    marketIds: [...selected.marketIds], exchangeIds: [...selected.exchangeIds], receivedAt: selected.receivedAt, priceGapMicros: selected.priceGapMicros,
  } : {})}});
  if (!Array.isArray(candidates)) return finish('hints_unavailable');
  const valid = [];
  for (const candidate of candidates) {
    if (!candidate || typeof candidate !== 'object' || Array.isArray(candidate)) continue;
    if (candidate.tournamentId !== scope.universe.tournamentId) throw Error('Foreign basket tournament');
    const ids = candidate.marketIds, exchanges = candidate.exchangeIds;
    if (!Array.isArray(ids) || !Array.isArray(exchanges) || ids.length !== 2 || exchanges.length !== 2) continue;
    if (ids[0] === ids[1] || exchanges[0] === exchanges[1] || ids.some((marketId, index) => !markets.has(marketId) || markets.get(marketId).exchangeId !== exchanges[index])) throw Error('Invalid or foreign basket pair scope');
    if (!canonicalClock(candidate.receivedAt) || Date.parse(candidate.receivedAt) > now || now - Date.parse(candidate.receivedAt) > maxAgeMs
      || !Number.isSafeInteger(candidate.priceGapMicros) || candidate.priceGapMicros <= 0 || candidate.priceGapMicros > 1000000 || ids.some(marketId => excluded.has(marketId))) continue;
    const legs = ids.map((marketId, index) => ({marketId, exchangeId: exchanges[index]})).sort((a, b) => Number(a.marketId) - Number(b.marketId));
    valid.push({marketIds: legs.map(x => x.marketId), exchangeIds: legs.map(x => x.exchangeId), receivedAt: candidate.receivedAt,
      priceGapMicros: candidate.priceGapMicros, lastObserved: Math.max(...ids.map(marketId => state.bookLastAttempt[marketId] || 0))});
  }
  if (cycle % every !== 0) return finish('cadence_reserved');
  if (!valid.length) return finish('no_fresh_positive_pair');
  // Unknown future read reasons are protected too. Only explicit comparison or
  // discovery slots may be replaced, and every protected read retains its order.
  const protectedReads = basePlan.reads.filter(read => !replaceable.has(read.reason)), protectedIds = new Set(protectedReads.map(read => read.marketId));
  const fitting = valid.filter(pair => new Set([...protectedIds, ...pair.marketIds]).size <= 4);
  if (!fitting.length) return finish('protected_capacity');
  fitting.sort((a, b) => b.priceGapMicros - a.priceGapMicros || a.lastObserved - b.lastObserved
    || Number(a.marketIds[0]) - Number(b.marketIds[0]) || Number(a.marketIds[1]) - Number(b.marketIds[1])
    || Date.parse(b.receivedAt) - Date.parse(a.receivedAt));
  const selected = fitting[0], reads = protectedReads.map(read => structuredClone(read)), chosen = new Set(protectedIds);
  for (const marketId of selected.marketIds) if (!chosen.has(marketId)) {
    const read = existing.get(marketId), spec = markets.get(marketId);
    reads.push(read ? structuredClone(read) : {marketId, exchangeId: spec.exchangeId, option: spec.option,
      path: core.has(marketId) ? 'sourceSnapshots' : 'supplementalDepth', reason: 'basket_measurement'});
    chosen.add(marketId);
  }
  for (const read of basePlan.reads) if (reads.length < 4 && !chosen.has(read.marketId)) { reads.push(structuredClone(read)); chosen.add(read.marketId); }
  output.reads = reads;
  return finish('paired_depth_measurement', selected);
}
