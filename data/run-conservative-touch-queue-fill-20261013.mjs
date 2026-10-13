import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import { createConservativeTouchQueueFillV1 } from '../research/conservative-touch-queue-fill-v1.mjs';
import { captureHeader, comparableScope, frozenCaptureLines } from './run-multiday-passive-markout-20261013.mjs';

export const FROZEN_SOURCES = Object.freeze([
  Object.freeze({
    day: '2026-10-05', role: 'development',
    file: 'realtime-2026-10-05T22-39-17-072Z-a9bb9f87-8ac2-4373-b7ec-b499ddf7176a.jsonl',
    prefixBytes: 918966309,
    sha256: '20c45ce84282c59efca2f6432666869ba5615b90e53cbdd15c985511751e3fe4',
    captureRecords: 226132,
  }),
  Object.freeze({
    day: '2026-10-06', role: 'development',
    file: 'realtime-2026-10-06T00-21-21-601Z-1175216a-92f4-49ad-9cb9-550c66ffb2ab.jsonl',
    prefixBytes: 1809102092,
    sha256: '3d77a1ba4a66ca16ff1c16cc51f9a2cf0b258f33c11b90329c00d39a16ebc2e6',
    captureRecords: 451485,
  }),
  Object.freeze({
    day: '2026-10-13', role: 'untouched_held_out',
    file: 'realtime-2026-10-13T13-28-06-669Z-a7674ae9-ed0e-41c6-90c8-390b1ce0dc8b.jsonl',
    prefixBytes: 364603159,
    sha256: '83dd7cc069ce221617425c533a6d1938c3056fcac20254c4938cddb7d4c96f61',
    captureRecords: 104446,
  }),
]);

export const OUTPUT = path.resolve('data/conservative-touch-queue-fill-20261013.json');
export const COMMON_IDENTITY_SHA256 = 'a946bcbdca57d7a1e7df388916bd5ba2007f6b846eae33fbec096490b4cda548';
export const TOURNAMENT_ID = 'bda92870-621e-47b0-bc3c-3602c5c26f55';

const scopeForReader = scope => ({ tournamentId: scope.tournamentId, exchanges: scope.members });
const validIso = value => typeof value === 'string' && Number.isFinite(Date.parse(value));

export function hashFrozenPrefix(file, prefixBytes) {
  const digest = createHash('sha256');
  let bytes = 0;
  let lines = 0;
  for (const row of frozenCaptureLines(file, prefixBytes)) {
    digest.update(row.bytes);
    bytes += row.bytes.length;
    lines++;
  }
  if (bytes !== prefixBytes) throw Error('QUEUE_FILL_PREFIX_BYTES');
  return { bytes, lines, sha256: digest.digest('hex') };
}

const SNAPSHOT_DIMENSIONS = Object.freeze({
  bySourceDay: ['sourceDay'],
  byFold: ['fold'],
  byAction: ['action'],
  byStatus: ['status'],
  bySourceDayFoldActionStatus: ['sourceDay', 'fold', 'action', 'status'],
});
const SNAPSHOT_SUM_FIELDS = Object.freeze([
  'spreadTicks',
  'initialQueueAheadShares',
  'observedQueueIncreaseSharesAdded',
  'observedDisplayReductionsIgnored',
  'finalQueueAheadShares',
  'touchPriceTradeReportsUnknownSide',
  'touchPriceTradeQuantityUnknownSide',
]);

function snapshotAccumulator() {
  return { rowCount: 0, markets: new Set(), exchanges: new Set(), sums: new Map() };
}

