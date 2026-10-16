import fs from 'node:fs/promises';
import crypto from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { setTimeout as sleep } from 'node:timers/promises';

export const ORIGIN = 'https://external-api.kalshi.com';
export const TICKER = /^[A-Z0-9][A-Z0-9_-]{0,127}$/;
export function readPath(suffix) {
  if (typeof suffix !== 'string' || !/^\/(account\/(limits|endpoint_costs)|series|events|markets(?:\/[A-Za-z0-9_-]+(?:\/orderbook)?)?)(\?|$)/.test(suffix)) throw Error('KALSHI_READ_PATH');
  const url = new URL('/trade-api/v2' + suffix, ORIGIN);
  if (url.origin !== ORIGIN || url.hash || url.username || url.password || url.pathname.includes('..')) throw Error('KALSHI_READ_PATH');
  const allowed = /^\/trade-api\/v2\/(account\/(limits|endpoint_costs)|series|events|markets(?:\/[A-Za-z0-9_-]+(?:\/orderbook)?)?)$/;
  if (!allowed.test(url.pathname)) throw Error('KALSHI_READ_PATH');
  return url;
}
export async function credentials(envFile) {
  const text = await fs.readFile(envFile, 'utf8'); const env = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim(); if (!line || line.startsWith('#')) continue;
    const i = line.indexOf('='); if (i < 1) throw Error('KALSHI_ENV_FORMAT');
    const k = line.slice(0, i).trim(); let v = line.slice(i + 1).trim();
    if ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'"))) v = v.slice(1, -1);
    if (k.startsWith('KALSHI_')) env[k] = v;
  }
  if (!env.KALSHI_API_KEY_ID || !env.KALSHI_PRIVATE_KEY_PATH) throw Error('KALSHI_CREDENTIALS_MISSING');
  const key = crypto.createPrivateKey(await fs.readFile(env.KALSHI_PRIVATE_KEY_PATH));
  if (key.asymmetricKeyType !== 'ed25519') throw Error('KALSHI_KEY_TYPE');
  return { id: env.KALSHI_API_KEY_ID, key };
}
export function costTable(body) {
  if (!Number.isSafeInteger(body?.default_cost) || body.default_cost < 1) throw Error('KALSHI_ENDPOINT_COST_SCHEMA');
  const table = new Map();
  // Accept only an explicit documented list or method/path dictionary. Unknown
  // response shapes stop initialization rather than assume every request costs10.
  const supplied = ['endpoint_costs', 'costs', 'endpoints'].filter(k => Object.hasOwn(body, k));
  if (supplied.length > 1) throw Error('KALSHI_ENDPOINT_COST_SCHEMA');
  const entries = supplied.length ? body[supplied[0]] : [];
  if (Array.isArray(entries)) {
    for (const x of entries) {
      const method = x.method ?? x.http_method; const path = x.path ?? x.endpoint;
      const cost = x.cost ?? x.token_cost;
      if (typeof method !== 'string' || typeof path !== 'string' || !Number.isSafeInteger(cost) || cost < 1) throw Error('KALSHI_ENDPOINT_COST_SCHEMA');
      table.set(method.toUpperCase() + ' ' + path, cost);
    }
  } else if (entries && typeof entries === 'object') {
    for (const [key, value] of Object.entries(entries)) {
      const cost = typeof value === 'number' ? value : value?.cost;
      if (!Number.isSafeInteger(cost) || cost < 1) throw Error('KALSHI_ENDPOINT_COST_SCHEMA');
      table.set(key, cost);
    }
  } else throw Error('KALSHI_ENDPOINT_COST_SCHEMA');
  return { defaultCost: body.default_cost, table };
}
export class Admission {
  constructor({ rate = 200, capacity = 600, maximumRps = 20, clock = () => performance.now(), wait = sleep } = {}) {
    if (!(rate > 0 && capacity > 0 && maximumRps > 0 && maximumRps <= 20)) throw Error('KALSHI_RATE_CONFIGURATION');
    Object.assign(this, { rate, capacity, maximumRps, clock, wait });
    this.tokens = 0; this.last = clock(); this.next = this.last; this.blockedUntil = 0; this.tail = Promise.resolve(); this.starts = [];
  }
  configure(body) {
    const rate = body?.read?.refill_rate, capacity = body?.read?.bucket_capacity;
    if (!Number.isSafeInteger(rate) || !Number.isSafeInteger(capacity) || rate < 1 || capacity < 1) throw Error('KALSHI_LIMIT_SCHEMA');
    this.rate = Math.min(200, rate); this.capacity = Math.min(600, capacity); this.tokens = 0; this.last = this.clock();
  }
  reserve(cost) {
    const operation = this.tail.then(async () => {
      if (!Number.isSafeInteger(cost) || cost < 1 || cost > this.capacity) throw Error('KALSHI_REQUEST_COST');
      for (;;) {
        const at = this.clock(); if (!Number.isFinite(at) || at < this.last) throw Error('KALSHI_CLOCK_REGRESSION');
        this.tokens = Math.min(this.capacity, this.tokens + (at - this.last) * this.rate / 1000); this.last = at;
        const recent = this.starts.filter(t => t > at - 1000);
        const rollingDelay = recent.length >= this.maximumRps ? recent[recent.length - this.maximumRps] + 1000 - at : 0;
        const delay = Math.max(this.blockedUntil - at, this.next - at, rollingDelay, (cost - this.tokens) * 1000 / this.rate, 0);
        if (delay > 0.01) { await this.wait(Math.ceil(delay)); continue; }
        // Keep a scheduled phase, rather than adding Windows timer rounding
        // to every interval. The rolling window and actual token bucket still
        // prevent catch-up from creating an extra account allowance.
        this.tokens -= cost; this.next = Math.max(this.next + 1000 / this.maximumRps, at);
        this.starts.push(at); this.starts = this.starts.filter(t => t > at - 60_000); return at;
      }
    });
    this.tail = operation.catch(() => {}); return operation;
  }
  backoff(milliseconds) { this.blockedUntil = Math.max(this.blockedUntil, this.clock() + milliseconds); }
}
export class ReadClient {
  constructor(auth, { admission = new Admission(), fetcher = fetch, timeoutMs = 20_000, onBackoff } = {}) {
    Object.assign(this, { auth, admission, fetcher, timeoutMs, onBackoff }); this.costs = { defaultCost: 10, table: new Map() }; this.consecutive429 = 0;
  }
  requestCost(url) {
    const actual = url.pathname.replace('/trade-api/v2', '');
    const exact = this.costs.table.get('GET ' + url.pathname) ?? this.costs.table.get('GET ' + actual);
    if (exact !== undefined) return exact;
    const matches = [];
    for (const [key, value] of this.costs.table) {
      if (!key.startsWith('GET ')) continue;
      const route = key.slice(4).replace(/^\/trade-api\/v2/, '');
      const segments = route.split('/').map(s => /^\{[A-Za-z_][A-Za-z0-9_]*\}$/.test(s) ? '[^/]+' : s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
      if (new RegExp('^' + segments.join('/') + '$').test(actual)) matches.push(value);
    }
    if (new Set(matches).size > 1) throw Error('KALSHI_ENDPOINT_COST_AMBIGUOUS');
    return matches[0] ?? this.costs.defaultCost;
  }
  async get(suffix) {
    const url = readPath(suffix); await this.admission.reserve(this.requestCost(url));
    const requestStartedAt = new Date().toISOString(); const timestamp = String(Date.now());
    const signature = crypto.sign(null, Buffer.from(timestamp + 'GET' + url.pathname), this.auth.key).toString('base64');
    let response;
    try {
      response = await this.fetcher(url, { method: 'GET', redirect: 'error', signal: AbortSignal.timeout(this.timeoutMs), headers: { Accept: 'application/json', 'KALSHI-ACCESS-KEY': this.auth.id, 'KALSHI-ACCESS-TIMESTAMP': timestamp, 'KALSHI-ACCESS-SIGNATURE': signature } });
    } catch { throw Object.assign(Error('KALSHI_NETWORK'), { code: 'network' }); }
    if (!response.ok) {
      if (response.status === 429) {
        this.consecutive429++; const header = response.headers.get('retry-after');
        const parsed = header === null ? NaN : (/^\d+(\.\d+)?$/.test(header) ? Number(header) * 1000 : Date.parse(header) - Date.now());
        const delay = Math.max(1000, Number.isFinite(parsed) ? parsed : Math.min(60_000, 1000 * 2 ** Math.min(6, this.consecutive429 - 1)));
        this.admission.backoff(delay);
        if (this.onBackoff) await this.onBackoff({ at: new Date().toISOString(), blockedUntil: new Date(Date.now() + Math.max(0, this.admission.blockedUntil - this.admission.clock())).toISOString(), status: 429 });
      }
      await response.body?.cancel().catch(() => {});
      throw Object.assign(Error('KALSHI_HTTP_' + response.status), { code: response.status });
    }
    const reader = response.body.getReader(); const chunks = []; let size = 0;
    for (;;) { const { done, value } = await reader.read(); if (done) break; size += value.length; if (size > 4 * 1024 * 1024) { await reader.cancel(); throw Error('KALSHI_BODY_LIMIT'); } chunks.push(value); }
    const bodyText = Buffer.concat(chunks).toString('utf8'); let body;
    try { body = JSON.parse(bodyText); } catch { throw Object.assign(Error('KALSHI_JSON'), { code: 'json' }); }
    this.consecutive429 = 0;
    return { source: 'kalshi_official_v2', path: url.pathname + url.search, requestStartedAt, receivedAt: new Date().toISOString(), status: response.status, bodySha256: crypto.createHash('sha256').update(bodyText).digest('hex'), bodyText, body };
  }
  async initialize() {
    const read = async suffix => {
      for (let attempt = 0; ; attempt++) {
        try { return await this.get(suffix); }
        catch (error) {
          if (attempt >= 2 || !(['network', 'json', 429].includes(error.code) || Number(error.code) >= 500)) throw error;
          await sleep(500 * 2 ** attempt);
        }
      }
    };
    const limits = await read('/account/limits'); this.admission.configure(limits.body);
    const costs = await read('/account/endpoint_costs'); this.costs = costTable(costs.body);
    return { limits, costs };
  }
}
function decimal(value) {
  if (typeof value !== 'string' || !/^\d+(?:\.\d{1,6})?$/.test(value)) throw Error('KALSHI_DECIMAL');
  const [a, b = ''] = value.split('.'); return BigInt(a) * 1_000_000n + BigInt(b.padEnd(6, '0'));
}
function formatted(n) { return `${n / 1_000_000n}.${String(n % 1_000_000n).padStart(6, '0')}`; }
export function normalizeBook(body) {
  const book = body?.orderbook_fp; if (!book || !Array.isArray(book.yes_dollars) || !Array.isArray(book.no_dollars)) throw Error('KALSHI_BOOK_SCHEMA');
  const side = rows => {
    if (rows.length > 2000) throw Error('KALSHI_BOOK_LEVEL_LIMIT');
    return rows.map(row => {
      if (!Array.isArray(row) || row.length !== 2) throw Error('KALSHI_BOOK_LEVEL');
      const p = decimal(row[0]), n = decimal(row[1]); if (p <= 0n || p >= 1_000_000n || n <= 0n) throw Error('KALSHI_BOOK_LEVEL');
      return { price: formatted(p), quantity: formatted(n), p };
    }).sort((a, b) => a.p > b.p ? -1 : a.p < b.p ? 1 : 0);
  };
  const yes = side(book.yes_dollars), no = side(book.no_dollars);
  const bids = rows => rows.map(({ price, quantity }) => ({ price, quantity }));
  const asks = rows => rows.map(({ p, quantity }) => ({ price: formatted(1_000_000n - p), quantity }));
  if (yes.length && no.length && yes[0].p + no[0].p > 1_000_000n) throw Error('KALSHI_CROSSED_BOOK');
  return { yesBids: bids(yes), yesAsks: asks(no), noBids: bids(no), noAsks: asks(yes), sourceTimestamp: null, sourceTimestampAvailable: false };
}
