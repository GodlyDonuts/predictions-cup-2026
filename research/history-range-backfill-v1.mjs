import { createHash } from 'node:crypto';

export const HISTORY_CANDLE_RESOLUTIONS_MS_V1 = Object.freeze({
  '1m': 60_000,
  '5m': 300_000,
  '1h': 3_600_000,
  '1d': 86_400_000,
  '1w': 604_800_000,
});

export const HISTORY_CANDLE_PAGE_LIMIT_V1 = 1_000;
export const HISTORY_TRADE_PAGE_LIMIT_V1 = 200;

function fail(code) {
  throw new Error(code);
}

function stableJson(value) {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableJson).join(',')}]`;
  return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${stableJson(value[key])}`).join(',')}}`;
}

function sha256(value) {
  return createHash('sha256').update(typeof value === 'string' ? value : stableJson(value)).digest('hex');
}

function instantMs(value, code = 'HISTORY_TIME') {
  if (typeof value !== 'string' || !/^\d{4}-\d\d-\d\dT\d\d:\d\d(?::\d\d(?:\.\d{1,3})?)?(?:Z|[+-]\d\d:\d\d)$/.test(value)) fail(code);
  const ms = Date.parse(value);
  if (!Number.isSafeInteger(ms) || ms < 0) fail(code);
  return ms;
}

function iso(ms) {
  if (!Number.isSafeInteger(ms) || ms < 0) fail('HISTORY_TIME');
  return new Date(ms).toISOString();
}

function boundedLimit(value, maximum, fallback, code) {
  const n = value ?? fallback;
  if (!Number.isSafeInteger(n) || n < 1 || n > maximum) fail(code);
  return n;
}

function exchangeId(value) {
  if (typeof value !== 'string' || value.length === 0 || value.trim() !== value) fail('HISTORY_SCOPE');
  return value;
}

function tournamentId(value) {
  if (value === undefined || value === null) return null;
  if (typeof value !== 'string' || value.length === 0 || value.trim() !== value) fail('HISTORY_SCOPE');
  return value;
}

function assertPlan(plan, kind) {
  if (!plan || plan.schemaVersion !== 1 || plan.kind !== kind || !plan.scope
    || typeof plan.scopeHash !== 'string' || sha256({ kind, scope: plan.scope }) !== plan.scopeHash) fail('HISTORY_PLAN');
  try {
    exchangeId(plan.scope.exchangeId);
    tournamentId(plan.scope.tournamentId);
    if (kind === 'candles') {
      const bucketMs = HISTORY_CANDLE_RESOLUTIONS_MS_V1[plan.scope.resolution];
      const limit = boundedLimit(plan.scope.limit, HISTORY_CANDLE_PAGE_LIMIT_V1,
        HISTORY_CANDLE_PAGE_LIMIT_V1, 'HISTORY_CANDLE_LIMIT');
      const fromMs = instantMs(plan.scope.from), toMs = instantMs(plan.scope.to);
      if (!bucketMs || toMs <= fromMs || fromMs % bucketMs || toMs % bucketMs
        || plan.scope.limit !== limit || plan.scope.from !== iso(fromMs) || plan.scope.to !== iso(toMs)
        || plan.bucketMs !== bucketMs || plan.pageCount !== 1) fail('HISTORY_PLAN');
    } else {
      const limit = boundedLimit(plan.scope.limit, HISTORY_TRADE_PAGE_LIMIT_V1,
        HISTORY_TRADE_PAGE_LIMIT_V1, 'HISTORY_TRADE_LIMIT');
      const fromMs = instantMs(plan.scope.from), toMs = instantMs(plan.scope.to);
      if (toMs <= fromMs || plan.scope.limit !== limit
        || plan.scope.from !== iso(fromMs) || plan.scope.to !== iso(toMs)) fail('HISTORY_PLAN');
    }
  } catch (error) {
    if (error?.message === 'HISTORY_PLAN') throw error;
    fail('HISTORY_PLAN');
  }
}

function validateProjectedSequence(coverage) {
  if (!coverage || typeof coverage.complete !== 'boolean') fail('HISTORY_COVERAGE');
  const sequence = coverage.projectedThroughSequence;
  if (sequence !== undefined && sequence !== null && (!Number.isSafeInteger(sequence) || sequence < 0)) fail('HISTORY_COVERAGE');
}

function sameCanonical(a, b) {
  return stableJson(a) === stableJson(b);
}

function replaceMap(target, replacement) {
  target.clear();
  for (const [key, value] of replacement) target.set(key, value);
}

function candleRow(row, bucketMs) {
  if (!row || typeof row !== 'object' || Array.isArray(row)) fail('HISTORY_CANDLE_ROW');
  const timeMs = instantMs(row.time, 'HISTORY_CANDLE_TIME');
  if (timeMs % bucketMs !== 0) fail('HISTORY_CANDLE_ALIGNMENT');
  const result = {
    time: iso(timeMs),
    open: row.open,
    high: row.high,
    low: row.low,
    close: row.close,
    vwap: row.vwap,
    volume: row.volume,
    tradeCount: row.tradeCount,
  };
  for (const key of ['open', 'high', 'low', 'close', 'vwap']) {
    const n = result[key];
    if (n !== null && (!Number.isFinite(n) || n < 0 || n > 1)) fail('HISTORY_CANDLE_PRICE');
  }
  if (!Number.isFinite(result.volume) || result.volume < 0
    || !Number.isSafeInteger(result.tradeCount) || result.tradeCount < 0) fail('HISTORY_CANDLE_VOLUME');
  if (result.high !== null && result.low !== null && result.high < result.low) fail('HISTORY_CANDLE_OHLC');
  if (result.high !== null && result.low !== null) {
    for (const key of ['open', 'close', 'vwap']) {
      if (result[key] !== null && (result[key] < result.low || result[key] > result.high)) fail('HISTORY_CANDLE_OHLC');
    }
  }
  return { row: result, timeMs };
}

function tradeRow(row, fromMs, toMs) {
  if (!row || typeof row !== 'object' || Array.isArray(row)
    || (typeof row.id !== 'string' && typeof row.id !== 'number')) fail('HISTORY_TRADE_ROW');
  const id = String(row.id);
  if (!id || id.trim() !== id) fail('HISTORY_TRADE_ID');
  const createdAtMs = instantMs(row.createdAt, 'HISTORY_TRADE_TIME');
  if (createdAtMs < fromMs || createdAtMs >= toMs) fail('HISTORY_TRADE_WINDOW');
  if (row.price !== null && (!Number.isFinite(row.price) || row.price < 0 || row.price > 1)) fail('HISTORY_TRADE_PRICE');
  if (!Number.isSafeInteger(row.size) || row.size < 1) fail('HISTORY_TRADE_SIZE');
  if (row.side !== 'YES' && row.side !== 'NO') fail('HISTORY_TRADE_SIDE');
  if (!Number.isFinite(row.volume) || row.volume < 0) fail('HISTORY_TRADE_VOLUME');
  const normalized = { id, createdAt: iso(createdAtMs), price: row.price, size: row.size, side: row.side, volume: row.volume };
  return { row: normalized, createdAtMs, fingerprint: sha256(normalized) };
}

function candleScopeHash(scope) {
  return sha256({ kind: 'candles', scope });
}

function tradeScopeHash(scope) {
  return sha256({ kind: 'trades', scope });
}

/**
 * Build a deterministic explicit-range request. `from` is inclusive and `to`
 * is exclusive after both are floored to UTC bucket boundaries. The API's
 * returned `to` advances capped pages; a 1w bucket starts Thursday UTC.
 */
export function planCandleBackfillV1({ exchangeId: rawExchangeId, tournamentId: rawTournamentId = null,
  resolution, from, to, limit } = {}) {
  const exchange = exchangeId(rawExchangeId);
  const tournament = tournamentId(rawTournamentId);
  const bucketMs = HISTORY_CANDLE_RESOLUTIONS_MS_V1[resolution];
  if (!bucketMs) fail('HISTORY_CANDLE_RESOLUTION');
  const pageLimit = boundedLimit(limit, HISTORY_CANDLE_PAGE_LIMIT_V1, HISTORY_CANDLE_PAGE_LIMIT_V1, 'HISTORY_CANDLE_LIMIT');
  const fromMs = Math.floor(instantMs(from) / bucketMs) * bucketMs;
  const toMs = Math.floor(instantMs(to) / bucketMs) * bucketMs;
  if (toMs <= fromMs) fail('HISTORY_WINDOW');
  const scope = Object.freeze({ exchangeId: exchange, tournamentId: tournament, resolution,
    from: iso(fromMs), to: iso(toMs), limit: pageLimit });
  return Object.freeze({ schemaVersion: 1, kind: 'candles', scope, scopeHash: candleScopeHash(scope),
    bucketMs, pageCount: 1 });
}

export function candlePageV1(plan, index) {
  assertPlan(plan, 'candles');
  if (!Number.isSafeInteger(index) || index < 0 || index >= plan.pageCount) fail('HISTORY_PAGE_INDEX');
  return Object.freeze({ index, request: Object.freeze({ exchangeId: plan.scope.exchangeId,
    tournamentId: plan.scope.tournamentId, resolution: plan.scope.resolution,
    from: plan.scope.from, to: plan.scope.to, limit: plan.scope.limit }) });
}

/** Plan the documented cursor-paginated tape with an explicit time window. */
export function planTradeBackfillV1({ exchangeId: rawExchangeId, tournamentId: rawTournamentId = null,
  from, to, limit } = {}) {
  const exchange = exchangeId(rawExchangeId);
  const tournament = tournamentId(rawTournamentId);
  const fromMs = instantMs(from), toMs = instantMs(to);
  if (toMs <= fromMs) fail('HISTORY_WINDOW');
  const pageLimit = boundedLimit(limit, HISTORY_TRADE_PAGE_LIMIT_V1, HISTORY_TRADE_PAGE_LIMIT_V1, 'HISTORY_TRADE_LIMIT');
  const scope = Object.freeze({ exchangeId: exchange, tournamentId: tournament,
    from: iso(fromMs), to: iso(toMs), limit: pageLimit });
  return Object.freeze({ schemaVersion: 1, kind: 'trades', scope, scopeHash: tradeScopeHash(scope) });
}

export function tradePageV1(plan, cursor = null) {
  assertPlan(plan, 'trades');
  if (cursor !== null && (typeof cursor !== 'string' || cursor.length === 0)) fail('HISTORY_CURSOR');
  return Object.freeze({ exchangeId: plan.scope.exchangeId, tournamentId: plan.scope.tournamentId,
    from: plan.scope.from, to: plan.scope.to, limit: plan.scope.limit, cursor });
}

/**
 * Validate one candle response. Sparse empty pages are valid when complete is
 * true; no synthetic zero-price candles are created. If the API reports a
 * capped page, the documented continuation is response.to: the next candle's
 * start, exclusive. Sparse gaps therefore remain covered by that boundary.
 */
export function validateCandlePageV1({ plan, page, response, seenCandles = new Map() } = {}) {
  assertPlan(plan, 'candles');
  if (!(seenCandles instanceof Map) || !page || !Number.isSafeInteger(page.index)) fail('HISTORY_PAGE');
  const base = candlePageV1(plan, page.index);
  const request = page.request;
  if (!request || request.exchangeId !== base.request.exchangeId
    || request.tournamentId !== base.request.tournamentId || request.resolution !== base.request.resolution
    || request.to !== base.request.to || request.limit !== base.request.limit) fail('HISTORY_SCOPE_MISMATCH');
  const requestFromMs = instantMs(request.from);
  const baseFromMs = instantMs(base.request.from);
  const baseToMs = instantMs(base.request.to);
  if (requestFromMs < baseFromMs || requestFromMs >= baseToMs || requestFromMs % plan.bucketMs !== 0) fail('HISTORY_SCOPE_MISMATCH');
  if (!response || String(response.exchangeId) !== plan.scope.exchangeId
    || response.resolution !== plan.scope.resolution
    || (response.tournamentId !== undefined && response.tournamentId !== plan.scope.tournamentId)) fail('HISTORY_RESPONSE_SCOPE');
  const responseFromMs = instantMs(response.from), responseToMs = instantMs(response.to);
  if (responseFromMs !== requestFromMs || responseToMs <= responseFromMs || responseToMs > baseToMs
    || responseToMs % plan.bucketMs !== 0) fail('HISTORY_RESPONSE_WINDOW');
  validateProjectedSequence(response.coverage);
  if (!Array.isArray(response.candles)) fail('HISTORY_CANDLE_DATA');
  if (response.coverage.complete && responseToMs !== baseToMs) fail('HISTORY_CANDLE_INCOMPLETE');
  if (!response.coverage.complete) {
    if (responseToMs >= baseToMs) fail('HISTORY_CANDLE_NO_PROGRESS');
    if (response.candles.length !== request.limit) fail('HISTORY_CANDLE_INCOMPLETE');
  }
  const workingCandles = new Map(seenCandles);
  const accepted = [];
  let duplicates = 0, priorTime = -1;
  for (const raw of response.candles) {
    const { row, timeMs } = candleRow(raw, plan.bucketMs);
    if (timeMs < responseFromMs || timeMs >= responseToMs || timeMs <= priorTime) fail('HISTORY_CANDLE_CHRONOLOGY');
    priorTime = timeMs;
    const existing = workingCandles.get(row.time);
    if (existing !== undefined) {
      if (existing !== stableJson(row)) fail('HISTORY_CANDLE_CONFLICT');
      duplicates++;
      continue;
    }
    workingCandles.set(row.time, stableJson(row));
    accepted.push(row);
  }
  const pageSha256 = sha256(response.candles);
  replaceMap(seenCandles, workingCandles);
  return Object.freeze({ index: page.index, accepted, duplicates, complete: response.coverage.complete,
    continuationFrom: response.coverage.complete ? null : response.to,
    projectedThroughSequence: response.coverage.projectedThroughSequence,
    pageSha256 });
}

/**
 * Validate a newest-first trade-tape response. Exact duplicate rows are
 * deduped by all documented fields; legacy IDs can collide with engine
 * sequence IDs, so differing rows with the same string ID are retained.
 */
export function validateTradePageV1({ plan, cursor = null, response, seenTrades = new Map(),
  seenTradeIds = new Map(), seenCursors = [], previousOldestAt = null } = {}) {
  assertPlan(plan, 'trades');
  if (!(seenTrades instanceof Map) || !(seenTradeIds instanceof Map) || !Array.isArray(seenCursors)) fail('HISTORY_PAGE_STATE');
  if (cursor !== null && (typeof cursor !== 'string' || cursor.length === 0)) fail('HISTORY_CURSOR');
  if (cursor !== null && seenCursors.includes(cursor)) fail('HISTORY_CURSOR_REPEAT');
  if (!response || String(response.exchangeId) !== plan.scope.exchangeId
    || (response.tournamentId !== undefined && response.tournamentId !== plan.scope.tournamentId)
    || response.from !== plan.scope.from || response.to !== plan.scope.to) fail('HISTORY_RESPONSE_SCOPE');
  validateProjectedSequence(response.coverage);
  if (response.coverage.complete !== true) fail('HISTORY_TRADE_INCOMPLETE');
  if (!Array.isArray(response.data) || !response.pagination || typeof response.pagination.hasMore !== 'boolean') fail('HISTORY_TRADE_DATA');
  const requestLimit = plan.scope.limit;
  const returnedLimit = response.pagination.limit;
  if (!Number.isSafeInteger(returnedLimit) || returnedLimit < 1 || returnedLimit > requestLimit
    || response.data.length > returnedLimit) fail('HISTORY_TRADE_PAGE_LIMIT');
  const nextCursor = response.pagination.nextCursor;
  if (nextCursor !== null && (typeof nextCursor !== 'string' || nextCursor.length === 0)) fail('HISTORY_CURSOR');
  if (response.pagination.hasMore !== (nextCursor !== null)) fail('HISTORY_CURSOR_STATE');
  if (nextCursor !== null && (nextCursor === cursor || seenCursors.includes(nextCursor))) fail('HISTORY_CURSOR_REPEAT');
  const fromMs = instantMs(plan.scope.from), toMs = instantMs(plan.scope.to);
  const previousOldestMs = previousOldestAt === null ? null : instantMs(previousOldestAt, 'HISTORY_TRADE_CHRONOLOGY');
  const workingSeenTrades = new Map(seenTrades);
  const workingSeenTradeIds = new Map([...seenTradeIds].map(([id, fingerprints]) =>
    [id, fingerprints instanceof Set ? new Set(fingerprints) : fingerprints]));
  const accepted = [], pageSeen = new Set();
  let duplicates = 0, idCollisions = 0, priorTime = Infinity, oldestAt = previousOldestAt;
  for (const raw of response.data) {
    const parsed = tradeRow(raw, fromMs, toMs);
    if (parsed.createdAtMs > priorTime) fail('HISTORY_TRADE_CHRONOLOGY');
    priorTime = parsed.createdAtMs;
    const priorForId = workingSeenTradeIds.get(parsed.row.id);
    const fingerprintsForId = priorForId instanceof Set
      ? priorForId
      : priorForId === undefined ? new Set() : new Set([priorForId]);
    if (fingerprintsForId.size > 0 && !fingerprintsForId.has(parsed.fingerprint)) idCollisions++;
    fingerprintsForId.add(parsed.fingerprint);
    workingSeenTradeIds.set(parsed.row.id, fingerprintsForId);
    const priorFingerprint = workingSeenTrades.get(parsed.fingerprint);
    const alreadySeen = priorFingerprint !== undefined || pageSeen.has(parsed.fingerprint);
    if (alreadySeen) {
      const knownFingerprint = priorFingerprint ?? parsed.fingerprint;
      if (knownFingerprint !== parsed.fingerprint) fail('HISTORY_TRADE_CONFLICT');
      duplicates++;
      continue;
    }
    if (previousOldestMs !== null && parsed.createdAtMs > previousOldestMs) fail('HISTORY_TRADE_CHRONOLOGY');
    pageSeen.add(parsed.fingerprint);
    workingSeenTrades.set(parsed.fingerprint, parsed.fingerprint);
    accepted.push(parsed.row);
    if (oldestAt === null || parsed.createdAtMs < instantMs(oldestAt)) oldestAt = parsed.row.createdAt;
  }
  // Record only cursors already consumed. The returned next cursor must remain
  // eligible on the next call; storing it here would make resume reject itself.
  const cursorHistory = cursor === null ? [...seenCursors] : [...seenCursors, cursor];
  const pageSha256 = sha256(response.data);
  replaceMap(seenTrades, workingSeenTrades);
  replaceMap(seenTradeIds, workingSeenTradeIds);
  return Object.freeze({ accepted, duplicates, idCollisions, nextCursor,
    hasMore: response.pagination.hasMore, oldestAt,
    seenCursors: Object.freeze(cursorHistory),
    pageSha256 });
}

/** Create a portable checkpoint; expiry is optional because API cursor expiry is undocumented. */
export function createHistoryResumeManifestV1({ plan, completedPageCount, nextPageIndex, nextCursor = null,
  nextFrom = null, done = false, lastTradeAt = null, seenCursors = [], outputSha256 = null,
  savedAt, expiresAt = null } = {}) {
  if (!plan || !['candles', 'trades'].includes(plan.kind)) fail('HISTORY_PLAN');
  assertPlan(plan, plan.kind);
  if (!Number.isSafeInteger(completedPageCount) || completedPageCount < 0
    || !Number.isSafeInteger(nextPageIndex) || nextPageIndex < 0
    || typeof done !== 'boolean' || !Array.isArray(seenCursors)) fail('HISTORY_RESUME');
  const savedMs = instantMs(savedAt, 'HISTORY_RESUME_TIME');
  const expiryMs = expiresAt === null ? null : instantMs(expiresAt, 'HISTORY_RESUME_TIME');
  if (expiryMs !== null && expiryMs <= savedMs) fail('HISTORY_RESUME_EXPIRY');
  if (outputSha256 !== null && !/^[a-f0-9]{64}$/.test(outputSha256)) fail('HISTORY_RESUME_HASH');
  if (plan.kind === 'candles') {
    if (nextCursor !== null || nextPageIndex > plan.pageCount || completedPageCount !== nextPageIndex) fail('HISTORY_RESUME');
    if (nextFrom !== null) {
      const base = candlePageV1(plan, nextPageIndex);
      const fromMs = instantMs(nextFrom);
      if (nextPageIndex >= plan.pageCount || fromMs < instantMs(base.request.from)
        || fromMs >= instantMs(base.request.to) || fromMs % plan.bucketMs !== 0 || done) fail('HISTORY_RESUME');
    }
    if (done !== (nextPageIndex === plan.pageCount && nextFrom === null)) fail('HISTORY_RESUME');
  } else {
    if (nextFrom !== null || nextPageIndex !== completedPageCount
      || nextCursor !== null && (typeof nextCursor !== 'string' || !nextCursor)
      || done && (nextCursor !== null || completedPageCount < 1)
      || !done && completedPageCount > 0 && nextCursor === null
      || completedPageCount === 0 && (nextCursor !== null || seenCursors.length > 0)
      || completedPageCount > 0 && seenCursors.length !== completedPageCount - 1) fail('HISTORY_RESUME');
    if (new Set(seenCursors).size !== seenCursors.length) fail('HISTORY_RESUME');
    for (const c of seenCursors) if (typeof c !== 'string' || !c) fail('HISTORY_RESUME');
    if (nextCursor !== null && seenCursors.includes(nextCursor)) fail('HISTORY_RESUME');
    if (lastTradeAt !== null) instantMs(lastTradeAt, 'HISTORY_RESUME_TIME');
  }
  const manifest = {
    schemaVersion: 1, kind: plan.kind, scope: plan.scope, scopeHash: plan.scopeHash,
    completedPageCount, nextPageIndex, nextCursor, nextFrom, done,
    lastTradeAt, seenCursors: [...seenCursors], outputSha256,
    savedAt: iso(savedMs), expiresAt: expiryMs === null ? null : iso(expiryMs),
  };
  manifest.manifestSha256 = sha256(manifest);
  return Object.freeze(manifest);
}

export function validateHistoryResumeManifestV1({ plan, manifest, now, priorObservations = null } = {}) {
  if (!plan || !manifest || manifest.schemaVersion !== 1 || manifest.kind !== plan.kind
    || manifest.scopeHash !== plan.scopeHash || !sameCanonical(manifest.scope, plan.scope)) fail('HISTORY_RESUME_SCOPE');
  const { manifestSha256, ...unsigned } = manifest;
  if (typeof manifestSha256 !== 'string' || manifestSha256 !== sha256(unsigned)) fail('HISTORY_RESUME_HASH');
  const nowMs = now === undefined ? null : instantMs(now, 'HISTORY_RESUME_TIME');
  const expiryMs = manifest.expiresAt === null ? null : instantMs(manifest.expiresAt, 'HISTORY_RESUME_TIME');
  if (expiryMs !== null && (nowMs === null || nowMs >= expiryMs)) fail('HISTORY_RESUME_EXPIRED');
  if (manifest.outputSha256 !== null) {
    if (!Array.isArray(priorObservations) || sha256(priorObservations) !== manifest.outputSha256) fail('HISTORY_RESUME_OUTPUT_MISMATCH');
  } else if (Array.isArray(priorObservations) && priorObservations.length > 0) fail('HISTORY_RESUME_OUTPUT_MISMATCH');
  if (manifest.kind === 'candles') {
    if (manifest.lastTradeAt !== null) fail('HISTORY_RESUME_LAST_TRADE_AT');
  } else {
    const observations = priorObservations ?? [];
    if (!Array.isArray(observations)) fail('HISTORY_RESUME_OUTPUT_MISMATCH');
    let oldestMs = null;
    const fromMs = instantMs(plan.scope.from), toMs = instantMs(plan.scope.to);
    for (const observation of observations) {
      const parsed = tradeRow(observation, fromMs, toMs);
      if (oldestMs === null || parsed.createdAtMs < oldestMs) oldestMs = parsed.createdAtMs;
    }
    if (oldestMs === null ? manifest.lastTradeAt !== null
      : manifest.lastTradeAt === null || instantMs(manifest.lastTradeAt, 'HISTORY_RESUME_LAST_TRADE_AT') !== oldestMs) {
      fail('HISTORY_RESUME_LAST_TRADE_AT');
    }
  }
  // Re-run all structural invariants, including the candle page index/cursor state.
  const rebuilt = createHistoryResumeManifestV1({ plan, ...manifest, manifestSha256: undefined });
  if (rebuilt.manifestSha256 !== manifestSha256) fail('HISTORY_RESUME_HASH');
  return Object.freeze({ ...manifest });
}

export function historyObservationsSha256V1(observations) {
  if (!Array.isArray(observations)) fail('HISTORY_OBSERVATIONS');
  return sha256(observations);
}

/** Stable canonical identity for a documented trade row, including legacy IDs. */
export function historyTradeFingerprintV1(row) {
  return tradeRow(row, 0, Number.MAX_SAFE_INTEGER).fingerprint;
}

/** Canonical candle identity for deterministic checkpoint deduplication. */
export function historyCandleCanonicalV1(row) {
  if (!row || typeof row !== 'object' || Array.isArray(row)) fail('HISTORY_CANDLE_ROW');
  return stableJson(row);
}

