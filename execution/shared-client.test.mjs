import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {ReadClient} from '../live-paper/runtime.mjs';
import {validateTiming} from '../live-paper/read-timing.mjs';
import {OrderTransport} from './transport.mjs';
import {canonicalPayload, createExecutionState, prepareIntent, beginDispatch, recordPlacement} from './core.mjs';

const TID = 'bda92870-621e-47b0-bc3c-3602c5c26f55', START = Date.parse('2026-10-02T12:00:00.000Z');
const scope = {tournamentId: TID, tournamentSlug: 'midterm-elections', exchanges: [{marketId: '377', exchangeId: '1066'}]};
const payload = () => ({quantity: 2, side: 'yes', price: 0.5, idempotencyKey: 'shared-pipeline-fixture', tournamentId: TID, action: 'buy', expirationDate: new Date(START + 120000).toISOString(), exchangeId: '1066'});
const authority = retry => ({permit: {runId: 'fixture', intentId: 'fixture-order', retry: retry === true}, lease: {runId: 'fixture'}});
const minimumPlacement = {orderId: 1001, exchangeId: '1066', open: true, quantityTraded: 0, totalCost: 0, fillPrice: null, all: null};

function pipeline(options = {}) {
  let now = START, stopped = false, owner = true, inFlight = 0, maxInFlight = 0, transport;
  const admissions = [], calls = [], timings = [], permits = [];
  const client = new ReadClient('synthetic-shared-client-test-key', {
    interval: 6000,
    now: () => now,
    sleepFn: async milliseconds => { now += milliseconds; options.onSleep?.({setOwner: value => { owner = value; }}); },
    authorizeDispatch: spec => transport.authorizeRequest(spec),
    onAdmission: next => { admissions.push(next); options.onAdmission?.({next, stop: () => { stopped = true; }, advance: milliseconds => { now += milliseconds; }}); },
    onTiming: timing => { validateTiming(timing); timings.push(timing); },
    fetchFn: async (url, init) => {
      inFlight++; maxInFlight = Math.max(maxInFlight, inFlight);
      const call = {at: now, method: init.method, endpoint: url.slice('https://sig.thesuper.market/api/v1'.length), body: init.body};
      calls.push(call);
      // This function is the only injected HTTP boundary. Nothing reaches fetch.
      const answer = options.answer?.(call, calls) || {status: 200, data: init.method === 'POST' ? minimumPlacement : {id: TID}};
      inFlight--;
      return {ok: answer.status >= 200 && answer.status < 300, status: answer.status, headers: new Headers(answer.headers), json: async () => { now += 7; return structuredClone(answer.data); }};
    },
  });
  transport = new OrderTransport({
    scope, dispatch: spec => client.dispatch(spec), now: () => now,
    canWrite: () => !stopped && owner, ownsOrder: value => value === '1001',
    validateLease: () => owner,
    validatePermit: (permit, spec) => { permits.push({at: now, retry: permit.retry, bodyJson: spec.bodyJson}); return spec.bodyJson === canonicalPayload(payload()); },
    validateNetworkPermit: (permit, spec) => options.validateNetworkPermit ? options.validateNetworkPermit({permit, spec, now}) : true,
  });
  return {client, transport, admissions, calls, timings, permits, get now() { return now; }, get maxInFlight() { return maxInFlight; }};
}

test('actual shared client serializes 100 queued collector reads and exact same-key recovery bytes', async () => {
  const p = pipeline();
  await p.transport.place(payload(), authority(false));
  const queued = Array.from({length: 100}, () => p.client.request('/tournaments/midterm-elections'));
  // Recovery is explicitly the same already-admitted logical operation. Its
  // original expiry passes while reads wait; no replacement order/key is made.
  queued.push(p.transport.place(payload(), authority(true)));
  await Promise.all(queued);
  assert.equal(p.calls.length, 102);
  assert.equal(p.maxInFlight, 1);
  assert.equal(p.client.requests, 102);
  assert.equal(p.admissions.length, 102);
  assert.equal(p.timings.length, 102);
  assert.ok(p.calls.slice(1).every((call, index) => call.at - p.calls[index].at >= 6000));
  assert.ok(p.admissions.every((next, index) => next >= p.calls[index].at + 6000));
  const posts = p.calls.filter(call => call.method === 'POST');
  assert.equal(posts.length, 2);
  assert.equal(posts[0].body, canonicalPayload(payload()));
  assert.equal(posts[1].body, posts[0].body);
  assert.equal(JSON.parse(posts[1].body).idempotencyKey, payload().idempotencyKey);
  assert.equal(p.permits.length, 2);
  assert.equal(p.permits[1].retry, true);
  assert.ok(p.now > Date.parse(payload().expirationDate));
});

test('actual beforeNetwork fence observes STOP during admission persistence before HTTP', async () => {
  const p = pipeline({onAdmission: ({stop}) => stop()});
  await assert.rejects(p.transport.place(payload(), authority(false)), error => error.code === 'WRITE_STOPPED_OR_OWNER_LOST');
  assert.equal(p.calls.length, 0);
  assert.equal(p.permits.length, 1, 'durable permit admission is not repeated by the final fence');
  assert.equal(p.admissions.length, 1);
  assert.equal(p.timings[0].outcome, 'blocked');
});

