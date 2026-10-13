import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  candlePageV1,
  createHistoryResumeManifestV1,
  historyObservationsSha256V1,
  planCandleBackfillV1,
  planTradeBackfillV1,
  tradePageV1,
  validateCandlePageV1,
  validateHistoryResumeManifestV1,
  validateTradePageV1,
} from './history-range-backfill-v1.mjs';
import { runHistoryRangeBackfillOfflineV1 } from '../data/run-history-range-backfill-20261013.mjs';

const minute = 60_000;
const candle = (at, price = 0.5) => ({ time: at, open: price, high: price, low: price,
  close: price, vwap: price, volume: 4, tradeCount: 1 });
const trade = (id, at, price = 0.5) => ({ id, createdAt: at, price, size: 2, side: 'YES', volume: 1 });
const candleCoverage = complete => ({ complete, projectedThroughSequence: null });
const candleResponse = (plan, from, to, candles, complete, extras = {}) => ({
  exchangeId: plan.scope.exchangeId,
  tournamentId: plan.scope.tournamentId,
  resolution: plan.scope.resolution,
  from,
  to,
  candles,
  coverage: candleCoverage(complete),
  ...extras,
});
const tradeResponse = (plan, data, hasMore, nextCursor, limit = plan.scope.limit) => ({
  exchangeId: plan.scope.exchangeId,
  tournamentId: plan.scope.tournamentId,
  from: plan.scope.from,
  to: plan.scope.to,
  data,
  coverage: candleCoverage(true),
  pagination: { limit, hasMore, nextCursor },
});

function throwsCode(code, operation) {
  assert.throws(operation, error => error?.message === code);
}

