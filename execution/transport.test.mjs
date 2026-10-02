import test from 'node:test';
import assert from 'node:assert/strict';
import {OrderTransport, OrderTransportError} from './transport.mjs';

const tid = 'bda92870-621e-47b0-bc3c-3602c5c26f55', at = '2026-10-02T10:00:00.000Z';
const scope = {tournamentId: tid, tournamentSlug: 'midterm-elections', exchanges: [{marketId: '377', exchangeId: '1066'}]};
const body = () => ({idempotencyKey: 'logical-request-1', exchangeId: '1066', side: 'yes', action: 'buy', quantity: 2, price: 0.5, expirationDate: '2026-10-02T10:01:00.000Z', tournamentId: tid});
const authority = () => ({permit: {intentId: 'intent-1', runId: 'run-1'}, lease: {id: 'lease-1'}});
const placed = {orderId: 1001, exchangeId: '1066', open: true, quantityTraded: 0, totalCost: 0, fillPrice: null, all: null};
const order = id => ({id, exchangeId: '1066', tournamentId: tid, side: 'yes', action: 'buy', quantity: 2, priceLimit: 0.5, open: true, createdAt: at, expirationDate: '2026-10-02T10:01:00.000Z'});
const page = (data, cursor = null) => ({data, pagination: {limit: 200, hasMore: cursor !== null, nextCursor: cursor}, coverage: {complete: true, projectedThroughSequence: 4}});
function fixture(overrides = {}) {
  const calls = [], options = {scope, now: () => Date.parse(at), canWrite: () => true, ownsOrder: id => id === '1001', validatePermit: () => true, validateLease: () => true};
  const dispatch = async spec => {
    if (spec.beforeDispatch) await spec.beforeDispatch();
    if (spec.beforeNetwork) await spec.beforeNetwork();
    calls.push({method: spec.method, endpoint: spec.endpoint, ...(spec.body ? {body: structuredClone(spec.body), bodyJson: spec.bodyJson} : {})});
    return {status: 200, data: structuredClone(placed), requestStartedAt: at, receivedAt: at, retryAfterMs: 0};
  };
  return {transport: new OrderTransport({...options, dispatch, ...overrides}), calls, options};
}
const response = data => ({status: 200, data, requestStartedAt: at, receivedAt: at, retryAfterMs: 0});

test('single bounded expiring limit uses injected dispatch and preserves exact retry body/key', async () => {
  const {transport, calls} = fixture();
  await transport.place(body(), authority());
  await transport.place(body(), authority());
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[0], calls[1]);
  assert.deepEqual(calls[0], {method: 'POST', endpoint: '/orders', body: body(), bodyJson: JSON.stringify(Object.fromEntries(Object.entries(body()).sort(([a], [b]) => a.localeCompare(b))))});
  assert.deepEqual(Object.keys(transport), []);
});

test('frozen canonical body bytes agree with ledger/core and cannot drift while queued', async () => {
  const {canonicalPayload} = await import('./core.mjs');
  const {canonicalJson} = await import('./ledger.mjs');
  let captured;
  const {transport} = fixture({dispatch: async spec => {
    captured = spec;
    assert.throws(() => { spec.body.quantity = 100; }, TypeError);
    assert.throws(() => { spec.bodyJson = '{}'; }, TypeError);
    await spec.beforeDispatch();
    return response(placed);
  }});
  await transport.place(body(), authority());
  assert.equal(captured.bodyJson, canonicalPayload(body()));
  assert.equal(captured.bodyJson, canonicalJson(body()));
  assert.throws(() => transport.authorizeRequest({method: 'POST', endpoint: '/orders', body: body(), bodyJson: '{}', beforeDispatch() {}}), /FROZEN_ORDER_BODY_MISMATCH/);
});

