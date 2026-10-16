import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { setTimeout as sleep } from 'node:timers/promises';
import * as collector from './kalshi-collector.mjs';
const { acquire, atomicJson } = collector;
async function fixture(fn) { const parent = path.resolve(os.tmpdir()); const root = await fs.mkdtemp(path.join(parent, 'kalshi-collector-offline-')); assert.equal(path.dirname(path.resolve(root)), parent); assert.match(path.basename(root), /^kalshi-collector-offline-/); try { await fn(root); } finally { await fs.rm(root, { recursive: true, force: true }); } }
async function deadPid() { const child = spawn(process.execPath, ['-e', 'process.exit(0)'], { stdio: 'ignore' }); const [code] = await once(child, 'exit'); assert.equal(code, 0); assert.throws(() => process.kill(child.pid, 0), error => error.code === 'ESRCH'); return child.pid; }
function timeout(promise, ms = 2000) { let timer; return Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(Error('TEST_OPERATION_HUNG')), ms); })]).finally(() => clearTimeout(timer)); }

test('actual live process owns the cache exclusively and normal release removes only its lock', async () => fixture(async root => {
 const release = await acquire(root); const file = path.join(root, 'collector.lock'); const original = await fs.readFile(file, 'utf8'); const row = JSON.parse(original);
 assert.equal(row.pid, process.pid); assert.match(row.identity, /^[a-f0-9-]{36}$/); await assert.rejects(acquire(root), /KALSHI_(COLLECTOR_ALREADY_RUNNING|COLLECTOR_LOCKED)/); assert.equal(await fs.readFile(file, 'utf8'), original);
 await release(); await assert.rejects(fs.stat(file), { code: 'ENOENT' });
}));

test('a positively dead child owner may be reclaimed without retaining its identity', async () => fixture(async root => {
 const file = path.join(root, 'collector.lock'); const pid = await deadPid(); await fs.writeFile(file, JSON.stringify({ pid, identity: 'original-dead-owner', at: new Date().toISOString() }));
 const release = await acquire(root); const row = JSON.parse(await fs.readFile(file, 'utf8')); assert.equal(row.pid, process.pid); assert.notEqual(row.identity, 'original-dead-owner'); await release();
}));

test('corrupt or unverifiable ownership is preserved and never silently reclaimed', async () => {
 for (const raw of ['{', '{}', JSON.stringify({ pid: 0 }), JSON.stringify({ pid: -1 }), JSON.stringify({ pid: '123' })]) await fixture(async root => {
  const file = path.join(root, 'collector.lock'); await fs.writeFile(file, raw); await assert.rejects(acquire(root)); assert.equal(await fs.readFile(file, 'utf8'), raw);
 });
});

test('two stale-recovery contenders cannot both acquire ownership across the unlink/recreate boundary', async () => fixture(async root => {
 const file = path.join(root, 'collector.lock'); const pid = await deadPid(); const raw = JSON.stringify({ pid, identity: 'dead-race-owner', at: new Date().toISOString() }); await fs.writeFile(file, raw);
 const read = fs.readFile, unlink = fs.unlink, open = fs.open; let oldReads = 0, removals = 0; let bothChecked, recreated; const checked = new Promise(resolve => { bothChecked = resolve; }); const created = new Promise(resolve => { recreated = resolve; });
 // Old implementation lets both readers validate the same dead-owner bytes.
 // A bounded rendezvous exposes that scheduling without requiring a fixed
 // implementation to let a second contender past its exclusive recovery guard.
 fs.readFile = async function (name, ...rest) { const result = await read.call(this, name, ...rest); if (path.resolve(String(name)) === file && String(result) === raw && ++oldReads >= 3) { if (oldReads >= 4) bothChecked(); await Promise.race([checked, sleep(100)]); } return result; };
 fs.open = async function (name, ...rest) { const fd = await open.call(this, name, ...rest); if (path.resolve(String(name)) === file && rest[0] === 'wx') recreated(); return fd; };
 fs.unlink = async function (name, ...rest) { if (path.resolve(String(name)) === file && ++removals === 2) await Promise.race([created, sleep(100)]); return unlink.call(this, name, ...rest); };
 let results;
 try { results = await timeout(Promise.allSettled([acquire(root), acquire(root)])); }
 finally { fs.readFile = read; fs.unlink = unlink; fs.open = open; }
 const owners = results.filter(x => x.status === 'fulfilled');
 try { assert.equal(owners.length, 1, 'stale replacement must have exactly one live owner'); assert.equal(JSON.parse(await read(file, 'utf8')).pid, process.pid); }
 finally { for (const owner of owners) await owner.value().catch(() => {}); }
}));

