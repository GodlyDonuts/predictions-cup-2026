import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';
import { journalLines } from '../live-paper/journal-lines.mjs';
import { sealedNativePrefix } from '../live-paper/native-dip-host-v1.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import { createMultiLevelBookFlowLatencyV1 } from '../research/multilevel-book-flow-latency-v1.mjs';

const EXPECTED = Object.freeze({
  tournamentId: 'bda92870-621e-47b0-bc3c-3602c5c26f55',
  file: 'realtime-2026-10-13T02-55-28-953Z-d53812dd-ec75-4852-9267-59c7d5bb7898.jsonl',
  bytes: 1_747_459_094,
  sha256: 'f9a20ef98e8e9c96d1229dbbc6a8eb17dd669e7c6f3f8d6e49337439945f4418',
  captureRecords: 467_044,
  scopeMarkets: 237,
  from: '2026-10-13T02:55:28.956Z',
  through: '2026-10-13T07:45:51.103Z',
  splitAt: '2026-10-13T06:18:44.458Z',
  purgeMs: 5000,
});

const root = process.cwd();
const here = path.dirname(fileURLToPath(import.meta.url));
const baselinePath = path.join(here, 'subsecond-microprice-edge-20261013.json');
const outputPath = path.join(here, 'multilevel-book-flow-latency-20261013.json');
if (fs.existsSync(outputPath)) throw Error('MLOFI_LATENCY_RESULT_EXISTS');
if (!process.env.LOCALAPPDATA) throw Error('MLOFI_LATENCY_LOCALAPPDATA');

const baselineBytes = fs.readFileSync(baselinePath);
const baseline = JSON.parse(baselineBytes.toString('utf8'));
if (baseline.source?.file !== EXPECTED.file || baseline.source?.bytes !== EXPECTED.bytes
  || baseline.source?.sha256 !== EXPECTED.sha256 || baseline.source?.captureRecords !== EXPECTED.captureRecords
  || baseline.source?.from !== EXPECTED.from || baseline.source?.through !== EXPECTED.through
  || baseline.protocol?.splitAt !== EXPECTED.splitAt || baseline.protocol?.chronologicalPurgeMs !== EXPECTED.purgeMs
  || JSON.stringify(baseline.protocol?.horizonsMs) !== JSON.stringify([100, 250, 500, 1000, 5000])) {
  throw Error('MLOFI_LATENCY_FROZEN_MANIFEST');
}
const sourcePath = path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime', EXPECTED.file);
const before = fs.statSync(sourcePath);
if (!before.isFile() || before.size < EXPECTED.bytes) throw Error('MLOFI_LATENCY_CAPTURE_SHORT');
const seal = sealedNativePrefix(sourcePath);
if (seal.bytes < EXPECTED.bytes) throw Error('MLOFI_LATENCY_CAPTURE_UNSEALED_PREFIX');

const topicPrefix = `tournament:${EXPECTED.tournamentId}:market:`;
const exchangeToMarket = new Map(), topicMarkets = new Set();
let firstObservedAt = null, through = null;
for (const line of journalLines(sourcePath, { endByte: EXPECTED.bytes })) {
  const record = JSON.parse(line);
  if (typeof record.observedAt !== 'string' || !Number.isFinite(Date.parse(record.observedAt))) throw Error('MLOFI_LATENCY_CAPTURE_CLOCK');
  firstObservedAt ??= record.observedAt;
  through = record.observedAt;
  if (record.kind !== 'event' || record.event !== 'market_batch' || !record.topic?.startsWith(topicPrefix)) continue;
  const marketId = record.topic.slice(topicPrefix.length);
  if (!/^\d+$/.test(marketId)) throw Error('MLOFI_LATENCY_MARKET_ID');
  topicMarkets.add(marketId);
  for (const book of record.payload?.books ?? []) {
    const exchangeId = String(book.exchangeId ?? '');
    if (!/^\d+$/.test(exchangeId)) throw Error('MLOFI_LATENCY_EXCHANGE_ID');
    const priorMarket = exchangeToMarket.get(exchangeId);
    if (priorMarket !== undefined && priorMarket !== marketId) throw Error('MLOFI_LATENCY_SCOPE_CONFLICT');
    exchangeToMarket.set(exchangeId, marketId);
  }
}
if (topicMarkets.size !== EXPECTED.scopeMarkets || exchangeToMarket.size !== EXPECTED.scopeMarkets) {
  throw Error('MLOFI_LATENCY_SCOPE_SIZE');
}
const scope = {
  tournamentId: EXPECTED.tournamentId,
  exchanges: [...exchangeToMarket].map(([exchangeId, marketId]) => ({ exchangeId, marketId })),
};
const membersByTopic = new Map();
for (const member of scope.exchanges) {
  const topic = `tournament:${scope.tournamentId}:market:${member.marketId}`;
  const members = membersByTopic.get(topic) ?? [];
  members.push(member);
  membersByTopic.set(topic, members);
}
const canonicalScope = scope.exchanges.map(member => [member.marketId, member.exchangeId])
  .sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));
