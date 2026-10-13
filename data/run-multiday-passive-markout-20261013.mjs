import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { amount, money } from '../paper/fixed.mjs';
import { sealedNativePrefix } from '../live-paper/native-dip-host-v1.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import { createConditionalFillMarkoutV1 } from '../research/conditional-fill-markout-v1.mjs';

const POLICY = Object.freeze({
  latencyMs: 3000,
  quoteTtlMs: 10000,
  sampleEveryMs: 30000,
  markoutHorizonsMs: [1000, 5000, 30000],
  maximumMarkoutDelayMs: 10000,
  exitResponseLatencyMs: 3000,
  maximumExecutableBookAgeMs: 3000,
  purgeMs: 60000,
  fees: 0,
});

const INPUTS = Object.freeze([
  Object.freeze({ day: '2026-10-05', role: 'development', file: 'realtime-2026-10-05T22-39-17-072Z-a9bb9f87-8ac2-4373-b7ec-b499ddf7176a.jsonl' }),
  Object.freeze({ day: '2026-10-06', role: 'development', file: 'realtime-2026-10-06T00-21-21-601Z-1175216a-92f4-49ad-9cb9-550c66ffb2ab.jsonl' }),
  Object.freeze({ day: '2026-10-13', role: 'untouched_held_out', file: 'realtime-2026-10-13T13-28-06-669Z-a7674ae9-ed0e-41c6-90c8-390b1ce0dc8b.jsonl' }),
]);

const OUTPUT = path.resolve('data/multiday-passive-markout-20261013.json');
const MAX_CAPTURE_RECORD_BYTES = 16 * 1024 * 1024;
const MAX_HEADER_SCAN_BYTES = 64 * 1024;
const HARD_INVALIDATIONS = new Set([
  'EXPIRY', 'STALE', 'BOOK_DIRTY', 'SCHEMA', 'MISSING_BOOK', 'SETTLEMENT',
  'RESYNC_REQUIRED', 'SOCKET_ERROR', 'RECONNECT', 'DISCONNECTED', 'REFRESH',
  'INITIAL', 'RECOVERY_FAILED',
]);
export const PRESPECIFIED_CELLS = Object.freeze([
  Object.freeze({ family: 'overall', bins: ['all'] }),
  Object.freeze({ family: 'bySpread', bins: ['1 tick', '2 ticks', '3+ ticks'] }),
  Object.freeze({ family: 'byQueueAhead', bins: ['1-10', '11-50', '51-250', '251+'] }),
  Object.freeze({ family: 'byAlignedFlowTercile', bins: ['lower tercile', 'middle tercile', 'upper tercile', 'missing'] }),
]);

const sha256 = value => createHash('sha256').update(value).digest('hex');
const validIso = value => typeof value === 'string' && Number.isFinite(Date.parse(value));
export const identityKey = member => [member.tournamentId, member.marketId, member.exchangeId].join('|');
const pairKey = member => [member.marketId, member.exchangeId].join('|');

// Read-only fixed-length line reader that also exposes exact source bytes.
export function* frozenCaptureLines(file, endByte, { chunkBytes = 1024 * 1024, maxRecordBytes = MAX_CAPTURE_RECORD_BYTES } = {}) {
  if (!Number.isSafeInteger(endByte) || endByte < 1 || !Number.isSafeInteger(chunkBytes) || chunkBytes < 1 || !Number.isSafeInteger(maxRecordBytes) || maxRecordBytes < chunkBytes) throw Error('MULTIDAY_READ_LIMIT');
  const fd = fs.openSync(file, 'r');
  const stat = fs.fstatSync(fd);
  if (!stat.isFile() || endByte > stat.size) {
    fs.closeSync(fd);
    throw Error('MULTIDAY_PREFIX_RANGE');
  }
  let offset = 0;
  let tail = Buffer.alloc(0);
  try {
    while (offset < endByte) {
      const buffer = Buffer.alloc(Math.min(chunkBytes, endByte - offset));
      const n = fs.readSync(fd, buffer, 0, buffer.length, offset);
      if (n !== buffer.length) throw Error('MULTIDAY_READ_TRUNCATED');
      offset += n;
      const data = tail.length ? Buffer.concat([tail, buffer]) : buffer;
      let begin = 0;
      let newline;
      while ((newline = data.indexOf(0x0a, begin)) >= 0) {
        if (newline - begin > maxRecordBytes) throw Error('MULTIDAY_RECORD_TOO_LARGE');
        const raw = data.subarray(begin, newline + 1);
        yield { text: raw.subarray(0, raw.length - 1).toString('utf8'), bytes: raw };
        begin = newline + 1;
      }
      tail = Buffer.from(data.subarray(begin));
      if (tail.length > maxRecordBytes) throw Error('MULTIDAY_RECORD_TOO_LARGE');
    }
    if (tail.length) throw Error('MULTIDAY_INCOMPLETE_PREFIX');
  } finally {
    fs.closeSync(fd);
  }
}

function readJsonString(fd, start, limit) {
  const bytes = [];
  let at = start;
  let escaped = false;
  const first = Buffer.allocUnsafe(1);
  if (fs.readSync(fd, first, 0, 1, at) !== 1 || first[0] !== 0x22) throw Error('MULTIDAY_HEADER_JSON');
  bytes.push(first[0]);
  at++;
  while (at - start <= limit) {
    const one = Buffer.allocUnsafe(1);
    if (fs.readSync(fd, one, 0, 1, at) !== 1) throw Error('MULTIDAY_HEADER_TRUNCATED');
    const byte = one[0];
    bytes.push(byte);
    at++;
    if (!escaped && byte === 0x22) {
      let value;
      try { value = JSON.parse(Buffer.from(bytes).toString('utf8')); } catch { throw Error('MULTIDAY_HEADER_JSON'); }
      return { value, next: at };
    }
    if (escaped) escaped = false;
    else if (byte === 0x5c) escaped = true;
    if (byte < 0x20 && byte !== 0x09 && byte !== 0x0a && byte !== 0x0d) throw Error('MULTIDAY_HEADER_JSON');
  }
  throw Error('MULTIDAY_HEADER_LIMIT');
}