function addSnapshotRow(accumulator, row, indexes) {
  accumulator.rowCount++;
  accumulator.markets.add(row[indexes.marketId]);
  accumulator.exchanges.add(row[indexes.exchangeId]);
  for (const field of SNAPSHOT_SUM_FIELDS) {
    const value = row[indexes[field]];
    if (!Number.isSafeInteger(value)) throw Error(`QUEUE_FILL_SNAPSHOT_METRIC_${field}`);
    const current = accumulator.sums.get(field) ?? { count: 0, sum: 0, min: Infinity, max: -Infinity };
    const sum = current.sum + value;
    if (!Number.isSafeInteger(sum)) throw Error(`QUEUE_FILL_SNAPSHOT_SUM_${field}`);
    current.count++;
    current.sum = sum;
    current.min = Math.min(current.min, value);
    current.max = Math.max(current.max, value);
    accumulator.sums.set(field, current);
  }
}

function finishSnapshotAccumulator(accumulator, keys = {}) {
  return {
    ...keys,
    rowCount: accumulator.rowCount,
    uniqueMarketCount: accumulator.markets.size,
    uniqueExchangeCount: accumulator.exchanges.size,
    metrics: Object.fromEntries([...accumulator.sums.entries()].map(([field, value]) => [field, value])),
  };
}

/**
 * Compact the large per-quote diagnostic without changing the replay result.
 * The caller still receives every timestamped row from runConservativeTouchQueueFill;
 * persisted artifacts retain exact counts and integer sums/min/max by declared groups.
 */
export function summarizeQueueAheadSnapshots(snapshots) {
  if (!Array.isArray(snapshots?.columns) || !Array.isArray(snapshots?.rows)) throw Error('QUEUE_FILL_SNAPSHOT_SHAPE');
  const indexes = Object.fromEntries(snapshots.columns.map((field, index) => [field, index]));
  const required = ['sourceDay', 'fold', 'marketId', 'exchangeId', 'action', 'status', ...SNAPSHOT_SUM_FIELDS];
  if (required.some(field => !Number.isSafeInteger(indexes[field]))) throw Error('QUEUE_FILL_SNAPSHOT_COLUMNS');

  const overall = snapshotAccumulator();
  const groups = Object.fromEntries(Object.entries(SNAPSHOT_DIMENSIONS).map(([name]) => [name, new Map()]));
  for (const row of snapshots.rows) {
    if (!Array.isArray(row) || row.length !== snapshots.columns.length) throw Error('QUEUE_FILL_SNAPSHOT_ROW');
    for (const field of ['sourceDay', 'fold', 'marketId', 'exchangeId', 'action', 'status']) {
      if (typeof row[indexes[field]] !== 'string' || row[indexes[field]].length === 0) throw Error(`QUEUE_FILL_SNAPSHOT_DIMENSION_${field}`);
    }
    addSnapshotRow(overall, row, indexes);
    for (const [name, fields] of Object.entries(SNAPSHOT_DIMENSIONS)) {
      const keys = Object.fromEntries(fields.map(field => [field, row[indexes[field]]]));
      const id = JSON.stringify(fields.map(field => keys[field]));
      let accumulator = groups[name].get(id);
      if (!accumulator) {
        accumulator = { keys, value: snapshotAccumulator() };
        groups[name].set(id, accumulator);
      }
      addSnapshotRow(accumulator.value, row, indexes);
    }
  }

  const grouped = Object.fromEntries(Object.entries(groups).map(([name, values]) => [name,
    [...values.values()]
      .sort((a, b) => JSON.stringify(a.keys).localeCompare(JSON.stringify(b.keys)))
      .map(({ keys, value }) => finishSnapshotAccumulator(value, keys)),
  ]));
  return {
    schemaVersion: 1,
    detailLevel: 'aggregated_timestamped_rows_omitted',
    rowCount: overall.rowCount,
    originalColumns: [...snapshots.columns],
    omittedRowFields: snapshots.columns.filter(field => !['sourceDay', 'fold', 'marketId', 'exchangeId', 'action', 'status', ...SNAPSHOT_SUM_FIELDS].includes(field)),
    aggregateMethod: 'exact row and unique-id counts; exact safe-integer sum/min/max for each declared metric',
    totals: finishSnapshotAccumulator(overall),
    groupings: grouped,
  };
}

