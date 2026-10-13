import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { journalLines } from '../live-paper/journal-lines.mjs';
import { sealedNativePrefix } from '../live-paper/native-dip-host-v1.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import { createConditionalFillMarkoutV1 } from '../research/conditional-fill-markout-v1.mjs';

const runtimeDir = path.join(process.env.LOCALAPPDATA, 'SigTradingCup/paper-runtime');
const session = JSON.parse(fs.readFileSync(path.join(runtimeDir, 'realtime-session.json')));
if (!/^realtime-[A-Za-z0-9.-]+\.jsonl$/.test(session.file) || !Array.isArray(session.scope?.markets) || typeof session.scope?.tournamentId !== 'string') throw Error('MARKOUT_SESSION');
const sourceFile = path.join(runtimeDir, session.file), sealed = sealedNativePrefix(sourceFile);
const scope = { tournamentId: session.scope.tournamentId, exchanges: session.scope.markets };
const membersByTopic = new Map();
for (const member of scope.exchanges) {
  const topic = `tournament:${scope.tournamentId}:market:${member.marketId}`;
  const members = membersByTopic.get(topic) ?? [];
  members.push(member);
  membersByTopic.set(topic, members);
}

const study = createConditionalFillMarkoutV1();
const prefixHash = createHash('sha256'), sampledAt = new Map(), revisions = new Map(), epochs = new Map();
let recordIndex = 0, acceptedPoints = 0, rejectedPoints = 0, firstObservedAt = null, through = null;
const started = performance.now();
function clearTopic(topic, at) { for (const member of membersByTopic.get(topic) ?? []) study.boundary(member.exchangeId, at); }
function clearMarket(topic, exchangeIds = null, at) {
  const members = membersByTopic.get(topic) ?? [];
  const selected = exchangeIds === null ? members : members.filter(x => exchangeIds.has(x.exchangeId));
  for (const member of selected) study.boundary(member.exchangeId, at);
}
function resetTopicFeatures(topic) { for (const member of membersByTopic.get(topic) ?? []) study.resetFeatures(member.exchangeId); }
const hardInvalidations = new Set(['EXPIRY', 'STALE', 'BOOK_DIRTY', 'SCHEMA', 'MISSING_BOOK', 'SETTLEMENT', 'RESYNC_REQUIRED', 'SOCKET_ERROR', 'RECONNECT', 'DISCONNECTED', 'REFRESH', 'INITIAL', 'RECOVERY_FAILED']);
function scopedTrade(trade) {
  if (!trade || trade.tournamentId !== scope.tournamentId || typeof trade.marketId !== 'string' || typeof trade.exchangeId !== 'string') return null;
  if (!scope.exchanges.some(x => x.marketId === trade.marketId && x.exchangeId === trade.exchangeId)) return null;
  const id = typeof trade.id === 'string' ? trade.id : String(trade.sequence);
  const sequence = Number(trade.sequence), quantity = trade.quantity;
  if (!Number.isSafeInteger(sequence) || sequence < 0 || String(sequence) !== id || !Number.isSafeInteger(quantity) || quantity < 1) return null;
  return { marketId: trade.marketId, exchangeId: trade.exchangeId, id, sequence, price: trade.price, quantity, executedAt: trade.executedAt };
}