// Stop as soon as the top-level kind value is known. In particular, a held-out
// market_batch payload is never read merely to identify the end of its header.
function scanTopLevelKind(fd, start, { maxBytes = MAX_HEADER_SCAN_BYTES } = {}) {
  let at = start;
  let depth = 0;
  while (at - start < maxBytes) {
    const one = Buffer.allocUnsafe(1);
    if (fs.readSync(fd, one, 0, 1, at) !== 1) throw Error('MULTIDAY_HEADER_TRUNCATED');
    const byte = one[0];
    if (byte === 0x22) {
      const token = readJsonString(fd, at, maxBytes - (at - start));
      at = token.next;
      if (depth === 1) {
        let look = at;
        let lookByte;
        do {
          const next = Buffer.allocUnsafe(1);
          if (fs.readSync(fd, next, 0, 1, look) !== 1) throw Error('MULTIDAY_HEADER_TRUNCATED');
          lookByte = next[0];
          if (lookByte === 0x20 || lookByte === 0x09 || lookByte === 0x0a || lookByte === 0x0d) look++;
          else break;
        } while (look - start < maxBytes);
        if (lookByte === 0x3a) {
          if (token.value === 'payload') throw Error('MULTIDAY_HEADER_PAYLOAD_BEFORE_KIND');
          if (token.value === 'kind') {
            look++;
            while (look - start < maxBytes) {
              const ws = Buffer.allocUnsafe(1);
              if (fs.readSync(fd, ws, 0, 1, look) !== 1) throw Error('MULTIDAY_HEADER_TRUNCATED');
              if (![0x20, 0x09, 0x0a, 0x0d].includes(ws[0])) break;
              look++;
            }
            const kind = readJsonString(fd, look, maxBytes - (look - start));
            return { kind: kind.value, next: kind.next };
          }
        }
      }
      continue;
    }
    at++;
    if (byte === 0x7b || byte === 0x5b) depth++;
    else if (byte === 0x7d || byte === 0x5d) depth--;
    if (depth < 0) throw Error('MULTIDAY_HEADER_JSON');
  }
  throw Error('MULTIDAY_HEADER_KIND_MISSING');
}

function readCompleteLine(fd, start, maxBytes = MAX_CAPTURE_RECORD_BYTES) {
  const chunks = [];
  let at = start;
  let total = 0;
  while (total < maxBytes) {
    const size = Math.min(64 * 1024, maxBytes - total);
    const buffer = Buffer.allocUnsafe(size);
    const n = fs.readSync(fd, buffer, 0, size, at);
    if (!n) throw Error('MULTIDAY_HEADER_TRUNCATED');
    const newline = buffer.subarray(0, n).indexOf(0x0a);
    const used = newline < 0 ? n : newline + 1;
    chunks.push(buffer.subarray(0, used));
    at += used;
    total += used;
    if (newline >= 0) return { bytes: Buffer.concat(chunks, total), next: at };
  }
  throw Error('MULTIDAY_RECORD_TOO_LARGE');
}

export function comparableScope(scopes) {
  if (!Array.isArray(scopes) || scopes.length < 2) throw Error('MULTIDAY_SCOPE_COUNT');
  const tournamentIds = new Set(scopes.flatMap(scope => [...scope.members.values()].map(member => member.tournamentId)));
  if (tournamentIds.size !== 1) throw Error('MULTIDAY_TOURNAMENT_MISMATCH');
  const allExchangeMappings = new Map();
  for (const scope of scopes) {
    for (const member of scope.members.values()) {
      const prior = allExchangeMappings.get(member.exchangeId);
      if (prior !== undefined && prior !== identityKey(member)) throw Error('MULTIDAY_EXCHANGE_ID_COLLISION');
      allExchangeMappings.set(member.exchangeId, identityKey(member));
    }
  }
  const [first, ...rest] = scopes;
  const members = [...first.members.values()]
    .filter(member => rest.every(scope => scope.members.has(identityKey(member))))
    .sort((a, b) => identityKey(a).localeCompare(identityKey(b)));
  if (!members.length) throw Error('MULTIDAY_NO_COMPARABLE_MARKETS');
  return {
    tournamentId: [...tournamentIds][0],
    members,
    keys: members.map(identityKey),
    sha256: sha256(members.map(identityKey).join('\n')),
    sourceIdentityCounts: scopes.map(scope => scope.members.size),
    comparableIdentityCount: members.length,
  };
}

export function captureHeader(file, { maxConfigs = 4096 } = {}) {
  const fd = fs.openSync(file, 'r');
  try {
    const members = new Map();
    let offset = 0;
    let recordIndex = 0;
    let firstObservedAt = null;
    let stoppedAtKind = null;
    for (; recordIndex < maxConfigs; recordIndex++) {
      const kindPrefix = scanTopLevelKind(fd, offset);
      if (kindPrefix.kind !== 'config') {
        stoppedAtKind = kindPrefix.kind;
        break;
      }
      const line = readCompleteLine(fd, offset);
      let record;
      try { record = JSON.parse(line.bytes.subarray(0, line.bytes.length - 1).toString('utf8')); } catch { throw Error('MULTIDAY_HEADER_JSON'); }
      if (record.format !== 'sig-feed-capture' || record.captureIndex !== recordIndex || record.kind !== 'config') throw Error('MULTIDAY_CAPTURE_HEADER_CHAIN');
      if (!validIso(record.observedAt)) throw Error('MULTIDAY_CAPTURE_CLOCK');
      firstObservedAt ??= record.observedAt;
      const scope = record.scope;
      if (!scope || typeof scope.tournamentId !== 'string' || typeof scope.marketId !== 'string' || !Array.isArray(scope.exchangeIds) || !scope.exchangeIds.length) throw Error('MULTIDAY_CONFIG_SCOPE');
      const topic = 'tournament:' + scope.tournamentId + ':market:' + scope.marketId;
      if (record.topic !== topic || scope.topic !== undefined && scope.topic !== topic) throw Error('MULTIDAY_CONFIG_TOPIC');
      for (const rawExchangeId of scope.exchangeIds) {
        const exchangeId = String(rawExchangeId);
        if (!/^[A-Za-z0-9_-]+$/.test(exchangeId)) throw Error('MULTIDAY_EXCHANGE_ID');
        const member = { tournamentId: scope.tournamentId, marketId: scope.marketId, exchangeId, topic };
        const key = identityKey(member);
        const prior = members.get(key);
        if (prior && prior.topic !== topic) throw Error('MULTIDAY_DUPLICATE_IDENTITY');
        members.set(key, member);
      }
      offset = line.next;
    }
    if (!firstObservedAt || !members.size || !stoppedAtKind) throw Error('MULTIDAY_HEADER_INCOMPLETE');
    return { members, firstObservedAt, headerRecords: recordIndex, headerPrefixBytes: offset, stoppedAtKind };
  } finally {
    fs.closeSync(fd);
  }
}