export function compactPersistedReport(report) {
  if (!Array.isArray(report?.outcomes?.prespecifiedCells) || report.outcomes.prespecifiedCells.length !== 96) {
    throw Error('QUEUE_FILL_PRESPECIFIED_CELLS');
  }
  if (report.outcomes.actualFillsProven !== false || report.outcomes.counts?.confirmedQueueFills !== 0) {
    throw Error('QUEUE_FILL_NO_FILL_REPORT_INVARIANT');
  }
  return {
    ...report,
    schemaVersion: 2,
    persistedDetailLevel: 'compact_summary; full timestamped queue rows remain on the in-memory replay result',
    outcomes: {
      ...report.outcomes,
      queueAheadSnapshots: summarizeQueueAheadSnapshots(report.outcomes.queueAheadSnapshots),
    },
  };
}

export function writeCompactReport(report, outputPath) {
  const persisted = compactPersistedReport(report);
  const text = `${JSON.stringify(persisted, null, 2)}\n`;
  fs.writeFileSync(outputPath, text, { encoding: 'utf8', flag: 'wx' });
  return { bytes: Buffer.byteLength(text), sha256: createHash('sha256').update(text).digest('hex') };
}

function prepareSources(runtimeDir) {
  const sources = FROZEN_SOURCES.map(input => {
    if (!/^realtime-2026-10-(05|06|13)T[A-Za-z0-9.-]+\.jsonl$/.test(input.file)) throw Error('QUEUE_FILL_SOURCE_NAME');
    const filePath = path.join(runtimeDir, input.file);
    const stat = fs.statSync(filePath);
    if (!stat.isFile() || stat.size < input.prefixBytes) throw Error('QUEUE_FILL_SOURCE_FILE');
    const header = captureHeader(filePath);
    if (header.firstObservedAt.slice(0, 10) !== input.day || header.members.size !== 237) throw Error('QUEUE_FILL_SOURCE_HEADER');
    return { ...input, filePath, fileDev: stat.dev, fileIno: stat.ino, fileBytesAtRead: stat.size, header };
  });
  const scope = comparableScope(sources.map(source => source.header));
  if (scope.tournamentId !== TOURNAMENT_ID || scope.comparableIdentityCount !== 237 || scope.sha256 !== COMMON_IDENTITY_SHA256) {
    throw Error('QUEUE_FILL_SCOPE_MISMATCH');
  }
  for (const source of sources) {
    if (source.header.members.size !== 237) throw Error('QUEUE_FILL_SOURCE_SCOPE');
  }
  return { sources, scope };
}

