import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { journalLines } from '../live-paper/journal-lines.mjs';
import { sealedNativePrefix } from '../live-paper/native-dip-host-v1.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import { createCausalDipReversionV1, causalDipReversionPolicyV1 } from '../research/causal-dip-reversion-v1.mjs';

const EXPECTED = Object.freeze({
  file: 'realtime-2026-10-13T02-55-28-953Z-d53812dd-ec75-4852-9267-59c7d5bb7898.jsonl',
  bytes: 1_747_459_094,
  sha256: 'f9a20ef98e8e9c96d1229dbbc6a8eb17dd669e7c6f3f8d6e49337439945f4418',
  records: 467_044,
  scopeMarkets: 237,
  from: '2026-10-13T02:55:28.956Z',
  through: '2026-10-13T07:45:51.103Z',
  tournamentId: 'bda92870-621e-47b0-bc3c-3602c5c26f55',
});
const root = process.cwd();
const runtimeDir = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime');
if (!process.env.LOCALAPPDATA) throw Error('DIP_LOCALAPPDATA');
const sourceFile = path.join(runtimeDir, EXPECTED.file);
const sealed = sealedNativePrefix(sourceFile);
if (sealed.bytes < EXPECTED.bytes) throw Error('DIP_CAPTURE_LENGTH');

// The prior source manifest freezes this capture's exact event-time bounds. It is
// not used for signal prices or outcome selection.
const priorStudy = JSON.parse(fs.readFileSync(path.join(root, 'data/conditional-fill-markout-20261013.json'), 'utf8'));
if (priorStudy.source?.file !== EXPECTED.file || priorStudy.source?.bytes !== EXPECTED.bytes
  || priorStudy.source?.sha256 !== EXPECTED.sha256 || priorStudy.source?.captureRecords !== EXPECTED.records
  || priorStudy.source?.from !== EXPECTED.from || priorStudy.source?.through !== EXPECTED.through) throw Error('DIP_CAPTURE_MANIFEST');

// Reconstruct the sealed capture's market/exchange membership from its own
// scoped market batches. realtime-session.json is mutable and may point at a
// later collector session, so it is not a valid historical scope dependency.
const topicPrefix = `tournament:${EXPECTED.tournamentId}:market:`;
const marketByExchange = new Map(), topicMarkets = new Set();
for (const line of journalLines(sourceFile, { endByte: EXPECTED.bytes })) {
  const record = JSON.parse(line);
  if (record.kind !== 'event' || record.event !== 'market_batch' || !record.topic?.startsWith(topicPrefix)) continue;
  const marketId = record.topic.slice(topicPrefix.length);
  if (!/^\d+$/.test(marketId)) throw Error('DIP_CAPTURE_MARKET_ID');
  topicMarkets.add(marketId);
  for (const book of record.payload?.books ?? []) {
    const exchangeId = String(book.exchangeId ?? '');
    if (!/^\d+$/.test(exchangeId)) throw Error('DIP_CAPTURE_EXCHANGE_ID');
    const priorMarketId = marketByExchange.get(exchangeId);
    if (priorMarketId !== undefined && priorMarketId !== marketId) throw Error('DIP_CAPTURE_SCOPE_CONFLICT');
    marketByExchange.set(exchangeId, marketId);
  }
}
if (topicMarkets.size !== EXPECTED.scopeMarkets || marketByExchange.size !== EXPECTED.scopeMarkets) throw Error('DIP_CAPTURE_SCOPE');
const scope = {
  tournamentId: EXPECTED.tournamentId,
  exchanges: [...marketByExchange].map(([exchangeId, marketId]) => ({ exchangeId, marketId })),
};
const membersByTopic = new Map();
for (const member of scope.exchanges) {
  const topic = `tournament:${scope.tournamentId}:market:${member.marketId}`;
  const members = membersByTopic.get(topic) ?? [];
  members.push(member);
  membersByTopic.set(topic, members);
}
const startMs = Date.parse(EXPECTED.from), endMs = Date.parse(EXPECTED.through);
const splitAt = new Date(startMs + Math.floor((endMs - startMs) * 0.70)).toISOString();
const study = createCausalDipReversionV1({ splitAt });
const prefixHash = createHash('sha256');
const revisions = new Map(), epochs = new Map();
let recordIndex = 0, acceptedPoints = 0, rejectedPoints = 0, invalidations = 0;
const sourceBoundaryReasons = Object.create(null);
let firstObservedAt = null, through = null, previousObservedMs = -1;
const started = performance.now();

function countSourceBoundary(reason) {
  sourceBoundaryReasons[reason] = (sourceBoundaryReasons[reason] ?? 0) + 1;
}
function boundaryTopic(topic, at, reason = 'source_boundary') {
  for (const member of membersByTopic.get(topic) ?? []) study.boundary(member.exchangeId, at, reason);
}
function boundaryMarket(topic, exchangeId, at, reason = 'source_boundary') {
  const member = (membersByTopic.get(topic) ?? []).find(x => x.exchangeId === exchangeId);
  if (member) study.boundary(member.exchangeId, at, reason);
}
const invalidationReasons = new Set(['INITIAL', 'EXPIRY', 'STALE', 'GAP', 'PERIODIC', 'RECOVERY_FAILED',
  'SOCKET_ERROR', 'RECONNECT', 'DISCONNECTED', 'REFRESH']);