function prepareInputs(runtimeDir, { devOnly }) {
  const sources = INPUTS.map(input => {
    if (!/^realtime-2026-10-(05|06|13)T[A-Za-z0-9.-]+\.jsonl$/.test(input.file)) throw Error('MULTIDAY_SOURCE_NAME');
    const file = path.join(runtimeDir, input.file);
    const before = fs.statSync(file);
    if (!before.isFile()) throw Error('MULTIDAY_SOURCE_FILE');
    const header = captureHeader(file);
    if (header.firstObservedAt.slice(0, 10) !== input.day) throw Error('MULTIDAY_SOURCE_DAY');
    // The dev-only path reads only Oct 13's config header; it does not even
    // inspect the tail to find a final newline or a frozen outcome prefix.
    const mayReadOutcomes = !(devOnly && input.role === 'untouched_held_out');
    const sealed = mayReadOutcomes ? sealedNativePrefix(file) : null;
    if (sealed && (sealed.bytes < header.headerPrefixBytes || sealed.bytes > before.size)) throw Error('MULTIDAY_SOURCE_SEAL');
    return {
      ...input, filePath: file, frozenBytes: sealed?.bytes ?? null, fileBytesAtFreeze: before.size,
      sourceDev: before.dev, sourceIno: before.ino, header,
    };
  });
  const scope = comparableScope(sources.map(source => source.header));
  for (const source of sources) source.excludedIdentityCount = source.header.members.size - scope.comparableIdentityCount;
  return { sources, scope };
}

const scopeForReader = scope => ({ tournamentId: scope.tournamentId, exchanges: scope.members });

function makeStudy() {
  return createConditionalFillMarkoutV1(POLICY);
}

function makeStudies(scope) {
  return new Map(scope.members.map(member => [identityKey(member), { member, study: makeStudy() }]));
}

function createPanel(scope, studies) {
  const membersByTopic = new Map();
  const memberByPair = new Map();
  for (const member of scope.members) {
    const members = membersByTopic.get(member.topic) ?? [];
    members.push(member);
    membersByTopic.set(member.topic, members);
    memberByPair.set(pairKey(member), member);
  }
  return {
    scope: scopeForReader(scope),
    members: scope.members,
    membersByTopic,
    memberByPair,
    studies,
    sampledAt: new Map(),
    revisions: new Map(),
    epochs: new Map(),
    tradeReceiptLagById: new Map(),
    recordCount: 0,
    bookCount: 0,
    acceptedPoints: 0,
    rejectedPoints: 0,
    firstObservedAt: null,
    through: null,
    lastObservedMs: -Infinity,
  };
}

function scopedTrade(panel, trade) {
  if (!trade || trade.tournamentId !== panel.scope.tournamentId || typeof trade.marketId !== 'string' || typeof trade.exchangeId !== 'string') return null;
  const member = panel.memberByPair.get(pairKey(trade));
  if (!member) return null;
  const id = typeof trade.id === 'string' ? trade.id : String(trade.sequence);
  const sequence = Number(trade.sequence), quantity = trade.quantity;
  if (!Number.isSafeInteger(sequence) || sequence < 0 || String(sequence) !== id || !Number.isSafeInteger(quantity) || quantity < 1) return null;
  return { marketId: trade.marketId, exchangeId: trade.exchangeId, id, sequence, price: trade.price, quantity, executedAt: trade.executedAt, member };
}

function noteTradeLag(panel, trade, observedAt) {
  const key = [trade.marketId, trade.exchangeId, trade.id].join('|');
  if (panel.tradeReceiptLagById.has(key)) return;
  const lagMs = Date.parse(observedAt) - Date.parse(trade.executedAt);
  if (Number.isSafeInteger(lagMs) && lagMs >= 0) panel.tradeReceiptLagById.set(key, lagMs);
}

function clearTopic(panel, topic, at) {
  for (const member of panel.membersByTopic.get(topic) ?? []) {
    panel.studies.get(identityKey(member)).study.boundary(member.exchangeId, at);
  }
}

function clearMarket(panel, topic, exchangeIds, at) {
  const members = panel.membersByTopic.get(topic) ?? [];
  const selected = exchangeIds === null ? members : members.filter(member => exchangeIds.has(member.exchangeId));
  for (const member of selected) panel.studies.get(identityKey(member)).study.boundary(member.exchangeId, at);
}

function resetTopicFeatures(panel, topic) {
  for (const member of panel.membersByTopic.get(topic) ?? []) panel.studies.get(identityKey(member)).study.resetFeatures(member.exchangeId);
}

