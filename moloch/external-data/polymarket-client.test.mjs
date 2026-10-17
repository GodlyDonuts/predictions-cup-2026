import test from 'node:test';
import assert from 'node:assert/strict';
import { Admission, ReadClient, GAMMA, CLOB, readUrl, normalizeBook, sha256 } from './polymarket-client.mjs';
const conditionId = '0x' + 'a'.repeat(64), identity = { assetId: '123', conditionId };
const body = () => ({ asset_id: '123', market: conditionId, timestamp: '1792250000000', hash: 'venue', bids: [{ price: '0.09', size: '12345678901234567890.123456789012345678' }, { price: '0.8', size: '2' }], asks: [{ price: '0.99', size: '3' }, { price: '0.9', size: '4' }] });
function fakeAdmission(rps = 30) { let at = 0; return new Admission({ rps, clock: () => at, wait: async ms => { at += ms; } }); }
test('public route allowlist rejects auth/orders/SIG/redirect injection and bounds IDs', () => {
  assert.equal(readUrl(CLOB, '/book?token_id=123').origin, CLOB);
  for (const [origin, suffix] of [[CLOB, '/orders'], [GAMMA, '//evil/book'], ['https://sig.example', '/events'], [CLOB, '/book?token_id=123&token_id=124'], [CLOB, '/book?token_id=1e3'], [GAMMA, '/events/slug/../orders'], [GAMMA, '/events?auth=secret']]) assert.throws(() => readUrl(origin, suffix));
});
test('100 simultaneous admissions are smooth <=30 in every rolling second', async () => {
  const a = fakeAdmission(), starts = await Promise.all(Array.from({ length: 100 }, () => a.reserve()));
  for (let n = 1; n < starts.length; n++) assert.ok(starts[n] - starts[n - 1] >= 32);
  for (const at of starts) assert.ok(starts.filter(t => t > at - 1000 && t <= at).length <= 30);
  assert.equal(a.admitted, 100);
});
test('shared backoff extends monotonically and queue is bounded', async () => {
  const a = fakeAdmission(); a.backoff(2000); a.backoff(100); assert.equal(await a.reserve(), 2000);
  let release; const b = new Admission({ maximumQueued: 1, clock: () => 0, wait: () => new Promise(r => { release = r; }) }); b.backoff(100);
  const ac = new AbortController(); const p = b.reserve(ac.signal); await Promise.resolve(); await assert.rejects(b.reserve(), /QUEUE_BOUND/); ac.abort(); release(); await assert.rejects(p);
});
test('normalized depth sorts numerically and preserves exact financial strings', () => {
  const n = normalizeBook(body(), identity); assert.deepEqual(n.bids.map(x => x.price), ['0.8', '0.09']); assert.deepEqual(n.asks.map(x => x.price), ['0.9', '0.99']); assert.equal(n.bids[1].size, '12345678901234567890.123456789012345678'); assert.equal(n.sourceTimestamp, '1792250000000'); assert.equal(n.pricingAuthority, false);
});
test('identity, clock, duplicate/zero depth, numeric floats and crossed books fail closed', () => {
  for (const mutate of [b => b.asset_id = '124', b => b.market = '0x' + 'b'.repeat(64), b => b.timestamp = 'bad', b => b.bids[0].price = .09, b => b.bids[0].size = '0', b => b.bids.push(b.bids[0]), b => b.asks[0].price = '0.1']) { const b = body(); mutate(b); assert.throws(() => normalizeBook(b, identity)); }
  const b = body(); b.bids = Array.from({ length: 2001 }, () => ({ price: '0.1', size: '1' })); assert.throws(() => normalizeBook(b, identity), /DEPTH_BOUND/);
});
test('client preserves original body bytes/hash/clocks and sends no credentials', async () => {
  const raw = ' {"value":123456789012345678901234567890}\n'; let seen;
  const c = new ReadClient({ admission: fakeAdmission(), fetcher: async (u, options) => { seen = { u, options }; return new Response(raw); } });
  const r = await c.getEvent('senate-2026'); assert.equal(r.bodyText, raw); assert.equal(r.bodySha256, sha256(raw)); assert.equal(r.sourceTimestamp, null); assert.equal(seen.options.method, 'GET'); assert.deepEqual(seen.options.headers, { Accept: 'application/json' }); assert.equal(seen.options.redirect, 'error'); assert.ok(Date.parse(r.receivedAt) >= Date.parse(r.requestStartedAt)); assert.deepEqual(c.stats, { admissions: 1, completions: 1, responses: 1, errors: 0, throttles: 0 });
});
test('429 persists original retry deadline and all clients sharing admission obey it', async () => {
  const a = fakeAdmission(); let persisted; const c = new ReadClient({ admission: a, onBackoff: async v => { persisted = v; }, fetcher: async () => new Response('{"error":"limited"}', { status: 429, headers: { 'Retry-After': '2' } }) });
  await assert.rejects(c.book('123'), e => e.code === 429 && e.receipt.bodyText.includes('limited')); assert.equal(persisted.status, 429); assert.equal(await a.reserve(), 2000); assert.equal(c.stats.throttles, 1);
});
test('stream body bound and invalid JSON are explicit failures with completed accounting', async () => {
  const c = new ReadClient({ admission: fakeAdmission(), maximumBodyBytes: 2, fetcher: async () => new Response('123') }); await assert.rejects(c.book('123'), /BODY_BOUND/); assert.equal(c.stats.completions, 1);
  const d = new ReadClient({ admission: fakeAdmission(), fetcher: async () => new Response('{') }); await assert.rejects(d.search('senate'), /JSON/);
});
test('exact public POST /books is bounded read-only and preserves original request/response', async () => {
  let call; const raw = JSON.stringify([body()]); const c = new ReadClient({ admission: fakeAdmission(), fetcher: async (u, options) => { call = { u, options }; return new Response(raw); } });
  const r = await c.books(['123']); assert.equal(call.u.href, CLOB + '/books'); assert.equal(call.options.method, 'POST'); assert.equal(call.options.body, '[{"token_id":"123"}]'); assert.equal(r.requestBodySha256, sha256(call.options.body)); assert.equal(r.bodyText, raw); assert.equal(c.stats.admissions, 1);
  await assert.rejects(c.books(Array.from({ length: 21 }, (_, i) => String(i + 1))), /BATCH_BOUND/); await assert.rejects(c.books(['123', '123']), /BATCH_BOUND/); await assert.rejects(c.request('POST', CLOB, '/order', []), /READ_METHOD/); await assert.rejects(c.request('POST', CLOB, '/books', [{ token_id: '1', side: 'BUY' }]), /BATCH_SCHEMA/); assert.equal(c.stats.admissions, 1);
});
test('backoff publication failure is fatal and cannot become a soft network error', async () => {
  const c = new ReadClient({ admission: fakeAdmission(), fetcher: async () => new Response('{}', { status: 429 }), onBackoff: async () => { throw Error('disk'); } }); await assert.rejects(c.book('123'), e => e.fatal === true && e.message === 'disk');
});