test('release refuses a replacement owner and leaves the replacement bytes intact', async () => fixture(async root => {
 const release = await acquire(root); const file = path.join(root, 'collector.lock'); const replacement = JSON.stringify({ pid: process.pid, identity: crypto.randomUUID(), at: new Date().toISOString() }); await fs.writeFile(file, replacement);
 await assert.rejects(release(), /KALSHI_LOCK_CHANGED/); assert.equal(await fs.readFile(file, 'utf8'), replacement);
}));

test('concurrent cache readers observe complete old or new JSON during atomic publication', async () => fixture(async root => {
 const file = path.join(root, 'latest.json'); await atomicJson(file, { version: -1, payload: 'old' }); let done = false; let observations = 0;
 const reader = (async () => { while (!done) { const row = JSON.parse(await fs.readFile(file, 'utf8')); assert.ok(row.version >= -1 && row.version < 8); assert.equal(row.payload, row.version === -1 ? 'old' : String(row.version).repeat(32768)); observations++; } })();
 try { await Promise.all(Array.from({ length: 8 }, (_, version) => atomicJson(file, { version, payload: String(version).repeat(32768) }))); }
 finally { done = true; await reader; }
 assert.ok(observations > 0); const final = JSON.parse(await fs.readFile(file, 'utf8')); assert.ok(final.version >= 0); assert.equal(final.payload, String(final.version).repeat(32768));
 if (process.platform !== 'win32') assert.equal((await fs.stat(file)).mode & 0o777, 0o600);
}));

test('serialization or rename failure preserves the prior complete cache', async () => fixture(async root => {
 const file = path.join(root, 'latest.json'); await atomicJson(file, { original: true }); const bytes = await fs.readFile(file, 'utf8'); const circular = {}; circular.self = circular;
 await assert.rejects(atomicJson(file, circular)); assert.equal(await fs.readFile(file, 'utf8'), bytes);
 const rename = fs.rename; fs.rename = async function (from, to) { if (path.resolve(to) === file) throw Object.assign(Error('synthetic disk failure'), { code: 'EIO' }); return rename.call(this, from, to); };
 try { await assert.rejects(atomicJson(file, { replacement: true }), { code: 'EIO' }); }
 finally { fs.rename = rename; }
 assert.equal(await fs.readFile(file, 'utf8'), bytes);
}));

test('downstream journal failure rejects backpressure and close without hanging shutdown', async () => fixture(async root => {
 assert.equal(typeof collector.Journal, 'function', 'existing Journal export is required for the focused I/O regression'); const journal = new collector.Journal(root);
 await journal.append({ kind: 'book', bodyText: '{}' }); const output = journal.output; const closed = new Promise(resolve => output.once('close', resolve));
 const waiting = journal.append({ kind: 'book', bodyText: crypto.randomBytes(512 * 1024).toString('hex') });
 queueMicrotask(() => output.destroy(Object.assign(Error('synthetic downstream disk failure'), { code: 'EIO' })));
 await assert.rejects(timeout(waiting), error => { assert.notEqual(error.message, 'TEST_OPERATION_HUNG'); return true; }); await timeout(closed);
 await assert.rejects(timeout(journal.close()), error => { assert.notEqual(error.message, 'TEST_OPERATION_HUNG'); return true; });
}));