function observeRecord(panel, record, bookPoints = null) {
  panel.recordCount++;
  panel.firstObservedAt ??= record.observedAt;
  panel.through = record.observedAt;
  const observedMs = Date.parse(record.observedAt);
  if (!Number.isFinite(observedMs) || observedMs < panel.lastObservedMs) throw Error('MULTIDAY_CAPTURE_CLOCK_ROLLBACK');
  panel.lastObservedMs = observedMs;
  if (record.kind === 'trade_observed') {
    const trade = scopedTrade(panel, record.trade);
    if (trade) {
      noteTradeLag(panel, trade, record.observedAt);
      panel.studies.get(identityKey(trade.member)).study.observeBatch({ observedAt: record.observedAt, trades: [trade] });
    }
    return;
  }
  const topic = record.topic;
  if (typeof topic !== 'string' || !panel.membersByTopic.has(topic)) return;
  if (['disconnected', 'stopped', 'subscribed', 'rejected'].includes(record.kind)) {
    clearTopic(panel, topic, record.observedAt);
    panel.epochs.delete(topic);
    return;
  }
  if (record.kind === 'invalidation') {
    const reason = String(record.reason ?? '');
    const only = record.exchangeId ? new Set([String(record.exchangeId)]) : null;
    if (HARD_INVALIDATIONS.has(reason)) clearMarket(panel, topic, only, record.observedAt);
    else if (reason === 'GAP' || reason === 'UNSEQUENCED') resetTopicFeatures(panel, topic);
    return;
  }
  if (record.kind !== 'event' || record.event !== 'market_batch') {
    if (record.kind === 'event') clearTopic(panel, topic, record.observedAt);
    return;
  }
  const payload = record.payload;
  if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
    clearTopic(panel, topic, record.observedAt);
    return;
  }
  const epochKey = topic + ':' + record.epoch;
  const previousEpoch = panel.epochs.get(topic);
  if (previousEpoch !== undefined && previousEpoch !== record.epoch) clearTopic(panel, topic, record.observedAt);
  panel.epochs.set(topic, record.epoch);
  const delivery = payload.delivery;
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) throw Error('MULTIDAY_REVISION');
  const priorRevision = panel.revisions.get(epochKey);
  if (priorRevision !== undefined && delivery.previousRevision !== priorRevision) resetTopicFeatures(panel, topic);
  if (priorRevision !== undefined && delivery.revision <= priorRevision) return;
  panel.revisions.set(epochKey, delivery.revision);

  const booksByExchange = new Map();
  for (const book of bookPoints?.books ?? []) {
    const list = booksByExchange.get(book.exchangeId) ?? [];
    list.push(book);
    booksByExchange.set(book.exchangeId, list);
  }
  panel.bookCount += (payload.books ?? []).length;
  panel.acceptedPoints += bookPoints?.books.length ?? 0;
  panel.rejectedPoints += bookPoints?.rejected.length ?? 0;
  const tradesByExchange = new Map();
  for (const raw of payload.trades ?? []) {
    const trade = scopedTrade(panel, raw);
    if (!trade) continue;
    noteTradeLag(panel, trade, record.observedAt);
    const list = tradesByExchange.get(trade.exchangeId) ?? [];
    list.push(trade);
    tradesByExchange.set(trade.exchangeId, list);
  }
  for (const member of panel.membersByTopic.get(topic) ?? []) {
    const sampleKey = topic + ':' + record.epoch + ':' + member.exchangeId;
    const lastSample = panel.sampledAt.get(sampleKey) ?? -Infinity;
    let books = [];
    if (observedMs - lastSample >= 1000) {
      panel.sampledAt.set(sampleKey, observedMs);
      books = booksByExchange.get(member.exchangeId) ?? [];
    }
    const trades = tradesByExchange.get(member.exchangeId) ?? [];
    if (books.length || trades.length) panel.studies.get(identityKey(member)).study.observeBatch({ observedAt: record.observedAt, books, trades });
  }
}

function tradeLagSummary(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const at = p => sorted.length ? sorted[Math.max(0, Math.ceil(sorted.length * p) - 1)] : null;
  return { uniqueTradePrints: sorted.length, p50Ms: at(0.5), p90Ms: at(0.9), p99Ms: at(0.99), maxMs: sorted.length ? sorted.at(-1) : null };
}

function verifyFrozenFile(source) {
  const after = fs.statSync(source.filePath);
  if (after.dev !== source.sourceDev || after.ino !== source.sourceIno || after.size < source.frozenBytes) throw Error('MULTIDAY_SOURCE_REPLACED');
}

function replaySource(source, panels) {
  if (!Number.isSafeInteger(source.frozenBytes) || source.frozenBytes < 1) throw Error('MULTIDAY_SOURCE_NOT_SEALED');
  const digest = createHash('sha256');
  let firstObservedAt = null;
  let through = null;
  let recordIndex = 0;
  let lastMs = -Infinity;
  for (const row of frozenCaptureLines(source.filePath, source.frozenBytes)) {
    digest.update(row.bytes);
    let record;
    try { record = JSON.parse(row.text); } catch { throw Error('MULTIDAY_CAPTURE_JSON'); }
    if (record.format !== 'sig-feed-capture' || record.captureIndex !== recordIndex++) throw Error('MULTIDAY_CAPTURE_CHAIN');
    if (!validIso(record.observedAt)) throw Error('MULTIDAY_CAPTURE_CLOCK');
    const observedMs = Date.parse(record.observedAt);
    if (observedMs < lastMs) throw Error('MULTIDAY_CAPTURE_CLOCK_ROLLBACK');
    lastMs = observedMs;
    firstObservedAt ??= record.observedAt;
    through = record.observedAt;
    let points = null;
    if (record.kind === 'event' && record.event === 'market_batch') {
      points = engineBookPointsV1(record, panels[0].scope, record.observedAt);
    }
    for (const panel of panels) observeRecord(panel, record, points);
  }
  if (!recordIndex || recordIndex <= source.header.headerRecords || !firstObservedAt || !through) throw Error('MULTIDAY_EMPTY_CAPTURE');
  const actualHash = digest.digest('hex');
  if (Date.parse(firstObservedAt) <= Date.parse(source.header.firstObservedAt) - 1000) throw Error('MULTIDAY_HEADER_AFTER_PREFIX');
  verifyFrozenFile(source);
  return { hash: actualHash, records: recordIndex, firstObservedAt, through };
}

function bindCaptureBoundary(panel, at) {
  for (const member of panel.members) panel.studies.get(identityKey(member)).study.boundary(member.exchangeId, at);
}

function captureSourceSummary(source, prefixHash, panel, replayed) {
  if (panel.recordCount !== replayed.records || panel.firstObservedAt !== replayed.firstObservedAt || panel.through !== replayed.through) throw Error('MULTIDAY_SOURCE_SUMMARY');
  return {
    day: source.day,
    role: source.role,
    file: source.file,
    prefixBytes: source.frozenBytes,
    fileBytesAtFreeze: source.fileBytesAtFreeze,
    sha256: prefixHash,
    captureRecords: panel.recordCount,
    from: panel.firstObservedAt,
    through: panel.through,
    scopeIdentity: {
      sourceIdentityCount: source.header.members.size,
      comparableIdentityCount: panel.scope.exchanges.length,
      excludedIdentityCount: source.excludedIdentityCount,
      sha256: sha256([...source.header.members.keys()].sort().join('\n')),
    },
    acceptedBookPoints: panel.acceptedPoints,
    rejectedBookPoints: panel.rejectedPoints,
    scopedUniqueTradePrints: panel.tradeReceiptLagById.size,
    tradeReportObservationLagMs: tradeLagSummary(panel.tradeReceiptLagById.values()),
  };
}

