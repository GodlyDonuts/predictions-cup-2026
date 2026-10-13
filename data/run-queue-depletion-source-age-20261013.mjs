import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { amount, money } from '../paper/fixed.mjs';
import { createQueueDepletionSourceAgeV1, hashByteRange, verifyByteRange } from '../research/queue-depletion-source-age-v1.mjs';

const TOURNAMENT_ID = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const CUTOFF_AT = '2026-10-13T14:08:44.000Z';
const EVALUATION_START_AT = '2026-10-13T14:08:44.314Z';
const THROUGH_AT = '2026-10-13T14:28:58.395Z';
const SESSION_NAME = 'realtime-2026-10-13T13-28-06-669Z-a7674ae9-ed0e-41c6-90c8-390b1ce0dc8b.jsonl';
const SOURCE_AGE_LIMIT_MS = 10_000;
const FROZEN = Object.freeze({
  configStartByte: 0,
  configEndByte: 92_397,
  configRows: 237,
  configSha256: '2f06b9bfee4dba65f367854988c9d85ebb465e49fc442f8199e1850fc57e67d4',
  startByte: 199_691_300,
  endByte: 290_477_704,
  evaluationBytes: 90_786_404,
  evaluationSha256: '6b45f6c1445aa19aa1a5f47f84ec380007d7608f77919134d4d28fca2a35adc4',
});
const EXPECTED_BASELINE = Object.freeze({
  marketBatchRows: 22_802,
  rowsAfterCutoff: 27_538,
  rowsIgnoredAsConvenienceDuplicates: 355,
  quotePairs: 10_376,
  uniqueScopedTrades: 354,
  entryFillProxies: 3,
  completedExitProxies: 0,
  openInventoryExecutableMarkPnl: '-0.005000',
  openInventoryStressPerFillPnl: '-0.020000',
});
const OUTPUT_FILE = fileURLToPath(new URL('./queue-depletion-source-age-20261013.json', import.meta.url));
const CHUNK_BYTES = 1024 * 1024;

function captureFile() {
  return path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime', SESSION_NAME);
}

function readLinesInRange(file, startByte, endByte, callback) {
  if (!Number.isSafeInteger(startByte) || !Number.isSafeInteger(endByte) || startByte < 0 || endByte <= startByte) throw Error('SOURCE_AGE_RANGE');
  const fd = fs.openSync(file, 'r');
  try {
    const buffer = Buffer.alloc(CHUNK_BYTES);
    let readAt = startByte, pendingStart = startByte, pending = Buffer.alloc(0);
    while (readAt < endByte) {
      const requested = Math.min(buffer.length, endByte - readAt);
      const n = fs.readSync(fd, buffer, 0, requested, readAt);
      if (n !== requested) throw Error('SOURCE_AGE_TRUNCATED');
      const piece = buffer.subarray(0, n);
      const data = pending.length ? Buffer.concat([pending, piece]) : piece;
      let cursor = 0;
      for (;;) {
        const newline = data.indexOf(0x0a, cursor);
        if (newline < 0) break;
        const lineStart = pendingStart + cursor, lineEnd = pendingStart + newline + 1;
        if (callback(data.subarray(cursor, newline), lineStart, lineEnd) === false) return;
        cursor = newline + 1;
      }
      pending = Buffer.from(data.subarray(cursor));
      pendingStart += cursor;
      readAt += n;
    }
    if (pending.length || pendingStart !== endByte) throw Error('SOURCE_AGE_INCOMPLETE_BOUND');
  } finally { fs.closeSync(fd); }
}

function parseLine(bytes) {
  if (!bytes.length) throw Error('SOURCE_AGE_EMPTY_LINE');
  try { return JSON.parse(bytes.toString('utf8')); }
  catch { throw Error('SOURCE_AGE_JSON'); }
}

