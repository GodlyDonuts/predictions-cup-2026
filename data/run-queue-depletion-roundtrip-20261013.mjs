import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createQueueDepletionRoundtripV1, hashByteRange, verifyByteRange } from '../research/queue-depletion-roundtrip-v1.mjs';

const CUTOFF_AT = '2026-10-13T14:08:44.000Z';
const MIN_THROUGH_AT = '2026-10-13T14:28:44.000Z';
const TOURNAMENT_ID = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const SESSION_NAME = 'realtime-2026-10-13T13-28-06-669Z-a7674ae9-ed0e-41c6-90c8-390b1ce0dc8b.jsonl';
const OUTPUT_FILE = new URL('./queue-depletion-roundtrip-20261013.json', import.meta.url);
const CHUNK_BYTES = 1024 * 1024;

function fileSizeAndCompleteBound(file) {
  const fd = fs.openSync(file, 'r');
  try {
    const stat = fs.fstatSync(fd), size = stat.size;
    if (size < 1) throw Error('QUEUE_CAPTURE_EMPTY');
    const block = Buffer.alloc(64 * 1024);
    let end = size;
    while (end > 0) {
      const start = Math.max(0, end - block.length), length = end - start;
      const n = fs.readSync(fd, block, 0, length, start);
      if (n !== length) throw Error('QUEUE_CAPTURE_READ');
      const newline = block.subarray(0, n).lastIndexOf(0x0a);
      if (newline >= 0) return { fileSizeAtFreeze: size, endByte: start + newline + 1, dev: stat.dev, ino: stat.ino };
      end = start;
    }
    throw Error('QUEUE_CAPTURE_NO_COMPLETE_LINES');
  } finally { fs.closeSync(fd); }
}

function lastCompleteRecordAt(file, endByte) {
  const fd = fs.openSync(file, 'r');
  try {
    let cursor = endByte - 1, lineStart = 0;
    const block = Buffer.alloc(64 * 1024);
    let found = false;
    while (cursor > 0 && !found) {
      const start = Math.max(0, cursor - block.length), length = cursor - start;
      const n = fs.readSync(fd, block, 0, length, start);
      if (n !== length) throw Error('QUEUE_CAPTURE_READ');
      const newline = block.subarray(0, n).lastIndexOf(0x0a);
      if (newline >= 0) { lineStart = start + newline + 1; found = true; }
      else cursor = start;
    }
    const length = endByte - 1 - lineStart;
    if (length <= 0) throw Error('QUEUE_CAPTURE_LAST_LINE');
    const bytes = Buffer.alloc(length), n = fs.readSync(fd, bytes, 0, length, lineStart);
    if (n !== length) throw Error('QUEUE_CAPTURE_READ');
    const row = parseJsonLine(bytes);
    if (typeof row.observedAt !== 'string') throw Error('QUEUE_CAPTURE_LAST_TIME');
    return row.observedAt;
  } finally { fs.closeSync(fd); }
}

function forEachCompleteLine(file, startByte, endByte, callback) {
  if (!Number.isSafeInteger(startByte) || !Number.isSafeInteger(endByte) || startByte < 0 || endByte <= startByte) throw Error('QUEUE_CAPTURE_RANGE');
  const fd = fs.openSync(file, 'r');
  try {
    const buffer = Buffer.alloc(CHUNK_BYTES);
    let readAt = startByte, pendingStart = startByte, pending = Buffer.alloc(0);
    while (readAt < endByte) {
      const requested = Math.min(buffer.length, endByte - readAt);
      const n = fs.readSync(fd, buffer, 0, requested, readAt);
      if (n !== requested) throw Error('QUEUE_CAPTURE_TRUNCATED');
      const piece = buffer.subarray(0, n);
      const data = pending.length ? Buffer.concat([pending, piece]) : piece;
      let cursor = 0;
      for (;;) {
        const newline = data.indexOf(0x0a, cursor);
        if (newline < 0) break;
        const lineStart = pendingStart + cursor, lineEnd = pendingStart + newline + 1;
        if (callback(data.subarray(cursor, newline), lineStart, lineEnd) === false) return { stoppedAt: lineEnd };
        cursor = newline + 1;
      }
      pending = Buffer.from(data.subarray(cursor));
      pendingStart += cursor;
      readAt += n;
    }
    if (pending.length || pendingStart !== endByte) throw Error('QUEUE_CAPTURE_INCOMPLETE_BOUND');
    return { stoppedAt: endByte };
  } finally { fs.closeSync(fd); }
}

