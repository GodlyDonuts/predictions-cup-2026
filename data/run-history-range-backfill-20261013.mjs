#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import {
  candlePageV1,
  createHistoryResumeManifestV1,
  historyCandleCanonicalV1,
  historyObservationsSha256V1,
  historyTradeFingerprintV1,
  planCandleBackfillV1,
  planTradeBackfillV1,
  tradePageV1,
  validateCandlePageV1,
  validateHistoryResumeManifestV1,
  validateTradePageV1,
} from '../research/history-range-backfill-v1.mjs';

function fail(code) {
  throw new Error(code);
}

function expectRequest(actual, expected, keys) {
  if (!actual || typeof actual !== 'object' || Array.isArray(actual)) fail('HISTORY_FIXTURE_REQUEST');
  for (const key of keys) if (actual[key] !== expected[key]) fail('HISTORY_FIXTURE_REQUEST');
  if (Object.keys(actual).some(key => !keys.includes(key))) fail('HISTORY_FIXTURE_REQUEST');
}

function createPlan(input) {
  if (input.kind === 'candles') return planCandleBackfillV1(input.scope);
  if (input.kind === 'trades') return planTradeBackfillV1(input.scope);
  fail('HISTORY_FIXTURE_KIND');
}

/**
 * Replay already-saved API pages. This deliberately has no network adapter;
 * it validates pagination, aggregation, and checkpoints from immutable input.
 */
export function runHistoryRangeBackfillOfflineV1(input) {
  if (!input || input.schemaVersion !== 1 || input.mode !== 'frozen-pages'
    || !Array.isArray(input.pages) || typeof input.savedAt !== 'string') fail('HISTORY_OFFLINE_INPUT');
  const plan = createPlan(input);
  const priorObservations = input.priorObservations ?? [];
  if (!Array.isArray(priorObservations)) fail('HISTORY_OFFLINE_PRIOR');
  let manifest = input.resumeManifest ?? null;
  if (manifest) validateHistoryResumeManifestV1({ plan, manifest, now: input.now, priorObservations });

  const observations = [...priorObservations];
  const pageResults = [];
  let completedPageCount = manifest?.completedPageCount ?? 0;
  let nextPageIndex = manifest?.nextPageIndex ?? 0;
  let nextCursor = manifest?.nextCursor ?? null;
  let nextFrom = manifest?.nextFrom ?? null;
  let lastTradeAt = manifest?.lastTradeAt ?? null;
  let seenCursors = [...(manifest?.seenCursors ?? [])];
  let done = manifest?.done ?? false;
  let duplicates = 0;
  let idCollisions = 0;
  let seenCandles = new Map(priorObservations.map(row => [row.time, historyCandleCanonicalV1(row)]));
  let seenTrades = new Map();
  let seenTradeIds = new Map();
  if (input.kind === 'trades') {
    seenTrades = new Map(priorObservations.map(row => {
      const fingerprint = historyTradeFingerprintV1(row);
      return [fingerprint, fingerprint];
    }));
    for (const row of priorObservations) {
      const fp = historyTradeFingerprintV1(row);
      const ids = seenTradeIds.get(String(row.id)) ?? new Set();
      ids.add(fp);
      seenTradeIds.set(String(row.id), ids);
    }
  }

  if (done && input.pages.length > 0) fail('HISTORY_OFFLINE_AFTER_DONE');
  for (const entry of input.pages) {
    if (done || !entry || !entry.response) fail('HISTORY_OFFLINE_PAGE');
    if (input.kind === 'candles') {
      const planned = candlePageV1(plan, nextPageIndex);
      const expectedFrom = nextFrom ?? planned.request.from;
      const expectedRequest = { ...planned.request, from: expectedFrom };
      expectRequest(entry.request, expectedRequest,
        ['exchangeId', 'tournamentId', 'resolution', 'from', 'to', 'limit']);
      const result = validateCandlePageV1({ plan,
        page: { index: nextPageIndex, request: expectedRequest }, response: entry.response, seenCandles });
      if (nextFrom !== null && Date.parse(entry.response.candles[0]?.time) !== Date.parse(nextFrom)) {
        fail('HISTORY_CANDLE_CONTINUATION');
      }
      observations.push(...result.accepted);
      duplicates += result.duplicates;
      pageResults.push({ pageIndex: nextPageIndex, accepted: result.accepted.length,
        duplicates: result.duplicates, complete: result.complete, continuationFrom: result.continuationFrom });
      if (result.complete) {
        completedPageCount++;
        nextPageIndex++;
        nextFrom = null;
        done = nextPageIndex === plan.pageCount;
      } else {
        nextFrom = result.continuationFrom;
      }
    } else {
      const expectedRequest = tradePageV1(plan, nextCursor);
      expectRequest(entry.request, expectedRequest,
        ['exchangeId', 'tournamentId', 'from', 'to', 'limit', 'cursor']);
      const result = validateTradePageV1({ plan, cursor: nextCursor, response: entry.response,
        seenTrades, seenTradeIds, seenCursors, previousOldestAt: lastTradeAt });
      observations.push(...result.accepted);
      duplicates += result.duplicates;
      idCollisions += result.idCollisions;
      completedPageCount++;
      nextPageIndex = completedPageCount;
      nextCursor = result.hasMore ? result.nextCursor : null;
      done = !result.hasMore;
      seenCursors = [...result.seenCursors];
      lastTradeAt = result.oldestAt;
      pageResults.push({ pageIndex: completedPageCount - 1, accepted: result.accepted.length,
        duplicates: result.duplicates, idCollisions: result.idCollisions,
        hasMore: result.hasMore, nextCursor: result.nextCursor, oldestAt: result.oldestAt });
    }
  }

  const outputSha256 = observations.length ? historyObservationsSha256V1(observations) : null;
  const resumeManifest = createHistoryResumeManifestV1({ plan, completedPageCount, nextPageIndex,
    nextCursor, nextFrom, done, lastTradeAt, seenCursors, outputSha256,
    savedAt: input.savedAt, expiresAt: input.resumeExpiresAt ?? null });
  return Object.freeze({ schemaVersion: 1, mode: 'offline-replay', kind: input.kind,
    scope: plan.scope, planSha256: plan.scopeHash, done, pagesProcessed: pageResults.length,
    completedPageCount, acceptedCount: observations.length, duplicates, idCollisions,
    pageResults, observations, outputSha256, resumeManifest });
}

async function main() {
  const inputPath = process.argv[2];
  if (!inputPath || process.argv.length !== 3) fail('USAGE: node data/run-history-range-backfill-20261013.mjs <frozen-pages.json>');
  const input = JSON.parse(await readFile(inputPath, 'utf8'));
  const result = runHistoryRangeBackfillOfflineV1(input);
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => {
    process.stderr.write(`${error?.message ?? 'HISTORY_OFFLINE_ERROR'}\n`);
    process.exitCode = 1;
  });
}