test('market routes, omitted limits/expiry, extras, off ticks and foreign scope never dispatch', async () => {
  const {transport, calls} = fixture();
  for (const change of [value => value.price = 1, value => value.price = 0, value => value.price = 0.501, value => delete value.price, value => delete value.expirationDate, value => value.tournamentId = 'foreign', value => value.exchangeId = '999', value => value.quantity = 0, value => value.quantity = 2147483648, value => value.headers = {Authorization: 'secret'}, value => value.idempotencyKey = '']) {
    const value = body(); change(value);
    await assert.rejects(transport.place(value, authority()), OrderTransportError);
  }
  assert.equal(calls.length, 0);
});

test('static allowlist rejects default context, URL/scope escapes and broad cancel routes', () => {
  const {transport} = fixture();
  for (const endpoint of ['/portfolio/positions', '/account', 'https://evil.test/orders', '/orders?status=all&limit=200', '/orders?status=all&limit=200&tournamentId=' + tid + '&tournamentId=foreign', '/markets/999?tournamentId=' + tid, '/markets/377?tournamentId=' + tid + '&extra=1', '/orders/1001/fills?limit=200&cursor=x&limit=1', '/tournaments/../account']) assert.throws(() => transport.authorizeRequest({method: 'GET', endpoint}), OrderTransportError);
  for (const endpoint of ['/orders/cancel-all', '/orders/batch', '/orders/multi-leg']) assert.throws(() => transport.authorizeRequest({method: 'POST', endpoint, body: body(), beforeDispatch() {}}), OrderTransportError);
});

test('every write needs a permit and lease; unknown own order cancellations are blocked', async () => {
  const {transport, calls} = fixture();
  await assert.rejects(transport.place(body()), /PERMIT_AND_LEASE_REQUIRED/);
  await assert.rejects(transport.place(body(), {permit: {intentId: 'intent-1'}}), /PERMIT_AND_LEASE_REQUIRED/);
  await assert.rejects(transport.cancel('1002', authority()), /ORDER_NOT_OWNED/);
  assert.throws(() => transport.fills('1002'), /ORDER_NOT_OWNED/);
  assert.equal(calls.length, 0);
});

test('STOP or owner loss while queued blocks network after shared admission wait', async () => {
  let enabled = true, networkCalls = 0;
  const {transport} = fixture({canWrite: () => enabled, dispatch: async spec => { enabled = false; await spec.beforeDispatch(); networkCalls++; return response(placed); }});
  await assert.rejects(transport.place(body(), authority()), /WRITE_STOPPED_OR_OWNER_LOST/);
  assert.equal(networkCalls, 0);
});

test('repeatable final fence blocks STOP during admission persistence without preparing twice', async () => {
  let stopped = false, permits = 0, networkCalls = 0;
  const {transport} = fixture({canWrite: () => !stopped, validatePermit: () => { permits++; return true; }, dispatch: async spec => {
    await spec.beforeDispatch();
    await spec.beforeNetwork();
    stopped = true;
    await spec.beforeNetwork();
    networkCalls++;
    return response(placed);
  }});
  await assert.rejects(transport.place(body(), authority()), /WRITE_STOPPED_OR_OWNER_LOST/);
  assert.equal(permits, 1);
  assert.equal(networkCalls, 0);
});

test('ownership is checked again for DELETE after queue and fresh lease/permit are required', async () => {
  let owned = true, networkCalls = 0;
  const {transport} = fixture({ownsOrder: () => owned, dispatch: async spec => { owned = false; await spec.beforeDispatch(); networkCalls++; return response({orderId: 1001, tournamentId: tid}); }});
  await assert.rejects(transport.cancel('1001', authority()), /ORDER_NOT_OWNED/);
  assert.equal(networkCalls, 0);
  for (const overrides of [{validateLease: () => false}, {validatePermit: () => false}]) {
    const f = fixture(overrides);
    await assert.rejects(f.transport.place(body(), authority()), OrderTransportError);
    assert.equal(f.calls.length, 0);
  }
});

