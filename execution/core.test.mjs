import test from 'node:test';
import assert from 'node:assert/strict';
import {DEFAULT_POLICY, createExecutionState, validateExecutionState, prepareIntent, beginDispatch, recordPlacement, reconcileIntent, abandonPreparedIntent, canonicalPayload, payloadHash, decimalMicros, formatMicros, conservativeCashFloor} from './core.mjs';

const TID = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const NOW = Date.parse('2026-10-02T12:00:00.000Z');
const at = n => new Date(n).toISOString();
const initial = policy => createExecutionState({tournamentId: TID, policy});
const terms = changes => ({intentId: 'intent-1', idempotencyKey: 'stable-key-1', marketId: '377', exchangeId: '700', side: 'yes', action: 'buy', quantity: 20, price: '0.500', expirationDate: at(NOW + 120_000), attribution: {runId: 'pilot-1', strategyId: 'quant-1', signalId: 'signal-1'}, ...changes});
function snapshot(now = NOW) {
  const scoped = {tournamentId: TID, at: at(now - 90_000)};
  return {tournamentId: TID, cash: {...scoped, amount: '100000.123456789'}, positions: {...scoped, complete: true, rows: []}, orders: {...scoped, complete: true, rows: []}, book: {tournamentId: TID, marketId: '377', exchangeId: '700', at: at(now - 1000), requestStartedAt: at(now - 16_000), bids: [{price: '0.495', quantity: 2000}], asks: [{price: '0.500', quantity: 2000}]}};
}
function dispatched(spec = terms(), state = initial(), snap = snapshot()) {
  const prepared = prepareIntent(state, spec, snap, NOW); assert.equal(prepared.ok, true, prepared.code);
  const result = beginDispatch(prepared.state, spec.intentId, {snapshot: snap, now: NOW}); assert.equal(result.ok, true, result.code); return result;
}
function acknowledged(spec = terms(), state = initial(), snap = snapshot()) {
  const d = dispatched(spec, state, snap);
  const a = recordPlacement(d.state, spec.intentId, {status: 200, response: {orderId: 1234 + Object.keys(state.intents).length, exchangeId: spec.exchangeId, action: spec.action, side: 'yes', price: Number(spec.price), quantity: spec.quantity, remainingQuantity: spec.quantity, quantityTraded: 0, totalCost: 0, open: true, terminalReasonCode: null}}, NOW + 1000);
  assert.equal(a.ok, true, a.code); return a;
}
function evidence(spec, {filled = 20, open = false, owned = filled, price = '0.500', fills, orderId = 1234} = {}) {
  const now = NOW + 2000, snap = snapshot(now);
  snap.cash.at = snap.positions.at = snap.orders.at = at(now);
  snap.positions.rows = owned ? [{exchangeId: spec.exchangeId, marketId: spec.marketId, quantity: owned}] : [];
  const order = {id: orderId, exchangeId: spec.exchangeId, tournamentId: TID, side: 'yes', action: spec.action, priceLimit: Number(spec.price), expirationDate: spec.expirationDate, open, quantity: open ? spec.quantity - filled : spec.quantity};
  snap.orders.rows = open ? [{...order}] : [];
  return {at: at(now), snapshot: snap, order, fills: {orderId, exchangeId: spec.exchangeId, tournamentId: TID, complete: true, totalQuantityFilled: filled, rows: fills ?? (filled ? [{id: 501, side: 'yes', quantity: filled, price, filledAt: at(NOW + 1500)}] : [])}};
}

test('exact decimals floor cash without rounding upward; IEEE cash is conservative', () => {
  assert.equal(decimalMicros('100.123456789', 'floor'), 100123456n);
  assert.equal(decimalMicros('100.123456789', 'ceil'), 100123457n);
  assert.throws(() => decimalMicros('100.123456789'), /EXCESS_DECIMAL_PRECISION/);
  assert.equal(conservativeCashFloor(100000), 99999999999n);
  assert.equal(conservativeCashFloor(0), 0n);
  assert.equal(conservativeCashFloor(Number.MIN_VALUE), 0n);
  assert.equal(formatMicros(conservativeCashFloor('100000.123456789')), '100000.123456');
  for (const x of [NaN, Infinity, -1, null]) assert.throws(() => conservativeCashFloor(x));
});

