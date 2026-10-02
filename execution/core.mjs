import {createHash} from 'node:crypto';

// Offline domain core. Callers persist every returned state before sending traffic.
// Ownership, STOP, account-wide admission, and durable storage belong to the host.
const SCALE = 1_000_000n, TICK = 5_000n;
export const DEFAULT_POLICY = Object.freeze({
  maxManagedMarkets: 3, maxAggregateCost: '500.000000', maxOrderCost: '100.000000',
  maxCumulativeBuyCost: '500.000000',
  cashReserve: '0.000000', maxAccountAgeMs: 180_000, maxBookAgeMs: 20_000,
  maxBookLatencyMs: 20_000, maxExpirationMs: 120_000,
});
const fail = code => { throw new Error(code); };
const check = (ok, code) => { if (!ok) fail(code); };
const id = x => typeof x === 'string' && /^[1-9]\d*$/.test(x) && Number.isSafeInteger(Number(x));
const qty = x => Number.isInteger(x) && x > 0 && x <= 2_147_483_647;
const stamp = x => typeof x === 'string' && Number.isFinite(Date.parse(x));
const clock = x => { const n = typeof x === 'string' ? Date.parse(x) : x; check(Number.isSafeInteger(n) && n >= 0, 'INVALID_CLOCK'); return n; };
const iso = x => new Date(clock(x)).toISOString();
const clone = x => structuredClone(x);
const plain = x => x !== null && typeof x === 'object' && !Array.isArray(x) && Object.getPrototypeOf(x) === Object.prototype;
const safeKey = x => !['__proto__', 'constructor', 'prototype'].includes(x);
function freeze(x) { if (x && typeof x === 'object') { Object.values(x).forEach(freeze); Object.freeze(x); } return x; }