function tableCells(table) {
  const values = [{ family: 'overall', bin: 'all', metric: table }];
  for (const spec of PRESPECIFIED_CELLS.slice(1)) {
    for (const bin of spec.bins) {
      const metric = table[spec.family].find(row => row.bin === bin);
      if (!metric) throw Error('MULTIDAY_PRESPECIFIED_CELL_MISSING');
      values.push({ family: spec.family, bin, metric });
    }
  }
  return values;
}

function groupedMarketRows(rows) {
  const clusters = new Map();
  for (const row of rows) {
    const cluster = clusters.get(row.marketId) ?? {
      marketId: row.marketId, quoteCount: 0, strictThroughProxies: 0,
      executableSamples: 0, executableNetWeightedMicro: 0n, midWeightedMicro: 0n,
      stressWeightedMicro: 0n, positiveWeighted: 0, eventSamples: 0,
      eventNetWeightedMicro: 0n, eventMidWeightedMicro: 0n, eventPositiveWeighted: 0,
    };
    cluster.quoteCount += row.candidateQuotes ?? 0;
    cluster.strictThroughProxies += row.strictThroughProxies ?? 0;
    const executable = row.executable;
    const n = Number(executable?.samples ?? 0);
    if (n && executable.meanExecutableNet !== null) {
      cluster.executableSamples += n;
      cluster.executableNetWeightedMicro += amount(executable.meanExecutableNet) * BigInt(n);
      if (executable.meanMidMarkout !== null) cluster.midWeightedMicro += amount(executable.meanMidMarkout) * BigInt(n);
      if (executable.oneTickEachWayStressMean !== null) cluster.stressWeightedMicro += amount(executable.oneTickEachWayStressMean) * BigInt(n);
      if (Number.isFinite(executable.positiveFraction)) cluster.positiveWeighted += executable.positiveFraction * n;
    }
    const event = row.eventTimeDiagnostic;
    const en = Number(event?.samples ?? 0);
    if (en && event.meanExecutableNet !== null) {
      cluster.eventSamples += en;
      cluster.eventNetWeightedMicro += amount(event.meanExecutableNet) * BigInt(en);
      if (event.meanMidMarkout !== null) cluster.eventMidWeightedMicro += amount(event.meanMidMarkout) * BigInt(en);
      if (Number.isFinite(event.positiveFraction)) cluster.eventPositiveWeighted += event.positiveFraction * en;
    }
    clusters.set(row.marketId, cluster);
  }
  return [...clusters.values()];
}

function kishEffective(values) {
  const total = values.reduce((sum, value) => sum + value, 0);
  const squares = values.reduce((sum, value) => sum + value * value, 0);
  return total > 0 && squares > 0 ? total * total / squares : 0;
}

function clusterMean(clusters, meanField, nField) {
  const usable = clusters.filter(cluster => cluster[nField] > 0);
  const n = usable.reduce((sum, cluster) => sum + cluster[nField], 0);
  if (!n) return { samples: 0, marketClusters: 0, mean: null, equalMarketMean: null, standardError: null, ci95: null, effectiveIndependentMarkets: 0 };
  const weighted = usable.reduce((sum, cluster) => sum + cluster[meanField] * cluster[nField], 0) / n;
  const equalMarketMean = usable.reduce((sum, cluster) => sum + cluster[meanField], 0) / usable.length;
  const g = usable.length;
  let standardError = null;
  let ci95 = null;
  if (g >= 2) {
    const centered = usable.reduce((sum, cluster) => sum + Math.pow(cluster[nField] * (cluster[meanField] - weighted), 2), 0);
    standardError = Math.sqrt(g / (g - 1) * centered) / n;
    ci95 = [weighted - 1.96 * standardError, weighted + 1.96 * standardError];
  }
  return {
    samples: n,
    marketClusters: g,
    mean: money(BigInt(Math.round(weighted * 1_000_000))),
    equalMarketMean: money(BigInt(Math.round(equalMarketMean * 1_000_000))),
    standardError: standardError === null ? null : Number(standardError.toFixed(8)),
    ci95: ci95 === null ? null : ci95.map(value => Number(value.toFixed(8))),
    effectiveIndependentMarkets: kishEffective(usable.map(cluster => cluster[nField])),
  };
}