test('stable canonical body hash and frozen explicit scoped intent', () => {
  assert.equal(canonicalPayload({b: 2, a: {z: 1, a: 2}}), '{"a":{"a":2,"z":1},"b":2}');
  assert.equal(payloadHash({b: 2, a: 1}), payloadHash({a: 1, b: 2}));
  const s = initial(), spec = terms(), p = prepareIntent(s, spec, snapshot(), NOW);
  assert.equal(p.ok, true); assert.equal(p.intent.status, 'prepared'); assert.equal(p.intent.body.tournamentId, TID);
  assert.equal(p.request.body.side, 'yes'); assert.equal(p.request.body.price, 0.5);
  assert.equal(p.intent.risk.maxNotional, '10.000000'); assert.equal(Object.keys(s.intents).length, 0);
  spec.quantity = 300; assert.equal(p.intent.quantity, 20);
  assert.throws(() => { p.intent.body.quantity = 500; }, TypeError);
});

test('only integer YES limit orders, explicit attribution and <=120 second expiry', () => {
  for (const patch of [{side: 'no'}, {action: 'BUY'}, {quantity: 1.5}, {quantity: 0}, {quantity: 2_147_483_648}, {exchangeId: 'x'}, {price: '1'}, {price: '0'}, {price: '0.501'}, {price: undefined}, {expirationDate: at(NOW)}, {expirationDate: at(NOW + 120001)}, {attribution: {}}, {tournamentId: 'different'}]) {
    const r = prepareIntent(initial(), terms(patch), snapshot(), NOW); assert.equal(r.ok, false, JSON.stringify(patch));
  }
});

test('scoped account evidence can be 90 seconds old but not stale/incomplete', () => {
  assert.equal(prepareIntent(initial(), terms(), snapshot(), NOW).ok, true);
  for (const key of ['cash', 'positions', 'orders']) {
    let b = snapshot(); b[key].at = at(NOW - DEFAULT_POLICY.maxAccountAgeMs - 1);
    assert.equal(prepareIntent(initial(), terms(), b, NOW).code, 'STALE_ACCOUNT_EVIDENCE');
    b = snapshot(); b[key].tournamentId = 'other';
    assert.equal(prepareIntent(initial(), terms(), b, NOW).code, 'SCOPE_MISMATCH');
  }
  for (const key of ['positions', 'orders']) { const b = snapshot(); b[key].complete = false; assert.equal(prepareIntent(initial(), terms(), b, NOW).code, 'INCOMPLETE_ACCOUNT_EVIDENCE'); }
});

test('book freshness, measured latency, identity, price order and depth are checked', () => {
  for (const patch of [{at: at(NOW - 20001)}, {requestStartedAt: at(NOW - 22000)}, {marketId: '999'}, {asks: []}, {asks: [{price: '0.495', quantity: 200}]}, {asks: [{price: '0.500', quantity: 19}]}, {asks: [{price: '0.500', quantity: 10}, {price: '0.495', quantity: 10}]}]) {
    const b = snapshot(); Object.assign(b.book, patch); assert.equal(prepareIntent(initial(), terms(), b, NOW).ok, false, JSON.stringify(patch));
  }
});

