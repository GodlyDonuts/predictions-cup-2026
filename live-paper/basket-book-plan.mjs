import {validateSchedulerState} from './book-scheduler.mjs';
import {createHash} from 'node:crypto';
import {amount} from '../paper/fixed.mjs';
import {instantNs, timestamp} from '../feed/contract.mjs';

const canonicalClock = value => typeof value === 'string' && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;
const replaceable = new Set(['comparison', 'discovery']);
const hex = x => typeof x === 'string' && /^[a-f0-9]{64}$/.test(x);
const hash = x => createHash('sha256').update(JSON.stringify(x)).digest('hex');
const object = x => x !== null && typeof x === 'object' && !Array.isArray(x);
const anchorHash = x => hash({marketIds: x.marketIds, exchangeIds: x.exchangeIds, observerBankHash: x.observerBankHash,
  pairId: x.pairId, sourceSequence: x.sourceSequence, sourceHash: x.sourceHash, anchorAt: x.anchorAt,
  pairVersions: x.pairVersions, pairReceipts: x.pairReceipts, sourceMeasurementHash: x.sourceMeasurementHash});

function pairLegs(marketIds, exchangeIds, markets) {
  if (!Array.isArray(marketIds) || !Array.isArray(exchangeIds) || marketIds.length !== 2 || exchangeIds.length !== 2
    || marketIds[0] === marketIds[1] || exchangeIds[0] === exchangeIds[1]
    || marketIds.some((id, i) => !markets.has(id) || markets.get(id).exchangeId !== exchangeIds[i])) throw Error('Invalid basket followup scope');
  return marketIds.map((marketId, index) => ({marketId, exchangeId: exchangeIds[index], index})).sort((a, b) => Number(a.marketId) - Number(b.marketId));
}

function followupLedger(raw, markets, now) {
  if (raw === undefined) return {schemaVersion: 1, claims: {}};
  if (!object(raw) || raw.schemaVersion !== 1 || !object(raw.claims) || Object.keys(raw.claims).length > 1000) throw Error('Invalid basket followup ledger');
  for (const [key, claim] of Object.entries(raw.claims)) {
    if (!object(claim) || !hex(claim.observerBankHash) || !hex(claim.pairId) || key !== hash([claim.observerBankHash, claim.pairId])
      || !Number.isSafeInteger(claim.sourceSequence) || claim.sourceSequence < 1 || !hex(claim.sourceHash) || !hex(claim.anchorHash) || !hex(claim.sourceMeasurementHash)
      || !canonicalClock(claim.anchorAt) || !canonicalClock(claim.claimedAt) || Date.parse(claim.claimedAt) <= Date.parse(claim.anchorAt)
      || Date.parse(claim.claimedAt) > now || !Array.isArray(claim.pairReceipts) || claim.pairReceipts.length !== 2 || !claim.pairReceipts.every(canonicalClock)
      || !Array.isArray(claim.pairVersions) || claim.pairVersions.length !== 2 || claim.pairVersions.some(v => typeof v !== 'string')) throw Error('Invalid basket followup claim');
    pairLegs(claim.marketIds, claim.exchangeIds, markets);
    if (anchorHash(claim) !== claim.anchorHash) throw Error('Basket followup claim identity mismatch');
  }
  return structuredClone(raw);
}

