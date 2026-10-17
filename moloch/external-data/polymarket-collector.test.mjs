import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { gunzipSync } from 'node:zlib';
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
test('compressed bounded journal retains all members and fails without pruning', async () => temp(async root => {
  const j = new Journal(root, { maximumBytes: 90, segmentBytes: 5 }); await j.initialize(); await j.append({ row: 1 }); await j.append({ row: 2 }); const names = await fs.readdir(root); assert.equal(names.length, 2);
  const decoded = await Promise.all(names.map(async n => JSON.parse(gunzipSync(await fs.readFile(path.join(root, n)))))); assert.deepEqual(decoded.sort((a, b) => a.row - b.row), [{ row: 1 }, { row: 2 }]);
  await assert.rejects(j.append({ random: 'a'.repeat(200) + '1234567890' }), /BOUND/); assert.equal((await fs.readdir(root)).length, 2);
}));
test('retained cache validates original body/hash/identity and never renews its clocks', () => {
  const identity = outcome('1'), at = '2026-10-16T00:00:00.000Z', body = { market: conditionId, asset_id: '1', timestamp: String(Date.parse(at)), bids: [], asks: [] }, bodyText = JSON.stringify(body), receipt = { source: 'polymarket_clob_public', endpoint: 'https://clob.polymarket.com/book?token_id=1', requestStartedAt: at, receivedAt: at, status: 200, bodyText, bodySha256: sha256(bodyText), body };
  const row = { identity, receipt, book: normalizeBook(body, identity), firstAvailableAt: at }; assert.equal(retained(row, identity).receipt.receivedAt, at); assert.throws(() => retained({ ...row, receipt: { ...receipt, bodyText: '{}' } }, identity)); assert.throws(() => retained(row, outcome('2')));
});
test('actual loopback cache service polls fairly with <=12 inflight and shuts down cleanly', async () => temp(async root => {
  const universe = path.join(root, 'universe.json'); await fs.writeFile(universe, JSON.stringify({ schemaVersion: 1, source: 'polymarket_gamma_public', outcomes: [outcome('1'), outcome('2'), outcome('3')] }));
  const seen = []; const client = new ReadClient({ admission: new Admission({ rps: 30 }), fetcher: async u => { const id = u.searchParams.get('token_id'); seen.push(id); return new Response(JSON.stringify({ market: conditionId, asset_id: id, timestamp: String(Date.now()), bids: [{ price: '0.4', size: '10' }], asks: [{ price: '0.5', size: '20' }] })); } });
  let stop; const result = await run({ cache: path.join(root, 'cache'), universe, port: '0', concurrency: '12', 'duration-seconds': '.3' }, { client, onReady: async x => { stop = x.stop; const response = await fetch('http://127.0.0.1:' + x.port + '/health'); assert.equal(response.status, 200); }, onEvent: row => { assert.equal(row.book.pricingAuthority, false); } });
  assert.equal(typeof stop, 'function'); assert.ok(result.stats.successfulBooks >= 3); assert.deepEqual(seen.slice(0, 3), ['1', '2', '3']); assert.ok(result.stats.maximumInFlight <= 12); assert.equal(result.admissions, result.completions); await assert.rejects(fs.stat(path.join(root, 'cache', 'collector.lock')), { code: 'ENOENT' }); const r = JSON.parse(await fs.readFile(path.join(root, 'cache', 'latest', '1.json'))); assert.equal(r.receipt.bodySha256, sha256(r.receipt.bodyText));
}));