test('risk includes own order cost, reserve and unrelated outstanding BUY reservations', () => {
  assert.equal(prepareIntent(initial(), terms({quantity: 201}), snapshot(), NOW).code, 'ORDER_COST_CAP');
  let b = snapshot(); b.cash.amount = '10.000000'; assert.equal(prepareIntent(initial(), terms(), b, NOW).ok, true);
  b.cash.amount = '9.99999999999'; assert.equal(prepareIntent(initial(), terms(), b, NOW).code, 'INSUFFICIENT_UNRESERVED_CASH');
  b.cash.amount = 10; assert.equal(prepareIntent(initial(), terms(), b, NOW).code, 'INSUFFICIENT_UNRESERVED_CASH');
  b.cash.amount = '20'; b.orders.rows = [{id: 999, exchangeId: '701', side: 'no', action: 'buy', quantity: 40, priceLimit: 0.5, open: true}];
  assert.equal(prepareIntent(initial(), terms(), b, NOW).code, 'INSUFFICIENT_UNRESERVED_CASH');
  b.orders.rows = []; assert.equal(prepareIntent(initial({cashReserve: '11'}), terms(), b, NOW).code, 'INSUFFICIENT_UNRESERVED_CASH');
});

test('cannot adopt old inventory, net against NO, or interact with an existing target order', () => {
  let b = snapshot(); b.positions.rows = [{exchangeId: '700', marketId: '377', quantity: -20}];
  assert.equal(prepareIntent(initial(), terms(), b, NOW).code, 'OPPOSITE_SIDE_POSITION');
  b.positions.rows[0].quantity = 20; assert.equal(prepareIntent(initial(), terms({action: 'sell'}), b, NOW).code, 'UNATTRIBUTED_POSITION');
  b = snapshot(); b.orders.rows = [{id: 123, exchangeId: '700', side: 'no', action: 'buy', quantity: 10, priceLimit: 0.5, open: true}];
  assert.equal(prepareIntent(initial(), terms(), b, NOW).code, 'EXISTING_EXCHANGE_ORDER');
});

test('cannot reuse intent IDs or idempotency keys or prepare alongside unresolved intent', () => {
  const p = prepareIntent(initial(), terms(), snapshot(), NOW);
  assert.equal(prepareIntent(p.state, terms(), snapshot(), NOW).code, 'INTENT_ID_ALREADY_USED');
  assert.equal(prepareIntent(p.state, terms({intentId: 'second'}), snapshot(), NOW).code, 'IDEMPOTENCY_KEY_ALREADY_USED');
  assert.equal(prepareIntent(p.state, terms({intentId: 'second', idempotencyKey: 'key-2'}), snapshot(), NOW).code, 'UNRESOLVED_INTENT');
});

test('first dispatch revalidates after queue wait, rejects expiry and state tampering', () => {
  const p = prepareIntent(initial(), terms(), snapshot(), NOW);
  assert.equal(beginDispatch(p.state, 'intent-1', {snapshot: snapshot(), now: NOW + 21000}).code, 'STALE_BOOK_EVIDENCE');
  assert.equal(beginDispatch(p.state, 'intent-1', {snapshot: snapshot(NOW + 120000), now: NOW + 120000}).code, 'INVALID_EXPIRATION');
  const corrupt = structuredClone(p.state); corrupt.intents['intent-1'].body.quantity = 100;
  assert.equal(beginDispatch(corrupt, 'intent-1', {snapshot: snapshot(), now: NOW}).code, 'INTENT_INTEGRITY_FAILURE');
});

test('ambiguous response blocks new intents and identical request retry preserves bytes/key', () => {
  const d = dispatched();
  const u = recordPlacement(d.state, 'intent-1', {status: 502, code: 'ORDER_STATUS_UNKNOWN'}, NOW + 1000);
  assert.equal(u.ok, false); assert.equal(u.intent.status, 'uncertain'); assert.equal(u.intent.retryAllowed, true);
  assert.equal(beginDispatch(u.state, 'intent-1', {now: NOW + 1500, retry: true}).code, 'RETRY_TOO_EARLY');
  assert.equal(beginDispatch(u.state, 'intent-1', {now: NOW + 5000}).code, 'DISPATCH_NOT_ALLOWED');
  const retry = beginDispatch(u.state, 'intent-1', {snapshot: snapshot(NOW + 5000), now: NOW + 5000, retry: true});
  assert.equal(retry.ok, true); assert.equal(retry.request.bodyJson, d.request.bodyJson); assert.equal(retry.request.payloadHash, d.request.payloadHash); assert.equal(retry.intent.attempts, 2);
});