export function canonicalPayload(value) {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return JSON.stringify(value);
  if (typeof value === 'number') { check(Number.isFinite(value), 'INVALID_JSON_NUMBER'); return JSON.stringify(value); }
  if (Array.isArray(value)) return '[' + value.map(canonicalPayload).join(',') + ']';
  check(plain(value) && Object.keys(value).every(safeKey), 'INVALID_JSON_VALUE');
  return '{' + Object.keys(value).sort().map(k => JSON.stringify(k) + ':' + canonicalPayload(value[k])).join(',') + '}';
}
export const payloadHash = value => createHash('sha256').update(canonicalPayload(value)).digest('hex');
export function decimalMicros(value, rounding = 'exact') {
  check(typeof value === 'string' && value.length <= 128 && /^(0|[1-9]\d*)(\.\d+)?$/.test(value), 'INVALID_DECIMAL');
  check(['exact', 'floor', 'ceil'].includes(rounding), 'INVALID_ROUNDING');
  const [whole, fraction = ''] = value.split('.');
  const tail = fraction.slice(6), extra = /[1-9]/.test(tail);
  check(rounding !== 'exact' || !extra, 'EXCESS_DECIMAL_PRECISION');
  return BigInt(whole) * SCALE + BigInt(fraction.slice(0, 6).padEnd(6, '0')) + (rounding === 'ceil' && extra ? 1n : 0n);
}
export function formatMicros(value) {
  check(typeof value === 'bigint' && value >= 0n, 'INVALID_MICROS');
  return `${value / SCALE}.${String(value % SCALE).padStart(6, '0')}`;
}
// API numbers have already passed through IEEE-754. Moving down one representable
// value gives a conservative lower bound on the original positive JSON decimal.
export function conservativeCashFloor(value) {
  if (typeof value === 'string') return decimalMicros(value, 'floor');
  check(typeof value === 'number' && Number.isFinite(value) && value >= 0, 'INVALID_CASH');
  if (value === 0) return 0n;
  const b = new DataView(new ArrayBuffer(8)); b.setFloat64(0, value);
  const bits = b.getBigUint64(0) - 1n, exponent = Number((bits >> 52n) & 2047n);
  const mantissa = (bits & ((1n << 52n) - 1n)) + (exponent ? 1n << 52n : 0n);
  const power = (exponent || 1) - 1023 - 52;
  return power >= 0 ? (mantissa << BigInt(power)) * SCALE : mantissa * SCALE / (1n << BigInt(-power));
}
function priceMicros(value) {
  const units = decimalMicros(typeof value === 'number' ? String(value) : value);
  check(units >= TICK && units <= SCALE - TICK && units % TICK === 0n, 'INVALID_LIMIT_PRICE');
  return units;
}
function policyOf(input = {}) {
  check(plain(input) && Object.keys(input).every(k => Object.hasOwn(DEFAULT_POLICY, k)), 'INVALID_POLICY');
  const p = {...DEFAULT_POLICY, ...input};
  check(Number.isInteger(p.maxManagedMarkets) && p.maxManagedMarkets >= 1 && p.maxManagedMarkets <= 3, 'INVALID_MARKET_CAP');
  check(decimalMicros(p.maxOrderCost) > 0n && decimalMicros(p.maxOrderCost) <= 100n * SCALE, 'INVALID_ORDER_CAP');
  check(decimalMicros(p.maxAggregateCost) >= decimalMicros(p.maxOrderCost) && decimalMicros(p.maxAggregateCost) <= 500n * SCALE, 'INVALID_AGGREGATE_CAP');
  check(decimalMicros(p.maxCumulativeBuyCost) >= decimalMicros(p.maxOrderCost) && decimalMicros(p.maxCumulativeBuyCost) <= 500n * SCALE, 'INVALID_CUMULATIVE_CAP');
  decimalMicros(p.cashReserve);
  for (const k of ['maxAccountAgeMs', 'maxBookAgeMs', 'maxBookLatencyMs', 'maxExpirationMs']) check(Number.isSafeInteger(p[k]) && p[k] > 0 && p[k] <= DEFAULT_POLICY[k], 'INVALID_' + k);
  return p;
}
function assertState(s) {
  check(plain(s) && s.schemaVersion === 1 && typeof s.tournamentId === 'string' && /^[0-9a-f-]{36}$/i.test(s.tournamentId) && plain(s.intents) && plain(s.owned), 'INVALID_STATE');
  policyOf(s.policy);
  const keys = new Set(), ordinals = new Set(), orderIds = new Set();
  for (const [intentId, i] of Object.entries(s.intents)) {
    check(safeKey(intentId) && plain(i) && plain(i.body) && Object.keys(i.body).length === 8 && Object.keys(i.body).every(k => ['idempotencyKey', 'exchangeId', 'side', 'action', 'quantity', 'price', 'expirationDate', 'tournamentId'].includes(k)), 'INVALID_INTENT');
    check(i.intentId === intentId && i.bodyJson === canonicalPayload(i.body) && i.payloadHash === payloadHash(i.body), 'INTENT_INTEGRITY_FAILURE');
    check(i.idempotencyKey === i.body.idempotencyKey && i.body.tournamentId === s.tournamentId && !keys.has(i.idempotencyKey) && i.exchangeId === i.body.exchangeId && i.side === i.body.side && i.action === i.body.action && i.quantity === i.body.quantity && i.expirationDate === i.body.expirationDate && priceMicros(i.price) === priceMicros(i.body.price), 'INTENT_IDENTITY_FAILURE');
    check(id(i.exchangeId) && id(i.marketId) && i.side === 'yes' && ['buy', 'sell'].includes(i.action) && qty(i.quantity) && typeof i.idempotencyKey === 'string' && i.idempotencyKey.length > 0 && i.idempotencyKey.length <= 255 && stamp(i.expirationDate), 'INVALID_INTENT_TERMS');
    check(['prepared', 'dispatching', 'uncertain', 'acknowledged', 'reconciled', 'closed'].includes(i.status) && Number.isSafeInteger(i.attempts) && i.attempts >= 0 && Number.isSafeInteger(i.ordinal) && i.ordinal >= 1 && !ordinals.has(i.ordinal), 'INVALID_INTENT_STATE');
    check(typeof i.retryAllowed === 'boolean' && (i.status === 'prepared' ? i.attempts === 0 && i.orderId === null : i.status === 'closed' && i.outcome === 'not_submitted' ? i.attempts === 0 : i.attempts > 0), 'INVALID_DISPATCH_HISTORY');
    check(i.orderId === null || Number.isSafeInteger(i.orderId) && i.orderId > 0, 'INVALID_ORDER_ID');
    if (i.orderId !== null) { check(!orderIds.has(i.orderId), 'DUPLICATE_ORDER_ATTRIBUTION'); orderIds.add(i.orderId); }
    check(!['acknowledged', 'reconciled'].includes(i.status) || i.orderId !== null, 'UNATTRIBUTABLE_STATE');
    check(!i.retryAllowed || i.status === 'uncertain' && i.orderId === null && stamp(i.retryAfterAt), 'INVALID_RETRY_STATE');
    check(typeof i.acceptanceUncertain === 'boolean', 'INVALID_ACCEPTANCE_STATE');
    check(Array.isArray(i.reconciledFills), 'INVALID_RECONCILED_FILLS');
    const fillIds = new Set(); let filled = 0;
    for (const f of i.reconciledFills) { check(plain(f) && Number.isSafeInteger(f.id) && f.id > 0 && !fillIds.has(f.id) && qty(f.quantity) && stamp(f.filledAt), 'INVALID_RECONCILED_FILL'); check(stamp(i.reconciledAt) && Date.parse(f.filledAt) <= Date.parse(i.reconciledAt) && Date.parse(f.filledAt) >= Date.parse(i.orderCreatedAt || i.preparedAt), 'INVALID_RECONCILED_FILL_TIME'); const p = priceMicros(f.price); check(i.action === 'buy' ? p <= priceMicros(i.price) : p >= priceMicros(i.price), 'FILL_OUTSIDE_LIMIT'); filled += f.quantity; fillIds.add(f.id); }
    check(filled <= i.quantity && (!i.reconciledFills.length || i.orderId !== null) && (i.filledQuantity === undefined || i.filledQuantity === filled), 'INVALID_FILL_HISTORY');
    keys.add(i.idempotencyKey);
    ordinals.add(i.ordinal);
  }
  check([...ordinals].every(n => n <= ordinals.size), 'INVALID_INTENT_SEQUENCE');
  for (const [exchangeId, p] of Object.entries(s.owned)) {
    check(id(exchangeId) && id(p.marketId) && Number.isSafeInteger(p.quantity) && p.quantity > 0 && Array.isArray(p.lots), 'INVALID_OWNED_POSITION');
    check(p.lots.every(l => qty(l.quantity)) && p.lots.reduce((n, l) => n + l.quantity, 0) === p.quantity, 'OWNED_QUANTITY_MISMATCH');
    check(p.lots.reduce((n, l) => n + decimalMicros(l.cost), 0n) === decimalMicros(p.cost), 'OWNED_COST_MISMATCH');
  }
  const replay = {owned: {}};
  for (const i of Object.values(s.intents).sort((a, b) => a.ordinal - b.ordinal)) applyFills(replay, i, i.reconciledFills);
  check(canonicalPayload(replay.owned) === canonicalPayload(s.owned), 'OWNERSHIP_PROVENANCE_MISMATCH');
  check(Object.values(s.intents).filter(i => i.status !== 'closed').length <= 1, 'MULTIPLE_UNRESOLVED_INTENTS');
}
export function validateExecutionState(state) { assertState(state); return freeze(clone(state)); }
export function createExecutionState({tournamentId, policy = {}}) {
  check(typeof tournamentId === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(tournamentId), 'INVALID_TOURNAMENT');
  return freeze({schemaVersion: 1, tournamentId, policy: policyOf(policy), intents: {}, owned: {}});
}
function fresh(part, tournamentId, now, maxAge) {
  check(part?.tournamentId === tournamentId, 'SCOPE_MISMATCH');
  check(stamp(part.at) && Date.parse(part.at) <= now && now - Date.parse(part.at) <= maxAge, 'STALE_ACCOUNT_EVIDENCE');
}
function accountSnapshot(s, snapshot, now) {
  check(snapshot?.tournamentId === s.tournamentId, 'SCOPE_MISMATCH');
  for (const k of ['cash', 'positions', 'orders']) fresh(snapshot[k], s.tournamentId, now, s.policy.maxAccountAgeMs);
  const cash = conservativeCashFloor(snapshot.cash.amount);
  for (const k of ['positions', 'orders']) check(snapshot[k].complete === true && Array.isArray(snapshot[k].rows), 'INCOMPLETE_ACCOUNT_EVIDENCE');
  const positions = new Map();
  for (const row of snapshot.positions.rows) {
    check(id(row.exchangeId) && id(row.marketId) && Number.isSafeInteger(row.quantity) && !positions.has(row.exchangeId), 'INVALID_POSITION');
    if (row.tournamentId !== undefined) check(row.tournamentId === s.tournamentId, 'SCOPE_MISMATCH');
    positions.set(row.exchangeId, row);
  }
  const orderIds = new Set();
  for (const row of snapshot.orders.rows) {
    check(Number.isSafeInteger(row.id) && row.id > 0 && !orderIds.has(row.id) && id(row.exchangeId) && qty(row.quantity) && row.open === true && ['yes', 'no'].includes(row.side) && ['buy', 'sell'].includes(row.action), 'INVALID_OPEN_ORDER');
    if (row.tournamentId !== undefined) check(row.tournamentId === s.tournamentId, 'SCOPE_MISMATCH');
    priceMicros(row.priceLimit); orderIds.add(row.id);
  }
  return {cash, positions, orders: snapshot.orders.rows};
}
function checkTerms(spec, s, now) {
  check(typeof spec.intentId === 'string' && safeKey(spec.intentId) && /^[A-Za-z0-9][A-Za-z0-9:_.-]{0,127}$/.test(spec.intentId), 'INVALID_INTENT_ID');
  check(typeof spec.idempotencyKey === 'string' && spec.idempotencyKey.length >= 1 && spec.idempotencyKey.length <= 255 && !/[\u0000-\u001f]/.test(spec.idempotencyKey), 'INVALID_IDEMPOTENCY_KEY');
  check(id(spec.marketId) && id(spec.exchangeId) && spec.side === 'yes' && ['buy', 'sell'].includes(spec.action) && qty(spec.quantity), 'INVALID_ORDER_TERMS');
  check(spec.tournamentId === undefined || spec.tournamentId === s.tournamentId, 'SCOPE_MISMATCH');
  check(stamp(spec.expirationDate) && Date.parse(spec.expirationDate) > now && Date.parse(spec.expirationDate) - now <= s.policy.maxExpirationMs, 'INVALID_EXPIRATION');
  check(typeof spec.attribution?.runId === 'string' && spec.attribution.runId.length > 0 && typeof spec.attribution.strategyId === 'string' && spec.attribution.strategyId.length > 0, 'ATTRIBUTION_REQUIRED');
  // Attribution is an explicit allowlist; never persist arbitrary caller metadata.
  check(Object.keys(spec.attribution).every(k => ['runId', 'strategyId', 'modelHash', 'signalId', 'reason'].includes(k)) && Object.values(spec.attribution).every(v => typeof v === 'string' && v.length <= 1000), 'INVALID_ATTRIBUTION');
  return priceMicros(spec.price);
}
function preflight(s, spec, snapshot, now, ignoreIntentId = null) {
  const price = checkTerms(spec, s, now), {cash, positions, orders} = accountSnapshot(s, snapshot, now);
  check(!Object.values(s.intents).some(i => i.intentId !== ignoreIntentId && i.status !== 'closed'), 'UNRESOLVED_INTENT');
  for (const [exchangeId, own] of Object.entries(s.owned)) check(positions.get(exchangeId)?.quantity === own.quantity && positions.get(exchangeId)?.marketId === own.marketId, 'MANAGED_POSITION_RECONCILIATION_REQUIRED');
  const owned = s.owned[spec.exchangeId], actual = positions.get(spec.exchangeId);
  check(!actual || actual.marketId === spec.marketId, 'MARKET_IDENTITY_MISMATCH');
  check((actual?.quantity || 0) >= 0, 'OPPOSITE_SIDE_POSITION');
  check((actual?.quantity || 0) === (owned?.quantity || 0), 'UNATTRIBUTED_POSITION');
  check(!owned || owned.marketId === spec.marketId, 'MARKET_IDENTITY_MISMATCH');
  check(!orders.some(o => o.exchangeId === spec.exchangeId), 'EXISTING_EXCHANGE_ORDER');
  const cost = price * BigInt(spec.quantity);
  check(cost <= decimalMicros(s.policy.maxOrderCost), 'ORDER_COST_CAP');
  const aggregate = Object.values(s.owned).reduce((n, p) => n + decimalMicros(p.cost), 0n);
  if (spec.action === 'buy') {
    // External pending BUYs reserve cash even when they are on another exchange.
    const reserved = orders.filter(o => o.action === 'buy').reduce((n, o) => n + priceMicros(o.priceLimit) * BigInt(o.quantity), 0n);
    check(cash - reserved - cost >= decimalMicros(s.policy.cashReserve), 'INSUFFICIENT_UNRESERVED_CASH');
    check(aggregate + cost <= decimalMicros(s.policy.maxAggregateCost), 'AGGREGATE_COST_CAP');
    const cumulative = Object.values(s.intents).filter(i => i.action === 'buy').reduce((n, i) => n + i.reconciledFills.reduce((sum, f) => sum + priceMicros(f.price) * BigInt(f.quantity), 0n), 0n);
    check(cumulative + cost <= decimalMicros(s.policy.maxCumulativeBuyCost), 'CUMULATIVE_BUY_COST_CAP');
    check(new Set([...Object.values(s.owned).map(p => p.marketId), spec.marketId]).size <= s.policy.maxManagedMarkets, 'MANAGED_MARKET_CAP');
  } else check(owned && spec.quantity <= owned.quantity, 'OWNED_INVENTORY_REQUIRED');
  const b = snapshot.book;
  check(b?.tournamentId === s.tournamentId && b.marketId === spec.marketId && b.exchangeId === spec.exchangeId, 'BOOK_SCOPE_MISMATCH');
  check(stamp(b.at) && stamp(b.requestStartedAt) && Date.parse(b.at) <= now && now - Date.parse(b.at) <= s.policy.maxBookAgeMs && Date.parse(b.at) >= Date.parse(b.requestStartedAt) && Date.parse(b.at) - Date.parse(b.requestStartedAt) <= s.policy.maxBookLatencyMs, 'STALE_BOOK_EVIDENCE');
  const levels = (rows, descending) => {
    check(Array.isArray(rows) && rows.length > 0, 'MISSING_BOOK_DEPTH');
    let previous = null;
    return rows.map(r => { const p = priceMicros(r.price); check(qty(r.quantity) && (previous === null || (descending ? p < previous : p > previous)), 'INVALID_BOOK_DEPTH'); previous = p; return {price: p, quantity: r.quantity}; });
  };
  const bids = levels(b.bids, true), asks = levels(b.asks, false);
  check(bids[0].price < asks[0].price, 'LOCKED_OR_CROSSED_BOOK');
  const depth = (spec.action === 'buy' ? asks : bids).filter(l => spec.action === 'buy' ? l.price <= price : l.price >= price).reduce((n, l) => n + BigInt(l.quantity), 0n);
  check(depth >= BigInt(spec.quantity), 'INSUFFICIENT_EXECUTABLE_DEPTH');
  return {maxNotional: formatMicros(cost), observedCashFloor: formatMicros(cash), aggregateCostAfter: formatMicros(aggregate + (spec.action === 'buy' ? cost : 0n)), checkedAt: iso(now)};
}
function result(s, intentId, code = 'OK', extra = {}) {
  assertState(s); freeze(s);
  const intent = intentId ? s.intents[intentId] : undefined;
  return {ok: true, code, state: s, ...(intent ? {intent} : {}), ...extra};
}
function denied(s, error) { return {ok: false, code: error.message, state: s}; }
function requestFor(i) { return freeze({method: 'POST', path: '/orders', body: i.body, bodyJson: i.bodyJson, payloadHash: i.payloadHash}); }
export function revalidateIntentEvidence(state, intentId, snapshot, now) {
  try { assertState(state); now = clock(now); const i = state.intents[intentId]; check(i && i.status !== 'closed', 'UNKNOWN_ACTIVE_INTENT'); const risk = preflight(state, i, snapshot, now, intentId); return {ok: true, code: 'EVIDENCE_REVALIDATED', state, risk}; } catch (e) { return denied(state, e); }
}
export function prepareIntent(state, spec, snapshot, now) {
  try {
    assertState(state); now = clock(now);
    check(!state.intents[spec.intentId], 'INTENT_ID_ALREADY_USED');
    check(!Object.values(state.intents).some(i => i.idempotencyKey === spec.idempotencyKey), 'IDEMPOTENCY_KEY_ALREADY_USED');
    const risk = preflight(state, spec, snapshot, now);
    const body = {idempotencyKey: spec.idempotencyKey, exchangeId: spec.exchangeId, side: 'yes', action: spec.action, quantity: spec.quantity, price: Number(priceMicros(spec.price)) / Number(SCALE), expirationDate: spec.expirationDate, tournamentId: state.tournamentId};
    const i = {intentId: spec.intentId, idempotencyKey: spec.idempotencyKey, marketId: spec.marketId, exchangeId: spec.exchangeId, side: spec.side, action: spec.action, quantity: spec.quantity, price: formatMicros(priceMicros(spec.price)), expirationDate: spec.expirationDate, attribution: clone(spec.attribution), method: 'POST', path: '/orders', ordinal: Object.keys(state.intents).length + 1, body, bodyJson: canonicalPayload(body), payloadHash: payloadHash(body), risk, status: 'prepared', preparedAt: iso(now), attempts: 0, orderId: null, retryAllowed: false, acceptanceUncertain: false, reconciledFills: []};
    const s = clone(state); s.intents[spec.intentId] = i;
    return result(s, spec.intentId, 'PREPARED', {request: requestFor(i)});
  } catch (e) { return denied(state, e); }
}
export function beginDispatch(state, intentId, {snapshot, now, retry = false} = {}) {
  try {
    assertState(state); now = clock(now); const prior = state.intents[intentId];
    check(prior, 'UNKNOWN_INTENT');
    if (prior.status === 'prepared') { check(!retry, 'RETRY_BEFORE_FIRST_DISPATCH'); preflight(state, prior, snapshot, now, intentId); }
    else {
      check(retry && prior.status === 'uncertain' && prior.retryAllowed, 'DISPATCH_NOT_ALLOWED'); check(now >= Date.parse(prior.retryAfterAt), 'RETRY_TOO_EARLY');
      // Idempotency alone does not rule out first acceptance after an earlier 429
      // or transport loss. Recheck current risk while the limit can still execute.
      // After expiry, a fresh placement is invalid; identical retries can only
      // recover an existing result or receive the expired-order rejection.
      if (now < Date.parse(prior.expirationDate)) preflight(state, prior, snapshot, now, intentId);
    }
    const s = clone(state), i = s.intents[intentId]; i.status = 'dispatching'; i.attempts++; i.dispatchedAt = iso(now); i.retryAllowed = false;
    return result(s, intentId, 'DISPATCHING', {request: requestFor(i)});
  } catch (e) { return denied(state, e); }
}
function uncertain(state, intentId, code, now, retryAllowed = false, delay = 0, couldHaveAccepted = true) {
  const s = clone(state), i = s.intents[intentId]; i.status = 'uncertain'; i.reasonCode = code; i.retryAllowed = retryAllowed; i.retryAfterAt = iso(now + delay); i.acceptanceUncertain ||= couldHaveAccepted;
  return {...result(s, intentId, code), ok: false};
}
export function recordPlacement(state, intentId, outcome, now) {
  try {
    assertState(state); now = clock(now); const prior = state.intents[intentId]; check(prior?.status === 'dispatching', 'PLACEMENT_STATE_MISMATCH');
    const {status = null, code = 'TRANSPORT_AMBIGUOUS', response: r} = outcome || {};
    if (status !== 200) {
      if (status === 409 && code !== 'REQUEST_IN_FLIGHT') return uncertain(state, intentId, code, now);
      if (status === null || [429, 500, 502, 503, 504].includes(status) || status === 409 && code === 'REQUEST_IN_FLIGHT') {
        const supplied = outcome?.retryAfterMs ?? 0;
        check(Number.isSafeInteger(supplied) && supplied >= 0 && supplied <= 86_400_000, 'MANUAL_RETRY_REVIEW_REQUIRED');
        return uncertain(state, intentId, code, now, true, Math.max(supplied, status === 409 ? 90_000 : status === 429 ? 60_000 : 1000), status !== 429);
      }
      if (Number.isInteger(status) && status >= 400 && status < 500) {
        if (prior.acceptanceUncertain) return uncertain(state, intentId, 'RETRY_REJECTED_AFTER_AMBIGUITY', now);
        const s = clone(state); Object.assign(s.intents[intentId], {status: 'closed', outcome: 'rejected', reasonCode: code, closedAt: iso(now)}); return result(s, intentId, 'REJECTED');
      }
      return uncertain(state, intentId, 'UNEXPECTED_PLACEMENT_STATUS', now);
    }
    if (!Number.isSafeInteger(r?.orderId) || r.orderId <= 0) return uncertain(state, intentId, 'UNATTRIBUTABLE_ORDER_ID', now);
    const s = clone(state); s.intents[intentId].orderId = r.orderId;
    try {
      check(r.exchangeId === prior.body.exchangeId && (r.tournamentId === undefined || r.tournamentId === state.tournamentId) && r.side === 'yes' && r.action === prior.body.action && r.quantity === prior.body.quantity && priceMicros(r.price) === priceMicros(prior.body.price), 'CANONICAL_ORDER_MISMATCH');
      check(typeof r.open === 'boolean' && Number.isSafeInteger(r.quantityTraded) && r.quantityTraded >= 0 && Number.isSafeInteger(r.remainingQuantity) && r.remainingQuantity >= 0 && r.quantityTraded + r.remainingQuantity <= prior.body.quantity, 'INVALID_PLACEMENT_QUANTITIES');
      check(!r.open || r.remainingQuantity > 0, 'INVALID_RESTING_ORDER');
      check(r.totalCost !== null && r.totalCost !== undefined && conservativeCashFloor(r.totalCost) >= 0n, 'INVALID_PLACEMENT_COST');
    } catch (e) { return uncertain(s, intentId, e.message, now); }
    Object.assign(s.intents[intentId], {status: 'acknowledged', acknowledgedAt: iso(now), responseSummary: {orderId: r.orderId, open: r.open, quantityTraded: r.quantityTraded, remainingQuantity: r.remainingQuantity, terminalReasonCode: r.terminalReasonCode ?? null}});
    return result(s, intentId, 'ACKNOWLEDGED');
  } catch (e) {
    if (state.intents?.[intentId]?.status === 'dispatching') return uncertain(state, intentId, e.message, clock(now));
    return denied(state, e);
  }
}
function applyFills(s, i, fills) {
  let owned = s.owned[i.exchangeId];
  for (const f of fills) {
    const cost = priceMicros(f.price) * BigInt(f.quantity);
    if (i.action === 'buy') {
      owned ??= {marketId: i.marketId, quantity: 0, cost: '0.000000', lots: []};
      owned.quantity += f.quantity; owned.cost = formatMicros(decimalMicros(owned.cost) + cost); owned.lots.push({quantity: f.quantity, cost: formatMicros(cost)});
    } else {
      check(owned && owned.quantity >= f.quantity, 'RECONCILIATION_OVERSELL');
      let left = f.quantity, used = 0n;
      while (left > 0) { const lot = owned.lots[0], n = Math.min(left, lot.quantity); const part = n === lot.quantity ? decimalMicros(lot.cost) : decimalMicros(lot.cost) * BigInt(n) / BigInt(lot.quantity); used += part; lot.quantity -= n; lot.cost = formatMicros(decimalMicros(lot.cost) - part); left -= n; if (!lot.quantity) owned.lots.shift(); }
      owned.quantity -= f.quantity; owned.cost = formatMicros(decimalMicros(owned.cost) - used);
    }
  }
  if (owned?.quantity) s.owned[i.exchangeId] = owned; else delete s.owned[i.exchangeId];
}
export function reconcileIntent(state, intentId, evidence, now) {
  try {
    assertState(state); now = clock(now); const prior = state.intents[intentId];
    check(prior && ['acknowledged', 'reconciled', 'uncertain'].includes(prior.status) && Number.isSafeInteger(prior.orderId) && prior.orderId > 0, 'RECONCILIATION_NOT_ATTRIBUTABLE');
    const {positions, orders} = accountSnapshot(state, evidence.snapshot, now), o = evidence.order, f = evidence.fills;
    check(o?.id === prior.orderId && o.exchangeId === prior.exchangeId && o.tournamentId === state.tournamentId && o.side === 'yes' && o.action === prior.action && priceMicros(o.priceLimit) === priceMicros(prior.price) && stamp(o.expirationDate) && Date.parse(o.expirationDate) === Date.parse(prior.expirationDate) && typeof o.open === 'boolean', 'ORDER_RECONCILIATION_MISMATCH');
    check(stamp(evidence.at) && Date.parse(evidence.at) <= now && now - Date.parse(evidence.at) <= state.policy.maxAccountAgeMs, 'STALE_ORDER_EVIDENCE');
    check(f?.complete === true && f.orderId === prior.orderId && f.exchangeId === prior.exchangeId && f.tournamentId === state.tournamentId && Array.isArray(f.rows), 'INCOMPLETE_FILL_EVIDENCE');
    const fillReadAt = evidence.fillsReceivedAt === undefined ? now : Date.parse(evidence.fillsReceivedAt);
    check(Number.isFinite(fillReadAt) && fillReadAt <= now, 'INVALID_FILL_READ_TIME');
    const createdAt = o.createdAt === undefined ? prior.preparedAt : o.createdAt;
    check(stamp(createdAt) && Date.parse(createdAt) <= fillReadAt && (prior.orderCreatedAt === undefined || Date.parse(prior.orderCreatedAt) === Date.parse(createdAt)), 'INVALID_ORDER_CREATION_TIME');
    const seen = new Set();
    const fills = f.rows.map(row => {
      check(Number.isSafeInteger(row.id) && row.id > 0 && !seen.has(row.id) && row.side === 'yes' && qty(row.quantity) && stamp(row.filledAt), 'INVALID_FILL'); seen.add(row.id);
      check(Date.parse(row.filledAt) <= fillReadAt && Date.parse(row.filledAt) >= Date.parse(createdAt), 'INVALID_FILL_TIME');
      const p = priceMicros(row.price); check(prior.action === 'buy' ? p <= priceMicros(prior.price) : p >= priceMicros(prior.price), 'FILL_OUTSIDE_LIMIT');
      return {id: row.id, quantity: row.quantity, price: formatMicros(p), filledAt: row.filledAt};
    }).sort((a, b) => Date.parse(a.filledAt) - Date.parse(b.filledAt) || a.id - b.id);
    const total = fills.reduce((n, row) => n + row.quantity, 0);
    check(Number.isSafeInteger(total) && total <= prior.quantity && f.totalQuantityFilled === total, 'FILL_TOTAL_MISMATCH');
    check(total >= (prior.responseSummary?.quantityTraded || 0), 'FILLS_BEHIND_PLACEMENT_RESPONSE');
    for (const old of prior.reconciledFills) check(fills.some(x => canonicalPayload(x) === canonicalPayload(old)), 'FILL_HISTORY_CHANGED');
    const expectedRemainder = prior.quantity - total;
    check(Number.isSafeInteger(o.quantity) && o.quantity >= 0 && (o.open ? o.quantity === expectedRemainder && o.quantity > 0 : o.quantity >= total && o.quantity <= prior.quantity), 'ORDER_QUANTITY_MISMATCH');
    const targetOrders = orders.filter(x => x.exchangeId === prior.exchangeId);
    check(o.open ? targetOrders.length === 1 && targetOrders[0].id === o.id && targetOrders[0].quantity === o.quantity && targetOrders[0].side === 'yes' && targetOrders[0].action === prior.action && priceMicros(targetOrders[0].priceLimit) === priceMicros(prior.price) : targetOrders.length === 0, 'OPEN_ORDER_RECONCILIATION_MISMATCH');
    const s = clone(state), i = s.intents[intentId], oldIds = new Set(prior.reconciledFills.map(x => x.id)); applyFills(s, i, fills.filter(x => !oldIds.has(x.id)));
    check((positions.get(prior.exchangeId)?.quantity || 0) === (s.owned[prior.exchangeId]?.quantity || 0), 'POSITION_RECONCILIATION_MISMATCH');
    check(!positions.has(prior.exchangeId) || positions.get(prior.exchangeId).marketId === prior.marketId, 'MARKET_IDENTITY_MISMATCH');
    i.reconciledFills = fills; i.reconciledAt = iso(now); i.orderCreatedAt = createdAt; i.retryAllowed = false; i.status = o.open ? 'reconciled' : 'closed'; i.filledQuantity = total;
    if (!o.open) { i.closedAt = iso(now); i.outcome = total === prior.quantity ? 'filled' : total ? 'partially_filled' : 'unfilled'; }
    return result(s, intentId, o.open ? 'RECONCILED_OPEN' : 'CLOSED');
  } catch (e) { return denied(state, e); }
}
export function abandonPreparedIntent(state, intentId, reason, now) {
  try { assertState(state); check(state.intents[intentId]?.status === 'prepared', 'CANNOT_ABANDON_DISPATCHED_INTENT'); check(typeof reason === 'string' && reason.length > 0 && reason.length <= 120, 'INVALID_REASON'); const s = clone(state); Object.assign(s.intents[intentId], {status: 'closed', outcome: 'not_submitted', reasonCode: reason, closedAt: iso(now)}); return result(s, intentId, 'ABANDONED'); } catch (e) { return denied(state, e); }
}