function canonicalJson(value) {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(',')}}`;
}

test('candle plans floor UTC bounds and retain one response-driven range', () => {
  const plan = planCandleBackfillV1({ exchangeId: 'e1', resolution: '1m', limit: 10,
    from: '2026-01-01T00:00:45Z', to: '2026-01-01T00:16:59Z' });
  assert.equal(plan.scope.from, '2026-01-01T00:00:00.000Z');
  assert.equal(plan.scope.to, '2026-01-01T00:16:00.000Z');
  assert.equal(plan.pageCount, 1);
  assert.deepEqual(candlePageV1(plan, 0).request,
    { exchangeId: 'e1', tournamentId: null, resolution: '1m', from: '2026-01-01T00:00:00.000Z',
      to: '2026-01-01T00:16:00.000Z', limit: 10 });
  throwsCode('HISTORY_PAGE_INDEX', () => candlePageV1(plan, 1));
});

test('weekly candle buckets use the API Thursday UTC anchor', () => {
  const plan = planCandleBackfillV1({ exchangeId: 'weekly', resolution: '1w', limit: 1000,
    from: '2026-01-07T12:00:00Z', to: '2026-01-16T12:00:00Z' });
  assert.equal(plan.scope.from, '2026-01-01T00:00:00.000Z');
  assert.equal(plan.scope.to, '2026-01-15T00:00:00.000Z');
});

test('invalid candle scopes, ranges, and page sizes fail closed', () => {
  throwsCode('HISTORY_CANDLE_RESOLUTION', () => planCandleBackfillV1({ exchangeId: 'e', resolution: '2m',
    from: '2026-01-01T00:00:00Z', to: '2026-01-01T00:10:00Z' }));
  throwsCode('HISTORY_WINDOW', () => planCandleBackfillV1({ exchangeId: 'e', resolution: '1m',
    from: '2026-01-01T00:00:00Z', to: '2026-01-01T00:00:50Z' }));
  throwsCode('HISTORY_CANDLE_LIMIT', () => planCandleBackfillV1({ exchangeId: 'e', resolution: '1m', limit: 1001,
    from: '2026-01-01T00:00:00Z', to: '2026-01-02T00:00:00Z' }));
});

test('sparse complete candle responses stay sparse and empty pages do not invent candles', () => {
  const plan = planCandleBackfillV1({ exchangeId: 'sparse', resolution: '1m', limit: 10,
    from: '2026-01-01T00:00:00Z', to: '2026-01-01T00:10:00Z' });
  const page = candlePageV1(plan, 0);
  const sparse = validateCandlePageV1({ plan, page, response: candleResponse(plan,
    page.request.from, page.request.to, [candle('2026-01-01T00:02:00Z'), candle('2026-01-01T00:08:00Z')], true) });
  assert.equal(sparse.accepted.length, 2);
  const empty = validateCandlePageV1({ plan, page, response: candleResponse(plan,
    page.request.from, page.request.to, [], true) });
  assert.deepEqual(empty.accepted, []);
});

test('candle continuation uses response.to verbatim, including weekly bucket boundaries', () => {
  const week = 7 * 24 * 60 * minute;
  const from = Date.parse('2026-01-01T00:00:00Z');
  const plan = planCandleBackfillV1({ exchangeId: 'long', resolution: '1w', limit: 10,
    from: new Date(from).toISOString(), to: new Date(from + 25 * week).toISOString() });
  const page = candlePageV1(plan, 0);
  const responseTo = new Date(from + 11 * week).toISOString().replace('.000Z', '+00:00');
  const cappedRows = Array.from({ length: 10 }, (_, index) => candle(new Date(from + index * week).toISOString()));
  const result = validateCandlePageV1({ plan, page, response: candleResponse(plan,
    page.request.from, responseTo, cappedRows, false) });
  assert.equal(result.continuationFrom, responseTo);
  assert.equal(result.accepted.length, 10);
  const shortTruncated = candleResponse(plan, page.request.from, responseTo, cappedRows.slice(0, 9), false);
  const emptyTruncated = candleResponse(plan, page.request.from, responseTo, [], false);
  throwsCode('HISTORY_CANDLE_INCOMPLETE', () => validateCandlePageV1({ plan, page, response: shortTruncated }));
  throwsCode('HISTORY_CANDLE_INCOMPLETE', () => validateCandlePageV1({ plan, page, response: emptyTruncated }));
});

test('candle duplicates require identical canonical rows; conflicts and scope errors fail', () => {
  const plan = planCandleBackfillV1({ exchangeId: 'dedupe', resolution: '1m', limit: 10,
    from: '2026-01-01T00:00:00Z', to: '2026-01-01T00:10:00Z' });
  const page = candlePageV1(plan, 0);
  const row = candle('2026-01-01T00:02:00Z');
  const seenCandles = new Map();
  validateCandlePageV1({ plan, page, seenCandles,
    response: candleResponse(plan, page.request.from, page.request.to, [row], true) });
  const duplicate = validateCandlePageV1({ plan, page, seenCandles,
    response: candleResponse(plan, page.request.from, page.request.to, [row], true) });
  assert.equal(duplicate.duplicates, 1);
  throwsCode('HISTORY_CANDLE_CONFLICT', () => validateCandlePageV1({ plan, page, seenCandles,
    response: candleResponse(plan, page.request.from, page.request.to, [candle(row.time, 0.6)], true) }));
  throwsCode('HISTORY_RESPONSE_SCOPE', () => validateCandlePageV1({ plan, page, response: candleResponse(plan,
    page.request.from, page.request.to, [], true, { exchangeId: 'other' }) }));
  throwsCode('HISTORY_CANDLE_NO_PROGRESS', () => validateCandlePageV1({ plan, page,
    response: candleResponse(plan, page.request.from, page.request.to,
      Array.from({ length: 10 }, (_, index) => candle(`2026-01-01T00:${String(index).padStart(2, '0')}:00Z`)), false) }));
  throwsCode('HISTORY_CANDLE_CHRONOLOGY', () => validateCandlePageV1({ plan, page,
    response: candleResponse(plan, page.request.from, page.request.to,
      [candle('2026-01-01T00:03:00Z'), candle('2026-01-01T00:02:00Z')], true) }));
});

test('failed candle validation leaves dedupe state unchanged for a corrected retry', () => {
  const plan = planCandleBackfillV1({ exchangeId: 'atomic-candle', resolution: '1m', limit: 10,
    from: '2026-01-01T00:00:00Z', to: '2026-01-01T00:10:00Z' });
  const page = candlePageV1(plan, 0);
  const seenCandles = new Map([['existing-time', 'existing-row']]);
  const before = [...seenCandles];
  const malformed = candleResponse(plan, page.request.from, page.request.to,
    [candle('2026-01-01T00:02:00Z'), candle('2026-01-01T00:01:00Z')], true);
  throwsCode('HISTORY_CANDLE_CHRONOLOGY', () => validateCandlePageV1({ plan, page, response: malformed, seenCandles }));
  assert.deepEqual([...seenCandles], before);
  const corrected = validateCandlePageV1({ plan, page, seenCandles,
    response: candleResponse(plan, page.request.from, page.request.to,
      [candle('2026-01-01T00:01:00Z'), candle('2026-01-01T00:02:00Z')], true) });
  assert.equal(corrected.accepted.length, 2);
});

test('trade plans enforce an explicit window and the documented page-size ceiling', () => {
  const plan = planTradeBackfillV1({ exchangeId: 't1', from: '2026-01-01T00:00:00Z',
    to: '2026-01-02T00:00:00Z', limit: 200 });
  assert.equal(plan.scope.limit, 200);
  assert.equal(tradePageV1(plan, 'opaque::cursor/+').cursor, 'opaque::cursor/+');
  throwsCode('HISTORY_TRADE_LIMIT', () => planTradeBackfillV1({ exchangeId: 't1', limit: 201,
    from: '2026-01-01T00:00:00Z', to: '2026-01-02T00:00:00Z' }));
  throwsCode('HISTORY_WINDOW', () => planTradeBackfillV1({ exchangeId: 't1',
    from: '2026-01-02T00:00:00Z', to: '2026-01-01T00:00:00Z' }));
});

test('trade pagination preserves opaque cursors, exact boundary duplicates, and ID collisions', () => {
  const plan = planTradeBackfillV1({ exchangeId: 'tape', from: '2026-01-01T00:00:00Z',
    to: '2026-01-01T01:00:00Z', limit: 3 });
  const seenTrades = new Map(), seenTradeIds = new Map();
  const first = validateTradePageV1({ plan, seenTrades, seenTradeIds,
    response: tradeResponse(plan, [trade('engine-1', '2026-01-01T00:09:00Z'),
      trade('legacy-x', '2026-01-01T00:08:00Z', null)], true, 'opaque::page/2?x=+1', 2) });
  assert.equal(first.nextCursor, 'opaque::page/2?x=+1');
  assert.deepEqual(first.seenCursors, []);
  const second = validateTradePageV1({ plan, cursor: first.nextCursor, seenTrades, seenTradeIds,
    seenCursors: first.seenCursors, previousOldestAt: first.oldestAt,
    response: tradeResponse(plan, [trade('legacy-x', '2026-01-01T00:08:00Z', null),
      trade('legacy-x', '2026-01-01T00:07:00Z')], true, 'cursor 3', 2) });
  assert.equal(second.duplicates, 1);
  assert.equal(second.idCollisions, 1);
  assert.equal(second.accepted.length, 1);
  assert.equal(second.accepted[0].id, 'legacy-x');
  assert.equal(second.seenCursors[0], first.nextCursor);
  const third = validateTradePageV1({ plan, cursor: second.nextCursor, seenTrades, seenTradeIds,
    seenCursors: second.seenCursors, previousOldestAt: second.oldestAt,
    response: tradeResponse(plan, [], false, null, 1) });
  assert.equal(third.hasMore, false);
});

test('trade pagination rejects repeated cursors, empty nonterminal pages, bad order, and scope drift', () => {
  const plan = planTradeBackfillV1({ exchangeId: 'checks', from: '2026-01-01T00:00:00Z',
    to: '2026-01-01T01:00:00Z', limit: 2 });
  const row = trade('a', '2026-01-01T00:09:00Z');
  throwsCode('HISTORY_CURSOR_REPEAT', () => validateTradePageV1({ plan, cursor: 'same', seenCursors: ['same'],
    response: tradeResponse(plan, [row], true, 'next', 1) }));
  const emptyProgress = validateTradePageV1({ plan,
    response: tradeResponse(plan, [], true, 'next', 1) });
  assert.equal(emptyProgress.hasMore, true);
  throwsCode('HISTORY_TRADE_CHRONOLOGY', () => validateTradePageV1({ plan,
    response: tradeResponse(plan, [row, trade('b', '2026-01-01T00:10:00Z')], false, null, 2) }));
  throwsCode('HISTORY_RESPONSE_SCOPE', () => validateTradePageV1({ plan,
    response: { ...tradeResponse(plan, [row], false, null, 1), to: '2026-01-01T00:30:00.000Z' } }));
});

test('failed trade validation preserves both dedupe maps and nested ID sets for retry', () => {
  const plan = planTradeBackfillV1({ exchangeId: 'atomic-trade', from: '2026-01-01T00:00:00Z',
    to: '2026-01-01T01:00:00Z', limit: 3 });
  const seenTrades = new Map([['existing-fingerprint', 'existing-fingerprint']]);
  const seededIds = new Set(['seed-fingerprint']);
  const seenTradeIds = new Map([['shared', seededIds]]);
  const beforeTrades = [...seenTrades];
  const beforeIds = [...seededIds];
  const malformed = tradeResponse(plan, [trade('shared', '2026-01-01T00:09:00Z'),
    trade('later', '2026-01-01T00:10:00Z')], false, null, 2);
  throwsCode('HISTORY_TRADE_CHRONOLOGY', () => validateTradePageV1({ plan, response: malformed, seenTrades, seenTradeIds }));
  assert.deepEqual([...seenTrades], beforeTrades);
  assert.equal(seenTradeIds.get('shared'), seededIds);
  assert.deepEqual([...seededIds], beforeIds);
  const corrected = validateTradePageV1({ plan, seenTrades, seenTradeIds,
    response: tradeResponse(plan, [trade('shared', '2026-01-01T00:09:00Z'),
      trade('later', '2026-01-01T00:08:00Z')], false, null, 2) });
  assert.equal(corrected.accepted.length, 2);
});

test('resume checkpoints bind exact scope and prior output and expire only when explicitly requested', () => {
  const plan = planTradeBackfillV1({ exchangeId: 'resume', from: '2026-01-01T00:00:00Z',
    to: '2026-01-01T01:00:00Z' });
  const observations = [trade('x', '2026-01-01T00:10:00Z')];
  const manifest = createHistoryResumeManifestV1({ plan, completedPageCount: 1, nextPageIndex: 1,
    nextCursor: 'opaque', lastTradeAt: observations[0].createdAt, seenCursors: [],
    outputSha256: historyObservationsSha256V1(observations), savedAt: '2026-01-01T00:20:00Z',
    expiresAt: '2026-01-01T01:00:00Z' });
  assert.equal(validateHistoryResumeManifestV1({ plan, manifest, now: '2026-01-01T00:30:00Z',
    priorObservations: observations }).done, false);
  throwsCode('HISTORY_RESUME_SCOPE', () => validateHistoryResumeManifestV1({
    plan: planTradeBackfillV1({ exchangeId: 'other', from: plan.scope.from, to: plan.scope.to }),
    manifest, now: '2026-01-01T00:30:00Z', priorObservations: observations }));
  throwsCode('HISTORY_RESUME_OUTPUT_MISMATCH', () => validateHistoryResumeManifestV1({ plan, manifest,
    now: '2026-01-01T00:30:00Z', priorObservations: [] }));
  throwsCode('HISTORY_RESUME_EXPIRED', () => validateHistoryResumeManifestV1({ plan, manifest,
    now: '2026-01-01T01:00:00Z', priorObservations: observations }));
  const wrongOldestUnsigned = { ...manifest, lastTradeAt: '2026-01-01T00:30:00.000Z' };
  delete wrongOldestUnsigned.manifestSha256;
  wrongOldestUnsigned.manifestSha256 = createHash('sha256').update(canonicalJson(wrongOldestUnsigned)).digest('hex');
  throwsCode('HISTORY_RESUME_LAST_TRADE_AT', () => validateHistoryResumeManifestV1({ plan,
    manifest: wrongOldestUnsigned, now: '2026-01-01T00:30:00Z', priorObservations: observations }));
  const nullOldestUnsigned = { ...manifest, lastTradeAt: null };
  delete nullOldestUnsigned.manifestSha256;
  nullOldestUnsigned.manifestSha256 = createHash('sha256').update(canonicalJson(nullOldestUnsigned)).digest('hex');
  throwsCode('HISTORY_RESUME_LAST_TRADE_AT', () => validateHistoryResumeManifestV1({ plan,
    manifest: nullOldestUnsigned, now: '2026-01-01T00:30:00Z', priorObservations: observations }));
  const unhashed = createHistoryResumeManifestV1({ plan, completedPageCount: 1, nextPageIndex: 1,
    nextCursor: 'opaque', savedAt: '2026-01-01T00:20:00Z' });
  throwsCode('HISTORY_RESUME_OUTPUT_MISMATCH', () => validateHistoryResumeManifestV1({ plan,
    manifest: unhashed, priorObservations: observations }));
  throwsCode('HISTORY_RESUME', () => createHistoryResumeManifestV1({ plan, completedPageCount: 0,
    nextPageIndex: 0, done: true, savedAt: '2026-01-01T00:20:00Z' }));
  const terminalEmpty = createHistoryResumeManifestV1({ plan, completedPageCount: 1, nextPageIndex: 1,
    done: true, savedAt: '2026-01-01T00:20:00Z' });
  assert.equal(validateHistoryResumeManifestV1({ plan, manifest: terminalEmpty, priorObservations: [] }).done, true);
  const falseOldestEmptyUnsigned = { ...terminalEmpty, lastTradeAt: '2026-01-01T00:08:00.000Z' };
  delete falseOldestEmptyUnsigned.manifestSha256;
  falseOldestEmptyUnsigned.manifestSha256 = createHash('sha256').update(canonicalJson(falseOldestEmptyUnsigned)).digest('hex');
  throwsCode('HISTORY_RESUME_LAST_TRADE_AT', () => validateHistoryResumeManifestV1({ plan,
    manifest: falseOldestEmptyUnsigned, priorObservations: [] }));

  const forgedUnsigned = { ...unhashed, completedPageCount: 0, nextPageIndex: 0, nextCursor: null,
    done: true, seenCursors: [], outputSha256: null };
  delete forgedUnsigned.manifestSha256;
  forgedUnsigned.manifestSha256 = createHash('sha256').update(canonicalJson(forgedUnsigned)).digest('hex');
  throwsCode('HISTORY_RESUME', () => validateHistoryResumeManifestV1({ plan,
    manifest: forgedUnsigned, priorObservations: [] }));
});

test('offline runner consumes frozen pages only and resumes cursor pagination without fetching', () => {
  const plan = planTradeBackfillV1({ exchangeId: 'runner', from: '2026-01-01T00:00:00Z',
    to: '2026-01-01T01:00:00Z', limit: 2 });
  const page1Row = trade('one', '2026-01-01T00:09:00Z');
  let fetchCalls = 0;
  const first = runHistoryRangeBackfillOfflineV1({ schemaVersion: 1, mode: 'frozen-pages', kind: 'trades',
    scope: { ...plan.scope }, savedAt: '2026-01-01T00:10:00Z',
    pages: [{ request: tradePageV1(plan, null), response: tradeResponse(plan, [page1Row], true, 'cursor/2', 1) }],
    fetchPage: () => { fetchCalls++; throw new Error('must not be called'); } });
  assert.equal(first.done, false);
  assert.equal(first.resumeManifest.nextCursor, 'cursor/2');
  const second = runHistoryRangeBackfillOfflineV1({ schemaVersion: 1, mode: 'frozen-pages', kind: 'trades',
    scope: { ...plan.scope }, savedAt: '2026-01-01T00:11:00Z', now: '2026-01-01T00:11:00Z',
    resumeManifest: first.resumeManifest, priorObservations: first.observations,
    pages: [{ request: tradePageV1(plan, 'cursor/2'), response: tradeResponse(plan,
      [trade('two', '2026-01-01T00:08:00Z')], false, null, 1) }] });
  assert.equal(second.done, true);
  assert.equal(second.acceptedCount, 2);
  assert.equal(fetchCalls, 0);
  const unqueried = runHistoryRangeBackfillOfflineV1({ schemaVersion: 1, mode: 'frozen-pages', kind: 'trades',
    scope: { ...plan.scope }, savedAt: '2026-01-01T00:11:00Z', pages: [] });
  assert.equal(unqueried.done, false);
  assert.equal(unqueried.resumeManifest.done, false);
  throwsCode('HISTORY_OFFLINE_INPUT', () => runHistoryRangeBackfillOfflineV1({ schemaVersion: 1,
    mode: 'live', kind: 'trades', scope: { ...plan.scope }, savedAt: '2026-01-01T00:11:00Z', pages: [] }));
});

test('offline runner rejects a forged trade watermark before admitting a newer page', () => {
  const plan = planTradeBackfillV1({ exchangeId: 'watermark', from: '2026-01-01T00:00:00Z',
    to: '2026-01-01T01:00:00Z', limit: 2 });
  const priorObservations = [trade('newer', '2026-01-01T00:09:00Z'), trade('oldest', '2026-01-01T00:08:00Z')];
  const valid = createHistoryResumeManifestV1({ plan, completedPageCount: 1, nextPageIndex: 1,
    nextCursor: 'next', lastTradeAt: '2026-01-01T00:08:00.000Z',
    outputSha256: historyObservationsSha256V1(priorObservations), savedAt: '2026-01-01T00:10:00Z' });
  const forged = { ...valid, lastTradeAt: '2026-01-01T00:30:00.000Z' };
  delete forged.manifestSha256;
  forged.manifestSha256 = createHash('sha256').update(canonicalJson(forged)).digest('hex');
  throwsCode('HISTORY_RESUME_LAST_TRADE_AT', () => runHistoryRangeBackfillOfflineV1({
    schemaVersion: 1, mode: 'frozen-pages', kind: 'trades', scope: { ...plan.scope },
    savedAt: '2026-01-01T00:11:00Z', now: '2026-01-01T00:11:00Z',
    resumeManifest: forged, priorObservations,
    pages: [{ request: tradePageV1(plan, 'next'), response: tradeResponse(plan,
      [trade('newly-admitted', '2026-01-01T00:20:00Z')], false, null, 1) }],
  }));
});

test('offline runner follows partial candle response.to and produces sparse deterministic pages', () => {
  const plan = planCandleBackfillV1({ exchangeId: 'runner-candle', resolution: '1m', limit: 1,
    from: '2026-01-01T00:00:00Z', to: '2026-01-01T00:10:00Z' });
  const req1 = { ...candlePageV1(plan, 0).request };
  const partialTo = '2026-01-01T00:04:00.000Z';
  const req2 = { ...req1, from: partialTo };
  const first = runHistoryRangeBackfillOfflineV1({ schemaVersion: 1, mode: 'frozen-pages', kind: 'candles',
    scope: { ...plan.scope }, savedAt: '2026-01-01T00:11:00Z', pages: [
      { request: req1, response: candleResponse(plan, req1.from, partialTo,
        [candle('2026-01-01T00:02:00Z')], false) },
    ] });
  assert.equal(first.resumeManifest.nextFrom, partialTo);
  const second = runHistoryRangeBackfillOfflineV1({ schemaVersion: 1, mode: 'frozen-pages', kind: 'candles',
    scope: { ...plan.scope }, savedAt: '2026-01-01T00:12:00Z', now: '2026-01-01T00:12:00Z',
    resumeManifest: first.resumeManifest, priorObservations: first.observations,
    pages: [{ request: req2, response: candleResponse(plan, partialTo, req1.to,
      [candle('2026-01-01T00:04:00Z')], true) }] });
  assert.equal(second.done, true);
  assert.deepEqual(second.observations.map(row => row.time),
    ['2026-01-01T00:02:00.000Z', '2026-01-01T00:04:00.000Z']);
  throwsCode('HISTORY_CANDLE_CONTINUATION', () => runHistoryRangeBackfillOfflineV1({ schemaVersion: 1,
    mode: 'frozen-pages', kind: 'candles', scope: { ...plan.scope }, savedAt: '2026-01-01T00:13:00Z',
    now: '2026-01-01T00:13:00Z', resumeManifest: first.resumeManifest, priorObservations: first.observations,
    pages: [{ request: req2, response: candleResponse(plan, partialTo, req1.to,
      [candle('2026-01-01T00:05:00Z')], true) }] }));
});

