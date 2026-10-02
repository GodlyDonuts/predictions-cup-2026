import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {ReadClient} from './runtime.mjs';
import {loadNetworkBudget, networkInterval, validateNetworkBudget} from './network-budget.mjs';

const budget = {schemaVersion: 1, ownerId: 'Codex', enabled: true, intervalMs: 3000, soleAccountClient: true, formerWriterStopCommentId: 5947535773};
const endpoint = '/tournaments/midterm-elections';
const ok = () => ({ok: true, status: 200, json: async () => ({})});

test('faster pacing requires explicit exact coordination; absent file leaves the frozen interval unchanged', t => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sig-network-budget-'));
  t.after(() => fs.rmSync(dir, {recursive: true, force: true}));
  const file = path.join(dir, 'budget.json');
  assert.equal(loadNetworkBudget(file), null);
  assert.equal(networkInterval(), 6000);
  assert.equal(networkInterval(6000, budget), 3000);
  assert.throws(() => networkInterval(3000), /coordinated/);
  assert.throws(() => networkInterval(NaN), /budget/);
  for (const change of [{ownerId: 'Other'}, {enabled: false}, {soleAccountClient: false}, {intervalMs: 2000}, {formerWriterStopCommentId: 1}, {unknown: true}]) assert.throws(() => validateNetworkBudget({...budget, ...change}), /Invalid/);
  fs.writeFileSync(file, JSON.stringify(budget));
  assert.deepEqual(loadNetworkBudget(file), budget);
});

test('mixed reads and writes share one nonbursting budget below both account limits in every rolling minute', async () => {
  let now = 0, inFlight = 0, maxInFlight = 0;
  const calls = [];
  const client = new ReadClient('FAKE', {networkBudget: budget, now: () => now, sleepFn: async ms => { now += ms; }, authorizeDispatch: () => true,
    fetchFn: async (_url, options) => { inFlight++; maxInFlight = Math.max(maxInFlight, inFlight); calls.push({at: now, method: options.method}); now += 359; inFlight--; return ok(); }});
  await Promise.all(Array.from({length: 100}, (_, i) => i % 3 ? client.request(endpoint) : client.dispatch({method: 'POST', endpoint: '/orders', beforeDispatch: async () => {}, beforeNetwork: async () => {}})));
  assert.equal(maxInFlight, 1);
  for (let i = 1; i < calls.length; i++) assert.ok(calls[i].at - calls[i - 1].at >= 3359);
  for (const call of calls) {
    const window = calls.filter(x => x.at >= call.at && x.at <= call.at + 60000);
    assert.ok(window.length <= 21);
    assert.ok(window.filter(x => x.method !== 'GET').length < 30);
    assert.ok(window.filter(x => x.method === 'GET').length < 100);
  }
});

test('slow durable admission cannot create a catch-up pair; completion pacing persists across restart', async () => {
  let now = 0, persisted, first = true;
  const starts = [];
  const options = {networkBudget: budget, now: () => now, sleepFn: async ms => { now += ms; }, onAdmission: next => { persisted = next; if (first) { first = false; now += 5000; } }, fetchFn: async () => { starts.push(now); now += 400; return ok(); }};
  const client = new ReadClient('FAKE', options);
  await client.request(endpoint);
  assert.equal(persisted, 8400);
  const restarted = new ReadClient('FAKE', options); restarted.next = persisted;
  await restarted.request(endpoint); await restarted.request(endpoint);
  assert.deepEqual(starts, [5000, 8400, 11800]);
});

test('429 Retry-After survives a faster-mode restart without shortening the prior reservation', async () => {
  let now = 0, persisted, first = true;
  const starts = [], options = {networkBudget: budget, now: () => now, sleepFn: async ms => { now += ms; }, onAdmission: next => { persisted = next; }, fetchFn: async () => { starts.push(now); if (first) { first = false; return {ok: false, status: 429, headers: new Headers({'Retry-After': '90'}), json: async () => ({})}; } return ok(); }};
  const initial = new ReadClient('FAKE', options);
  await assert.rejects(initial.request(endpoint), /429/);
  assert.equal(persisted, 90000);
  const restarted = new ReadClient('FAKE', options); restarted.next = persisted;
  await restarted.request(endpoint);
  assert.deepEqual(starts, [0, 90000]);
});

test('STOP while waiting and a final network-permit rejection both prevent HTTP writes', async () => {
  let now = 0, writes = 0, fenceCalls = 0;
  const stop = new AbortController();
  const client = new ReadClient('FAKE', {networkBudget: budget, signal: stop.signal, now: () => now, sleepFn: async ms => { now += ms; if (now >= 1000) stop.abort(); }, authorizeDispatch: () => true, fetchFn: async (_url, options) => { if (options.method !== 'GET') writes++; return ok(); }});
  await client.request(endpoint);
  await assert.rejects(client.dispatch({method: 'POST', endpoint: '/orders', beforeDispatch: async () => {}, beforeNetwork: async () => { fenceCalls++; }}), /Stopped/);
  assert.equal(writes, 0); assert.equal(fenceCalls, 0);
  const guarded = new ReadClient('FAKE', {networkBudget: budget, now: () => now, authorizeDispatch: () => true, onAdmission: () => { now += 25000; }, fetchFn: async () => { writes++; return ok(); }});
  const deadline = now + 20000;
  await assert.rejects(guarded.dispatch({method: 'POST', endpoint: '/orders', beforeDispatch: async () => {}, beforeNetwork: async () => { fenceCalls++; if (now > deadline) throw Error('STALE_NETWORK_PERMIT'); }}), /STALE_NETWORK_PERMIT/);
  assert.equal(writes, 0); assert.equal(fenceCalls, 1);
});

test('manual-review Retry-After overflow cannot be overwritten by completion pacing', async () => {
  let persisted;
  const client = new ReadClient('FAKE', {networkBudget: budget, now: () => 0, onAdmission: next => { persisted = next; }, fetchFn: async () => ({ok: false, status: 429, headers: new Headers({'Retry-After': '99999999999999999999999999999999999'})})});
  await assert.rejects(client.request(endpoint), /manual review/);
  assert.equal(persisted, null); assert.equal(client.manualReview, true);
});