function parseJsonLine(bytes) {
  try { return JSON.parse(bytes.toString('utf8')); }
  catch { throw Error('QUEUE_CAPTURE_JSON'); }
}

function addConfig(config, row) {
  if (row.kind !== 'config' || row.scope?.tournamentId !== TOURNAMENT_ID || !Array.isArray(row.scope.exchangeIds) || !row.scope.exchangeIds.length || typeof row.scope.marketId !== 'string') throw Error('QUEUE_CAPTURE_CONFIG');
  const prefix = `tournament:${TOURNAMENT_ID}:market:`;
  if (typeof row.topic !== 'string' || !row.topic.startsWith(prefix) || row.topic.slice(prefix.length) !== row.scope.marketId || row.scope.topic !== row.topic) throw Error('QUEUE_CAPTURE_CONFIG_TOPIC');
  const exchangeIds = [...new Set(row.scope.exchangeIds)];
  if (exchangeIds.some(id => typeof id !== 'string' || !id)) throw Error('QUEUE_CAPTURE_CONFIG_EXCHANGE');
  const previous = config.get(row.scope.marketId);
  if (previous && previous.join('\u0000') !== exchangeIds.join('\u0000')) throw Error('QUEUE_CAPTURE_CONFIG_CONFLICT');
  config.set(row.scope.marketId, exchangeIds);
}

function discoverPrefix(file, endByte) {
  const config = new Map();
  const configHash = createHash('sha256');
  let configBlock = true, configEndByte = 0, configRows = 0, startByte = null, firstAt = null;
  forEachCompleteLine(file, 0, endByte, (bytes, lineStart, lineEnd) => {
    const line = bytes.toString('utf8');
    if (configBlock) {
      const row = parseJsonLine(bytes);
      if (row.kind === 'config') {
        addConfig(config, row);
        configHash.update(bytes);
        configHash.update(Buffer.from('\n'));
        configRows++;
        return true;
      }
      configBlock = false;
      configEndByte = lineStart;
    }
    const match = /"observedAt":"([^"]+)"/.exec(line);
    if (match && match[1] >= CUTOFF_AT) {
      const row = parseJsonLine(bytes);
      if (row.observedAt !== match[1]) throw Error('QUEUE_CAPTURE_TIME_FIELD');
      startByte = lineStart;
      firstAt = row.observedAt;
      return false;
    }
    return true;
  });
  if (!configRows || configEndByte <= 0 || !config.size || startByte === null) throw Error('QUEUE_CAPTURE_PREFIX_NOT_FOUND');
  const members = [];
  for (const [marketId, exchangeIds] of config) for (const exchangeId of exchangeIds) members.push({ marketId, exchangeId });
  return { members, configRows, configEndByte, startByte, firstAt, discoveredConfigSha256: configHash.digest('hex') };
}

function topicMarket(row) {
  const prefix = `tournament:${TOURNAMENT_ID}:market:`;
  if (typeof row.topic !== 'string' || !row.topic.startsWith(prefix)) return null;
  return row.topic.slice(prefix.length);
}

function normalizedBatch(row, marketId, members) {
  const allowed = new Set(members.filter(member => member.marketId === marketId).map(member => member.exchangeId));
  const rawBooks = (row.payload?.books ?? []).map(book => {
    if (!allowed.has(book.exchangeId) || !book.asOf || !Number.isSafeInteger(book.asOf.sequence) || book.asOf.sequence < 0) throw Error('QUEUE_CAPTURE_BOOK_SCOPE');
    return {
      marketId, exchangeId: book.exchangeId, sequence: String(book.asOf.sequence), sourceAt: book.asOf.at,
      receivedAt: row.receivedAt, bids: book.bids, asks: book.asks,
    };
  });
  const rawTrades = (row.payload?.trades ?? []).map(trade => {
    if (trade.marketId !== marketId || !allowed.has(trade.exchangeId) || trade.tournamentId !== TOURNAMENT_ID) throw Error('QUEUE_CAPTURE_TRADE_SCOPE');
    return trade;
  });
  return { observedAt: row.observedAt, books: rawBooks, trades: rawTrades };
}