test('blind new key or changed payload for one logical intent is rejected after ambiguous dispatch', async () => {
  let count = 0;
  const {transport} = fixture({dispatch: async spec => { await spec.beforeDispatch(); count++; throw Error('secret upstream payload'); }});
  await assert.rejects(transport.place(body(), authority()), error => error.code === 'REQUEST_UNCONFIRMED' && error.mayHaveExecuted && error.retrySameRequestOnly && !JSON.stringify(error).includes('secret'));
  await assert.rejects(transport.place({...body(), idempotencyKey: 'replacement-key'}, authority()), /LOGICAL_REQUEST_CHANGED/);
  await assert.rejects(transport.place({...body(), quantity: 3}, authority()), /LOGICAL_REQUEST_CHANGED/);
  await assert.rejects(transport.place(body(), authority()), /REQUEST_UNCONFIRMED/);
  assert.equal(count, 2);
});

test('persisted permit blocks changed body on restart; identical reconciliation retry can outlive expiry', async () => {
  const original = body();
  const options = {now: () => Date.parse('2026-10-02T10:02:00.000Z'), validatePermit: (_permit, spec) => JSON.stringify(spec.body) === JSON.stringify(original)};
  const {transport, calls} = fixture(options);
  await assert.rejects(transport.place({...original, idempotencyKey: 'new-key'}, {...authority(), permit: {...authority().permit, retry: true}}), /INVALID_EXECUTION_PERMIT/);
  await transport.place(original, {...authority(), permit: {...authority().permit, retry: true}});
  assert.equal(calls.length, 1);
  await assert.rejects(fixture({now: options.now}).transport.place(original, authority()), /LIMIT_ORDER_EXPIRED/);
});

test('HTTP 429 honors 90 second shared cooldown and 409 REQUEST_IN_FLIGHT requires identical retry', async () => {
  for (const [status, apiCode, expected, mayHaveExecuted] of [[429, 'RATE_LIMITED', 'rate_limited', false], [409, 'REQUEST_IN_FLIGHT', 'in_flight', true], [502, 'ORDER_STATUS_UNKNOWN', 'unknown', true]]) {
    const {transport} = fixture({dispatch: async spec => { await spec.beforeDispatch(); throw Object.assign(Error('sensitive upstream text'), {status, apiCode, retryAfterMs: 90000}); }});
    await assert.rejects(transport.place(body(), authority()), error => error.status === status && error.apiCode === apiCode && error.outcome === expected && error.mayHaveExecuted === mayHaveExecuted && error.retrySameRequestOnly && error.retryAfterMs === 90000 && !String(error).includes('sensitive'));
  }
  const {transport} = fixture({dispatch: async spec => { await spec.beforeDispatch(); return {status: 400, data: {error: {code: 'secret arbitrary code', message: 'secret'}}, retryAfterMs: 0}; }});
  await assert.rejects(transport.place(body(), authority()), error => error.status === 400 && error.apiCode === null && !error.retrySameRequestOnly && !JSON.stringify(error).includes('secret'));
});

test('parsed HTTP200 semantic mismatches retain status/order ID for core reconciliation', async () => {
  const unexpected = {...placed, exchangeId: '999', side: 'no', action: 'buy'};
  const {transport} = fixture({dispatch: async spec => { await spec.beforeDispatch(); await spec.beforeNetwork(); return response(unexpected); }});
  const result = await transport.place(body(), authority());
  assert.equal(result.status, 200);
  assert.equal(result.data.orderId, 1001);
  assert.equal(result.data.exchangeId, '999');
  const malformed = fixture({dispatch: async spec => { await spec.beforeDispatch(); await spec.beforeNetwork(); return response('private nonobject body'); }});
  await assert.rejects(malformed.transport.place(body(), authority()), error => error.status === 200 && error.semanticMismatch && error.mayHaveExecuted && !error.retrySameRequestOnly && !JSON.stringify(error).includes('private'));
});