test('readonly final permit blocks a book that ages past20seconds during admission fsync', async () => {
  let readonlyChecks = 0;
  const p = pipeline({
    onAdmission: ({advance}) => advance(25000),
    validateNetworkPermit: ({permit, spec, now}) => {
      readonlyChecks++;
      return permit.intentId === 'fixture-order' && spec.bodyJson === canonicalPayload(payload()) && now - START <= 20000 && now < Date.parse(spec.body.expirationDate);
    },
  });
  await assert.rejects(p.transport.place(payload(), authority(false)), error => error.code === 'INVALID_NETWORK_EXECUTION_PERMIT');
  assert.equal(p.calls.length, 0);
  assert.equal(p.permits.length, 1, 'the durable transition occurs only once');
  assert.equal(readonlyChecks, 1);
  assert.equal(p.timings[0].outcome, 'blocked');
});

test('actual shared admission wait rechecks owner after queued read pacing', async () => {
  const p = pipeline({onSleep: ({setOwner}) => setOwner(false)});
  await p.client.request('/tournaments/midterm-elections');
  await assert.rejects(p.transport.place(payload(), authority(false)), error => error.code === 'WRITE_STOPPED_OR_OWNER_LOST');
  assert.equal(p.calls.length, 1);
  assert.equal(p.calls[0].method, 'GET');
  assert.equal(p.permits.length, 0);
});

test('HTTP429 Retry-After90 persists one cooldown shared by the next collector read', async () => {
  const p = pipeline({answer: call => call.method === 'POST' ? {status: 429, headers: {'Retry-After': '90'}, data: {error: {code: 'RATE_LIMITED', message: 'private server diagnostic'}}} : {status: 200, data: {id: TID}}});
  await assert.rejects(p.transport.place(payload(), authority(false)), error => error.status === 429 && error.outcome === 'rate_limited' && error.retryAfterMs >= 89990 && error.retrySameRequestOnly && !JSON.stringify(error).includes('private'));
  const cooldown = p.client.cooldown;
  assert.ok(p.admissions.includes(cooldown));
  await p.client.request('/tournaments/midterm-elections');
  assert.equal(p.calls.length, 2);
  assert.ok(p.calls[1].at >= cooldown);
  assert.ok(p.calls[1].at - p.calls[0].at >= 90000);
  assert.equal(p.calls.filter(call => call.method === 'POST').length, 1, 'transport never runs an automatic retry loop');
});

test('official REQUEST_IN_FLIGHT code survives shared-client sanitization with its90second retry floor', async () => {
  const p = pipeline({answer: () => ({status: 409, data: {error: {code: 'REQUEST_IN_FLIGHT', message: 'private account text', details: {secret: 'never surface'}}}})});
  await assert.rejects(p.transport.place(payload(), authority(false)), error => error.status === 409 && error.apiCode === 'REQUEST_IN_FLIGHT' && error.retryAfterMs === 90000 && error.mayHaveExecuted && error.retrySameRequestOnly && !JSON.stringify(error).includes('private') && !JSON.stringify(error).includes('secret'));
  assert.equal(p.calls.length, 1);
});

test('wiring execution transport does not widen ordinary collector request allowlist', async () => {
  const p = pipeline();
  for (const endpoint of ['/orders', '/orders/1001', '/account', '/markets/377?tournamentId=' + TID, 'https://example.invalid/orders']) await assert.rejects(p.client.request(endpoint), /allowlist/);
  assert.equal(p.calls.length, 0);
});

test('documented minimal placement schema reaches canonical reconciliation without inventing engine fields', async () => {
  const schema = JSON.parse(fs.readFileSync(new URL('../openapi.json', import.meta.url))).paths['/orders'].post.responses['200'].content['application/json'].schema;
  assert.deepEqual(Object.keys(minimumPlacement).sort(), schema.required.slice().sort());
  for (const key of ['side', 'action', 'price', 'quantity', 'remainingQuantity']) assert.equal(schema.required.includes(key), false);
  const p = pipeline(), body = payload(), at = new Date(START).toISOString();
  const snapshot = {tournamentId: TID, cash: {tournamentId: TID, at, amount: '100000'}, positions: {tournamentId: TID, at, complete: true, rows: []}, orders: {tournamentId: TID, at, complete: true, rows: []}, book: {tournamentId: TID, marketId: '377', exchangeId: '1066', at, requestStartedAt: at, bids: [{price: '0.495', quantity: 200}], asks: [{price: '0.500', quantity: 200}]}};
  const prepared = prepareIntent(createExecutionState({tournamentId: TID}), {...body, intentId: 'fixture-order', marketId: '377', attribution: {runId: 'fixture', strategyId: 'fixture'}}, snapshot, START);
  assert.equal(prepared.ok, true, prepared.code);
  const sent = beginDispatch(prepared.state, 'fixture-order', {snapshot, now: START});
  assert.equal(sent.ok, true, sent.code);
  const response = await p.transport.place(body, authority(false));
  const recorded = recordPlacement(sent.state, 'fixture-order', {status: response.status, response: response.data}, p.now);
  assert.equal(recorded.intent.status, 'uncertain');
  assert.equal(recorded.intent.orderId, 1001);
  assert.equal(recorded.intent.retryAllowed, false);
  assert.equal(recorded.code, 'CANONICAL_ORDER_MISMATCH');
});
