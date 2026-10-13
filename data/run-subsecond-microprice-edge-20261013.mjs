import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { journalLines } from '../live-paper/journal-lines.mjs';
import { sealedNativePrefix } from '../live-paper/native-dip-host-v1.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import { createSubsecondMicropriceEdgeV1 } from '../research/subsecond-microprice-edge-v1.mjs';

const EXPECTED = Object.freeze({
  tournamentId: 'bda92870-621e-47b0-bc3c-3602c5c26f55',
  file: 'realtime-2026-10-13T02-55-28-953Z-d53812dd-ec75-4852-9267-59c7d5bb7898.jsonl',
  bytes: 1_747_459_094,
  sha256: 'f9a20ef98e8e9c96d1229dbbc6a8eb17dd669e7c6f3f8d6e49337439945f4418',
  captureRecords: 467_044,
  scopeMarkets: 237,
});

if (!process.env.LOCALAPPDATA) throw Error('SUBSECOND_LOCALAPPDATA');
const root = process.cwd();
const runtime = path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime');
const sourcePath = path.join(runtime, EXPECTED.file);
const seal = sealedNativePrefix(sourcePath);
if (seal.bytes < EXPECTED.bytes) throw Error('SUBSECOND_CAPTURE_SHORT');

// Validate the exact source prefix against the already-published #18 study; its
// outputs are not replay targets and do not select thresholds or outcomes.
const baselinePath = path.join(root, 'data/microprice-edge-calibration-20261013.json');
const baseline = JSON.parse(fs.readFileSync(baselinePath, 'utf8'));
if (baseline.source?.file !== EXPECTED.file || baseline.source?.bytes !== EXPECTED.bytes
  || baseline.source?.sha256 !== EXPECTED.sha256 || baseline.source?.captureRecords !== EXPECTED.captureRecords) {
  throw Error('SUBSECOND_SOURCE_MANIFEST');
}

const topicPrefix = `tournament:${EXPECTED.tournamentId}:market:`;
const exchangeToMarket = new Map();
const topicMarkets = new Set();
let firstObservedAt = null, through = null;
for (const line of journalLines(sourcePath, { endByte: EXPECTED.bytes })) {
  const record = JSON.parse(line);
  if (typeof record.observedAt !== 'string' || !Number.isFinite(Date.parse(record.observedAt))) {
    throw Error('SUBSECOND_CAPTURE_CLOCK');
  }
  firstObservedAt ??= record.observedAt;
  through = record.observedAt;
  if (record.kind !== 'event' || record.event !== 'market_batch' || !record.topic?.startsWith(topicPrefix)) continue;
  const marketId = record.topic.slice(topicPrefix.length);
  if (!/^\d+$/.test(marketId)) throw Error('SUBSECOND_MARKET_ID');
  topicMarkets.add(marketId);
  for (const book of record.payload?.books ?? []) {
    const exchangeId = String(book.exchangeId ?? '');
    if (!/^\d+$/.test(exchangeId)) throw Error('SUBSECOND_EXCHANGE_ID');
    const oldMarket = exchangeToMarket.get(exchangeId);
    if (oldMarket !== undefined && oldMarket !== marketId) throw Error('SUBSECOND_SCOPE_CONFLICT');
    exchangeToMarket.set(exchangeId, marketId);
  }
}
if (topicMarkets.size !== EXPECTED.scopeMarkets || exchangeToMarket.size !== EXPECTED.scopeMarkets) {
  throw Error('SUBSECOND_SCOPE_SIZE');
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

const fromMs = Date.parse(firstObservedAt), throughMs = Date.parse(through);
if (!Number.isFinite(fromMs) || throughMs <= fromMs) throw Error('SUBSECOND_CAPTURE_SPAN');
const splitAt = new Date(fromMs + Math.floor((throughMs - fromMs) * 0.70)).toISOString();
const study = createSubsecondMicropriceEdgeV1({ splitAt });
const digest = createHash('sha256');
const revisions = new Map(), epochs = new Map();
const rejectionReasons = Object.create(null);
let captureIndex = 0, processedBatches = 0, rawBookPoints = 0, acceptedUpdates = 0, rejectedPoints = 0;
let previousObservedMs = -1;
const started = performance.now();

function boundaryTopic(topic, at, reason) {
  for (const member of membersByTopic.get(topic) ?? []) study.boundary(member.exchangeId, at, reason);
}

for (const line of journalLines(sourcePath, { endByte: EXPECTED.bytes })) {
  digest.update(line + '\n');
  const record = JSON.parse(line);
  if (record.format !== 'sig-feed-capture' || record.captureIndex !== captureIndex++) throw Error('SUBSECOND_CAPTURE_CHAIN');
  const observedMs = Date.parse(record.observedAt);
  if (!Number.isFinite(observedMs) || observedMs < previousObservedMs) throw Error('SUBSECOND_CAPTURE_ROLLBACK');
  previousObservedMs = observedMs;
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

  const oldEpoch = epochs.get(topic);
  if (oldEpoch !== undefined && oldEpoch !== record.epoch) boundaryTopic(topic, record.observedAt, 'epoch_change');
  epochs.set(topic, record.epoch);
  const payload = record.payload;
  if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
    boundaryTopic(topic, record.observedAt, payload.marketSettled?.length ? 'market_settled' : 'resync_required');
    continue;
  }
  const delivery = payload.delivery;
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) {
    throw Error('SUBSECOND_DELIVERY_REVISION');
  }
  const revisionKey = `${topic}:${record.epoch}`;
  const priorRevision = revisions.get(revisionKey);
  if (priorRevision !== undefined && delivery.previousRevision !== priorRevision) {
    boundaryTopic(topic, record.observedAt, 'delivery_gap');
  }
  if (priorRevision !== undefined && delivery.revision <= priorRevision) continue;
  revisions.set(revisionKey, delivery.revision);

  const points = engineBookPointsV1(record, scope, record.observedAt);
  rejectedPoints += points.rejected.length;
  for (const rejected of points.rejected) {
    const reason = /^[A-Za-z0-9_]+$/.test(rejected.reason ?? '') ? rejected.reason : 'other';
    rejectionReasons[reason] = (rejectionReasons[reason] ?? 0) + 1;
  }
  processedBatches++;
  rawBookPoints += points.books.length;
  for (const book of points.books) if (study.observe(book, {
    observedAt: record.observedAt,
    receivedAt: book.receivedAt,
  })) acceptedUpdates++;
}