// Caller supplies an observation from a verified basket journal row, never a
// stats heartbeat or a newly computed bulk-price gap. Claiming precedes network
// attempts: the returned scheduler state MUST be persisted before any reads.
// Failed/partial attempts are deliberately consumed rather than retried.
// Shape: {tournamentId, observerBankHash, pairId, sourceSequence, sourceHash,
// cutoff, marketIds, exchangeIds, observation, sourceMeasurement}; leg order
// matches observation. sourceMeasurement is event.basketMeasurement from the
// exact hash-bound source cycle, not a current planner result or heartbeat.
function positiveDepthFollowup(raw, scope, markets, ledger, excluded, now, maxAgeMs, latencyMs) {
  if (!object(raw) || raw.tournamentId !== scope.universe.tournamentId || !hex(raw.observerBankHash) || !hex(raw.pairId)
    || !Number.isSafeInteger(raw.sourceSequence) || raw.sourceSequence < 1 || !hex(raw.sourceHash) || !canonicalClock(raw.cutoff)) throw Error('Invalid basket followup provenance');
  const legs = pairLegs(raw.marketIds, raw.exchangeIds, markets), o = raw.observation;
  if (!object(o) || o.pairId !== raw.pairId || !canonicalClock(o.at)) throw Error('Invalid basket followup observation');
  const origin = raw.sourceMeasurement;
  // The observer resets its positive anchor when the first refreshed leg finds
  // the other leg stale. A follow-up can therefore look like a new positive.
  // Only the initiating measurement's causal plan may seed another read.
  if (!object(origin) || origin.reason !== 'paired_depth_measurement' || origin.followup !== undefined) return null;
  const originLegs = pairLegs(origin.marketIds, origin.exchangeIds, markets);
  if (originLegs.some((leg, index) => leg.marketId !== legs[index].marketId || leg.exchangeId !== legs[index].exchangeId)
    || !canonicalClock(origin.receivedAt) || Date.parse(origin.receivedAt) > Date.parse(o.at)
    || !Number.isSafeInteger(origin.cycle) || origin.cycle < 0 || !Number.isSafeInteger(origin.every) || origin.every < 1
    || origin.cycle % origin.every !== 0) throw Error('Invalid basket followup origin');
  // A follow-up result cannot recursively create another follow-up chain.
  if (o.status !== 'apparent_positive_basket' || o.apparentPositive !== true || o.newPairEvidence !== true
    || o.independentPair !== false || o.positivePersistenceCount !== 1) return null;
  if (o.eligibleForExecution !== false || o.fills !== null || o.pnl !== null || !object(o.basket)
    || !Number.isSafeInteger(o.basket.quantity) || o.basket.quantity < 1 || o.basket.quantity > 100
    || !Number.isSafeInteger(o.basket.depthFractionBps) || o.basket.depthFractionBps < 1 || o.basket.depthFractionBps > 100
    || !Array.isArray(o.pairReceipts) || o.pairReceipts.length !== 2 || !o.pairReceipts.every(canonicalClock)
    || !Array.isArray(o.pairVersions) || o.pairVersions.length !== 2) throw Error('Invalid positive basket depth');
  const at = Date.parse(o.at), receipts = o.pairReceipts.map(Date.parse);
  if (at !== Math.max(...receipts) || Math.abs(receipts[0] - receipts[1]) > 20000
    || receipts.some(t => t <= Date.parse(raw.cutoff))) throw Error('Invalid basket followup clocks');
  for (const [index, key] of o.pairVersions.entries()) {
    const m = /^(0|[1-9]\d*):(.*)$/.exec(key);
    if (!m || !Number.isSafeInteger(Number(m[1])) || timestamp(m[2]) !== m[2]
      || instantNs(m[2]) > BigInt(receipts[index] + 2000) * 1000000n) throw Error('Invalid basket followup version');
    if (instantNs(m[2]) > BigInt(now) * 1000000n) return null;
  }
  const cost = amount(o.basket.totalCost), margin = amount(o.basket.conditionalMargin), unit = amount(o.basket.marginalUnitCost);
  if (cost <= 0n || cost > 25000000n || margin <= 0n || cost + margin !== BigInt(o.basket.quantity) * 1000000n
    || unit <= 0n || unit >= 1000000n) throw Error('Invalid positive basket economics');
  const selected = {marketIds: legs.map(l => l.marketId), exchangeIds: legs.map(l => l.exchangeId), receivedAt: o.at,
    priceGapMicros: Number(1000000n - unit), lastObserved: 0, followup: {observerBankHash: raw.observerBankHash, pairId: raw.pairId,
      sourceSequence: raw.sourceSequence, sourceHash: raw.sourceHash, anchorAt: o.at,
      pairVersions: legs.map(l => o.pairVersions[l.index]), pairReceipts: legs.map(l => o.pairReceipts[l.index]),
      sourceMeasurementHash: hash({reason: origin.reason, cycle: origin.cycle, every: origin.every, receivedAt: origin.receivedAt,
        marketIds: originLegs.map(l => l.marketId), exchangeIds: originLegs.map(l => l.exchangeId)})}};
  const f = selected.followup, key = hash([raw.observerBankHash, raw.pairId]);
  f.anchorHash = anchorHash({marketIds: selected.marketIds, exchangeIds: selected.exchangeIds, ...f}); f.claimKey = key;
  const prior = ledger.claims[key];
  if (prior && raw.sourceSequence === prior.sourceSequence && (raw.sourceHash !== prior.sourceHash || f.anchorHash !== prior.anchorHash)) throw Error('Conflicting basket followup source');
  if (prior && (raw.sourceSequence <= prior.sourceSequence || at <= Date.parse(prior.anchorAt)
    || f.pairVersions.every((v, i) => v === prior.pairVersions[i]))) return null;
  if (at > now || now - at > maxAgeMs || now <= at + latencyMs || selected.marketIds.some(id => excluded.has(id))) return null;
  return selected;
}