export function summarizeMarketClusters(rows) {
  const clusters = groupedMarketRows(rows);
  const quoteMarkets = clusters.filter(cluster => cluster.quoteCount > 0);
  const exec = clusters.map(cluster => ({
    marketId: cluster.marketId, n: cluster.executableSamples,
    mean: cluster.executableSamples ? Number(cluster.executableNetWeightedMicro / BigInt(cluster.executableSamples)) / 1_000_000 : null,
  })).filter(cluster => cluster.n > 0);
  const mid = clusters.map(cluster => ({
    marketId: cluster.marketId, n: cluster.executableSamples,
    mean: cluster.executableSamples ? Number(cluster.midWeightedMicro / BigInt(cluster.executableSamples)) / 1_000_000 : null,
  })).filter(cluster => cluster.n > 0);
  const stress = clusters.map(cluster => ({
    marketId: cluster.marketId, n: cluster.executableSamples,
    mean: cluster.executableSamples ? Number(cluster.stressWeightedMicro / BigInt(cluster.executableSamples)) / 1_000_000 : null,
  })).filter(cluster => cluster.n > 0);
  const events = clusters.map(cluster => ({
    marketId: cluster.marketId, n: cluster.eventSamples,
    mean: cluster.eventSamples ? Number(cluster.eventNetWeightedMicro / BigInt(cluster.eventSamples)) / 1_000_000 : null,
  })).filter(cluster => cluster.n > 0);
  const eventMid = clusters.map(cluster => ({
    marketId: cluster.marketId, n: cluster.eventSamples,
    mean: cluster.eventSamples ? Number(cluster.eventMidWeightedMicro / BigInt(cluster.eventSamples)) / 1_000_000 : null,
  })).filter(cluster => cluster.n > 0);
  const eventPositive = clusters.map(cluster => ({
    marketId: cluster.marketId, n: cluster.eventSamples,
    mean: cluster.eventSamples ? cluster.eventPositiveWeighted / cluster.eventSamples : null,
  })).filter(cluster => cluster.n > 0);
  const positive = clusters.map(cluster => ({
    marketId: cluster.marketId, n: cluster.executableSamples,
    mean: cluster.executableSamples ? cluster.positiveWeighted / cluster.executableSamples : null,
  })).filter(cluster => cluster.n > 0);
  const executableSummary = clusterMean(exec, 'mean', 'n');
  const midSummary = clusterMean(mid, 'mean', 'n');
  const stressSummary = clusterMean(stress, 'mean', 'n');
  const eventSummary = clusterMean(events, 'mean', 'n');
  const eventMidSummary = clusterMean(eventMid, 'mean', 'n');
  const eventPositiveSummary = clusterMean(eventPositive, 'mean', 'n');
  const positiveSummary = clusterMean(positive, 'mean', 'n');
  const quoteCount = clusters.reduce((sum, cluster) => sum + cluster.quoteCount, 0);
  const strictThroughProxies = clusters.reduce((sum, cluster) => sum + cluster.strictThroughProxies, 0);
  const markoutSamples = clusters.reduce((sum, cluster) => sum + cluster.executableSamples, 0);
  const eventTimeDiagnosticSamples = clusters.reduce((sum, cluster) => sum + cluster.eventSamples, 0);
  return {
    candidateQuotes: quoteCount,
    strictThroughProxies,
    markoutSamples,
    quoteMarketClusters: quoteMarkets.length,
    effectiveIndependentQuoteMarkets: kishEffective(quoteMarkets.map(cluster => cluster.quoteCount)),
    markoutMarketClusters: executableSummary.marketClusters,
    effectiveIndependentMarkoutMarkets: executableSummary.effectiveIndependentMarkets,
    eventTimeDiagnosticSamples,
    eventMarkoutMarketClusters: eventSummary.marketClusters,
    effectiveIndependentEventMarkets: eventSummary.effectiveIndependentMarkets,
    proxyFillRate: quoteCount ? strictThroughProxies / quoteCount : null,
    eventTimeDiagnostic: {
      samples: eventSummary.samples,
      meanExecutableNet: eventSummary.mean,
      equalMarketMeanExecutableNet: eventSummary.equalMarketMean,
      meanMidMarkout: eventMidSummary.mean,
      positiveFraction: eventPositiveSummary.mean === null ? null : Number(eventPositiveSummary.mean),
      marketClusters: eventSummary.marketClusters,
      effectiveIndependentMarkets: eventSummary.effectiveIndependentMarkets,
      marketClusterStandardError: eventSummary.standardError,
      approximate95PercentCI: eventSummary.ci95,
    },
    executable: {
      samples: executableSummary.samples,
      meanExecutableNet: executableSummary.mean,
      equalMarketMeanExecutableNet: executableSummary.equalMarketMean,
      meanMidMarkout: midSummary.mean,
      oneTickEachWayStressMean: stressSummary.mean,
      positiveFraction: positiveSummary.mean === null ? null : Number(positiveSummary.mean),
      marketClusters: executableSummary.marketClusters,
      effectiveIndependentMarkets: executableSummary.effectiveIndependentMarkets,
      marketClusterStandardError: executableSummary.standardError,
      approximate95PercentCI: executableSummary.ci95,
    },
  };
}

export function aggregatePrespecifiedCells(marketReports, folds) {
  const accumulators = new Map();
  for (const fold of folds) for (const action of ['buy', 'sell']) for (const horizonMs of POLICY.markoutHorizonsMs) {
    for (const spec of PRESPECIFIED_CELLS) for (const bin of spec.bins) {
      const id = [fold, action, horizonMs, spec.family, bin].join('|');
      accumulators.set(id, { fold, action, horizonMs, cellFamily: spec.family, cell: bin, rows: [] });
    }
  }
  for (const marketReport of marketReports) {
    for (const table of marketReport.tables) {
      if (!folds.includes(table.fold)) continue;
      for (const cell of tableCells(table)) {
        const id = [table.fold, table.action, table.horizonMs, cell.family, cell.bin].join('|');
        const accumulator = accumulators.get(id);
        if (!accumulator) throw Error('MULTIDAY_UNPRESPECIFIED_CELL');
        const metric = cell.metric;
        accumulator.rows.push({
          marketId: marketReport.marketId,
          candidateQuotes: metric.candidateQuotes,
          strictThroughProxies: metric.strictThroughProxies,
          executable: metric.executable,
          eventTimeDiagnostic: metric.eventTimeDiagnostic,
        });
      }
    }
  }
  return [...accumulators.values()].map(({ rows, ...cell }) => ({ ...cell, ...summarizeMarketClusters(rows) }));
}

function summarizePanel(panel, splitAt) {
  const reports = [];
  const flowCalibration = [];
  for (const { member, study } of panel.studies.values()) {
    const report = study.report({ splitAt });
    reports.push({
      marketId: member.marketId,
      exchangeId: member.exchangeId,
      tables: report.tables,
      counts: report.counts,
      flowThresholds: report.flowThresholds,
    });
    flowCalibration.push({
      marketId: member.marketId,
      exchangeId: member.exchangeId,
      thresholds: report.flowThresholds,
      quoteCount: report.counts.flowCalibrationQuotes,
    });
  }
  const calibration = flowCalibration.filter(row => row.thresholds);
  return {
    cells: aggregatePrespecifiedCells(reports, ['development', 'heldOut']),
    flowCalibration: {
      method: 'each stable market/exchange identity uses its own chronological development-only quote-flow terciles; no cross-market feature pooling',
      calibratedIdentityCount: calibration.length,
      uncalibratedIdentityCount: flowCalibration.length - calibration.length,
      thresholdsByMarket: calibration,
    },
    counts: {
      candidateQuotesByFold: reports.reduce((counts, report) => {
        for (const [fold, n] of Object.entries(report.counts.candidateQuotesByFold)) counts[fold] = (counts[fold] ?? 0) + n;
        return counts;
      }, {}),
      strictThroughProxiesByFold: Object.fromEntries(['development', 'heldOut'].map(fold => [fold,
        aggregatePrespecifiedCells(reports, [fold])
          .filter(cell => cell.cellFamily === 'overall' && cell.horizonMs === POLICY.markoutHorizonsMs[0])
          .reduce((sum, cell) => sum + cell.strictThroughProxies, 0),
      ])),
    },
  };
}