const scopeSha256 = createHash('sha256').update(JSON.stringify({ tournamentId: scope.tournamentId, pairs: canonicalScope })).digest('hex');

const study = createMultiLevelBookFlowLatencyV1({ scope, splitAt: EXPECTED.splitAt, purgeMs: EXPECTED.purgeMs });
const sourceDigest = createHash('sha256');
const rejectionReasons = Object.create(null);
const revisions = new Map(), epochs = new Map();
let captureIndex = 0, firstReplayAt = null, replayThrough = null;
let processedBatches = 0, rawBookPoints = 0, acceptedUpdates = 0, rejectedPoints = 0;
let sourceReceiptClockSkews = 0, previousObservedMs = -1;
const started = performance.now();

function boundaryTopic(topic, at, reason) {
  for (const member of membersByTopic.get(topic) ?? []) study.boundary(member.exchangeId, at, reason);
}

for (const line of journalLines(sourcePath, { endByte: EXPECTED.bytes })) {
  sourceDigest.update(line + '\n');
  const record = JSON.parse(line);
  if (record.format !== 'sig-feed-capture' || record.captureIndex !== captureIndex++) throw Error('MLOFI_LATENCY_CAPTURE_CHAIN');
  if (typeof record.observedAt !== 'string' || !Number.isFinite(Date.parse(record.observedAt))) throw Error('MLOFI_LATENCY_CAPTURE_CLOCK');
  const observedMs = Date.parse(record.observedAt);
  if (observedMs < previousObservedMs) throw Error('MLOFI_LATENCY_CAPTURE_ROLLBACK');
  previousObservedMs = observedMs;
  firstReplayAt ??= record.observedAt;
  replayThrough = record.observedAt;
  const topic = record.topic;
  if (typeof topic !== 'string' || !membersByTopic.has(topic)) continue;

  if (['disconnected', 'stopped', 'subscribed', 'rejected'].includes(record.kind)) {
    boundaryTopic(topic, record.observedAt, `source_${record.kind}`);
    epochs.delete(topic);
    revisions.delete(topic);
    continue;
  }
  if (record.kind === 'invalidation') {
    const reason = /^[A-Z_]+$/.test(record.reason ?? '') ? `invalidation_${record.reason}` : 'invalidation_other';
    boundaryTopic(topic, record.observedAt, reason);
    continue;
  }
  if (record.kind !== 'event' || record.event !== 'market_batch') {
    if (record.kind === 'event') boundaryTopic(topic, record.observedAt, 'unsupported_event');
    continue;
  }

  const priorEpoch = epochs.get(topic);
  if (priorEpoch !== undefined && priorEpoch !== record.epoch) boundaryTopic(topic, record.observedAt, 'epoch_change');
  epochs.set(topic, record.epoch);
  const payload = record.payload;
  if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
    boundaryTopic(topic, record.observedAt, payload.marketSettled?.length ? 'market_settled' : 'resync_required');
    continue;
  }
  const delivery = payload.delivery;
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) throw Error('MLOFI_LATENCY_DELIVERY_REVISION');
  const revisionKey = `${topic}:${record.epoch}`;
  const previousRevision = revisions.get(revisionKey);
  if (previousRevision !== undefined && delivery.previousRevision !== previousRevision) {
    boundaryTopic(topic, record.observedAt, 'delivery_gap');
  }
  if (previousRevision !== undefined && delivery.revision <= previousRevision) continue;
  revisions.set(revisionKey, delivery.revision);

  const points = engineBookPointsV1(record, scope, record.observedAt);
  rejectedPoints += points.rejected.length;
  for (const rejected of points.rejected) {
    const reason = /^[A-Za-z0-9_]+$/.test(rejected.reason ?? '') ? rejected.reason : 'other';
    rejectionReasons[reason] = (rejectionReasons[reason] ?? 0) + 1;
  }
  processedBatches++;
  rawBookPoints += points.books.length;
  for (const book of points.books) {
    try {
      if (study.observe(book, { observedAt: record.observedAt })) acceptedUpdates++;
    } catch (error) {
      if (!/^MLOFI_LATENCY_[A-Z_]+$/.test(error?.message ?? '') && !/^MLOFI_[A-Z_]+$/.test(error?.message ?? '')) throw error;
      const reason = error.message === 'MLOFI_RECEIPT_BEFORE_SOURCE' ? 'source_after_receipt' : error.message.toLowerCase();
      rejectionReasons[reason] = (rejectionReasons[reason] ?? 0) + 1;
      rejectedPoints++;
      if (error.message === 'MLOFI_RECEIPT_BEFORE_SOURCE') sourceReceiptClockSkews++;
      study.boundary(book.exchangeId, record.observedAt, `invalid_${reason}`);
    }
  }
}