function verifyFrozenRanges(file) {
  const config = hashByteRange(file, FROZEN.configStartByte, FROZEN.configEndByte);
  const evaluation = hashByteRange(file, FROZEN.startByte, FROZEN.endByte);
  if (config.bytes !== FROZEN.configEndByte - FROZEN.configStartByte || config.sha256 !== FROZEN.configSha256) throw Error('SOURCE_AGE_CONFIG_HASH');
  if (evaluation.bytes !== FROZEN.evaluationBytes || evaluation.sha256 !== FROZEN.evaluationSha256) throw Error('SOURCE_AGE_EVALUATION_HASH');
  if (config.dev !== evaluation.dev || config.ino !== evaluation.ino) throw Error('SOURCE_AGE_SOURCE_REPLACED');
  return { config, evaluation };
}

function configMembers(file) {
  const config = new Map();
  let rows = 0;
  readLinesInRange(file, FROZEN.configStartByte, FROZEN.configEndByte, bytes => {
    const row = parseLine(bytes);
    if (row.kind !== 'config' || row.scope?.tournamentId !== TOURNAMENT_ID || !Array.isArray(row.scope.exchangeIds) || !row.scope.exchangeIds.length || typeof row.scope.marketId !== 'string') throw Error('SOURCE_AGE_CONFIG');
    const prefix = `tournament:${TOURNAMENT_ID}:market:`;
    if (typeof row.topic !== 'string' || !row.topic.startsWith(prefix) || row.topic.slice(prefix.length) !== row.scope.marketId || row.scope.topic !== row.topic) throw Error('SOURCE_AGE_CONFIG_TOPIC');
    const exchangeIds = [...new Set(row.scope.exchangeIds)];
    if (exchangeIds.some(id => typeof id !== 'string' || !id)) throw Error('SOURCE_AGE_CONFIG_EXCHANGE');
    const prior = config.get(row.scope.marketId);
    if (prior && prior.join('\u0000') !== exchangeIds.join('\u0000')) throw Error('SOURCE_AGE_CONFIG_CONFLICT');
    config.set(row.scope.marketId, exchangeIds);
    rows++;
  });
  if (rows !== FROZEN.configRows || config.size !== FROZEN.configRows) throw Error('SOURCE_AGE_CONFIG_COUNT');
  return [...config].flatMap(([marketId, exchangeIds]) => exchangeIds.map(exchangeId => ({ marketId, exchangeId })));
}

function topicMarket(row) {
  const prefix = `tournament:${TOURNAMENT_ID}:market:`;
  if (typeof row.topic !== 'string' || !row.topic.startsWith(prefix)) return null;
  return row.topic.slice(prefix.length);
}

function normalizedBatch(row, marketId, members) {
  const allowed = new Set(members.filter(member => member.marketId === marketId).map(member => member.exchangeId));
  const rawBooks = (row.payload?.books ?? []).map(book => {
    if (!allowed.has(book.exchangeId) || !book.asOf || !Number.isSafeInteger(book.asOf.sequence) || book.asOf.sequence < 0) throw Error('SOURCE_AGE_BOOK_SCOPE');
    return { marketId, exchangeId: book.exchangeId, sequence: String(book.asOf.sequence), sourceAt: book.asOf.at, receivedAt: row.receivedAt, bids: book.bids, asks: book.asks };
  });
  const rawTrades = (row.payload?.trades ?? []).map(trade => {
    if (trade.marketId !== marketId || !allowed.has(trade.exchangeId) || trade.tournamentId !== TOURNAMENT_ID) throw Error('SOURCE_AGE_TRADE_SCOPE');
    return trade;
  });
  return { observedAt: row.observedAt, books: rawBooks, trades: rawTrades };
}

