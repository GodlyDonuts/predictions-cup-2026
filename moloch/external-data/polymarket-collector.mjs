import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import http from 'node:http';
import { gzipSync } from 'node:zlib';
import { setTimeout as sleep } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import { Admission, ReadClient, assetId, CONDITION, normalizeBook, sha256 } from './polymarket-client.mjs';

const publications = new Map();
export async function atomicJson(file, value) {
  const key = path.resolve(file), prior = publications.get(key) ?? Promise.resolve();
  const next = prior.catch(() => {}).then(async () => {
    const tmp = key + '.' + crypto.randomUUID() + '.tmp'; let fd;
    try {
      fd = await fs.open(tmp, 'wx', 0o600); await fd.writeFile(JSON.stringify(value) + '\n'); await fd.sync(); await fd.close(); fd = null;
      for (let n = 0; ; n++) { try { await fs.rename(tmp, key); break; } catch (e) { if (process.platform !== 'win32' || !['EPERM', 'EBUSY', 'EACCES'].includes(e.code) || n >= 7) throw e; await sleep(8 * 2 ** n); } }
    } finally { await fd?.close(); await fs.unlink(tmp).catch(e => { if (e.code !== 'ENOENT') throw e; }); }
  }); publications.set(key, next);
  try { await next; } finally { if (publications.get(key) === next) publications.delete(key); }
}
export async function acquire(cache) {
  await fs.mkdir(cache, { recursive: true, mode: 0o700 });
  const name = path.join(cache, 'collector.lock'), guardName = name + '.reclaim', identity = crypto.randomUUID();
  for (let pass = 0; pass < 2; pass++) {
    let fd;
    try { fd = await fs.open(name, 'wx', 0o600); }
    catch (e) {
      if (e.code !== 'EEXIST' || pass) throw Error('POLYMARKET_COLLECTOR_LOCKED');
      let guard; try { guard = await fs.open(guardName, 'wx', 0o600); } catch { throw Error('POLYMARKET_LOCK_RECLAIM_BUSY'); }
      try {
        const st = await fs.lstat(name); if (!st.isFile() || st.isSymbolicLink() || st.size > 4096) throw Error('POLYMARKET_LOCK_UNVERIFIABLE');
        const raw = await fs.readFile(name, 'utf8'); let old; try { old = JSON.parse(raw); } catch { throw Error('POLYMARKET_LOCK_UNVERIFIABLE'); }
        if (!Number.isSafeInteger(old.pid) || old.pid < 1 || typeof old.identity !== 'string') throw Error('POLYMARKET_LOCK_UNVERIFIABLE');
        try { process.kill(old.pid, 0); throw Error('POLYMARKET_COLLECTOR_ALREADY_RUNNING'); } catch (error) { if (error.code !== 'ESRCH') throw Error('POLYMARKET_COLLECTOR_ALREADY_RUNNING'); }
        if (await fs.readFile(name, 'utf8') !== raw) throw Error('POLYMARKET_LOCK_CHANGED'); await fs.unlink(name);
      } finally { await guard.close(); await fs.unlink(guardName); }
      continue;
    }
    try { await fd.writeFile(JSON.stringify({ pid: process.pid, identity, at: new Date().toISOString() })); await fd.sync(); }
    catch (e) { await fd.close(); throw e; }
    return async () => { await fd.close(); const old = JSON.parse(await fs.readFile(name, 'utf8')); if (old.identity !== identity) throw Error('POLYMARKET_LOCK_CHANGED'); await fs.unlink(name); };
  }
}
export function validateUniverse(value) {
  if (value?.schemaVersion !== 1 || value.source !== 'polymarket_gamma_public' || !Array.isArray(value.outcomes) || value.outcomes.length < 1 || value.outcomes.length > 2000) throw Error('POLYMARKET_UNIVERSE');
  const ids = new Set();
  for (const row of value.outcomes) {
    assetId(row.assetId);
    if (ids.has(row.assetId) || !CONDITION.test(row.conditionId ?? '') || !['marketId', 'eventSlug', 'question', 'outcome'].every(k => typeof row[k] === 'string' && row[k].length > 0 && row[k].length <= 8192) || !Array.isArray(row.sigReferences) || row.sigReferences.length > 64 || row.pricingAuthority !== false || row.resolutionEquivalenceVerified !== false) throw Error('POLYMARKET_UNIVERSE_IDENTITY');
    ids.add(row.assetId);
  }
  return value;
}
export class Journal {
  constructor(root, { maximumBytes = 2 * 1024 ** 3, segmentBytes = 32 * 1024 ** 2 } = {}) { Object.assign(this, { root, maximumBytes, segmentBytes }); this.total = 0; this.segment = 0; this.file = null; this.tail = Promise.resolve(); }
  async initialize() {
    await fs.mkdir(this.root, { recursive: true, mode: 0o700 }); const names = await fs.readdir(this.root);
    if (names.length > 8192) throw Error('POLYMARKET_JOURNAL_FILES_BOUND');
    for (const name of names) { if (!/^books-[a-f0-9-]+\.ndjson\.gz$/.test(name)) throw Error('POLYMARKET_JOURNAL_UNKNOWN_FILE'); const st = await fs.lstat(path.join(this.root, name)); if (!st.isFile() || st.isSymbolicLink()) throw Error('POLYMARKET_JOURNAL_FILE'); this.total += st.size; }
    this.files = names.length; if (this.total >= this.maximumBytes) throw Error('POLYMARKET_JOURNAL_BOUND');
  }
  append(value) {
    const operation = this.tail.then(async () => {
      const raw = Buffer.from(JSON.stringify(value) + '\n'); if (raw.length > 20 * 1024 ** 2) throw Error('POLYMARKET_JOURNAL_ROW_BOUND');
      const compressed = gzipSync(raw, { level: 1 }); if (this.total + compressed.length > this.maximumBytes) throw Error('POLYMARKET_JOURNAL_BOUND');
      if (!this.file || this.segment + raw.length > this.segmentBytes) { if (this.files >= 8192) throw Error('POLYMARKET_JOURNAL_FILES_BOUND'); this.file = path.join(this.root, 'books-' + crypto.randomUUID() + '.ndjson.gz'); await fs.writeFile(this.file, '', { flag: 'wx', mode: 0o600 }); this.files++; this.segment = 0; }
      await fs.appendFile(this.file, compressed); this.total += compressed.length; this.segment += raw.length;
    }); this.tail = operation; return operation;
  }
  async close() { await this.tail; }
}
export function retained(row, identity) {
  if (row?.identity?.assetId !== identity.assetId || row.identity.conditionId !== identity.conditionId || row.receipt?.status !== 200 || row.receipt.source !== 'polymarket_clob_public' || row.receipt.endpoint !== 'https://clob.polymarket.com/book?token_id=' + identity.assetId || sha256(row.receipt.bodyText ?? '') !== row.receipt.bodySha256) throw Error('POLYMARKET_RETAINED_RECEIPT');
  const start = Date.parse(row.receipt.requestStartedAt), end = Date.parse(row.receipt.receivedAt), first = Date.parse(row.firstAvailableAt);
  if (![start, end, first].every(Number.isFinite) || start > end || end > Date.now() || first > end) throw Error('POLYMARKET_RETAINED_CLOCK');
  const body = JSON.parse(row.receipt.bodyText), book = normalizeBook(body, identity);
  if (JSON.stringify(body) !== JSON.stringify(row.receipt.body) || JSON.stringify(book) !== JSON.stringify(row.book) || Number(book.sourceTimestamp) > end + 5000) throw Error('POLYMARKET_RETAINED_BOOK');
  return row;
}
export async function run(options, dependencies = {}) {
  const cache = path.resolve(options.cache ?? 'polymarket-cache'), port = Number(options.port ?? 17842), rps = Number(options.rps ?? 30), concurrency = Number(options.concurrency ?? 12), duration = Number(options['duration-seconds'] ?? 0);
  if (!options.universe || !Number.isInteger(port) || port < 0 || port > 65535 || !Number.isInteger(concurrency) || concurrency < 1 || concurrency > 64 || !Number.isInteger(rps) || rps < 1 || rps > 30 || !Number.isFinite(duration) || duration < 0 || duration > 86400) throw Error('POLYMARKET_CONFIGURATION');
  const universeBytes = await fs.readFile(options.universe); if (universeBytes.length > 16 * 1024 ** 2) throw Error('POLYMARKET_UNIVERSE_BOUND');
  const universe = validateUniverse(JSON.parse(universeBytes)); const release = await acquire(cache); const controller = new AbortController();
  const latest = new Map(), tasks = new Set(), states = universe.outcomes.map(identity => ({ identity, pending: false })); let cursor = 0, stopped = false, fatal, server, statusTimer, durationTimer;
  const stats = { scheduled: 0, successfulBooks: 0, invalidBooks: 0, errors: 0, oldBooks: 0, conflicts: 0, maximumInFlight: 0, lastError: null };
  const startedAt = new Date().toISOString(); const stop = () => { stopped = true; controller.abort(); }; let maximumBackoff = 0, backoffTail = Promise.resolve();
  const admission = dependencies.client?.admission ?? new Admission({ rps });
  const client = dependencies.client ?? new ReadClient({ admission, onBackoff: value => { maximumBackoff = Math.max(maximumBackoff, Date.parse(value.blockedUntil)); backoffTail = backoffTail.then(() => atomicJson(path.join(cache, 'backoff.json'), { ...value, blockedUntil: new Date(maximumBackoff).toISOString() })); return backoffTail; } });
  const journal = new Journal(path.join(cache, 'journal'), dependencies.journalOptions);
  const status = () => { const now = Date.now(), ages = [...latest.values()].map(x => now - Number(x.book.sourceTimestamp)).sort((a, b) => a - b); return { source: 'polymarket_clob_public', pricingAuthority: false, resolutionEquivalenceVerified: false, pid: process.pid, startedAt, at: new Date(now).toISOString(), state: fatal ? 'failed' : stopped ? 'stopped' : 'running', universeSha256: sha256(universeBytes), outcomes: states.length, cachedBooks: latest.size, inFlight: tasks.size, configuredMaximumRps: rps, concurrency, blockedUntil: new Date(Date.now() + Math.max(0, admission.blockedUntil - admission.clock())).toISOString(), admissions: client.stats.admissions, completions: client.stats.completions, client: client.stats, stats: { ...stats }, sourceAgeMs: { minimum: ages[0] ?? null, median: ages[Math.floor(ages.length / 2)] ?? null, maximum: ages.at(-1) ?? null } }; };
  const publishStatus = () => atomicJson(path.join(cache, 'status.json'), status());
  async function ingest({ identity, receipt }) {
    let book; try { book = normalizeBook(receipt.body, identity); if (Number(book.sourceTimestamp) > Date.parse(receipt.receivedAt) + 5000) throw Error('POLYMARKET_BOOK_FUTURE'); }
    catch (e) { stats.invalidBooks++; stats.lastError = e.message; await journal.append({ identity, receipt, invalid: e.message }); return; }
    const previous = latest.get(identity.assetId); const content = sha256(JSON.stringify({ bids: book.bids, asks: book.asks }));
    await journal.append({ identity, receipt, book });
    if (previous && Number(book.sourceTimestamp) < Number(previous.book.sourceTimestamp)) { stats.oldBooks++; return; }
    if (previous && book.sourceTimestamp === previous.book.sourceTimestamp && content !== previous.contentSha256) { stats.conflicts++; latest.delete(identity.assetId); await fs.unlink(path.join(cache, 'latest', identity.assetId + '.json')).catch(e => { if (e.code !== 'ENOENT') throw e; }); return; }
    const row = { identity, receipt, book, contentSha256: content, firstAvailableAt: previous && book.sourceTimestamp === previous.book.sourceTimestamp ? previous.firstAvailableAt : receipt.receivedAt };
    retained(row, identity); await atomicJson(path.join(cache, 'latest', identity.assetId + '.json'), row); latest.set(identity.assetId, row); stats.successfulBooks++; await dependencies.onEvent?.(row);
  }
  process.once('SIGINT', stop); process.once('SIGTERM', stop);
  try {
    await fs.mkdir(path.join(cache, 'latest'), { recursive: true, mode: 0o700 }); await journal.initialize();
    try { const b = JSON.parse(await fs.readFile(path.join(cache, 'backoff.json'), 'utf8')); const until = Date.parse(b.blockedUntil); if (!Number.isFinite(until)) throw Error('POLYMARKET_BACKOFF_STATE'); maximumBackoff = until; admission.backoff(Math.max(0, until - Date.now())); } catch (e) { if (e.code !== 'ENOENT') throw e; }
    for (const state of states) { try { const row = retained(JSON.parse(await fs.readFile(path.join(cache, 'latest', state.identity.assetId + '.json'), 'utf8')), state.identity); latest.set(state.identity.assetId, row); } catch (e) { if (e.code !== 'ENOENT') throw e; } }
    server = http.createServer((req, res) => {
      if (req.method !== 'GET') { res.writeHead(405); res.end(); return; }
      const url = new URL(req.url, 'http://127.0.0.1'); let value;
      if (url.pathname === '/health' || url.pathname === '/status') value = status();
      else if (url.pathname === '/quotes') value = { at: new Date().toISOString(), pricingAuthority: false, resolutionEquivalenceVerified: false, quotes: [...latest.values()].map(x => ({ identity: x.identity, book: { ...x.book, bids: undefined, asks: undefined }, requestStartedAt: x.receipt.requestStartedAt, receivedAt: x.receipt.receivedAt, firstAvailableAt: x.firstAvailableAt, bodySha256: x.receipt.bodySha256 })) };
      else if (url.pathname === '/book' && latest.has(url.searchParams.get('assetId'))) value = latest.get(url.searchParams.get('assetId'));
      else { res.writeHead(404); res.end(); return; }
      res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); res.end(JSON.stringify(value));
    }); server.maxConnections = 64; server.requestTimeout = 5000;
    await new Promise((resolve, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolve); });
    await publishStatus(); await dependencies.onReady?.({ port: server.address().port, stop, onEvent: ingest, status });
    statusTimer = setInterval(() => { publishStatus().catch(e => { fatal ??= e; stop(); }); }, 1000);
    if (duration) durationTimer = setTimeout(stop, duration * 1000);
    while (!stopped) {
      try { await fs.access(path.join(cache, 'STOP')); stop(); break; } catch (e) { if (e.code !== 'ENOENT') throw e; }
      let found;
      if (tasks.size < concurrency) for (let n = 0; n < states.length; n++) { const state = states[cursor++ % states.length]; if (!state.pending) { found = state; break; } }
      if (!found) { if (tasks.size) await Promise.race([...tasks]); else await sleep(10); continue; }
      found.pending = true; stats.scheduled++;
      const task = (async () => { let receipt;
        try { receipt = await client.book(found.identity.assetId, { signal: controller.signal }); }
        catch (e) { if (e.fatal) throw e; if (!stopped) { stats.errors++; stats.lastError = String(e.message).slice(0, 256); if (e.receipt) await journal.append({ identity: found.identity, receipt: e.receipt, invalid: e.message }); } return; }
        await ingest({ identity: found.identity, receipt });
      })().catch(e => { fatal ??= e; stop(); }).finally(() => { found.pending = false; tasks.delete(task); });
      tasks.add(task); stats.maximumInFlight = Math.max(stats.maximumInFlight, tasks.size);
    }
  } finally {
    stop(); clearInterval(statusTimer); clearTimeout(durationTimer); process.removeListener('SIGINT', stop); process.removeListener('SIGTERM', stop);
    try { await Promise.all([...tasks]); await backoffTail; await journal.close(); await publishStatus(); }
    finally { try { if (server) { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); } } finally { await release(); } }
  }
  if (fatal) throw fatal; return status();
}
function args(argv) { const out = {}; for (let i = 0; i < argv.length; i += 2) { if (!['--cache', '--universe', '--rps', '--concurrency', '--port', '--duration-seconds'].includes(argv[i]) || argv[i + 1] === undefined) throw Error('POLYMARKET_ARGUMENT'); out[argv[i].slice(2)] = argv[i + 1]; } return out; }
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) run(args(process.argv.slice(2))).catch(e => { process.stderr.write(e.message + '\n'); process.exitCode = 1; });