test('in-flight retries wait 90 seconds; conflicts and excessive retry delays fail closed', () => {
  const d = dispatched();
  const u = recordPlacement(d.state, 'intent-1', {status: 409, code: 'REQUEST_IN_FLIGHT'}, NOW);
  assert.equal(beginDispatch(u.state, 'intent-1', {now: NOW + 89999, retry: true}).code, 'RETRY_TOO_EARLY');
  assert.equal(beginDispatch(u.state, 'intent-1', {snapshot: snapshot(NOW + 90000), now: NOW + 90000, retry: true}).ok, true);
  const conflict = recordPlacement(d.state, 'intent-1', {status: 409, code: 'CONFLICT'}, NOW);
  assert.equal(conflict.intent.retryAllowed, false);
  const excessive = recordPlacement(d.state, 'intent-1', {status: 429, code: 'RATE_LIMITED', retryAfterMs: Number.MAX_SAFE_INTEGER}, NOW);
  assert.equal(excessive.intent.status, 'uncertain'); assert.equal(excessive.intent.retryAllowed, false);
});

test('canonical response mismatch and null IDs never become success or auto-retry', () => {
  const d = dispatched(), response = {orderId: 1234, exchangeId: '700', action: 'buy', side: 'yes', quantity: 20, price: 0.5, open: true, quantityTraded: 0, remainingQuantity: 20, totalCost: 0};
  for (const patch of [{orderId: null}, {side: 'no'}, {action: 'sell'}, {quantity: 19}, {price: 0.505}, {exchangeId: '701'}, {remainingQuantity: 21}]) {
    const r = recordPlacement(d.state, 'intent-1', {status: 200, response: {...response, ...patch}}, NOW);
    assert.equal(r.ok, false, JSON.stringify(patch)); assert.equal(r.intent.status, 'uncertain'); assert.equal(r.intent.retryAllowed, false);
  }
});

test('a terminal 4xx rejects the intent while dispatching crash recovery remains ambiguous', () => {
  const d = dispatched();
  const rejected = recordPlacement(d.state, 'intent-1', {status: 403, code: 'TERMS_NOT_ACKNOWLEDGED'}, NOW);
  assert.equal(rejected.intent.status, 'closed'); assert.equal(rejected.intent.outcome, 'rejected');
  const crash = recordPlacement(d.state, 'intent-1', {status: null, code: 'PROCESS_DIED_AFTER_DISPATCH'}, NOW);
  assert.equal(crash.intent.status, 'uncertain'); assert.equal(crash.intent.retryAllowed, true);
});

test('official fill reconciliation creates managed inventory exactly once', () => {
  const spec = terms(), a = acknowledged(spec), e = evidence(spec);
  const r = reconcileIntent(a.state, 'intent-1', e, NOW + 2000);
  assert.equal(r.ok, true, r.code); assert.equal(r.intent.status, 'closed'); assert.equal(r.intent.outcome, 'filled');
  assert.equal(r.state.owned['700'].quantity, 20); assert.equal(r.state.owned['700'].cost, '10.000000');
  assert.equal(reconcileIntent(r.state, 'intent-1', e, NOW + 2000).ok, false);
});

test('partial fill reconciles open remainder; repeat reconciliation cannot double-count fills', () => {
  const spec = terms(), a = acknowledged(spec), e = evidence(spec, {filled: 5, open: true});
  const r = reconcileIntent(a.state, 'intent-1', e, NOW + 2000);
  assert.equal(r.ok, true, r.code); assert.equal(r.intent.status, 'reconciled'); assert.equal(r.state.owned['700'].quantity, 5);
  const repeat = reconcileIntent(r.state, 'intent-1', e, NOW + 2000); assert.equal(repeat.ok, true); assert.equal(repeat.state.owned['700'].quantity, 5);
  const ended = evidence(spec, {filled: 5, open: false});
  const closed = reconcileIntent(repeat.state, 'intent-1', ended, NOW + 2000);
  assert.equal(closed.intent.outcome, 'partially_filled'); assert.equal(closed.state.owned['700'].cost, '2.500000');
});