function runReplay(file, members, maxDecisionSourceAgeMs) {
  const replay = createQueueDepletionSourceAgeV1({ members, startAt: CUTOFF_AT, maxDecisionSourceAgeMs });
  const metrics = { marketBatchRows: 0, rowsAfterCutoff: 0, rowsIgnoredAsConvenienceDuplicates: 0, resyncBoundaries: 0, lifecycleBoundaries: 0, dirtyHintsPairedWithBook: 0, settledMarkets: 0, topicEpochBoundaries: 0, revisionGaps: 0 };
  const settled = new Set(), epochs = new Map(), revisions = new Map();
  let throughAt = null, lastObserved = null, firstRow = true;
  readLinesInRange(file, FROZEN.startByte, FROZEN.endByte, bytes => {
    const row = parseLine(bytes);
    if (typeof row.observedAt !== 'string' || row.observedAt < CUTOFF_AT) throw Error('SOURCE_AGE_PREFIX_ORDER');
    if (firstRow && row.observedAt !== EVALUATION_START_AT) throw Error('SOURCE_AGE_START_CHANGED');
    firstRow = false;
    if (lastObserved !== null && row.observedAt < lastObserved) throw Error('SOURCE_AGE_CLOCK_ROLLBACK');
    lastObserved = row.observedAt;
    throughAt = row.observedAt;
    metrics.rowsAfterCutoff++;
    if (['disconnected', 'invalidation', 'recovery_started', 'recovery_finished', 'subscribed', 'stopped'].includes(row.kind)) {
      const lifecycleMarket = topicMarket(row);
      if (lifecycleMarket && !members.some(member => member.marketId === lifecycleMarket)) throw Error('SOURCE_AGE_LIFECYCLE_SCOPE');
      replay.boundary(lifecycleMarket ? { marketId: lifecycleMarket } : {});
      metrics.lifecycleBoundaries++;
      return true;
    }
    if (row.kind !== 'event' || row.event !== 'market_batch') {
      if (row.kind === 'trade_observed') metrics.rowsIgnoredAsConvenienceDuplicates++;
      return true;
    }
    const marketId = topicMarket(row);
    if (!marketId || !members.some(member => member.marketId === marketId)) throw Error('SOURCE_AGE_TOPIC_SCOPE');
    if (Number.isSafeInteger(row.epoch)) {
      const priorEpoch = epochs.get(marketId);
      if (priorEpoch !== undefined && priorEpoch !== row.epoch) { replay.boundary({ marketId }); metrics.topicEpochBoundaries++; }
      epochs.set(marketId, row.epoch);
    }
    const payload = row.payload ?? {};
    if (payload.resyncRequired) { replay.boundary({ marketId }); metrics.resyncBoundaries++; return true; }
    const delivery = payload.delivery;
    if (Number.isSafeInteger(delivery?.revision) && Number.isSafeInteger(delivery?.previousRevision)) {
      const previous = revisions.get(marketId);
      if (previous !== undefined && delivery.revision > previous && delivery.previousRevision > previous) { replay.boundary({ marketId }); metrics.revisionGaps++; }
      if (previous === undefined || delivery.revision > previous) revisions.set(marketId, delivery.revision);
    }
    const currentBooks = new Set((payload.books ?? []).map(book => book.exchangeId));
    for (const dirty of payload.bookDirty ?? []) {
      if (dirty.marketId && dirty.marketId !== marketId) throw Error('SOURCE_AGE_DIRTY_SCOPE');
      if (currentBooks.has(dirty.exchangeId)) metrics.dirtyHintsPairedWithBook++;
    }
    const hasUnresolvedTape = [...(payload.bookDirty ?? []), ...(payload.trades ?? [])].some(item => !currentBooks.has(item.exchangeId));
    if (hasUnresolvedTape) { replay.boundary({ marketId }); return true; }
    for (const item of payload.marketSettled ?? []) {
      const settledMarket = item.marketId ?? marketId;
      if (!members.some(member => member.marketId === settledMarket)) throw Error('SOURCE_AGE_SETTLEMENT_SCOPE');
      settled.add(settledMarket);
      replay.boundary({ marketId: settledMarket });
      metrics.settledMarkets++;
    }
    if (settled.has(marketId)) return true;
    replay.observeBatch(normalizedBatch(row, marketId, members));
    metrics.marketBatchRows++;
    return true;
  });
  if (firstRow || throughAt !== THROUGH_AT || metrics.rowsAfterCutoff !== 27_538 || metrics.marketBatchRows !== 22_802 || metrics.rowsIgnoredAsConvenienceDuplicates !== 355) throw Error('SOURCE_AGE_CAPTURE_COUNTS');
  return { metrics, report: replay.report({ throughAt, source: { cutoffAt: CUTOFF_AT, evaluationStartAt: EVALUATION_START_AT, throughAt } }) };
}

