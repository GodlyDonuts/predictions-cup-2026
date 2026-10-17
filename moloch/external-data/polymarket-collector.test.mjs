import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { gunzipSync } from 'node:zlib';
import crypto from 'node:crypto';
import { setTimeout as sleep } from 'node:timers/promises';
import { acquire, atomicJson, Journal, retained, run, validateUniverse } from './polymarket-collector.mjs';
import { Admission, ReadClient, normalizeBook, sha256 } from './polymarket-client.mjs';
const conditionId = '0x' + 'a'.repeat(64);
const outcome = id => ({ assetId: id, conditionId, marketId: '1234', eventSlug: 'senate', question: 'Party winner?', outcome: id === '1' ? 'Yes' : 'No', sigReferences: [], pricingAuthority: false, resolutionEquivalenceVerified: false });
async function temp(fn) { const root = await fs.mkdtemp(path.join(os.tmpdir(), 'polymarket-offline-')); try { return await fn(root); } finally { assert.equal(path.dirname(root), os.tmpdir()); await fs.rm(root, { recursive: true, force: true }); } }
test('exclusive live lock, changed-owner release and corrupt lock refusal', async () => temp(async root => {
  const release = await acquire(root); await assert.rejects(acquire(root), /ALREADY_RUNNING|LOCKED/); const p = path.join(root, 'collector.lock'); const old = await fs.readFile(p); await fs.writeFile(p, '{"pid":1,"identity":"replacement"}'); await assert.rejects(release(), /LOCK_CHANGED/); await fs.writeFile(p, old); await fs.unlink(p); await fs.writeFile(p, 'broken'); await assert.rejects(acquire(root), /UNVERIFIABLE/);
}));
test('two stale-owner reclaimers yield exactly one owner', async () => temp(async root => {
  const p = spawn(process.execPath, ['-e', 'process.exit(0)']); await once(p, 'exit'); await fs.writeFile(path.join(root, 'collector.lock'), JSON.stringify({ pid: p.pid, identity: 'dead' }));
  const r = await Promise.allSettled([acquire(root), acquire(root)]); const ok = r.filter(x => x.status === 'fulfilled'); assert.equal(ok.length, 1); await ok[0].value();
}));
test('atomic per-file publications serialize and preserve complete JSON', async () => temp(async root => {
  const file = path.join(root, 'latest.json'); await Promise.all(Array.from({ length: 20 }, (_, i) => atomicJson(file, { i, x: 'x'.repeat(2000) }))); assert.equal(JSON.parse(await fs.readFile(file)).i, 19); assert.deepEqual(await fs.readdir(root), ['latest.json']);
}));
test('universe exact asset-condition mapping and no pricing/equivalence authority', () => {
  const u = { schemaVersion: 1, source: 'polymarket_gamma_public', outcomes: [outcome('1'), outcome('2')] }; assert.equal(validateUniverse(u), u); assert.throws(() => validateUniverse({ ...u, outcomes: [outcome('1'), outcome('1')] })); assert.throws(() => validateUniverse({ ...u, outcomes: [{ ...outcome('1'), pricingAuthority: true }] }));
});
test('compressed retention prunes only old closed owned segments and preserves current', async () => temp(async root => {
  const j = new Journal(root, { maximumBytes: 90, segmentBytes: 5 }); await j.initialize(); await j.append({ row: 1 }); await j.flush(); await j.append({ row: 2 }); await j.flush(); const names = await fs.readdir(root); assert.equal(names.length, 2);
  const decoded = await Promise.all(names.map(async n => JSON.parse(gunzipSync(await fs.readFile(path.join(root, n)))))); assert.deepEqual(decoded.sort((a, b) => a.row - b.row), [{ row: 1 }, { row: 2 }]);
  await j.append({ row: 3 }); await j.flush(); await j.append({ row: 4 }); await j.flush(); assert.equal(j.prunedSegments, 1); assert.ok(j.prunedBytes > 0); assert.equal((await fs.readdir(root)).length, 3); assert.ok((await fs.readdir(root)).includes(path.basename(j.file)));
  await j.append({ random: crypto.randomBytes(500).toString('hex') }); await assert.rejects(j.flush(), /BOUND/); await assert.rejects(j.close(), /BOUND/);
}));
test('100 original records share one gzip append; close flushes the final record in exact order', async () => temp(async root => {
  const j = new Journal(root); await j.initialize();
  const records = Array.from({ length: 100 }, (_, sequence) => ({ sequence, bodyText: '{"original":"unchanged ' + sequence + '"}', receivedAt: '2026-10-17T19:00:00.000Z' }));
  for (const row of records) await j.append(row); assert.equal(j.acceptedRows, 100); assert.equal(j.appendedRows, 0); assert.ok(j.bufferBytes <= 1024 ** 2); assert.equal((await fs.readdir(root)).length, 0);
  await j.flush(); assert.equal(j.flushes, 1); assert.equal(j.appendedRows, 100); await j.append({ sequence: 100, stopped: true }); await j.close(); assert.equal(j.flushes, 2); assert.equal(j.appendedRows, 101);
  const raw = gunzipSync(await fs.readFile(j.file)).toString(); assert.equal(raw, [...records, { sequence: 100, stopped: true }].map(x => JSON.stringify(x) + '\n').join('')); await assert.rejects(j.append({ late: true }), /CLOSED/);
}));
test('100ms timer flushes quiet data and an asynchronous write failure poisons the shared journal', async () => temp(async root => {
  const j = new Journal(root); await j.initialize(); await j.append({ original: 1 }); await sleep(150); await j.flush(); assert.equal(j.appendedRows, 1); await j.close();
  const sub = path.join(root, 'fail'); const errors = []; const failed = new Journal(sub, { flushMs: 5, onError: e => errors.push(e) }); await failed.initialize();
  const original = fs.appendFile; fs.appendFile = async file => { if (String(file).startsWith(sub)) throw Error('TEST_DISK_FAILURE'); throw Error('UNEXPECTED_PATH'); };
  try { await failed.append({ original: 'accepted_not_durable' }); await sleep(30); assert.equal(failed.acceptedRows, 1); assert.equal(failed.appendedRows, 0); assert.equal(errors.length, 1); await assert.rejects(failed.append({ later: true }), /TEST_DISK_FAILURE/); await assert.rejects(failed.close(), /TEST_DISK_FAILURE/); }
  finally { fs.appendFile = original; }
}));
test('slow append backpressures a full bounded buffer without reordering or concurrent writes', async () => temp(async root => {
  const j = new Journal(root, { bufferBytes: 80, flushMs: 100 }); await j.initialize();
  const original = fs.appendFile; let release, reached; const blocked = new Promise(r => { reached = r; }); const gate = new Promise(r => { release = r; }); let active = 0, peak = 0;
  fs.appendFile = async (...args) => { active++; peak = Math.max(peak, active); reached(); await gate; try { return await original(...args); } finally { active--; } };
  try {
    await j.append({ n: 0, original: 'a'.repeat(30) }); const second = j.append({ n: 1, original: 'b'.repeat(30) }); await blocked; let finished = false; second.then(() => { finished = true; }); await sleep(10); assert.equal(finished, false); assert.ok(j.bufferBytes <= 80); assert.equal(peak, 1); release(); await second; await j.close(); assert.equal(peak, 1);
    const lines = gunzipSync(await fs.readFile(j.file)).toString().trim().split('\n').map(JSON.parse); assert.deepEqual(lines.map(x => x.n), [0, 1]);
  } finally { release?.(); fs.appendFile = original; }
}));
test('retained cache validates original body/hash/identity and never renews its clocks', () => {
  const identity = outcome('1'), at = '2026-10-16T00:00:00.000Z', body = { market: conditionId, asset_id: '1', timestamp: String(Date.parse(at)), bids: [], asks: [] }, bodyText = JSON.stringify(body), receipt = { source: 'polymarket_clob_public', endpoint: 'https://clob.polymarket.com/book?token_id=1', requestStartedAt: at, receivedAt: at, status: 200, bodyText, bodySha256: sha256(bodyText), body };
  const row = { identity, receipt, book: normalizeBook(body, identity), firstAvailableAt: at }; assert.equal(retained(row, identity).receipt.receivedAt, at); assert.throws(() => retained({ ...row, receipt: { ...receipt, bodyText: '{}' } }, identity)); assert.throws(() => retained(row, outcome('2')));
});
test('actual loopback cache service polls fairly with <=12 inflight and shuts down cleanly', async () => temp(async root => {
  const universe = path.join(root, 'universe.json'); await fs.writeFile(universe, JSON.stringify({ schemaVersion: 1, source: 'polymarket_gamma_public', outcomes: [outcome('1'), outcome('2'), outcome('3')] }));
  const seen = []; const client = new ReadClient({ admission: new Admission({ rps: 30 }), fetcher: async u => { const id = u.searchParams.get('token_id'); seen.push(id); return new Response(JSON.stringify({ market: conditionId, asset_id: id, timestamp: String(Date.now()), bids: [{ price: '0.4', size: '10' }], asks: [{ price: '0.5', size: '20' }] })); } });
  let stop; const result = await run({ cache: path.join(root, 'cache'), universe, port: '0', concurrency: '12', 'duration-seconds': '1.3' }, { client, onReady: async x => { stop = x.stop; assert.equal(typeof x.recordEvent, 'function'); const response = await fetch('http://127.0.0.1:' + x.port + '/health'); assert.equal(response.status, 200); }, onEvent: row => { assert.equal(row.book.pricingAuthority, false); } });
  assert.equal(typeof stop, 'function'); assert.ok(result.stats.successfulBooks >= 3); assert.deepEqual(seen.slice(0, 3), ['1', '2', '3']); assert.ok(result.stats.maximumInFlight <= 12); assert.equal(result.admissions, result.completions); await assert.rejects(fs.stat(path.join(root, 'cache', 'collector.lock')), { code: 'ENOENT' }); const r = JSON.parse(await fs.readFile(path.join(root, 'cache', 'latest', '1.json'))); assert.equal(r.receipt.bodySha256, sha256(r.receipt.bodyText));
}));
test('paired batch maps out-of-order response exactly and retains whole original body per outcome', async () => temp(async root => {
  const universe = path.join(root, 'universe.json'); await fs.writeFile(universe, JSON.stringify({ schemaVersion: 1, source: 'polymarket_gamma_public', outcomes: [outcome('1'), outcome('2')] }));
  let stop; const calls = []; const client = new ReadClient({ admission: new Admission({ rps: 30 }), fetcher: async (u, options) => { calls.push(options); assert.equal(u.pathname, '/books'); return new Response(JSON.stringify(JSON.parse(options.body).reverse().map(x => ({ market: conditionId, asset_id: x.token_id, timestamp: String(Date.now()), bids: [], asks: [] })))); } });
  const result = await run({ cache: path.join(root, 'cache'), universe, port: '0', 'batch-size': '2', 'duration-seconds': '2' }, { client, onReady: x => { stop = x.stop; }, onEvent: row => { assert.equal(row.receipt.method, 'POST'); if (row.identity.assetId === '2') stop(); } });
  assert.equal(result.admissions, 1); assert.equal(result.stats.successfulBooks, 2); assert.equal(result.completeMarketCoverage, 1); assert.equal(calls[0].body, '[{"token_id":"1"},{"token_id":"2"}]');
  const row = JSON.parse(await fs.readFile(path.join(root, 'cache', 'latest', '1.json'))); assert.equal(retained(row, outcome('1')).bodyIndex, 1); assert.equal(row.receipt.body.length, 2); assert.throws(() => retained({ ...row, bodyIndex: 0 }, outcome('1')), /IDENTITY/);
}));
test('equal-version depth conflict remains withdrawn across restart; stopping hook drains originals', async () => temp(async root => {
  const universe = path.join(root, 'universe.json'), cache = path.join(root, 'cache'), identity = outcome('1'); await fs.writeFile(universe, JSON.stringify({ schemaVersion: 1, source: 'polymarket_gamma_public', outcomes: [identity] }));
  const client = new ReadClient({ fetcher: async () => { throw Error('NO_HTTP_EXPECTED'); } }); let record, sourceAt = Date.now() - 1000;
  const row = price => { const now = new Date().toISOString(), body = { asset_id: '1', market: conditionId, timestamp: String(sourceAt), bids: [{ price, size: '2' }], asks: [{ price: '0.8', size: '2' }] }, bodyText = JSON.stringify(body); return { identity, receipt: { source: 'polymarket_clob_public', endpoint: 'https://clob.polymarket.com/book?token_id=1', requestStartedAt: now, receivedAt: now, status: 200, body, bodyText, bodySha256: sha256(bodyText) } }; };
  await run({ cache, universe, port: '0' }, { client, onReady: async x => { record = x.recordEvent; await x.onEvent(row('0.4')); await x.onEvent(row('0.5')); await x.onEvent(row('0.4')); assert.equal(x.status().cachedBooks, 0); x.stop(); }, onStopping: async () => { await record({ source: 'synthetic_ws_stop', invalid: true }); } });
  assert.equal(JSON.parse(await fs.readFile(path.join(cache, 'latest', '1.json'))).invalidReason, 'POLYMARKET_BOOK_CONFLICT');
  await run({ cache, universe, port: '0' }, { client, onReady: async x => { assert.equal(x.status().cachedBooks, 0); await x.onEvent(row('0.4')); assert.equal(x.status().cachedBooks, 0); sourceAt++; await x.onEvent(row('0.4')); assert.equal(x.status().cachedBooks, 1); x.stop(); } });
  assert.equal(client.stats.admissions, 0); await assert.rejects(record({ late: true }), /JOURNAL_CLOSED/);
  const lines = (await Promise.all((await fs.readdir(path.join(cache, 'journal'))).map(async n => gunzipSync(await fs.readFile(path.join(cache, 'journal', n))).toString()))).join('').trim().split('\n').map(JSON.parse); assert.ok(lines.some(x => x.source === 'synthetic_ws_stop' && x.invalid));
}));