function makeSequenceAudit() {
  const counts = {
    tradeReports: 0,
    reportsWithPreviousBook: 0,
    previousBookSequenceLessThanTrade: 0,
    previousBookExactlyTradeMinusOne: 0,
    reportsWithSameBatchBook: 0,
    sameBatchBookSequenceAtLeastTrade: 0,
    sameBatchBookSequenceGreaterThanTrade: 0,
    exactBookTradeBookAdjacency: 0,
    tradeWithinDeliverySequenceBounds: 0,
    invalidOrNullSequence: 0,
  };
  const lastBookByExchange = new Map();
  const lastRevisionByTopic = new Map();
  const lastEpochByTopic = new Map();

  function clearTopic(topic) {
    for (const [exchangeId, value] of lastBookByExchange) if (value.topic === topic) lastBookByExchange.delete(exchangeId);
    lastRevisionByTopic.delete(topic);
    lastEpochByTopic.delete(topic);
  }

  function observeRecord(record, validBooks) {
    const topic = record.topic;
    if (!topic) return;
    const payload = record.kind === 'event' && record.event === 'market_batch' ? record.payload : null;
    if (record.kind === 'invalidation') {
      if (!record.exchangeId) clearTopic(topic);
      // Per-exchange invalidation does not erase a historical snapshot from
      // this diagnostic; comparisons remain descriptive, never a fill join.
      return;
    }
    if (['subscribed', 'disconnected', 'stopped', 'rejected', 'recovery_started', 'recovery_finished', 'snapshot'].includes(record.kind)
      || record.kind === 'event' && record.event !== 'market_batch') {
      clearTopic(topic);
      return;
    }
    if (!payload) return; // Derived trade_observed rows do not invalidate the source book stream.
    if (payload.resyncRequired !== false || payload.marketSettled?.length) {
      clearTopic(topic);
      return;
    }
    if (!Number.isSafeInteger(record.epoch) || record.epoch < 1) {
      clearTopic(topic);
      return;
    }
    const priorEpoch = lastEpochByTopic.get(topic);
    if (priorEpoch !== undefined && priorEpoch !== record.epoch) clearTopic(topic);
    lastEpochByTopic.set(topic, record.epoch);
    const delivery = payload.delivery;
    const priorRevision = lastRevisionByTopic.get(topic);
    if (priorRevision !== undefined && delivery?.previousRevision !== priorRevision) clearTopic(topic);
    if (Number.isSafeInteger(delivery?.revision)) lastRevisionByTopic.set(topic, delivery.revision);
    const previous = new Map();
    const current = new Map(validBooks.map(book => [book.exchangeId, BigInt(book.sequence)]));
    for (const trade of payload.trades ?? []) {
      counts.tradeReports++;
      if (!Number.isSafeInteger(trade.sequence) || trade.sequence < 0) {
        counts.invalidOrNullSequence++;
        continue;
      }
      const sequence = BigInt(trade.sequence);
      const old = lastBookByExchange.get(String(trade.exchangeId));
      if (old && old.topic === topic) {
        previous.set(String(trade.exchangeId), old.sequence);
        counts.reportsWithPreviousBook++;
        if (old.sequence < sequence) counts.previousBookSequenceLessThanTrade++;
        if (sequence > 0n && old.sequence === sequence - 1n) counts.previousBookExactlyTradeMinusOne++;
      }
      const contemporaneous = current.get(String(trade.exchangeId));
      if (contemporaneous !== undefined) {
        counts.reportsWithSameBatchBook++;
        if (contemporaneous >= sequence) counts.sameBatchBookSequenceAtLeastTrade++;
        if (contemporaneous > sequence) counts.sameBatchBookSequenceGreaterThanTrade++;
        const priorSequence = previous.get(String(trade.exchangeId));
        if (priorSequence !== undefined && priorSequence + 1n === sequence && sequence + 1n === contemporaneous) {
          counts.exactBookTradeBookAdjacency++;
        }
      }
      const from = delivery?.sourceSequenceFrom;
      const through = delivery?.sourceSequenceThrough;
      if (Number.isSafeInteger(from) && Number.isSafeInteger(through) && from <= trade.sequence && trade.sequence <= through) {
        counts.tradeWithinDeliverySequenceBounds++;
      }
    }
    for (const book of validBooks) {
      lastBookByExchange.set(book.exchangeId, { topic, sequence: BigInt(book.sequence) });
    }
  }

  return { counts, observeRecord };
}

