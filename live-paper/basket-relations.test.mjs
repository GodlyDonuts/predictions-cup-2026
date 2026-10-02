import test from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {createBasketRelationshipReader} from './basket-relations.mjs';
import {ReadClient} from './runtime.mjs';

const tournamentId = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const base = '/relationships?tournamentId=' + tournamentId + '&limit=200';
const time = Date.parse('2026-10-02T11:00:00.000Z');
const stamp = n => new Date(time + n).toISOString();
const relation = (n = 1) => ({id: '11111111-1111-4111-8111-' + String(n).padStart(12, '0'), type: 'mutually_exclusive',
  source: 'auto_derived', status: 'active', label: null, reason: null, isExhaustive: false, expression: null,
  nodes: ['1077', '1076'].map((exchangeId, index) => ({exchangeId, marketId: String(388 - index), marketTitle: 'Public market',
    outcome: 'YES', currentPrice: index ? 0.8 : 0.2, role: 'member', contractId: 'contract:' + exchangeId})),
  direction: {kind: 'symmetric', fromExchangeIds: ['1077', '1076'], toExchangeIds: ['1077', '1076'], description: 'At most one YES'},
  constraint: {description: 'Normal mutex', priceRule: 'sum(P)<=1', boundSemantics: 'maximum sum', evaluationEndpoint: '/api/v1/relationships/constraints'},
  createdAt: null, updatedAt: stamp(-1000), version: 1});
const body = (rows = [], next = null) => ({data: rows, pagination: {limit: 200, hasMore: next !== null, nextCursor: next}});
const canonical = value => JSON.stringify(value && typeof value === 'object' ? Array.isArray(value)
  ? value.map(x => JSON.parse(canonical(x))) : Object.fromEntries(Object.keys(value).sort().map(k => [k, JSON.parse(canonical(value[k]))])) : value);
function fixture(pages, options = {}) {
  let tick = time, allowed = true, reader;
  const calls = [];
  reader = createBasketRelationshipReader({tournamentId, now: () => tick, canOperate: () => allowed,
    dispatch: async spec => {
      calls.push(spec); assert.equal(reader.authorizes(spec), true);
      await spec.beforeDispatch(); tick += 1;
      await options.beforeNetwork?.({spec, stop: () => { allowed = false; }});
      await spec.beforeNetwork();
      const data = pages[calls.length - 1]; if (data instanceof Error) throw data;
      const requestStartedAt = new Date(tick).toISOString(); tick += 1;
      return {endpoint: spec.endpoint, status: 200, requestStartedAt, receivedAt: new Date(tick).toISOString(), data: structuredClone(data), ...options.response};
    }, maxPages: options.maxPages ?? 10});
  return {reader, calls, stop: () => { allowed = false; }};
}

test('empty scoped graph is complete hashed evidence, never a relationship proof or order permission', async () => {
  const {reader, calls} = fixture([body()]), result = await reader.capture();
  assert.equal(calls.length, 1); assert.equal(calls[0].endpoint, base);
  assert.equal(result.complete, true); assert.equal(result.reasonCode, 'COMPLETE'); assert.deepEqual(result.relationships, []);
  assert.equal(result.orderEligible, false); assert.equal(result.proofConstructed, false);
  assert.equal(result.snapshots[0].requestStartedAt, stamp(1)); assert.equal(result.snapshots[0].receivedAt, stamp(2));
  const {evidenceHash, ...evidence} = result;
  assert.equal(evidenceHash, createHash('sha256').update(canonical(evidence)).digest('hex'));
  assert.ok(Object.isFrozen(result.snapshots[0].data.pagination));
  assert.equal(reader.authorizes(calls[0]), false); assert.throws(() => calls[0].beforeNetwork(), /STOPPED/);
});

test('pagination preserves page provenance and exact encoded cursor, schema fields and version', async () => {
  const cursor = 'opaque /?&=+#%';
  const {reader, calls} = fixture([body([relation(1)], cursor), body([relation(2)])]);
  const result = await reader.capture();
  assert.equal(result.complete, true); assert.equal(result.snapshots.length, 2); assert.equal(result.relationships.length, 2);
  assert.equal(calls[1].endpoint, base + '&cursor=' + encodeURIComponent(cursor));
  assert.deepEqual(result.relationships[0], relation(1));
  assert.equal(result.snapshots[1].requestStartedAt, stamp(3));
});

