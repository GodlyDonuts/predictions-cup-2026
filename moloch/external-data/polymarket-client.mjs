// Public documentation: https://docs.polymarket.com/api-reference/rate-limits
// GET-only, no wallet, credentials, order routes or SIG account operations.
import crypto from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { setTimeout as sleep } from 'node:timers/promises';

export const GAMMA = 'https://gamma-api.polymarket.com';
export const CLOB = 'https://clob.polymarket.com';
export const ASSET = /^[0-9]{1,78}$/;
export const CONDITION = /^0x[0-9a-fA-F]{64}$/;
export const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');
export function assetId(value) {
  if (typeof value !== 'string' || !ASSET.test(value) || BigInt(value) <= 0n || BigInt(value) >= 1n << 256n || String(BigInt(value)) !== value) throw Error('POLYMARKET_ASSET');
  return value;
}
export function readUrl(origin, suffix) {
  if (![GAMMA, CLOB].includes(origin) || typeof suffix !== 'string' || !suffix.startsWith('/') || suffix.startsWith('//')) throw Error('POLYMARKET_READ_PATH');
  const u = new URL(suffix, origin);
  if (u.origin !== origin || u.hash || u.username || u.password) throw Error('POLYMARKET_READ_PATH');
  const keys = [...u.searchParams.keys()];
  if (new Set(keys).size !== keys.length) throw Error('POLYMARKET_READ_QUERY');
  const allowed = origin === CLOB && u.pathname === '/book' ? ['token_id']
    : origin === GAMMA && /^\/events\/slug\/[a-zA-Z0-9][a-zA-Z0-9_-]{0,255}$/.test(u.pathname) ? []
      : origin === GAMMA && u.pathname === '/public-search' ? ['q', 'limit_per_type']
        : origin === GAMMA && u.pathname === '/events' ? ['limit', 'offset', 'active', 'closed'] : null;
  if (!allowed || keys.some(k => !allowed.includes(k))) throw Error('POLYMARKET_READ_PATH');
  if (origin === CLOB) assetId(u.searchParams.get('token_id'));
  if (u.pathname === '/public-search' && !(u.searchParams.get('q')?.length > 0 && u.searchParams.get('q').length <= 256)) throw Error('POLYMARKET_SEARCH');
  for (const k of ['limit', 'limit_per_type', 'offset']) if (u.searchParams.has(k) && !/^\d{1,6}$/.test(u.searchParams.get(k))) throw Error('POLYMARKET_READ_QUERY');
  for (const k of ['active', 'closed']) if (u.searchParams.has(k) && !['true', 'false'].includes(u.searchParams.get(k))) throw Error('POLYMARKET_READ_QUERY');
  return u;
}
export class Admission {
  constructor({ rps = 30, clock = () => performance.now(), wait = sleep, maximumQueued = 256 } = {}) {
    if (!Number.isInteger(rps) || rps < 1 || rps > 30 || !Number.isInteger(maximumQueued) || maximumQueued < 1 || maximumQueued > 1024) throw Error('POLYMARKET_RATE_CONFIGURATION');
    Object.assign(this, { rps, clock, wait, maximumQueued }); this.next = clock(); this.last = this.next; this.starts = []; this.blockedUntil = 0; this.tail = Promise.resolve(); this.queued = 0; this.admitted = 0;
  }
  reserve(signal) {
    if (this.queued >= this.maximumQueued) return Promise.reject(Error('POLYMARKET_ADMISSION_QUEUE_BOUND'));
    this.queued++;
    const operation = this.tail.then(async () => {
      for (;;) {
        signal?.throwIfAborted(); const at = this.clock();
        if (!Number.isFinite(at) || at < this.last) throw Error('POLYMARKET_CLOCK_REGRESSION');
        this.last = at; this.starts = this.starts.filter(t => t > at - 1000);
        const rolling = this.starts.length >= this.rps ? this.starts[0] + 1000 - at : 0;
        const delay = Math.max(this.next - at, this.blockedUntil - at, rolling, 0);
        if (delay > 0) { await this.wait(Math.min(60000, Math.ceil(delay)), undefined, { signal }); continue; }
        this.starts.push(at); this.next = at + 1000 / this.rps; this.admitted++; return at;
      }
    }).finally(() => { this.queued--; });
    this.tail = operation.catch(() => {}); return operation;
  }
  backoff(ms) { if (!Number.isFinite(ms) || ms < 0) throw Error('POLYMARKET_BACKOFF'); this.blockedUntil = Math.max(this.blockedUntil, this.clock() + ms); }
}
// All default clients in this process share Gamma+CLOB admissions/backoff.
export const sharedAdmission = new Admission();
export class ReadClient {
  constructor({ admission = sharedAdmission, fetcher = fetch, timeoutMs = 15000, maximumBodyBytes = 4 * 1024 ** 2, onBackoff, onAdmission } = {}) {
    if (!Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 60000 || !Number.isInteger(maximumBodyBytes) || maximumBodyBytes < 1 || maximumBodyBytes > 16 * 1024 ** 2) throw Error('POLYMARKET_REQUEST_BOUND');
    Object.assign(this, { admission, fetcher, timeoutMs, maximumBodyBytes, onBackoff, onAdmission });
    this.stats = { admissions: 0, completions: 0, responses: 0, errors: 0, throttles: 0 }; this.consecutive429 = 0;
  }
  async get(origin, suffix, { signal } = {}) {
    const u = readUrl(origin, suffix); await this.admission.reserve(signal); signal?.throwIfAborted();
    const requestStartedAt = new Date().toISOString(); this.stats.admissions++; this.onAdmission?.({ requestStartedAt, endpoint: u.href });
    try {
      const timeout = AbortSignal.timeout(this.timeoutMs); const combined = signal ? AbortSignal.any([signal, timeout]) : timeout;
      const response = await this.fetcher(u, { method: 'GET', redirect: 'error', signal: combined, headers: { Accept: 'application/json' } });
      this.stats.responses++;
      if (response.status === 429 || response.status === 503) {
        this.consecutive429++; this.stats.throttles++;
        const h = response.headers.get('retry-after'); const parsed = /^\d+(?:\.\d+)?$/.test(h ?? '') ? Number(h) * 1000 : Date.parse(h) - Date.now();
        const delay = Math.max(1000, Number.isFinite(parsed) ? parsed : Math.min(60000, 1000 * 2 ** Math.min(6, this.consecutive429 - 1)));
        this.admission.backoff(delay);
        try { await this.onBackoff?.({ status: response.status, observedAt: new Date().toISOString(), blockedUntil: new Date(Date.now() + Math.max(0, this.admission.blockedUntil - this.admission.clock())).toISOString() }); }
        catch (error) { await response.body?.cancel().catch(() => {}); throw Object.assign(error, { fatal: true }); }
      }
      if (!response.body) throw Error('POLYMARKET_BODY_MISSING');
      const reader = response.body.getReader(), chunks = []; let bytes = 0;
      try {
        for (;;) { const { done, value } = await reader.read(); if (done) break; bytes += value.length; if (bytes > this.maximumBodyBytes) throw Error('POLYMARKET_BODY_BOUND'); chunks.push(value); }
      } catch (e) { await reader.cancel().catch(() => {}); throw e; }
      const bodyText = new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks)); const receivedAt = new Date().toISOString();
      const receipt = { source: origin === CLOB ? 'polymarket_clob_public' : 'polymarket_gamma_public', endpoint: u.href, requestStartedAt, receivedAt, status: response.status, bodyText, bodySha256: sha256(bodyText), bodyBytes: bytes, sourceTimestamp: null };
      if (!response.ok) throw Object.assign(Error('POLYMARKET_HTTP_' + response.status), { code: response.status, receipt });
      try { receipt.body = JSON.parse(bodyText); } catch { throw Object.assign(Error('POLYMARKET_JSON'), { receipt }); }
      if (origin === CLOB && typeof receipt.body?.timestamp === 'string') receipt.sourceTimestamp = receipt.body.timestamp;
      this.consecutive429 = 0; return receipt;
    } catch (error) { this.stats.errors++; throw error; }
    finally { this.stats.completions++; }
  }
  getEvent(slug, options) { return this.get(GAMMA, '/events/slug/' + slug, options); }
  search(q, options) { return this.get(GAMMA, '/public-search?' + new URLSearchParams({ q, limit_per_type: '50' }), options); }
  book(id, options) { return this.get(CLOB, '/book?' + new URLSearchParams({ token_id: assetId(id) }), options); }
  listEvents({ limit = 100, offset = 0, active = true, closed = false } = {}, options) {
    if (!Number.isInteger(limit) || limit < 1 || limit > 500 || !Number.isInteger(offset) || offset < 0 || offset > 100000) throw Error('POLYMARKET_EVENTS_BOUND');
    return this.get(GAMMA, '/events?' + new URLSearchParams({ limit, offset, active, closed }), options);
  }
}
const SCALE = 10n ** 18n;
function decimal(s) {
  if (typeof s !== 'string' || !/^\d{1,30}(?:\.\d{1,18})?$/.test(s)) throw Error('POLYMARKET_DECIMAL');
  const [a, b = ''] = s.split('.'); return BigInt(a) * SCALE + BigInt(b.padEnd(18, '0'));
}
function text(n) { const fraction = String(n % SCALE).padStart(18, '0').replace(/0+$/, ''); return String(n / SCALE) + (fraction ? '.' + fraction : ''); }
export function normalizeBook(body, identity) {
  assetId(identity?.assetId);
  if (!CONDITION.test(identity?.conditionId ?? '') || body?.asset_id !== identity.assetId || body?.market?.toLowerCase() !== identity.conditionId.toLowerCase()) throw Error('POLYMARKET_BOOK_IDENTITY');
  if (typeof body.timestamp !== 'string' || !/^\d{1,16}$/.test(body.timestamp) || !Number.isSafeInteger(Number(body.timestamp)) || Number(body.timestamp) < 1 || !Number.isFinite(new Date(Number(body.timestamp)).getTime())) throw Error('POLYMARKET_BOOK_CLOCK');
  const levels = (rows, descending) => {
    if (!Array.isArray(rows) || rows.length > 2000) throw Error('POLYMARKET_DEPTH_BOUND');
    const seen = new Set(); return rows.map(row => {
      const p = decimal(row?.price), n = decimal(row?.size);
      if (p < 0n || p > SCALE || n <= 0n || seen.has(String(p))) throw Error('POLYMARKET_LEVEL'); seen.add(String(p));
      return { price: text(p), size: text(n), p };
    }).sort((a, b) => a.p === b.p ? 0 : (a.p < b.p ? -1 : 1) * (descending ? -1 : 1)).map(({ price, size }) => ({ price, size }));
  };
  const bids = levels(body.bids, true), asks = levels(body.asks, false);
  if (bids.length && asks.length && decimal(bids[0].price) > decimal(asks[0].price)) throw Error('POLYMARKET_CROSSED_BOOK');
  return { assetId: body.asset_id, conditionId: body.market, bids, asks, bestBid: bids[0] ?? null, bestAsk: asks[0] ?? null, sourceTimestamp: body.timestamp, sourceAt: new Date(Number(body.timestamp)).toISOString(), bookHash: typeof body.hash === 'string' ? body.hash : null, pricingAuthority: false, resolutionEquivalenceVerified: false };
}