function replayOne(source, scope, audit, sequenceAudit) {
  const before = fs.statSync(source.filePath);
  if (before.dev !== source.fileDev || before.ino !== source.fileIno || before.size < source.prefixBytes) throw Error('QUEUE_FILL_SOURCE_REPLACED');
  const digest = createHash('sha256');
  const readerScope = scopeForReader(scope);
  let recordCount = 0;
  let acceptedBookPoints = 0;
  let rejectedBookPoints = 0;
  let firstAt = null;
  let through = null;
  let lastObservedMs = -Infinity;
  let invalidSequenceBatches = 0;
  audit.setFold(source.role);

  for (const row of frozenCaptureLines(source.filePath, source.prefixBytes)) {
    digest.update(row.bytes);
    let record;
    try { record = JSON.parse(row.text); } catch { throw Error('QUEUE_FILL_SOURCE_JSON'); }
    if (record.format !== 'sig-feed-capture' || record.schemaVersion !== 1 || record.captureIndex !== recordCount || !validIso(record.observedAt)) {
      throw Error('QUEUE_FILL_CAPTURE_CHAIN');
    }
    const observedMs = Date.parse(record.observedAt);
    if (observedMs < lastObservedMs) throw Error('QUEUE_FILL_CAPTURE_CLOCK');
    lastObservedMs = observedMs;
    firstAt ??= record.observedAt;
    through = record.observedAt;

    const points = engineBookPointsV1(record, readerScope, record.observedAt);
    acceptedBookPoints += points.books.length;
    rejectedBookPoints += points.rejected.length;
    if (record.kind === 'event' && record.event === 'market_batch') {
      const delivery = record.payload?.delivery;
      if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) invalidSequenceBatches++;
    }
    audit.noteRejectedBookPoints(points.rejected.length);
    audit.observeRecord(record, points.books);
    sequenceAudit.observeRecord(record, points.books);
    recordCount++;
  }

  const after = fs.statSync(source.filePath);
  if (after.dev !== source.fileDev || after.ino !== source.fileIno || after.size < source.prefixBytes) throw Error('QUEUE_FILL_SOURCE_REPLACED_DURING_READ');
  const actualSha = digest.digest('hex');
  if (recordCount !== source.captureRecords || source.header.headerRecords < 1 || source.header.headerRecords >= recordCount
    || actualSha !== source.sha256 || !through || firstAt.slice(0, 10) !== source.day) throw Error('QUEUE_FILL_SOURCE_SEAL_MISMATCH');
  audit.endCapture(through);

  return {
    day: source.day,
    role: source.role,
    file: source.file,
    prefixBytes: source.prefixBytes,
    sha256: actualSha,
    captureRecords: recordCount,
    from: firstAt,
    through,
    scopeIdentity: {
      sourceIdentityCount: source.header.members.size,
      comparableIdentityCount: scope.comparableIdentityCount,
      excludedIdentityCount: source.header.members.size - scope.comparableIdentityCount,
      sha256: scope.sha256,
    },
    acceptedBookPoints,
    rejectedBookPoints,
    marketBatchRowsWithInvalidRevision: invalidSequenceBatches,
    currentFileBytes: after.size,
  };
}