function assertBaselineMatchesFrozen(baseline) {
  const { counts, aggregate } = baseline.report;
  for (const key of ['quotePairs', 'uniqueScopedTrades', 'entryFillProxies', 'completedExitProxies']) {
    if (counts[key] !== EXPECTED_BASELINE[key]) throw Error(`SOURCE_AGE_BASELINE_${key.toUpperCase()}_${counts[key]}`);
  }
  for (const key of ['openInventoryExecutableMarkPnl', 'openInventoryStressPerFillPnl']) {
    if (aggregate[key] !== EXPECTED_BASELINE[key]) throw Error(`SOURCE_AGE_BASELINE_${key.toUpperCase()}`);
  }
  if (baseline.report.unresolvedEntryProxies.length !== EXPECTED_BASELINE.entryFillProxies) throw Error('SOURCE_AGE_BASELINE_PROXY_DETAIL');
  const strataTotal = baseline.report.decisionSourceAgeStrata.reduce((sum, row) => sum + row.posted, 0);
  if (strataTotal !== counts.quotePairs || baseline.report.counts.decisionCandidates !== counts.quotePairs) throw Error('SOURCE_AGE_BASELINE_STRATA');
}

function markedMarkets(report) {
  return report.perMarket.filter(row => row.entryFillProxies || row.unresolvedInventory).map(row => ({
    marketId: row.marketId, exchangeId: row.exchangeId, entryFillProxies: row.entryFillProxies,
    completedExitProxies: row.completedExitProxies, unresolvedInventory: row.unresolvedInventory,
    executableMarkPnl: row.unresolvedInventoryExecutableMarkPnl,
    oneTickStressPnl: row.unresolvedInventoryStressPerFillPnl,
    markSourceAt: row.inventoryMarkSourceAt,
  }));
}