function observeBookPoint(book, at) {
  const bid = book.bids?.[0], ask = book.asks?.[0];
  if (!bid || !ask) { rejectedPoints++; return; }
  study.observeBook({
    marketId: book.marketId,
    exchangeId: book.exchangeId,
    at,
    bid: bid.price,
    ask: ask.price,
    bidQuantity: bid.quantity,
    askQuantity: ask.quantity,
  });
  acceptedPoints++;
}

for (const line of journalLines(sourceFile, { endByte: EXPECTED.bytes })) {
  prefixHash.update(line + '\n');
  const record = JSON.parse(line);
  if (record.format !== 'sig-feed-capture' || record.captureIndex !== recordIndex++) throw Error('DIP_CAPTURE_CHAIN');
  if (typeof record.observedAt !== 'string' || !Number.isFinite(Date.parse(record.observedAt))) throw Error('DIP_CAPTURE_CLOCK');
  const observedMs = Date.parse(record.observedAt);
  if (observedMs < previousObservedMs) throw Error('DIP_CAPTURE_CLOCK_ROLLBACK');
  previousObservedMs = observedMs;
  firstObservedAt ??= record.observedAt;
  through = record.observedAt;
  if (record.kind === 'trade_observed') continue;

  const topic = record.topic;
  if (typeof topic !== 'string' || !membersByTopic.has(topic)) continue;
  if (['disconnected', 'stopped', 'subscribed', 'rejected'].includes(record.kind)) {
    const reason = `source_${record.kind}`;
    countSourceBoundary(reason);
    boundaryTopic(topic, record.observedAt, reason);
    epochs.delete(topic);
    revisions.delete(topic);
    continue;
  }
  if (record.kind === 'invalidation') {
    invalidations++;
    const code = invalidationReasons.has(record.reason) ? record.reason : 'OTHER';
    const reason = `invalidation_${code}`;
    countSourceBoundary(reason);
    if (record.exchangeId) boundaryMarket(topic, String(record.exchangeId), record.observedAt, reason);
    else boundaryTopic(topic, record.observedAt, reason);
    continue;
  }
  if (record.kind !== 'event' || record.event !== 'market_batch') {
    if (record.kind === 'event') {
      const reason = 'unsupported_event';
      countSourceBoundary(reason);
      boundaryTopic(topic, record.observedAt, reason);
    }
    continue;
  }

  const oldEpoch = epochs.get(topic);
  if (oldEpoch !== undefined && oldEpoch !== record.epoch) {
    countSourceBoundary('epoch_change');
    boundaryTopic(topic, record.observedAt, 'epoch_change');
  }
  epochs.set(topic, record.epoch);
  const payload = record.payload;
  if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
    const reason = payload.marketSettled?.length ? 'market_settled' : 'resync_required';
    countSourceBoundary(reason);
    boundaryTopic(topic, record.observedAt, reason);
    continue;
  }
  const delivery = payload.delivery;
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) throw Error('DIP_REVISION');
  const revisionKey = `${topic}:${record.epoch}`;
  const priorRevision = revisions.get(revisionKey);
  if (priorRevision !== undefined && delivery.previousRevision !== priorRevision) {
    countSourceBoundary('delivery_gap');
    boundaryTopic(topic, record.observedAt, 'delivery_gap');
  }
  if (priorRevision !== undefined && delivery.revision <= priorRevision) continue;
  revisions.set(revisionKey, delivery.revision);

  const result = engineBookPointsV1(record, scope, record.observedAt);
  rejectedPoints += result.rejected.length;
  const books = result.books.slice().sort((a, b) => Date.parse(a.receivedAt) - Date.parse(b.receivedAt)
    || Number(a.exchangeId) - Number(b.exchangeId));
  for (const book of books) observeBookPoint(book, record.observedAt);
}

const after = fs.statSync(sourceFile);
if (after.dev !== sealed.dev || after.ino !== sealed.ino || after.size < EXPECTED.bytes) throw Error('DIP_SOURCE_CHANGED');
if (recordIndex !== EXPECTED.records || firstObservedAt !== EXPECTED.from || through !== EXPECTED.through) throw Error('DIP_CAPTURE_SPAN');
const sourceSha = prefixHash.digest('hex');
if (sourceSha !== EXPECTED.sha256) throw Error('DIP_CAPTURE_HASH');
const source = {
  file: EXPECTED.file,
  bytes: EXPECTED.bytes,
  sha256: sourceSha,
  captureRecords: recordIndex,
  from: firstObservedAt,
  through,
  scopeMarkets: scope.exchanges.length,
};
const report = {
  ...study.report({ splitAt, source }),
  at: new Date().toISOString(),
  replayRuntimeMs: performance.now() - started,
  acceptedPoints,
  rejectedPoints,
  sourceInvalidations: invalidations,
  sourceBoundaryReasons,
  fixedProtocolVersion: 'causal-dip-reversion-v1',
};
report.protocol.policy = { ...causalDipReversionPolicyV1 };
const output = path.resolve(root, 'data/causal-dip-reversion-20261013.json');
fs.writeFileSync(output, JSON.stringify(report) + '\n');
console.log(JSON.stringify({
  output: path.relative(root, output), source, replayRuntimeMs: report.replayRuntimeMs,
  acceptedPoints, rejectedPoints, invalidations, sourceBoundaryReasons,
  development: report.folds.development,
  heldOut: report.folds.heldOut,
  purged: report.folds.purged,
  actualFillsProven: false,
  profitabilityEstablished: false,
}));