const after = fs.statSync(sourcePath);
if (after.dev !== before.dev || after.ino !== before.ino || after.size < EXPECTED.bytes) throw Error('MLOFI_LATENCY_CAPTURE_REPLACED');
if (captureIndex !== EXPECTED.captureRecords || firstObservedAt !== EXPECTED.from || through !== EXPECTED.through
  || firstReplayAt !== EXPECTED.from || replayThrough !== EXPECTED.through) throw Error('MLOFI_LATENCY_CAPTURE_EXPECTATION');
const sha256 = sourceDigest.digest('hex');
if (sha256 !== EXPECTED.sha256) throw Error('MLOFI_LATENCY_CAPTURE_HASH');
study.finish();

const rootSource = {
  file: EXPECTED.file,
  bytes: EXPECTED.bytes,
  sha256,
  captureRecords: captureIndex,
  from: firstObservedAt,
  through,
  scopeMarkets: scope.exchanges.length,
  scopeSha256,
  existingBaseline: 'data/subsecond-microprice-edge-20261013.json',
};
const modulePath = path.join(root, 'research/multilevel-book-flow-latency-v1.mjs');
const runnerPath = fileURLToPath(import.meta.url);
const result = {
  ...study.report({ source: rootSource }),
  at: new Date().toISOString(),
  experiment: {
    runner: path.basename(runnerPath),
    replayMode: 'offline exact frozen-prefix replay; zero API/account requests',
    baselineManifestSha256: createHash('sha256').update(baselineBytes).digest('hex'),
    analysisSourceSha256: {
      module: createHash('sha256').update(fs.readFileSync(modulePath)).digest('hex'),
      runner: createHash('sha256').update(fs.readFileSync(runnerPath)).digest('hex'),
    },
    scopeReconstruction: 'same frozen initial-config membership as #72; each subsequent point revalidated by the official engine book-point extractor',
    scopeMarkets: topicMarkets.size,
    processedBatches,
    rawBookPoints,
    acceptedUpdates,
    rejectedPoints,
    sourceReceiptClockSkews,
    rejectionReasons: Object.fromEntries(Object.entries(rejectionReasons).sort(([a], [b]) => a.localeCompare(b))),
    runtimeMs: performance.now() - started,
  },
};
fs.writeFileSync(outputPath, JSON.stringify(result) + '\n', { flag: 'wx' });
console.log(JSON.stringify({
  output: path.relative(root, outputPath),
  source: rootSource,
  counts: result.counts,
  sourceReplay: result.experiment,
  latencyCells: Object.fromEntries([0, 1000, 3000].map(latency => [latency, result.cells
    .filter(cell => cell.period === 'heldOut' && cell.responseLatencyMs === latency)
    .map(cell => ({ feature: cell.feature, horizonMs: cell.horizonMs, n: cell.nonzeroSignals,
      markets: cell.uniqueMarkets, opportunityNetTicks: cell.meanOpportunityNetPerDecisionTicks,
      pairedDeltaVsTop1Ticks: cell.meanPairedDeltaVsTop1Ticks }))])),
  actualFillsProven: result.actualFillsProven,
  profitabilityEstablished: result.profitabilityEstablished,
}, null, 2));