export function hasUnresolvedBookForTape(payload) {
  const currentBooks = new Set((payload?.books ?? []).map(book => book.exchangeId));
  return [...(payload?.bookDirty ?? []), ...(payload?.trades ?? [])].some(item => !currentBooks.has(item.exchangeId));
}

function runReplay(file, frozen) {
  const replay = createQueueDepletionRoundtripV1({ members: frozen.members, startAt: CUTOFF_AT });
  const metrics = { marketBatchRows: 0, rowsAfterCutoff: 0, rowsIgnoredAsConvenienceDuplicates: 0, resyncBoundaries: 0, lifecycleBoundaries: 0, dirtyMarketBoundaries: 0, dirtyHintsPairedWithBook: 0, missingBookBoundaries: 0, settledMarkets: 0, topicEpochBoundaries: 0, revisionGaps: 0 };
  const settled = new Set(), epochs = new Map(), revisions = new Map();
  let throughAt = null, lastObserved = null;
  let firstRow = true;
  forEachCompleteLine(file, frozen.startByte, frozen.endByte, (bytes) => {
    if (!bytes.length) throw Error('QUEUE_CAPTURE_EMPTY_LINE');
    const row = parseJsonLine(bytes);
    if (typeof row.observedAt !== 'string' || row.observedAt < CUTOFF_AT) throw Error('QUEUE_CAPTURE_PREFIX_ORDER');
    if (firstRow && row.observedAt !== frozen.firstAt) throw Error('QUEUE_CAPTURE_START_CHANGED');
    firstRow = false;
    if (lastObserved !== null && row.observedAt < lastObserved) throw Error('QUEUE_CAPTURE_CLOCK_ROLLBACK');
    lastObserved = row.observedAt;
    throughAt = row.observedAt;
    metrics.rowsAfterCutoff++;
    if (['disconnected', 'invalidation', 'recovery_started', 'recovery_finished', 'subscribed', 'stopped'].includes(row.kind)) {
      const lifecycleMarket = topicMarket(row);
      if (lifecycleMarket && !frozen.members.some(member => member.marketId === lifecycleMarket)) throw Error('QUEUE_CAPTURE_LIFECYCLE_SCOPE');
      replay.boundary(lifecycleMarket ? { marketId: lifecycleMarket } : {});
      metrics.lifecycleBoundaries++;
      return true;
    }
    if (row.kind !== 'event' || row.event !== 'market_batch') {
      if (row.kind === 'trade_observed') metrics.rowsIgnoredAsConvenienceDuplicates++;
      return true;
    }
    const marketId = topicMarket(row);
    if (!marketId || !frozen.members.some(member => member.marketId === marketId)) throw Error('QUEUE_CAPTURE_TOPIC_SCOPE');
    if (Number.isSafeInteger(row.epoch)) {
      const priorEpoch = epochs.get(marketId);
      if (priorEpoch !== undefined && priorEpoch !== row.epoch) {
        replay.boundary({ marketId });
        metrics.topicEpochBoundaries++;
      }
      epochs.set(marketId, row.epoch);
    }
    const payload = row.payload ?? {};
    if (payload.resyncRequired) {
      replay.boundary({ marketId });
      metrics.resyncBoundaries++;
      return true;
    }
    const delivery = payload.delivery;
    if (Number.isSafeInteger(delivery?.revision) && Number.isSafeInteger(delivery?.previousRevision)) {
      const previous = revisions.get(marketId);
      if (previous !== undefined && delivery.revision > previous && delivery.previousRevision > previous) {
        replay.boundary({ marketId });
        metrics.revisionGaps++;
      }
      if (previous === undefined || delivery.revision > previous) revisions.set(marketId, delivery.revision);
    }
    const currentBooks = new Set((payload.books ?? []).map(book => book.exchangeId));
    for (const dirty of payload.bookDirty ?? []) {
      if (dirty.marketId && dirty.marketId !== marketId) throw Error('QUEUE_CAPTURE_DIRTY_SCOPE');
      if (currentBooks.has(dirty.exchangeId)) metrics.dirtyHintsPairedWithBook++;
    }
    if (hasUnresolvedBookForTape(payload)) {
      replay.boundary({ marketId });
      metrics.dirtyMarketBoundaries++;
      metrics.missingBookBoundaries++;
      return true;
    }
    for (const item of payload.marketSettled ?? []) {
      const settledMarket = item.marketId ?? marketId;
      replay.boundary({ marketId: settledMarket });
      settled.add(settledMarket);
      metrics.settledMarkets++;
    }
    if (settled.has(marketId)) return true;
    replay.observeBatch(normalizedBatch(row, marketId, frozen.members));
    metrics.marketBatchRows++;
    return true;
  });
  if (!throughAt || throughAt < MIN_THROUGH_AT) throw Error('QUEUE_CAPTURE_HELDOUT_WINDOW_TOO_SHORT');
  const result = replay.report({ throughAt, source: {
    captureFileName: path.basename(file),
    tournamentId: TOURNAMENT_ID,
    cutoffAt: CUTOFF_AT,
    minimumThroughAt: MIN_THROUGH_AT,
    evaluationStartAt: frozen.firstAt,
    evaluationStartByte: frozen.startByte,
    evaluationEndByte: frozen.endByte,
    evaluationBytes: frozen.endByte - frozen.startByte,
    evaluationSha256: frozen.evaluationHash.sha256,
    scopeConfigStartByte: 0,
    scopeConfigEndByte: frozen.configEndByte,
    scopeConfigRows: frozen.configRows,
    scopeConfigSha256: frozen.scopeHash.sha256,
    fileSizeAtFreeze: frozen.fileSizeAtFreeze,
    throughAt,
    immutableBoundVerifiedAfterReplay: true,
    marketMappingCount: frozen.members.length,
    dataQuality: metrics,
  } });
  return result;
}