// This is a read-only measurement overlay. A positive supplied gap is a reason
// to inspect both books, never proof of a hedge, signal eligibility, or profit.
// The caller constructs each candidate from one scoped bulk response; receivedAt
// is that shared receipt, not a merge of independently timed leg quotes.
export function planBasketBooks(basePlan, scope, {candidates = [], followups = [], excludedMarketIds = [], cycle, now} = {},
  {every = 4, maxAgeMs = 120000, followupMaxAgeMs = 600000, followupLatencyMs = 1000} = {}) {
  if (!Number.isSafeInteger(cycle) || cycle < 0 || !Number.isSafeInteger(now) || now < 0 || now > 8640000000000000
    || !Number.isSafeInteger(every) || every < 1 || !Number.isSafeInteger(maxAgeMs) || maxAgeMs < 1 || maxAgeMs > 120000
    || !Number.isSafeInteger(followupMaxAgeMs) || followupMaxAgeMs < 1 || followupMaxAgeMs > 600000
    || !Number.isSafeInteger(followupLatencyMs) || followupLatencyMs < 1000 || followupLatencyMs > 60000) throw Error('Invalid basket measurement cadence/clock');
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
  const ledger = followupLedger(state.basketFollowups, markets, now);
  if (!Array.isArray(followups) || followups.length > 1000) throw Error('Invalid basket followups');
  const followupPairs = followups.map(f => positiveDepthFollowup(f, scope, markets, ledger, excluded, now, followupMaxAgeMs, followupLatencyMs)).filter(Boolean);
  const followupSources = new Map();
  for (const pair of followupPairs) {
    const f = pair.followup, key = f.claimKey + ':' + f.sourceSequence;
    if (followupSources.has(key) && followupSources.get(key) !== f.anchorHash) throw Error('Conflicting basket followup source');
    followupSources.set(key, f.anchorHash);
  }
  const finish = (reason, selected) => ({...output, basketMeasurement: {reason, cycle, every, ...(selected ? {
    marketIds: [...selected.marketIds], exchangeIds: [...selected.exchangeIds], receivedAt: selected.receivedAt, priceGapMicros: selected.priceGapMicros,
    ...(selected.followup ? {followup: structuredClone(selected.followup), persistSchedulerStateBeforeReads: true} : {}),
  } : {})}});
  if (!Array.isArray(candidates) && !followupPairs.length) return finish('hints_unavailable');
  const valid = [...followupPairs];
  for (const candidate of Array.isArray(candidates) ? candidates : []) {
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
  const budget = basePlan.reads.length, fitting = valid.filter(pair => new Set([...protectedIds, ...pair.marketIds]).size <= budget);
  if (!fitting.length) return finish('protected_capacity');
  fitting.sort((a, b) => Number(Boolean(b.followup)) - Number(Boolean(a.followup))
    || (a.followup && b.followup ? Date.parse(a.receivedAt) - Date.parse(b.receivedAt) : 0)
    || b.priceGapMicros - a.priceGapMicros || a.lastObserved - b.lastObserved
    || Number(a.marketIds[0]) - Number(b.marketIds[0]) || Number(a.marketIds[1]) - Number(b.marketIds[1])
    || Date.parse(b.receivedAt) - Date.parse(a.receivedAt));
  const selected = fitting[0], reads = protectedReads.map(read => structuredClone(read)), chosen = new Set(protectedIds);
  for (const marketId of selected.marketIds) if (!chosen.has(marketId)) {
    const read = existing.get(marketId), spec = markets.get(marketId);
    reads.push(read ? structuredClone(read) : {marketId, exchangeId: spec.exchangeId, option: spec.option,
      path: core.has(marketId) ? 'sourceSnapshots' : 'supplementalDepth', reason: 'basket_measurement'});
    chosen.add(marketId);
  }
  for (const read of basePlan.reads) if (reads.length < budget && !chosen.has(read.marketId)) { reads.push(structuredClone(read)); chosen.add(read.marketId); }
  output.reads = reads;
  if (selected.followup) {
    const {claimKey, ...claim} = selected.followup;
    ledger.claims[claimKey] = {...claim, marketIds: selected.marketIds, exchangeIds: selected.exchangeIds, claimedAt: new Date(now).toISOString()};
    if (Object.keys(ledger.claims).length > 1000) throw Error('Basket followup ledger capacity reached');
    output.state.basketFollowups = ledger;
  }
  return finish(selected.followup ? 'paired_depth_followup' : 'paired_depth_measurement', selected);
}
