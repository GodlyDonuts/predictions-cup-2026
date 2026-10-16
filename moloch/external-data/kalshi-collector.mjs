import fs from 'node:fs/promises';
import { createWriteStream } from 'node:fs';
import path from 'node:path';
import http from 'node:http';
import crypto from 'node:crypto';
import { createGzip } from 'node:zlib';
import { once } from 'node:events';
import { performance } from 'node:perf_hooks';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { ReadClient, credentials, normalizeBook, TICKER } from './kalshi-client.mjs';
import { loadUniverse, metadataIdentitySha256 } from './kalshi-universe.mjs';

const publications = new Map();
export async function atomicJson(file, data) {
  const key = path.resolve(file), previous = publications.get(key) ?? Promise.resolve();
  const operation = previous.catch(() => {}).then(() => publishJson(key, data)); publications.set(key, operation);
  try { await operation; } finally { if (publications.get(key) === operation) publications.delete(key); }
}
async function publishJson(file, data) {
  const tmp = file + '.' + crypto.randomUUID() + '.tmp';
  try {
    await fs.writeFile(tmp, JSON.stringify(data) + '\n', { mode: 0o600 });
    for (let attempt = 0; ; attempt++) {
      try { await fs.rename(tmp, file); break; }
      catch (error) {
        if (process.platform !== 'win32' || !['EPERM', 'EBUSY', 'EACCES'].includes(error.code) || attempt >= 9) throw error;
        await sleep(8 * 2 ** attempt);
      }
    }
  } catch (error) { await fs.unlink(tmp).catch(() => {}); throw error; }
}
export async function acquire(cache) {
  await fs.mkdir(cache, { recursive: true, mode: 0o700 });
  const name = path.join(cache, 'collector.lock'); const identity = crypto.randomUUID();
  for (let pass = 0; pass < 2; pass++) {
    try {
      const fd = await fs.open(name, 'wx', 0o600); await fd.writeFile(JSON.stringify({ pid: process.pid, identity, at: new Date().toISOString() }));
      return async () => { await fd.close(); const old = JSON.parse(await fs.readFile(name, 'utf8')); if (old.identity !== identity) throw Error('KALSHI_LOCK_CHANGED'); await fs.unlink(name); };
    } catch (error) {
      if (error.code !== 'EEXIST' || pass) throw Error('KALSHI_COLLECTOR_LOCKED');
      let guard;
      try { guard = await fs.open(name + '.reclaim', 'wx', 0o600); }
      catch { throw Error('KALSHI_LOCK_RECLAIM_BUSY'); }
      try {
        const bytes = await fs.readFile(name, 'utf8'); const old = JSON.parse(bytes);
        if (!Number.isSafeInteger(old.pid) || old.pid < 1) throw Error('KALSHI_LOCK_UNVERIFIABLE');
        try { process.kill(old.pid, 0); throw Error('KALSHI_COLLECTOR_ALREADY_RUNNING'); }
        catch (e) { if (e.code !== 'ESRCH') throw Error('KALSHI_COLLECTOR_ALREADY_RUNNING'); }
        if (await fs.readFile(name, 'utf8') !== bytes) throw Error('KALSHI_LOCK_CHANGED'); await fs.unlink(name);
      } finally { await guard.close(); await fs.unlink(name + '.reclaim'); }
    }
  }
}
export class Journal {
  constructor(root) { this.root = root; this.hour = null; this.gz = null; this.error = null; this.pending = Promise.resolve(); }
  async prune() {
    const files = [];
    for (const name of await fs.readdir(this.root)) {
      if (!/^\d{8}T\d{2}-[a-f0-9-]+\.ndjson\.gz$/.test(name)) continue;
      const file = path.join(this.root, name); const stat = await fs.lstat(file);
      if (stat.isFile() && !stat.isSymbolicLink()) files.push({ file, bytes: stat.size, time: stat.mtimeMs });
    }
    let bytes = files.reduce((n, x) => n + x.bytes, 0);
    for (const file of files.sort((a, b) => a.time - b.time)) {
      if (bytes <= 2 * 1024 ** 3 && Date.now() - file.time <= 72 * 60 * 60 * 1000) break;
      await fs.unlink(file.file); bytes -= file.bytes;
    }
  }
  async append(row) {
    // Serialized output preserves receipt-completion order, bounded by the
    // collector's64 outstanding tasks. Full original response text is retained.
    const operation = this.pending.then(async () => {
      if (this.error) throw this.error;
      const hour = new Date().toISOString().slice(0, 13).replace(/[:-]/g, '');
      if (this.hour !== hour || this.output?.bytesWritten >= 64 * 1024 ** 2) {
        await this.closeStream(); await this.prune(); this.hour = hour;
        const output = createWriteStream(path.join(this.root, `${hour}-${crypto.randomUUID()}.ndjson.gz`), { flags: 'wx', mode: 0o600 });
        this.gz = createGzip({ level: 1 });
        output.on('error', () => { this.error = Error('KALSHI_JOURNAL_IO'); this.gz?.destroy(this.error); });
        this.gz.on('error', () => { this.error = Error('KALSHI_JOURNAL_IO'); }); this.gz.pipe(output);
        this.output = output;
      }
      if (!this.gz.write(JSON.stringify(row) + '\n')) await once(this.gz, 'drain');
    }); this.pending = operation.catch(() => {}); return operation;
  }
  async closeStream() {
    const gz = this.gz, output = this.output; this.gz = null; this.output = null;
    if (gz && !output.closed) { const done = once(output, 'close'); gz.end(); await done; }
    if (this.error) throw this.error;
  }
  async close() { await this.pending; await this.closeStream(); if (this.error) throw this.error; }
}
function args(argv) {
  const out = {}; for (let i = 0; i < argv.length; i += 2) { if (!argv[i]?.startsWith('--') || argv[i + 1] === undefined) throw Error('KALSHI_ARGUMENT'); out[argv[i].slice(2)] = argv[i + 1]; } return out;
}
function validMetadata(state, market) {
  if (market?.ticker !== state.ticker || typeof market.event_ticker !== 'string') return false;
  if (state.metadataIdentitySha256) return state.verifiedElectionYear === 2026 && metadataIdentitySha256(market) === state.metadataIdentitySha256;
  return /(?:^|-)26(?:[A-Z]{3})?(?:-|$)/.test(market.event_ticker);
}
function retainedReceipt(row) {
  return row?.status === 200 && typeof row.bodyText === 'string' && crypto.createHash('sha256').update(row.bodyText).digest('hex') === row.bodySha256 && JSON.stringify(JSON.parse(row.bodyText)) === JSON.stringify(row.body) && Number.isFinite(Date.parse(row.receivedAt)) && Date.parse(row.receivedAt) <= Date.now();
}
export async function run(options) {
  const repositoryRoot = path.resolve(options['repository-root'] ?? path.join(path.dirname(fileURLToPath(import.meta.url)), '../..'));
  const cache = path.resolve(options.cache ?? path.join(process.env.LOCALAPPDATA ?? repositoryRoot, 'Moloch', 'kalshi-live', 'v1'));
  const env = path.resolve(options['env-file'] ?? path.join(repositoryRoot, '.env'));
  const port = Number(options.port ?? 17841), maxInFlight = Number(options['max-in-flight'] ?? 64);
  const duration = Number(options['duration-seconds'] ?? 0), maximumMarkets = Number(options['maximum-markets'] ?? 1024);
  if (!Number.isSafeInteger(port) || port < 1024 || port > 65535 || !Number.isSafeInteger(maxInFlight) || maxInFlight < 1 || maxInFlight > 128 || !Number.isFinite(duration) || duration < 0) throw Error('KALSHI_CONFIGURATION');
  const useWebsocket = options.websocket === 'true';
  if (options.websocket !== undefined && !['true', 'false'].includes(options.websocket)) throw Error('KALSHI_WEBSOCKET_CONFIGURATION');
  const release = await acquire(cache); let server, statusTimer, stopTimer, journal, shadow, shadowTask; let stopped = false; let fatal;
  const tasks = new Set(); const latest = new Map(); const startedAt = new Date().toISOString();
  const stats = { attempts: 0, successfulBooks: 0, successfulMetadata: 0, errors: 0, throttles: 0, maximumInFlightObserved: 0, lastError: null, websocket: { enabled: useWebsocket, state: useWebsocket ? 'initializing' : 'disabled', frames: 0, snapshots: 0, deltas: 0, trades: 0, tickers: 0, lastReceiptAt: null } };
  let client, universe, states, initialized = false;
  const stop = () => { stopped = true; }; process.once('SIGINT', stop); process.once('SIGTERM', stop);
  try {
    for (const dir of ['latest', 'metadata', 'journal']) await fs.mkdir(path.join(cache, dir), { recursive: true, mode: 0o700 });
    let backoffWrite = Promise.resolve(), maximumBackoff = 0;
    const auth = await credentials(env); client = new ReadClient(auth, { onBackoff: value => {
      maximumBackoff = Math.max(maximumBackoff, Date.parse(value.blockedUntil));
      backoffWrite = backoffWrite.then(() => atomicJson(path.join(cache, 'backoff.json'), { ...value, blockedUntil: new Date(maximumBackoff).toISOString() }));
      return backoffWrite;
    } });
    // Original rate limits/costs are fetched once, through the same admission
    // queue. No independent probe or background worker spends this budget.
    for (const file of ['status.json', 'backoff.json']) {
      try { const previous = JSON.parse(await fs.readFile(path.join(cache, file), 'utf8')); const deadline = Date.parse(previous.blockedUntil); if (typeof previous.blockedUntil !== 'string' || !Number.isFinite(deadline)) throw Error('KALSHI_BACKOFF_STATE'); maximumBackoff = Math.max(maximumBackoff, deadline); const wait = deadline - Date.now(); if (wait > 0) client.admission.backoff(wait); } catch (e) { if (e.code !== 'ENOENT') throw e; }
    }
    const startup = await client.initialize();
    await atomicJson(path.join(cache, 'admission-profile.json'), { at: new Date().toISOString(), limits: startup.limits, costs: startup.costs });
    universe = loadUniverse({ repositoryRoot, maximumTickers: maximumMarkets });
    if (!universe.markets.length) throw Error('KALSHI_EMPTY_UNIVERSE');
    stats.coverage = universe.summary;
    states = [...universe.markets, ...(universe.candidateAnalogMarkets ?? [])].map(market => ({ ...market, pending: false, due: 0, metadata: false, failures: 0, lastStarted: null, lastReceived: null, lastScheduled: 0 }));
    // Original verified receipts retain their clocks. Restart never renews a
    // cached quote, and unverified/changed metadata receives a new real GET.
    for (const state of states) {
      try {
        const row = JSON.parse(await fs.readFile(path.join(cache, 'metadata', state.ticker + '.json'), 'utf8'));
        if (retainedReceipt(row) && validMetadata(state, row.body.market) && Date.now() - Date.parse(row.receivedAt) < 6 * 60 * 60 * 1000 && ['active', 'open'].includes(row.body.market.status)) { state.metadata = true; state.metadataRefreshAt = Date.parse(row.receivedAt) + 6 * 60 * 60 * 1000; }
      } catch (error) { if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error; }
      try {
        const row = JSON.parse(await fs.readFile(path.join(cache, 'latest', state.ticker + '.json'), 'utf8'));
        if (row.ticker === state.ticker && retainedReceipt(row) && JSON.stringify(row.sigMarketIds) === JSON.stringify(state.sigMarketIds)) latest.set(state.ticker, { ...row, mappingKind: state.mappingKind ?? 'direct_party_reference', pricingAuthority: false, ruleMismatch: state.ruleMismatch ?? null, normalized: normalizeBook(row.body) });
      } catch (error) { if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error; }
    }
    await atomicJson(path.join(cache, 'universe.json'), universe); journal = new Journal(path.join(cache, 'journal'));
    if (useWebsocket) {
      const { ShadowFeed } = await import('./kalshi-websocket.mjs');
      shadow = new ShadowFeed({ auth, tickers: states.map(s => s.ticker),
        onReceipt: async receipt => {
          await journal.append({ kind: 'websocket', ...receipt });
          const s = stats.websocket; s.frames++; s.lastReceiptAt = receipt.receivedAt; s.state = 'receiving';
          const name = { orderbook_snapshot: 'snapshots', orderbook_delta: 'deltas', trade: 'trades', ticker: 'tickers' }[receipt.type]; if (name) s[name]++;
        },
        onStatus: value => { Object.assign(stats.websocket, { state: value.state, reason: value.reason, reconnectAttempt: value.attempt, delayMs: value.delayMs }); }
      });
      // Failure of the extra public data stream does not stop the independently
      // budgeted REST cache or grant pricing authority. Consumers inspect validity.
      shadowTask = shadow.run().catch(() => { stats.websocket.state = 'failed'; stats.websocket.reason = 'KALSHI_WS_FAILED'; });
    }
    const shadowQuotes = () => states.filter(s => s.metadata).map(s => {
      const view = shadow?.state?.view(s.ticker, Date.now());
      return view && { ...view, sigMarketIds: s.sigMarketIds, mappingKind: s.mappingKind ?? 'direct_party_reference', ruleMismatch: s.ruleMismatch ?? null, resolutionEquivalenceVerified: false };
    }).filter(Boolean);
    let sequence = 0;
    const status = () => {
      const at = Date.now(); const monotonic = performance.now(); const ages = [...latest.values()].map(x => at - Date.parse(x.receivedAt));
      const active = states.filter(s => s.metadata).length;
      return { schemaVersion: 1, at: new Date(at).toISOString(), pid: process.pid, startedAt, state: fatal ? 'failed' : stopped ? 'stopped' : initialized ? 'collecting' : 'initializing', origin: 'kalshi_official_v2', readOnly: true, targetRequestsPerSecond: 20, targetStartIntervalMs: 50, rollingRequestCeiling: 20, totalMappedTickers: states.length, activeTickers: active, successfullyCachedTickers: latest.size, unmappedSigMarketIds: universe.unmappedSigMarketIds, idealBookRotationSeconds: active / 20, inFlight: tasks.size, observedStartsLastSecond: client.admission.starts.filter(t => t > monotonic - 1000).length, observedStartsLastMinute: client.admission.starts.filter(t => t > monotonic - 60_000).length, oldestCachedReceiptAgeMs: ages.length ? Math.max(...ages) : null, blockedUntil: new Date(at + Math.max(0, client.admission.blockedUntil - monotonic)).toISOString(), ...stats, websocket: { ...stats.websocket, coverage: shadow?.status(at) ?? null }, fatalCode: fatal?.message ?? null };
    };
    let discoveryPending = false;
    server = http.createServer((req, res) => {
      res.setHeader('content-type', 'application/json'); res.setHeader('cache-control', 'no-store');
      if (req.method !== 'GET') { res.writeHead(405).end('{}'); return; }
      const url = new URL(req.url, 'http://127.0.0.1');
      if (url.pathname === '/v1/discover') {
        const suffix = url.searchParams.get('path');
        if (typeof suffix !== 'string' || suffix.length > 8192 || !/^\/(series|events|markets)(?:[/?]|$)/.test(suffix)) { res.writeHead(400).end('{}'); return; }
        if (stopped || discoveryPending || tasks.size >= maxInFlight) { res.writeHead(409).end(JSON.stringify({ code: 'DISCOVERY_BUSY' })); return; }
        discoveryPending = true;
        const task = (async () => {
          try { const receipt = await client.get(suffix); await journal.append({ kind: 'discovery', ...receipt }); res.end(JSON.stringify(receipt)); }
          catch (error) { res.writeHead(503).end(JSON.stringify({ code: error.code ?? 'DISCOVERY_FAILED' })); }
          finally { discoveryPending = false; }
        })().finally(() => tasks.delete(task)); tasks.add(task); return;
      }
      if (url.pathname === '/v1/status') { res.end(JSON.stringify(status())); return; }
      if (url.pathname === '/v1/ws/quotes') { res.end(JSON.stringify({ at: new Date().toISOString(), source: 'kalshi_official_ws_v2', pricingAuthority: false, websocket: stats.websocket, markets: shadowQuotes() })); return; }
      if (url.pathname === '/v1/quotes') { res.end(JSON.stringify({ at: new Date().toISOString(), source: 'kalshi_official_v2', resolutionEquivalenceVerified: false, mappingStatus: 'conditional', markets: [...latest.values()].map(x => ({ ticker: x.ticker, sigMarketIds: x.sigMarketIds, mappingKind: x.mappingKind, pricingAuthority: false, ruleMismatch: x.ruleMismatch, requestStartedAt: x.requestStartedAt, receivedAt: x.receivedAt, bodySha256: x.bodySha256, normalized: x.normalized })) })); return; }
      if (url.pathname.startsWith('/v1/book/')) { const ticker = url.pathname.slice('/v1/book/'.length); const row = TICKER.test(ticker) && latest.get(ticker); if (row) { res.end(JSON.stringify(row)); return; } }
      res.writeHead(404).end('{}');
    });
    server.listen(port, '127.0.0.1'); await once(server, 'listening');
    const persist = async () => {
      if (fatal) return;
      try { await atomicJson(path.join(cache, 'status.json'), status()); if (journal.error) throw journal.error; try { await fs.access(path.join(cache, 'STOP')); stopped = true; } catch (e) { if (e.code !== 'ENOENT') throw e; } }
      catch { fatal = Error('KALSHI_LOCAL_IO'); stopped = true; }
    };
    let persisting = false;
    statusTimer = setInterval(() => { if (!persisting) { persisting = true; void persist().finally(() => { persisting = false; }); } }, 1000);
    if (duration) stopTimer = setTimeout(stop, duration * 1000);
    const execute = async state => {
      const kind = state.metadata && Date.now() < state.metadataRefreshAt ? 'book' : 'metadata'; stats.attempts++;
      try {
        const suffix = '/markets/' + state.ticker + (kind === 'book' ? '/orderbook?depth=20' : '');
        const receipt = await client.get(suffix); await journal.append({ kind, ticker: state.ticker, ...receipt });
        if (kind === 'metadata') {
          const market = receipt.body?.market;
          if (!validMetadata(state, market)) throw Error('KALSHI_METADATA_IDENTITY');
          await atomicJson(path.join(cache, 'metadata', state.ticker + '.json'), receipt);
          stats.successfulMetadata++; state.metadata = ['active', 'open'].includes(market.status); state.metadataRefreshAt = Date.now() + 6 * 60 * 60 * 1000; state.due = state.metadata ? 0 : state.metadataRefreshAt;
        } else {
          const normalized = normalizeBook(receipt.body);
          if (receipt.body.market_ticker && receipt.body.market_ticker !== state.ticker) throw Error('KALSHI_BOOK_IDENTITY');
          const row = { schemaVersion: 1, ticker: state.ticker, sigMarketIds: state.sigMarketIds, mappingKind: state.mappingKind ?? 'direct_party_reference', pricingAuthority: false, ruleMismatch: state.ruleMismatch ?? null, resolutionEquivalenceVerified: false, mappingStatus: 'conditional', ...receipt, normalized };
          await atomicJson(path.join(cache, 'latest', state.ticker + '.json'), row); latest.set(state.ticker, row);
          stats.successfulBooks++; state.lastStarted = receipt.requestStartedAt; state.lastReceived = receipt.receivedAt;
          state.due = Date.parse(receipt.requestStartedAt) + states.filter(s => s.metadata).length * 50;
        }
        state.failures = 0;
      } catch (error) {
        stats.errors++; state.failures++; stats.lastError = { at: new Date().toISOString(), ticker: state.ticker, code: error.code ?? error.message ?? 'unknown' };
        if (error.code === 429) stats.throttles++;
        if ([401, 403, 'EIO', 'ENOSPC', 'EACCES', 'EPERM', 'EMFILE'].includes(error.code) || /^(KALSHI_(LOCAL|JOURNAL|ENV|KEY|CREDENTIAL))/.test(error.message)) { fatal = Error('KALSHI_FATAL_' + (error.code ?? 'LOCAL_IO')); stopped = true; }
        state.due = Date.now() + (error.code === 404 ? 6 * 60 * 60 * 1000 : Math.min(60_000, 1000 * 2 ** Math.min(state.failures, 6)));
      } finally { state.pending = false; }
    };
    initialized = true; await persist();
    while (!stopped) {
      if (tasks.size >= maxInFlight) { await Promise.race(tasks); continue; }
      const at = Date.now(); const state = states.filter(s => !s.pending && s.due <= at).sort((a, b) => a.due - b.due || a.lastScheduled - b.lastScheduled)[0];
      if (!state) { await sleep(10); continue; }
      state.pending = true; state.lastScheduled = ++sequence;
      const task = execute(state).finally(() => tasks.delete(task)); tasks.add(task);
      stats.maximumInFlightObserved = Math.max(stats.maximumInFlightObserved, tasks.size);
      // Admission itself sets actual start pacing. This yield merely bounds
      // scheduler work; it does not create an independent per-worker allowance.
      await sleep(1);
    }
    await Promise.allSettled(tasks); clearInterval(statusTimer); statusTimer = null;
    shadow?.stop(); if (shadowTask) await shadowTask;
    await journal.close(); journal = null; await atomicJson(path.join(cache, 'status.json'), status());
    if (fatal) throw fatal;
  } finally {
    stopped = true; if (statusTimer) clearInterval(statusTimer); if (stopTimer) clearTimeout(stopTimer);
    shadow?.stop();
    try { if (shadowTask) await shadowTask; await Promise.allSettled(tasks); if (journal) await journal.close(); }
    finally {
      try { if (server) await new Promise(resolve => server.close(resolve)); }
      finally { process.off('SIGINT', stop); process.off('SIGTERM', stop); await release(); }
    }
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  run(args(process.argv.slice(2))).catch(error => { console.error(JSON.stringify({ state: 'failed', code: /^KALSHI_[A-Z0-9_]+$/.test(error.message) ? error.message : 'KALSHI_STARTUP_OR_IO', at: new Date().toISOString() })); process.exitCode = 1; });
}