test('authorization requires the live identity-bound fence and exact scoped GET without bodies or headers', async () => {
  let reader, captured;
  reader = createBasketRelationshipReader({tournamentId, now: () => time, dispatch: async spec => {
    captured = spec;
    assert.equal(reader.authorizes({...spec}), true);
    for (const patch of [{method: 'POST'}, {method: 'DELETE'}, {body: {}}, {bodyJson: '{}'}, {headers: {}},
      {endpoint: '/relationships'}, {endpoint: base + '&marketId=388'}, {endpoint: base.replace(tournamentId, 'foreign')},
      {beforeNetwork: () => true}, {beforeDispatch: () => true}]) assert.equal(reader.authorizes({...spec, ...patch}), false);
    assert.equal(reader.authorizes({method: 'GET', endpoint: base, beforeDispatch: () => true, beforeNetwork: () => true}), false);
    return {endpoint: spec.endpoint, status: 200, requestStartedAt: stamp(0), receivedAt: stamp(0), data: body()};
  }});
  const unknownFence = () => true;
  assert.equal(reader.authorizes({method: 'GET', beforeDispatch: unknownFence, beforeNetwork: unknownFence}), false);
  assert.equal(reader.authorizes(null), false); await reader.capture(); assert.equal(reader.authorizes(captured), false);
});

test('STOP before capture and after queue admission prevents the page network fence', async () => {
  const stopped = fixture([body()]); stopped.stop();
  assert.equal((await stopped.reader.capture()).reasonCode, 'STOPPED'); assert.equal(stopped.calls.length, 0);
  const queued = fixture([body()], {beforeNetwork: ({stop}) => stop()});
  const result = await queued.reader.capture(); assert.equal(result.complete, false); assert.equal(result.reasonCode, 'STOPPED');
  assert.equal(result.snapshots.length, 0); assert.equal(queued.reader.authorizes(queued.calls[0]), false);
});

test('page bound, cursor cycles and duplicate relationship identities cannot claim complete graph', async () => {
  let f = fixture([body([relation(1)], 'next')], {maxPages: 1});
  let result = await f.reader.capture(); assert.equal(result.complete, false); assert.equal(result.reasonCode, 'PAGE_LIMIT'); assert.equal(f.calls.length, 1);
  f = fixture([body([relation(1)], 'same'), body([relation(2)], 'same')]);
  result = await f.reader.capture(); assert.equal(result.reasonCode, 'CURSOR_CYCLE'); assert.equal(result.complete, false); assert.equal(f.calls.length, 2);
  f = fixture([body([relation(1)], 'next'), body([relation(1)])]);
  result = await f.reader.capture(); assert.equal(result.reasonCode, 'DUPLICATE_RELATIONSHIP'); assert.equal(result.complete, false); assert.equal(result.relationships.length, 1);
  f = fixture([body([relation(1), relation(1)])]); assert.equal((await f.reader.capture()).reasonCode, 'DUPLICATE_RELATIONSHIP');
  const first = relation(), upper = relation(); first.id = 'aaaaaaaa-1111-4111-8111-111111111111'; upper.id = first.id.toUpperCase();
  f = fixture([body([first], 'next'), body([upper])]); assert.equal((await f.reader.capture()).reasonCode, 'DUPLICATE_RELATIONSHIP');
});