const finalStat = fs.statSync(sourcePath);
if (finalStat.dev !== seal.dev || finalStat.ino !== seal.ino || finalStat.size < EXPECTED.bytes) {
  throw Error('SUBSECOND_CAPTURE_REPLACED');
}
if (captureIndex !== EXPECTED.captureRecords || firstObservedAt !== '2026-10-13T02:55:28.956Z'
  || through !== '2026-10-13T07:45:51.103Z') throw Error('SUBSECOND_CAPTURE_EXPECTATION');
const sha256 = digest.digest('hex');
if (sha256 !== EXPECTED.sha256) throw Error('SUBSECOND_CAPTURE_HASH');
study.finish();
const source = {
  file: EXPECTED.file,
  bytes: EXPECTED.bytes,
  sha256,
  captureRecords: captureIndex,
  from: firstObservedAt,
  through,
  scopeMarkets: scope.exchanges.length,
  existingBaseline: 'data/microprice-edge-calibration-20261013.json',
};
const result = {
  ...study.report({ source }),
  at: new Date().toISOString(),
  replayRuntimeMs: performance.now() - started,
  sourceReplay: {
    processedBatches,
    rawBookPoints,
    acceptedUpdates,
    rejectedPoints,
    rejectionReasons: Object.fromEntries(Object.entries(rejectionReasons).sort(([a], [b]) => a.localeCompare(b))),
  },
};
const outputPath = path.resolve(root, 'data/subsecond-microprice-edge-20261013.json');
fs.writeFileSync(outputPath, JSON.stringify(result) + '\n');
console.log(JSON.stringify({
  output: path.relative(root, outputPath),
  source,
  replayRuntimeMs: result.replayRuntimeMs,
  counts: result.counts,
  heldOutByHorizon: Object.fromEntries(result.protocol.horizonsMs.map(h => [h, result.folds.heldOut[h]])),
  pointLatencyByPeriod: result.pointLatencyByPeriod,
  actualFillsProven: result.actualFillsProven,
  profitabilityEstablished: result.profitabilityEstablished,
}));