export function runConservativeTouchQueueFill({
  runtimeDir = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime'),
  outputPath = OUTPUT,
  writeOutput = true,
} = {}) {
  if (!runtimeDir || !path.isAbsolute(runtimeDir)) throw Error('QUEUE_FILL_RUNTIME_PATH');
  const { sources, scope } = prepareSources(runtimeDir);
  for (let i = 1; i < sources.length; i++) {
    if (Date.parse(sources[i].header.firstObservedAt) <= Date.parse(sources[i - 1].header.firstObservedAt)
      || sources[i].day !== FROZEN_SOURCES[i].day) throw Error('QUEUE_FILL_SOURCE_ORDER');
  }
  if (sources.slice(0, -1).some(source => source.role !== 'development' || source.day >= '2026-10-13')
    || sources.at(-1).role !== 'untouched_held_out' || sources.at(-1).day !== '2026-10-13') throw Error('QUEUE_FILL_SPLIT');

  const audit = createConservativeTouchQueueFillV1({ members: scope.members });
  const sequenceAudit = makeSequenceAudit();
  const sourceReports = [];
  for (const source of sources) {
    audit.setSourceDay(source.day);
    sourceReports.push(replayOne(source, scope, audit, sequenceAudit));
  }
  const result = audit.report();
  const queueAheadSnapshots = audit.queueAheadSnapshotRows();
  for (let i = 1; i < sourceReports.length; i++) {
    if (Date.parse(sourceReports[i].from) <= Date.parse(sourceReports[i - 1].through)) throw Error('QUEUE_FILL_SOURCE_OVERLAP');
  }
  const developmentThrough = sourceReports[1].through;
  const holdoutFrom = sourceReports[2].from;
  if (Date.parse(holdoutFrom) <= Date.parse(developmentThrough)) throw Error('QUEUE_FILL_HOLDOUT_NOT_CHRONOLOGICAL');

  const report = {
    schemaVersion: 1,
    mode: 'conservative_displayed_queue_touch_audit_v1',
    status: result.status,
    runMode: 'fixed_source_hashes_fixed_policy_single_chronological_replay',
    generatedAt: new Date().toISOString(),
    policy: result.policy,
    split: {
      developmentDays: ['2026-10-05', '2026-10-06'],
      developmentThrough,
      untouchedHeldOutDay: '2026-10-13',
      untouchedHeldOutFrom: holdoutFrom,
      order: 'all development prefixes replayed before the holdout prefix; policy fixed in code and no fitting, selection, or tuning occurs',
    },
    scope: {
      tournamentId: scope.tournamentId,
      sourceIdentityCounts: sources.map(source => ({ day: source.day, count: source.header.members.size })),
      commonIdentityCount: scope.comparableIdentityCount,
      commonIdentitySha256: scope.sha256,
    },
    sources: sourceReports,
    replaySummary: {
      captureRecords: sourceReports.reduce((total, source) => total + source.captureRecords, 0),
      observedMarketBatchRecords: result.counts.marketBatchRecords,
      helperAcceptedBookPoints: sourceReports.reduce((total, source) => total + source.acceptedBookPoints, 0),
      helperRejectedBookPoints: sourceReports.reduce((total, source) => total + source.rejectedBookPoints, 0),
    },
    sequenceDiagnostic: {
      ...sequenceAudit.counts,
      interpretation: 'saved-field comparability audit only. No common trade/book ordering guarantee, aggressor side, or consumed queue is established; sequence values never create or decrement a fill.',
      previousBookRelationDefinition: 'last earlier captured valid book for the same exchange since the latest topic-wide lifecycle/revision boundary; per-exchange dirty hints do not erase this descriptive historical snapshot, and no continuity is implied',
    },
    outcomes: {
      status: result.status,
      counts: result.counts,
      outcomeTables: result.outcomeTables,
      prespecifiedCells: result.prespecifiedCells,
      queueAheadSnapshots,
      actualFillsProven: false,
      profitabilityEstablished: false,
      contestOrders: 0,
      networkRequests: 0,
      limitations: result.limitations,
    },
    interpretation: [
      'This replay records hypothetical touch quotes and timestamped displayed queue-ahead proxies only. The full in-memory result retains timestamped rows; the persisted artifact aggregates them. It confirms no passive fills and yields no P&L or profitability estimate.',
      'Touch-price trade reports are unknown-side observations and do not decrement queue or proxy a fill. Queue reductions are treated as possible cancellations and ignored; observed size increases are added, but missing snapshots can hide intermediate changes.',
      'All prespecified 1s, 5s and 30s markout cells are reported as empty because no queue fill can be causally confirmed. One-tick stress is declared, but no stressed return can be calculated without fills/outcomes.',
      'Historical captures are local read-only inputs. This run made no API requests and did not access runtime accounts or orders.',
    ],
  };

  if (writeOutput) {
    writeCompactReport(report, outputPath);
  }
  return report;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const report = runConservativeTouchQueueFill();
  process.stdout.write(`${JSON.stringify({ output: OUTPUT, status: report.status, sources: report.sources.map(({ day, sha256, captureRecords }) => ({ day, sha256, captureRecords })), confirmedFills: report.outcomes.counts.confirmedQueueFills })}\n`);
}