test('fill pages, lifecycle totals, changed history, positions and limit violations block reconciliation', () => {
  const spec = terms(), a = acknowledged(spec);
  for (const mutate of [e => {e.fills.complete = false;}, e => {e.fills.totalQuantityFilled = 19;}, e => {e.fills.rows[0].price = '0.505';}, e => {e.snapshot.positions.rows[0].quantity = 21;}, e => {e.order.expirationDate = null;}, e => {e.fills.rows.push({...e.fills.rows[0]});}]) {
    const e = evidence(spec); mutate(e); const r = reconcileIntent(a.state, 'intent-1', e, NOW + 2000); assert.equal(r.ok, false); assert.deepEqual(r.state.owned, {});
  }
  const p = reconcileIntent(a.state, 'intent-1', evidence(spec, {filled: 5, open: true}), NOW + 2000);
  const changed = evidence(spec, {filled: 5, open: true, price: '0.495'});
  assert.equal(reconcileIntent(p.state, 'intent-1', changed, NOW + 2000).code, 'FILL_HISTORY_CHANGED');
});

test('SELL must use owned YES inventory, cannot oversell, and removes FIFO cost', () => {
  const buy = terms(), a = acknowledged(buy), bought = reconcileIntent(a.state, buy.intentId, evidence(buy), NOW + 2000);
  const snap = snapshot(); snap.positions.rows = [{exchangeId: '700', marketId: '377', quantity: 20}];
  const sell = terms({intentId: 'sell-1', idempotencyKey: 'sell-key-1', action: 'sell', price: '0.495'});
  assert.equal(prepareIntent(bought.state, {...sell, quantity: 21}, snap, NOW).code, 'OWNED_INVENTORY_REQUIRED');
  const pending = acknowledged(sell, bought.state, snap);
  const sold = reconcileIntent(pending.state, sell.intentId, evidence(sell, {filled: 20, owned: 0, price: '0.495', orderId: pending.intent.orderId}), NOW + 2000);
  assert.equal(sold.ok, true, sold.code); assert.deepEqual(sold.state.owned, {});
});

test('managed aggregate and market caps prevent further exposure', () => {
  let state = initial();
  function buy(n, exchangeId, marketId) {
    const spec = terms({intentId: 'buy-' + n, idempotencyKey: 'buy-key-' + n, exchangeId, marketId, quantity: 200});
    const snap = snapshot(); snap.positions.rows = Object.entries(state.owned).map(([exchangeId, p]) => ({exchangeId, marketId: p.marketId, quantity: p.quantity})); Object.assign(snap.book, {exchangeId, marketId});
    const a = acknowledged(spec, state, snap), e = evidence(spec, {filled: 200, owned: (state.owned[exchangeId]?.quantity || 0) + 200, orderId: a.intent.orderId});
    state = reconcileIntent(a.state, spec.intentId, e, NOW + 2000).state;
  }
  for (let n = 1; n <= 3; n++) buy(n, String(n), String(n));
  const snap = snapshot(); snap.positions.rows = Object.entries(state.owned).map(([exchangeId, p]) => ({exchangeId, marketId: p.marketId, quantity: p.quantity}));
  assert.equal(prepareIntent(state, terms(), snap, NOW).code, 'MANAGED_MARKET_CAP');
  buy(4, '1', '1'); buy(5, '1', '1'); snap.positions.rows = Object.entries(state.owned).map(([exchangeId, p]) => ({exchangeId, marketId: p.marketId, quantity: p.quantity}));
  assert.equal(prepareIntent(state, terms(), snap, NOW).code, 'AGGREGATE_COST_CAP');
});