export function runQueueDepletionCapture({ file = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime', SESSION_NAME), output = OUTPUT_FILE, frozenPrefix = null } = {}) {
  const currentBound = fileSizeAndCompleteBound(file);
  const endByte = frozenPrefix?.endByte ?? currentBound.endByte;
  if (!Number.isSafeInteger(endByte) || endByte <= 0 || endByte > currentBound.endByte) throw Error('QUEUE_CAPTURE_FROZEN_BOUND');
  const fileSizeAtFreeze = frozenPrefix?.fileSizeAtFreeze ?? currentBound.fileSizeAtFreeze;
  const throughAt = lastCompleteRecordAt(file, endByte);
  if (throughAt < MIN_THROUGH_AT) throw Error('QUEUE_CAPTURE_HELDOUT_WINDOW_TOO_SHORT');
  const prefix = discoverPrefix(file, endByte);
  if (frozenPrefix && (prefix.startByte !== frozenPrefix.startByte || prefix.firstAt !== frozenPrefix.firstAt)) throw Error('QUEUE_CAPTURE_FROZEN_START');
  const scopeHash = hashByteRange(file, 0, prefix.configEndByte);
  const evaluationHash = hashByteRange(file, prefix.startByte, endByte);
  if (frozenPrefix?.sha256 && evaluationHash.sha256 !== frozenPrefix.sha256) throw Error('QUEUE_CAPTURE_FROZEN_HASH');
  if (scopeHash.sha256 !== prefix.discoveredConfigSha256 || scopeHash.dev !== currentBound.dev || scopeHash.ino !== currentBound.ino || evaluationHash.dev !== currentBound.dev || evaluationHash.ino !== currentBound.ino) throw Error('QUEUE_CAPTURE_CHANGED_DURING_FREEZE');
  const frozen = { ...prefix, endByte, fileSizeAtFreeze, scopeHash, evaluationHash };
  const result = runReplay(file, frozen);
  verifyByteRange(file, scopeHash);
  verifyByteRange(file, evaluationHash);
  try {
    fs.writeFileSync(output, `${JSON.stringify(result, null, 2)}\n`, { encoding: 'utf8', flag: 'wx' });
  } catch (error) {
    if (error?.code === 'EEXIST') throw Error('QUEUE_OUTPUT_EXISTS');
    throw error;
  }
  return result;
}

export { fileSizeAndCompleteBound, forEachCompleteLine, lastCompleteRecordAt };

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  try {
    const result = runQueueDepletionCapture();
    console.log(JSON.stringify({ verdict: result.verdict, counts: result.counts, aggregate: result.aggregate, source: result.source }, null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