function summarizeDaily(source, panel, replayed) {
  bindCaptureBoundary(panel, replayed.through);
  const meta = captureSourceSummary(source, replayed.hash, panel, replayed);
  const report = summarizePanel(panel, new Date(Date.parse(replayed.through) + POLICY.purgeMs + 1).toISOString());
  return {
    day: source.day,
    role: 'development_only',
    source: meta,
    fold: 'development only; descriptive and not independent validation',
    candidateQuotesByFold: report.counts.candidateQuotesByFold,
    flowCalibration: report.flowCalibration,
    cells: report.cells.filter(cell => cell.fold === 'development'),
  };
}

function ensureChronological(sources, replays) {
  for (let i = 0; i < replays.length; i++) {
    if (sources[i].day !== INPUTS[i].day) throw Error('MULTIDAY_SOURCE_ORDER');
    if (i && Date.parse(replays[i].firstObservedAt) <= Date.parse(replays[i - 1].through)) throw Error('MULTIDAY_SOURCE_ORDER');
  }
  if (sources.at(-1).day !== '2026-10-13' || sources.at(-1).role !== 'untouched_held_out') throw Error('MULTIDAY_HOLDOUT_PLAN');
  if (sources.slice(0, -1).some(source => source.day >= '2026-10-13' || source.role !== 'development')) throw Error('MULTIDAY_DEVELOPMENT_PLAN');
}

function createOutput({ sources, scope, daily, combined, splitAt, sourceSummaries, replayCounts }) {
  return {
    schemaVersion: 2,
    mode: 'sealed_multiday_receipt_causal_passive_markout_v1',
    status: 'complete',
    runMode: 'fixed_sources_fixed_policy_one_pass',
    policy: {
      quote: 'captured best-price YES bid/ask; one paired hypothetical quote per comparable exchange every 30 seconds',
      activationLatencyMs: POLICY.latencyMs,
      quoteTtlAfterActivationMs: POLICY.quoteTtlMs,
      markoutHorizonsMs: POLICY.markoutHorizonsMs,
      exitResponseLatencyMs: POLICY.exitResponseLatencyMs,
      maximumMarkoutDelayMs: POLICY.maximumMarkoutDelayMs,
      maximumExecutableBookAgeMs: POLICY.maximumExecutableBookAgeMs,
      purgeMs: POLICY.purgeMs,
      fees: '0.000000',
      stress: 'separate one-tick each-way deduction of 0.010000 SUSQie per unit',
      fillProxy: 'one captured same-market print at least one tick through limit after activation; proxy only, no aggressor side or queue position',
      entryAndExit: 'entry joins captured best bid/ask; liquidation crosses to captured future bid/ask after event horizon and 3-second post-receipt response',
      marketClusterUncertainty: 'cluster-robust CR1 standard error of the sample-weighted mean by stable marketId; Kish effective independent-market counts use per-market quote or markout sample counts; approximate normal 95% interval only when at least two market clusters contribute',
    },
    scopeComparability: {
      tournamentId: scope.tournamentId,
      sourceIdentityCounts: sources.map(source => ({ day: source.day, count: source.header.members.size })),
      commonIdentityCount: scope.comparableIdentityCount,
      commonIdentitySha256: scope.sha256,
      commonIdentities: scope.members.map(({ marketId, exchangeId }) => ({ marketId, exchangeId })),
      poolingRule: 'only exact tournamentId/marketId/exchangeId identities in every frozen source are pooled; exchange-id remaps are rejected; uncertainty is clustered by marketId, not quote',
    },
    sources: sourceSummaries,
    developmentDays: daily,
    developmentAggregate: {
      role: 'development_only',
      cutoff: splitAt,
      featureCalibration: combined.flowCalibration,
      candidateQuotesByFold: combined.counts.candidateQuotesByFold,
      cells: combined.cells.filter(cell => cell.fold === 'development'),
    },
    holdout: {
      day: '2026-10-13',
      role: 'untouched_held_out',
      splitAt,
      developmentFeatureCalibration: combined.flowCalibration,
      candidateQuotesByFold: combined.counts.candidateQuotesByFold,
      strictThroughProxyCount: combined.counts.strictThroughProxiesByFold.heldOut,
      acceptedBookPoints: replayCounts.holdoutAcceptedBookPoints,
      rejectedBookPoints: replayCounts.holdoutRejectedBookPoints,
      tradeReportObservationLagMs: replayCounts.holdoutTradeReceiptLagMs,
      cells: combined.cells.filter(cell => cell.fold === 'heldOut'),
    },
    cellDisclosure: {
      selection: 'every prespecified overall, spread, queue-ahead and aligned-flow cell is included, including empty cells; no best-cell selection or holdout-driven parameter choices',
      bins: PRESPECIFIED_CELLS,
      flowBins: 'market/exchange-specific terciles fitted only from that identity’s development quote features; ties use evaluator’s strict-below/equal/strictly-above rule',
      counts: 'quote counts and proxy fills are descriptive opportunities; repeated quotes are dependent and are not independent trials',
      uncertainty: 'effective market counts and clustered intervals are descriptive, approximate, and not a profitability guarantee',
    },
    limits: [
      'All results are captured-book counterfactual proxies, not accepted contest orders or realized fills; queue priority, aggressor side, capital constraints, and venue execution are unknown.',
      'Each source has isolated receipt/revision state and all pending hypothetical quotes are cleared at every capture boundary; markouts are receipt/response gated and censored across gaps.',
      'October 13 is used once as the final chronological holdout; only configuration headers were inspected before the frozen run. Flow cutpoints come from October 5–6 development quote features.',
      'No fees are assumed; one-tick-each-way stress is separate. These per-unit markouts are not portfolio returns.',
      'The common identity intersection avoids pooling changed contracts. Market-cluster uncertainty accounts for within-market quote dependence but not shared tournament-wide shocks or model selection.',
      'The two earlier capture days are development diagnostics, not independent validation. Even positive proxy markouts would not establish profitability.',
    ],
    actualFillsProven: false,
    profitabilityEstablished: false,
    contestOrders: 0,
    networkRequests: 0,
    generatedAt: new Date().toISOString(),
  };
}