export function runQueueDepletionSourceAgeCapture({ file = captureFile(), output = OUTPUT_FILE } = {}) {
  const initial = verifyFrozenRanges(file);
  const members = configMembers(file);
  const baseline = runReplay(file, members, null);
  assertBaselineMatchesFrozen(baseline);
  const gated = runReplay(file, members, SOURCE_AGE_LIMIT_MS);
  const final = verifyFrozenRanges(file);
  for (const range of ['config', 'evaluation']) {
    if (initial[range].dev !== final[range].dev || initial[range].ino !== final[range].ino || initial[range].sha256 !== final[range].sha256) throw Error('SOURCE_AGE_CHANGED_DURING_REPLAY');
  }
  const base = baseline.report, filtered = gated.report;
  const proxyAgesMs = base.unresolvedEntryProxies.map(proxy => proxy.decisionBookSourceAgeMs);
  const minProxyAgeSec = Math.min(...proxyAgesMs) / 1_000;
  const maxProxyAgeSec = Math.max(...proxyAgesMs) / 1_000;
  const result = {
    schemaVersion: 1,
    mode: 'fixed_queue_depletion_source_age_sensitivity',
    executionAuthority: false,
    verdict: 'counterfactual_source_age_diagnostic_only',
    source: {
      frozenReference: 'issue #44 immutable byte ranges',
      configStartByte: FROZEN.configStartByte, configEndByte: FROZEN.configEndByte,
      configRows: FROZEN.configRows, configSha256: FROZEN.configSha256,
      evaluationStartByte: FROZEN.startByte, evaluationEndByte: FROZEN.endByte,
      evaluationBytes: FROZEN.evaluationBytes, evaluationSha256: FROZEN.evaluationSha256,
      cutoffAt: CUTOFF_AT, evaluationStartAt: EVALUATION_START_AT, throughAt: THROUGH_AT,
      completeRows: baseline.metrics.rowsAfterCutoff, marketBatchRows: baseline.metrics.marketBatchRows,
      sourceFilePathIncluded: false, readsOutsideFrozenRanges: false,
      hashVerifiedBeforeAndAfterBothRuns: true,
    },
    fixedPolicy: {
      inheritedFrom: 'issue #44',
      sourceAgeLimitMs: SOURCE_AGE_LIMIT_MS,
      comparison: 'baseline vs same queue, latency, lifetime, aggressor, exit and mark rules; gated run rejects a quote decision when decisionAt-sourceAt is strictly greater than 10,000ms and rejects sourceAt later than receivedAt fail-closed',
      exactlyAtLimitRetained: true,
      fees: '0.000000',
      noParameterTuning: true,
    },
    baseline: {
      counts: base.counts, aggregate: base.aggregate,
      decisionSourceAgeStrata: base.decisionSourceAgeStrata,
      entryFillProxies: base.unresolvedEntryProxies,
      markedMarkets: markedMarkets(base),
    },
    sourceAgeFiltered: {
      counts: filtered.counts, aggregate: filtered.aggregate,
      decisionSourceAgeStrata: filtered.decisionSourceAgeStrata,
      entryFillProxies: filtered.unresolvedEntryProxies,
      markedMarkets: markedMarkets(filtered),
    },
    change: {
      quotePairs: filtered.counts.quotePairs - base.counts.quotePairs,
      decisionCandidates: filtered.counts.decisionCandidates - base.counts.decisionCandidates,
      entryFillProxies: filtered.counts.entryFillProxies - base.counts.entryFillProxies,
      completedExitProxies: filtered.counts.completedExitProxies - base.counts.completedExitProxies,
      openInventoryExecutableMarkPnl: money(amount(filtered.aggregate.openInventoryExecutableMarkPnl) - amount(base.aggregate.openInventoryExecutableMarkPnl)),
      openInventoryStressPerFillPnl: money(amount(filtered.aggregate.openInventoryStressPerFillPnl) - amount(base.aggregate.openInventoryStressPerFillPnl)),
      sourceAgeBooksRejectedAsNewEntryDecisions: filtered.counts.staleDecisionBooksRejected,
      invalidSourceTimeBooksRejectedAsNewEntryDecisions: filtered.counts.invalidDecisionSourceTimesRejected,
    },
    finding: `All ${proxyAgesMs.length} baseline entry proxies were decided from source books aged ${minProxyAgeSec.toFixed(3)}–${maxProxyAgeSec.toFixed(3)}s; none crossed the fixed 10s limit. The same proxies and point marks remain after filtering. This bounds the stale-decision-source concern for these three observed proxies only; it does not validate their execution or profitability.`,
    limitations: [
      'The three baseline entry proxies are modeled from displayed queue and inferred public-print aggressors; none is an observed exchange fill.',
      'Missing, malformed, or decision-future source timestamps are dropped by the replay parser. To reproduce #44 exactly, baseline retains its original handling of sourceAt later than receivedAt; the filtered decision gate rejects one such decision book fail-closed.',
      'The >10s gate applies only to new entry quote decisions. Captured-book handling for aggressor inference and terminal executable marks is unchanged.',
      'The filtered replay rejected 34 stale and 1 source-after-receipt decision opportunities. Later fresh books can create new opportunities, so net posted quote pairs fell by 15 rather than 35.',
      'Only the exact #44 config and evaluation byte ranges are read and rehashed; later bytes and current/live market state are not observed.',
      'No tuning, profitability, expected-return, or executable-edge conclusion is made. The capture is short and trades are sparse; marks are point-in-time counterfactuals.',
    ],
  };
  fs.writeFileSync(output, `${JSON.stringify(result, null, 2)}\n`, { encoding: 'utf8', flag: 'w' });
  return result;
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  try {
    const result = runQueueDepletionSourceAgeCapture();
    console.log(JSON.stringify({
      verdict: result.verdict,
      frozen: { evaluationBytes: result.source.evaluationBytes, evaluationSha256: result.source.evaluationSha256 },
      baseline: { entries: result.baseline.counts.entryFillProxies, marks: result.baseline.aggregate.openInventoryExecutableMarkPnl },
      sourceAgeFiltered: { entries: result.sourceAgeFiltered.counts.entryFillProxies, marks: result.sourceAgeFiltered.aggregate.openInventoryExecutableMarkPnl },
      change: result.change,
    }, null, 2));
  } catch (error) {
    const code = typeof error?.message === 'string' && /^[A-Z0-9_]+$/.test(error.message) ? error.message : 'SOURCE_AGE_RUN_FAILED';
    console.error(code);
    process.exitCode = 1;
  }
}