for (const line of journalLines(sourceFile, { endByte: sealed.bytes })) {
  prefixHash.update(line + '\n');
  const record = JSON.parse(line);
  if (record.format !== 'sig-feed-capture' || record.captureIndex !== recordIndex++) throw Error('MARKOUT_CAPTURE_CHAIN');
  if (typeof record.observedAt !== 'string' || !Number.isFinite(Date.parse(record.observedAt))) throw Error('MARKOUT_CAPTURE_CLOCK');
  firstObservedAt ??= record.observedAt;
  through = record.observedAt;
  if (record.kind === 'trade_observed') {
    const trade = scopedTrade(record.trade);
    if (trade) study.observeBatch({ observedAt: record.observedAt, trades: [trade] });
    continue;
  }
  const topic = record.topic;
  if (typeof topic !== 'string' || !membersByTopic.has(topic)) continue;

  if (['disconnected', 'stopped', 'subscribed', 'rejected'].includes(record.kind)) {
    clearTopic(topic, record.observedAt);
    epochs.delete(topic);
    continue;
  }
  if (record.kind === 'invalidation') {
    const reason = String(record.reason ?? '');
    const only = record.exchangeId ? new Set([String(record.exchangeId)]) : null;
    if (hardInvalidations.has(reason)) clearMarket(topic, only, record.observedAt);
    else if (reason === 'GAP' || reason === 'UNSEQUENCED') resetTopicFeatures(topic);
    continue;
  }
  if (record.kind !== 'event' || record.event !== 'market_batch') {
    if (record.kind === 'event') clearTopic(topic, record.observedAt);
    continue;
  }

  const key = `${topic}:${record.epoch}`;
  const previousEpoch = epochs.get(topic);
  if (previousEpoch !== undefined && previousEpoch !== record.epoch) clearTopic(topic, record.observedAt);
  epochs.set(topic, record.epoch);
  const payload = record.payload;
  if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
    clearTopic(topic, record.observedAt);
    continue;
  }
  const delivery = payload.delivery;
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) throw Error('MARKOUT_REVISION');
  const priorRevision = revisions.get(key);
  if (priorRevision !== undefined && delivery.previousRevision !== priorRevision) resetTopicFeatures(topic);
  if (priorRevision !== undefined && delivery.revision <= priorRevision) continue;
  revisions.set(key, delivery.revision);

  const observedMs = Date.parse(record.observedAt), lastSample = sampledAt.get(key) ?? -Infinity;
  let books = [];
  if (observedMs - lastSample >= 1000) {
    sampledAt.set(key, observedMs);
    const result = engineBookPointsV1(record, scope, record.observedAt);
    rejectedPoints += result.rejected.length;
    books = result.books;
    acceptedPoints += books.length;
  }
  const trades = (payload.trades ?? []).map(scopedTrade).filter(Boolean);
  study.observeBatch({ observedAt: record.observedAt, books, trades });
}

const after = fs.statSync(sourceFile);
if (after.dev !== sealed.dev || after.ino !== sealed.ino || after.size < sealed.bytes) throw Error('MARKOUT_SOURCE_REPLACED');
const fromMs = Date.parse(firstObservedAt), throughMs = Date.parse(through);
if (!Number.isFinite(fromMs) || !Number.isFinite(throughMs) || throughMs <= fromMs) throw Error('MARKOUT_SOURCE_SPAN');
const splitAt = new Date(fromMs + (throughMs - fromMs) * 0.7).toISOString();
const source = { file: session.file, bytes: sealed.bytes, sha256: prefixHash.digest('hex'), captureRecords: recordIndex, from: firstObservedAt, through };
const report = {
  ...study.report({ splitAt, source }),
  at: new Date().toISOString(),
  source,
  runtimeMs: performance.now() - started,
  acceptedPoints,
  rejectedPoints,
};
const output = path.resolve('data/conditional-fill-markout-20261013.json');
fs.writeFileSync(output, JSON.stringify(report) + '\n');
console.log(JSON.stringify({ output: path.relative(process.cwd(), output), source: report.source, runtimeMs: report.runtimeMs, acceptedPoints, candidates: report.counts.candidateQuotes, strictThroughProxies: report.counts.strictThroughProxies, tables: report.tables.map(x => ({ fold: x.fold, side: x.action, horizonMs: x.horizonMs, proxyFills: x.strictThroughProxies, markouts: x.markoutSamples, meanExecutableNet: x.overall.meanExecutableNet, stress: x.overall.oneTickEachWayStressMean })) }));