export function createMarketBatchStudy(sourceIds = INPUTS.map(source => source.day)) {
  if (!Array.isArray(sourceIds) || sourceIds.length < 3 || new Set(sourceIds).size !== sourceIds.length) throw Error('MULTIDAY_SOURCE_PLAN');
  const holdoutIndex = sourceIds.findIndex(id => id === '2026-10-13');
  if (holdoutIndex !== sourceIds.length - 1 || sourceIds.slice(0, -1).some(day => day >= '2026-10-13')) throw Error('MULTIDAY_HOLDOUT_ORDER');
  return { holdoutIndex, sourceIds: [...sourceIds] };
}

export async function runMultiday({ devOnly = false, writeReport = true } = {}) {
  const runtimeDir = process.env.LOCALAPPDATA ? path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime') : null;
  if (!runtimeDir) throw Error('MULTIDAY_LOCALAPPDATA_REQUIRED');
  if (!devOnly && writeReport && fs.existsSync(OUTPUT)) throw Error('MULTIDAY_OUTPUT_EXISTS');
  const { sources, scope } = prepareInputs(runtimeDir, { devOnly });
  const combinedStudies = makeStudies(scope);
  const dailyResults = [];
  const replayRows = [];
  const sourceSummaries = [];
  const processCount = devOnly ? 2 : sources.length;

  for (let i = 0; i < processCount; i++) {
    const source = sources[i];
    const combinedPanel = createPanel(scope, combinedStudies);
    const dailyStudies = source.role === 'development' ? makeStudies(scope) : null;
    const dailyPanel = dailyStudies ? createPanel(scope, dailyStudies) : null;
    const panels = dailyPanel ? [combinedPanel, dailyPanel] : [combinedPanel];
    const replayed = replaySource(source, panels);
    const prev = replayRows.at(-1);
    if (prev && Date.parse(replayed.firstObservedAt) <= Date.parse(prev.through)) throw Error('MULTIDAY_SOURCE_ORDER');
    replayRows.push(replayed);
    bindCaptureBoundary(combinedPanel, replayed.through);
    const summary = captureSourceSummary(source, replayed.hash, combinedPanel, replayed);
    sourceSummaries.push(summary);
    if (dailyPanel) dailyResults.push(summarizeDaily(source, dailyPanel, replayed));
  }

  if (devOnly) {
    const holdout = sources.at(-1);
    const splitAt = holdout.header.firstObservedAt;
    const combinedPanel = createPanel(scope, combinedStudies);
    const combined = summarizePanel(combinedPanel, splitAt);
    return {
      mode: 'development_only_oct13_header_only_no_holdout_outcomes_read',
      holdoutHeader: {
        day: holdout.day,
        firstConfigObservedAt: holdout.header.firstObservedAt,
        configRecords: holdout.header.headerRecords,
        headerPrefixBytes: holdout.header.headerPrefixBytes,
        stopKind: holdout.header.stoppedAtKind,
        marketCount: holdout.header.members.size,
        outcomePrefixSealed: false,
      },
      developmentSources: sourceSummaries,
      developmentAggregate: {
        featureCalibration: combined.flowCalibration,
        candidateQuotesByFold: combined.counts.candidateQuotesByFold,
        cells: combined.cells.filter(cell => cell.fold === 'development'),
      },
      dailyDevelopment: dailyResults,
      sourcePrefixHashesVerified: true,
      networkRequests: 0,
      contestOrders: 0,
      actualFillsProven: false,
      profitabilityEstablished: false,
    };
  }

  ensureChronological(sources, replayRows);
  const splitAt = sources.at(-1).header.firstObservedAt;
  const combinedPanel = createPanel(scope, combinedStudies);
  const combined = summarizePanel(combinedPanel, splitAt);
  const holdoutReplay = replayRows.at(-1);
  const holdoutSummary = sourceSummaries.at(-1);
  const report = createOutput({
    sources, scope, daily: dailyResults, combined, splitAt, sourceSummaries,
    replayCounts: {
      holdoutAcceptedBookPoints: holdoutSummary.acceptedBookPoints,
      holdoutRejectedBookPoints: holdoutSummary.rejectedBookPoints,
      holdoutTradeReceiptLagMs: holdoutSummary.tradeReportObservationLagMs,
    },
  });
  report.holdout.source = holdoutSummary;
  report.holdout.captureThrough = holdoutReplay.through;
  if (writeReport) fs.writeFileSync(OUTPUT, JSON.stringify(report, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
  return {
    output: writeReport ? path.relative(process.cwd(), OUTPUT) : null,
    report,
    summary: {
      sourceDays: sourceSummaries.map(({ day, captureRecords, acceptedBookPoints, scopedUniqueTradePrints }) => ({ day, captureRecords, acceptedBookPoints, scopedUniqueTradePrints })),
      commonIdentityCount: scope.comparableIdentityCount,
      holdoutCells: report.holdout.cells.map(({ action, horizonMs, cellFamily, cell, candidateQuotes, strictThroughProxies, markoutSamples, markoutMarketClusters, effectiveIndependentMarkoutMarkets, executable }) => ({ action, horizonMs, cellFamily, cell, candidateQuotes, strictThroughProxies, markoutSamples, markoutMarketClusters, effectiveIndependentMarkoutMarkets, meanExecutableNet: executable.meanExecutableNet, marketClusterStandardError: executable.marketClusterStandardError })),
      networkRequests: 0,
      contestOrders: 0,
    },
  };
}

const runningDirectly = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (runningDirectly) {
  const devOnly = process.argv.includes('--dev-only');
  runMultiday({ devOnly }).then(result => {
    if (devOnly) console.log(JSON.stringify({
      mode: result.mode,
      holdoutHeader: result.holdoutHeader,
      developmentSources: result.developmentSources.map(({ day, captureRecords, acceptedBookPoints, scopedUniqueTradePrints }) => ({ day, captureRecords, acceptedBookPoints, scopedUniqueTradePrints })),
      developmentAggregateCells: result.developmentAggregate.cells.length,
      dailyDevelopmentCellCounts: result.dailyDevelopment.map(day => ({ day: day.day, cells: day.cells.length })),
      networkRequests: result.networkRequests,
      contestOrders: result.contestOrders,
      actualFillsProven: result.actualFillsProven,
      profitabilityEstablished: result.profitabilityEstablished,
    }));
    else console.log(JSON.stringify({ output: result.output, ...result.summary }));
  }).catch(error => {
    console.error(JSON.stringify({ error: /^[A-Z0-9_]+$/.test(error.message) ? error.message : 'MULTIDAY_RUN_FAILED' }));
    process.exitCode = 1;
  });
}