test('retry rechecks fresh risk until expiry; post-expiry rejection cannot erase unknown acceptance', () => {
  const d = dispatched(), limited = recordPlacement(d.state, 'intent-1', {status: 429, code: 'RATE_LIMITED'}, NOW);
  const bad = snapshot(NOW + 60000); bad.cash.amount = '0';
  assert.equal(beginDispatch(limited.state, 'intent-1', {snapshot: bad, now: NOW + 60000, retry: true}).code, 'INSUFFICIENT_UNRESERVED_CASH');
  assert.equal(beginDispatch(limited.state, 'intent-1', {now: NOW + 60000, retry: true}).ok, false);
  const unknown = recordPlacement(d.state, 'intent-1', {status: 502, code: 'ORDER_STATUS_UNKNOWN'}, NOW);
  const replay = beginDispatch(unknown.state, 'intent-1', {now: NOW + 120001, retry: true});
  assert.equal(replay.ok, true); assert.equal(replay.request.bodyJson, d.request.bodyJson);
  const rejected = recordPlacement(replay.state, 'intent-1', {status: 400, code: 'VALIDATION_ERROR'}, NOW + 120002);
  assert.equal(rejected.intent.status, 'uncertain'); assert.equal(rejected.intent.retryAllowed, false); assert.equal(rejected.code, 'RETRY_REJECTED_AFTER_AMBIGUITY');
});

test('durable replay validation rejects prototype keys, malformed state and unproven ownership', () => {
  const p = prepareIntent(initial(), terms(), snapshot(), NOW);
  assert.equal(Object.isFrozen(validateExecutionState(JSON.parse(JSON.stringify(p.state)))), true);
  for (const change of [s => {s.intents['intent-1'].status = 'invented';}, s => {s.intents['intent-1'].quantity = 0;}, s => {s.intents['intent-1'].attempts = -1;}, s => {s.intents['intent-1'].ordinal = 2;}, s => {s.owned['700'] = {marketId: '377', quantity: 10, cost: '5.000000', lots: [{quantity: 10, cost: '5.000000'}]};}]) { const s = structuredClone(p.state); change(s); assert.throws(() => validateExecutionState(s)); }
  const poison = JSON.parse('{"__proto__":{"x":1}}'); assert.throws(() => canonicalPayload(poison));
  assert.equal(prepareIntent(initial(), terms({intentId: 'constructor'}), snapshot(), NOW).ok, false);
});

test('only undispatched prepared intents may be abandoned', () => {
  const p = prepareIntent(initial(), terms(), snapshot(), NOW);
  assert.equal(abandonPreparedIntent(p.state, 'intent-1', 'expired_in_queue', NOW).intent.outcome, 'not_submitted');
  assert.equal(abandonPreparedIntent(dispatched().state, 'intent-1', 'ignore', NOW).code, 'CANNOT_ABANDON_DISPATCHED_INTENT');
});

test('future, pre-intent and after-read fills cannot alter attributed holding time', () => {
  const spec = terms(), a = acknowledged(spec);
  for (const filledAt of [at(NOW - 1), at(NOW + 2001)]) {
    const e = evidence(spec); e.fills.rows[0].filledAt = filledAt;
    assert.equal(reconcileIntent(a.state, spec.intentId, e, NOW + 2000).code, 'INVALID_FILL_TIME');
  }
  const e = evidence(spec); e.fillsReceivedAt = at(NOW + 1400);
  assert.equal(reconcileIntent(a.state, spec.intentId, e, NOW + 2000).code, 'INVALID_FILL_TIME');
  e.fillsReceivedAt = at(NOW + 1800); e.order.createdAt = at(NOW + 1600);
  assert.equal(reconcileIntent(a.state, spec.intentId, e, NOW + 2000).code, 'INVALID_FILL_TIME');
});