test('scoped cash, market detail and book reads use only explicit tournament routes', async () => {
  const called = [], {transport} = fixture({dispatch: async spec => {
    called.push(spec.endpoint);
    if (spec.endpoint.startsWith('/tournaments/')) return response({id: tid, slug: 'midterm-elections', myBalance: 100, status: 'active', isPendingEnrolment: false});
    return response({id: '377', exchanges: [{id: '1066'}], contexts: [{type: 'tournament', tournament: {id: tid}, orderbook: {exchanges: [{exchangeId: '1066'}]}}]});
  }});
  await transport.tournament(); await transport.market('377'); await transport.orderbook('377');
  assert.deepEqual(called, ['/tournaments/midterm-elections', '/markets/377?tournamentId=' + tid, '/markets/377/orderbook?tournamentId=' + tid + '&depth=20']);
  const bad = fixture({dispatch: async () => response({id: '377', contexts: [{type: 'tournament', tournament: {id: 'foreign'}}]})});
  await assert.rejects(bad.transport.market('377'), /RESPONSE_SCOPE_MISMATCH/);
});

test('positions accepts the documented complete array and rejects invented pagination/truncation', async () => {
  const data = {positions: [{exchangeId: '1066', marketId: '377', quantity: 2, lots: []}], summary: {totalMarketValue: 1, totalCostBasis: 1, totalUnrealizedPnl: 0}};
  const f = fixture({dispatch: async () => response(structuredClone(data))});
  assert.equal((await f.transport.positions()).data.positions.length, 1);
  data.pagination = {hasMore: true, nextCursor: 'next'};
  await assert.rejects(f.transport.positions(), /INCOMPLETE_OFFICIAL_POSITIONS/);
});

test('orders follow every opaque cursor and retain complete rows with scope validation', async () => {
  const calls = [], {transport} = fixture({dispatch: async spec => {
    calls.push(spec.endpoint);
    return response(calls.length === 1 ? page([order(1001)], 'opaque/+==') : page([order(1002)]));
  }});
  const result = await transport.orders();
  assert.equal(result.data.data.length, 2);
  assert.equal(result.data.pages.length, 2);
  assert.equal(result.data.pagination.hasMore, false);
  assert.ok(calls[1].endsWith('&cursor=opaque%2F%2B%3D%3D'));
  const foreign = fixture({dispatch: async () => response(page([{...order(1001), tournamentId: 'foreign'}]))});
  await assert.rejects(foreign.transport.orders(), /RESPONSE_SCOPE_MISMATCH/);
});

test('incomplete, repeated and exhausted pagination never presents partial reconciliation as complete', async () => {
  const malformed = fixture({dispatch: async () => response({data: [order(1001)]})});
  await assert.rejects(malformed.transport.orders(), /INCOMPLETE_OFFICIAL_PAGINATION/);
  const repeated = fixture({dispatch: async () => response(page([order(1001)], 'same'))});
  await assert.rejects(repeated.transport.orders(), /INCONSISTENT_OFFICIAL_PAGINATION/);
  const bounded = fixture({maxPages: 1, dispatch: async () => response(page([order(1001)], 'next'))});
  await assert.rejects(bounded.transport.orders(), /PAGINATION_LIMIT_EXCEEDED/);
});

test('known own order fills paginate with stable lifecycle totals; totals are not summed per page', async () => {
  let reads = 0;
  const {transport} = fixture({dispatch: async () => {
    reads++;
    return response({...page([{id: reads, side: 'yes', quantity: 1, price: 0.5, filledAt: at}], reads === 1 ? 'next' : null), orderId: 1001, exchangeId: '1066', tournamentId: tid, totalQuantityFilled: 2, avgFillPrice: 0.5});
  }});
  const result = await transport.fills('1001');
  assert.equal(result.data.data.length, 2);
  assert.equal(result.data.totalQuantityFilled, 2);
});

test('successful DELETE remains owned, guarded, and explicitly verified against tournament', async () => {
  let calls = 0;
  const {transport} = fixture({dispatch: async spec => { await spec.beforeDispatch(); calls++; return response({orderId: 1001, tournamentId: tid, message: 'cancelled'}); }});
  await transport.cancel('1001', authority());
  assert.equal(calls, 1);
  const foreign = fixture({dispatch: async spec => { await spec.beforeDispatch(); return response({orderId: 1001, tournamentId: 'foreign'}); }});
  await assert.rejects(foreign.transport.cancel('1001', authority()), error => error.code === 'INVALID_CANCEL_RESPONSE' && error.mayHaveExecuted);
});