test('invalid typed fields, wrong scope, response clocks and pagination fail closed', async () => {
  const broken = [r => { r.version = 0; }, r => { r.version = 1.1; }, r => { r.status = 'unknown'; }, r => { r.type = 'correlated'; },
    r => { r.tournamentId = 'foreign'; }, r => { r.nodes[0].exchangeId = '0'; }, r => { r.nodes[0].currentPrice = NaN; },
    r => { r.nodes[0].role = 'winner'; }, r => { r.isExhaustive = 'false'; }, r => { r.direction.kind = 'random'; },
    r => { r.expression = {kind: 'leaf', exchangeId: 0}; }, r => { r.constraint.priceRule = null; },
    r => { r.updatedAt = '2026-02-31T00:00:00.000Z'; }];
  for (const change of broken) { const row = relation(); change(row); const result = await fixture([body([row])]).reader.capture(); assert.equal(result.complete, false); assert.equal(result.reasonCode, 'INVALID_RESPONSE'); }
  for (const response of [{endpoint: '/relationships'}, {status: 201}, {requestStartedAt: stamp(-1)}, {receivedAt: stamp(-1)}, {receivedAt: stamp(10)}]) {
    assert.equal((await fixture([body()], {response}).reader.capture()).reasonCode, 'INVALID_RESPONSE');
  }
  for (const pagination of [{limit: 201, hasMore: false, nextCursor: null}, {limit: 200, hasMore: true, nextCursor: null},
    {limit: 200, hasMore: true, nextCursor: 'bad\nvalue'}, {limit: 200, hasMore: true, nextCursor: '\ud800'}]) assert.equal((await fixture([{data: [], pagination}]).reader.capture()).reasonCode, 'INVALID_RESPONSE');
});

test('only documented public fields are captured and dispatcher errors never leak content', async () => {
  const row = relation(); row.unknownSecret = 'do not store'; row.nodes[0].unknownSecret = 'do not store';
  const result = await fixture([body([row])]).reader.capture(); assert.equal(result.complete, true); assert.ok(!JSON.stringify(result).includes('do not store'));
  const failed = await fixture([new Error('credential in unknown exception')]).reader.capture();
  assert.equal(failed.complete, false); assert.equal(failed.reasonCode, 'DISPATCH_UNAVAILABLE'); assert.ok(!JSON.stringify(failed).includes('credential'));
  const unsafe = JSON.parse(JSON.stringify(relation()).replace('"version":1', '"version":1,"__proto__":{}'));
  assert.equal((await fixture([body([unsafe])]).reader.capture()).reasonCode, 'INVALID_RESPONSE');
});

test('nested documented expression, nullable metadata and inactive statuses are evidence without proof', async () => {
  const row = relation(); row.status = 'stale'; row.type = 'boolean_expression'; row.expression = {kind: 'cond',
    condition: {kind: 'and', children: [{kind: 'leaf', exchangeId: 1077}]}, consequence: {kind: 'not', child: {kind: 'leaf', exchangeId: 1076}}};
  row.nodes[0].marketId = null; row.nodes[0].currentPrice = null; row.nodes[0].contractId = null;
  row.createdAt = '2026-10-02T07:00:00.123456789-04:00';
  const result = await fixture([body([row])]).reader.capture(); assert.equal(result.complete, true); assert.deepEqual(result.relationships[0], row); assert.equal(result.proofConstructed, false);
});

test('actual shared ReadClient preserves the fence, serialization and STOP immediately before fetch', async () => {
  let tick = time, allowed = true, reader, writes = 0;
  const api = new ReadClient('offline-fixture', {now: () => tick, sleepFn: async ms => { tick += ms; },
    authorizeDispatch: spec => reader.authorizes(spec), onAdmission: () => { allowed = false; },
    fetchFn: async () => { writes++; throw Error('Unexpected network call'); }});
  reader = createBasketRelationshipReader({tournamentId, dispatch: spec => api.dispatch(spec), now: () => tick, canOperate: () => allowed});
  const result = await reader.capture(); assert.equal(result.reasonCode, 'STOPPED'); assert.equal(writes, 0); assert.equal(api.requests, 1);
  api.close();
});

test('concurrent capture is refused and a later capture can start after completion', async () => {
  let resolve, spec;
  const reader = createBasketRelationshipReader({tournamentId, now: () => time, dispatch: value => { spec = value; return new Promise(r => { resolve = r; }); }});
  const first = reader.capture(); await assert.rejects(reader.capture(), /CAPTURE_BUSY/);
  resolve({endpoint: spec.endpoint, status: 200, requestStartedAt: stamp(0), receivedAt: stamp(0), data: body()}); assert.equal((await first).complete, true);
  const second = reader.capture(); resolve({endpoint: spec.endpoint, status: 200, requestStartedAt: stamp(0), receivedAt: stamp(0), data: body()}); assert.equal((await second).complete, true);
});
