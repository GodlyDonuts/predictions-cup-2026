import test from 'node:test';
import assert from 'node:assert/strict';
import {setTimeout as wait} from 'node:timers/promises';
import {createLatencyClock} from './latency-clock.mjs';
import {ReadClient} from './transport-runtime.mjs';
import {validateTiming, timingSummary, timingDistribution, analyzeReadTimings} from './read-timing.mjs';
const route = '/tournaments/midterm-elections';

test('transport durations ignore wall-clock corrections without changing persisted wall dates', async () => {
  let wall = Date.parse('2026-10-04T01:00:00Z'), mono = 0;
  const events = [];
  const client = new ReadClient('SENSITIVE_KEY', {now: () => wall, monotonicNow: () => mono,
    onTiming: e => events.push(e), fetchFn: async () => {
      wall -= 10000; mono += 25;
      return {ok: true, status: 200, json: async () => {wall += 20000; mono += 5; return {private: 'SENSITIVE_BODY'};}};
    }});
  try { await client.request(route); } finally { client.close(); }
  const e = events[0]; validateTiming(e);
  assert.equal(e.schemaVersion, 2); assert.equal(e.durationClock, 'injected');
  assert.equal(e.headersMs, 25); assert.equal(e.bodyMs, 5); assert.equal(e.serviceMs, 30); assert.equal(e.totalMs, 30);
  assert.equal(Date.parse(e.finishedAt) - Date.parse(e.queuedAt), 10000);
  assert.equal(e.eventLoopSamples, 0); assert.equal(e.eventLoopP99Ms, null);
  assert.equal(JSON.stringify(e).includes('SENSITIVE'), false);
});

test('HTTP error body work is measured without leaking a response or losing Retry-After', async () => {
  let now = 0; const events = [];
  const client = new ReadClient('FAKE', {now: () => now, onTiming: e => events.push(e), fetchFn: async () => {
    now += 7;
    return {ok: false, status: 503, headers: new Headers({'Retry-After': '60'}), json: async () => {now += 9; return {private: 'SENSITIVE_ERROR', code: 'INVALID_ORDER'};}};
  }});
  try {
    await assert.rejects(client.request(route), e => e.status === 503 && e.apiCode === 'INVALID_ORDER' && e.retryAfterMs === 59991);
  } finally { client.close(); }
  assert.equal(events[0].outcome, 'http_error'); assert.equal(events[0].headersMs, 7);
  assert.equal(events[0].bodyMs, 9); assert.equal(events[0].serviceMs, 16);
  validateTiming(events[0]); assert.equal(JSON.stringify(events).includes('SENSITIVE'), false);
});

test('failed error-body decoding is timed and does not replace the HTTP failure', async () => {
  let now = 0; const events = [];
  const client = new ReadClient('FAKE', {now: () => now, onTiming: e => events.push(e), fetchFn: async () => ({ok: false, status: 500, json: async () => {now += 19; throw Error('SENSITIVE_JSON');}})});
  try { await assert.rejects(client.request(route), /Market API HTTP 500/); } finally { client.close(); }
  assert.equal(events[0].bodyMs, 19); assert.equal(events[0].outcome, 'http_error');
  assert.equal(JSON.stringify(events).includes('SENSITIVE'), false);
});

test('clock probe resets bounded event-loop histograms, reports missing samples honestly and closes idempotently', async () => {
  const clock = createLatencyClock();
  try {
    assert.equal(clock.source, 'monotonic');
    const first = clock.now(); await wait(85); assert.ok(clock.now() > first);
    const sample = clock.snapshot();
    assert.ok(sample.eventLoopSamples > 0);
    assert.ok(sample.eventLoopMaxMs >= sample.eventLoopP99Ms && sample.eventLoopP99Ms >= 0);
    assert.deepEqual(clock.snapshot(), {eventLoopSamples: 0, eventLoopP99Ms: null, eventLoopMaxMs: null});
  } finally { clock.close(); clock.close(); }
  assert.equal(clock.snapshot().eventLoopSamples, 0);
});

test('mixed legacy/v2 journals keep their clock provenance and expose p99 without rewriting history', async () => {
  let now = 0; const events = [];
  const client = new ReadClient('FAKE', {now: () => now, onTiming: e => events.push(e), fetchFn: async () => {now += 10; return {ok: true, json: async () => ({})};}});
  try { await client.request(route); } finally { client.close(); }
  const {durationClock, eventLoopSamples, eventLoopP99Ms, eventLoopMaxMs, ...old} = events[0]; old.schemaVersion = 1;
  const summary = timingSummary([old, events[0]]);
  assert.deepEqual(summary.clocks, {'legacy-wall-clock': 1, injected: 1});
  assert.equal(summary.groups.tournament.totalMs.p99, 10);
  assert.equal(timingDistribution([1, 100]).p99, 100);
  assert.equal(timingDistribution([]).p99, null);
  const raw = [old, events[0]].map(e => JSON.stringify(e)).join('\n') + '\n';
  assert.equal(analyzeReadTimings(raw + '{').partialTailExcluded, true);
  assert.equal(analyzeReadTimings(raw).records, 2);
  assert.throws(() => validateTiming({...events[0], Authorization: 'SENSITIVE'}), /schema/);
  assert.throws(() => validateTiming({...events[0], durationClock: 'guess'}));
  assert.throws(() => validateTiming({...events[0], eventLoopSamples: 0, eventLoopP99Ms: 0}));
});
