import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { journalLines } from '../live-paper/journal-lines.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import { createPassiveRoundtripV1, verifyFixedPrefix } from '../research/passive-roundtrip-v1.mjs';

const EXPECTED = Object.freeze({
  file: 'realtime-2026-10-13T13-28-06-669Z-a7674ae9-ed0e-41c6-90c8-390b1ce0dc8b.jsonl',
  bytes: 148_491_313,
  sha256: '7ae01418412c5e2d11c8818478791fa55f5093d289b1a7982cb3a374186b3668',
  captureRecords: 40_287,
  marketBatchRecords: 37_068,
  rawBookPoints: 37_068,
  topLevelTradeRecords: 441,
  markets: 237,
  from: '2026-10-13T13:28:06.671Z',
  through: '2026-10-13T13:57:52.632Z',
});

const runtimeDir = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime');
if (!process.env.LOCALAPPDATA) throw Error('ROUNDTRIP_LOCALAPPDATA');
const sourceFile = path.join(runtimeDir, EXPECTED.file);
const started = performance.now();
const frozen = verifyFixedPrefix(sourceFile, EXPECTED);
const membersByMarket = new Map(), topics = new Map();
const audit = { records: 0, marketBatchRecords: 0, rawBookPoints: 0, topLevelTradeRecords: 0, firstObservedAt: null, through: null, tournamentId: null, lastObservedMs: -Infinity };

function captureRecord(line, expectedIndex) {
  const record = JSON.parse(line);
  if (record.format !== 'sig-feed-capture' || record.captureIndex !== expectedIndex) throw Error('ROUNDTRIP_CAPTURE_CHAIN');
  if (typeof record.observedAt !== 'string' || !Number.isFinite(Date.parse(record.observedAt))) throw Error('ROUNDTRIP_CAPTURE_CLOCK');
  const observedMs = Date.parse(record.observedAt);
  if (observedMs < audit.lastObservedMs) throw Error('ROUNDTRIP_CAPTURE_ROLLBACK');
  audit.lastObservedMs = observedMs;
  audit.firstObservedAt ??= record.observedAt;
  audit.through = record.observedAt;
  return record;
}

for (const line of journalLines(sourceFile, { endByte: EXPECTED.bytes })) {
  const record = captureRecord(line, audit.records++);
  if (record.kind === 'trade_observed') audit.topLevelTradeRecords++;
  if (record.kind !== 'event' || record.event !== 'market_batch') continue;
  audit.marketBatchRecords++;
  audit.rawBookPoints += Array.isArray(record.payload?.books) ? record.payload.books.length : 0;
  const match = /^tournament:([^:]+):market:(.+)$/.exec(record.topic ?? '');
  if (!match) throw Error('ROUNDTRIP_CAPTURE_TOPIC');
  const [, tournamentId, marketId] = match;
  if (audit.tournamentId !== null && audit.tournamentId !== tournamentId) throw Error('ROUNDTRIP_TOURNAMENT_MIX');
  audit.tournamentId = tournamentId;
  const topicMarkets = topics.get(record.topic) ?? new Set();
  topicMarkets.add(marketId);
  topics.set(record.topic, topicMarkets);
  for (const book of record.payload?.books ?? []) {
    if (typeof book.exchangeId !== 'string' || !book.exchangeId) throw Error('ROUNDTRIP_CAPTURE_EXCHANGE');
    const prior = membersByMarket.get(marketId);
    if (prior && prior.exchangeId !== book.exchangeId) throw Error('ROUNDTRIP_MARKET_EXCHANGE_MIX');
    membersByMarket.set(marketId, { marketId, exchangeId: book.exchangeId });
  }
}

if (audit.records !== EXPECTED.captureRecords || audit.marketBatchRecords !== EXPECTED.marketBatchRecords || audit.rawBookPoints !== EXPECTED.rawBookPoints || audit.topLevelTradeRecords !== EXPECTED.topLevelTradeRecords || membersByMarket.size !== EXPECTED.markets || audit.firstObservedAt !== EXPECTED.from || audit.through !== EXPECTED.through) throw Error('ROUNDTRIP_PREFIX_CONTENT');
const members = [...membersByMarket.values()].sort((a, b) => a.marketId.localeCompare(b.marketId));
const scope = { tournamentId: audit.tournamentId, exchanges: members };
const study = createPassiveRoundtripV1({ members });
const topicMembers = new Map();
for (const [topic, marketIds] of topics) topicMembers.set(topic, members.filter(member => marketIds.has(member.marketId)));
const epochs = new Map(), revisions = new Map();
const hardInvalidations = new Set(['EXPIRY', 'STALE', 'BOOK_DIRTY', 'SCHEMA', 'MISSING_BOOK', 'SETTLEMENT', 'RESYNC_REQUIRED', 'SOCKET_ERROR', 'RECONNECT', 'DISCONNECTED', 'REFRESH', 'INITIAL', 'RECOVERY_FAILED', 'GAP', 'UNSEQUENCED']);
let replayIndex = 0, acceptedPoints = 0, rejectedPoints = 0, scopedTrades = 0, unscopedTrades = 0;
audit.lastObservedMs = -Infinity;
audit.firstObservedAt = null;
audit.through = null;

function clearTopic(topic) {
  for (const member of topicMembers.get(topic) ?? []) study.boundary({ marketId: member.marketId });
}

function clearTopicMarket(topic, exchangeId = null) {
  const members = topicMembers.get(topic) ?? [];
  for (const member of members) if (exchangeId === null || member.exchangeId === exchangeId) study.boundary({ marketId: member.marketId });
}

function scopedTrade(trade) {
  if (!trade || trade.tournamentId !== scope.tournamentId || typeof trade.marketId !== 'string' || typeof trade.exchangeId !== 'string') return null;
  const member = membersByMarket.get(trade.marketId);
  if (!member || member.exchangeId !== trade.exchangeId) return null;
  const id = typeof trade.id === 'string' ? trade.id : String(trade.sequence);
  const sequence = Number(trade.sequence), quantity = trade.quantity;
  if (!Number.isSafeInteger(sequence) || sequence < 0 || String(sequence) !== id || !Number.isSafeInteger(quantity) || quantity < 1) return null;
  return { marketId: trade.marketId, exchangeId: trade.exchangeId, id, sequence, price: trade.price, quantity, executedAt: trade.executedAt };
}

for (const line of journalLines(sourceFile, { endByte: EXPECTED.bytes })) {
  const record = captureRecord(line, replayIndex++);
  if (record.kind === 'trade_observed') {
    const trade = scopedTrade(record.trade);
    if (trade) { scopedTrades++; study.observeBatch({ observedAt: record.observedAt, trades: [trade] }); }
    else unscopedTrades++;
    continue;
  }
  const topic = record.topic;
  if (typeof topic !== 'string' || !topicMembers.has(topic)) continue;
  if (['disconnected', 'stopped', 'subscribed', 'rejected'].includes(record.kind)) {
    clearTopic(topic);
    epochs.delete(topic);
    for (const key of revisions.keys()) if (key.startsWith(`${topic}:`)) revisions.delete(key);
    continue;
  }
  if (record.kind === 'invalidation') {
    const reason = String(record.reason ?? '');
    const only = record.exchangeId ? String(record.exchangeId) : null;
    if (hardInvalidations.has(reason)) clearTopicMarket(topic, only);
    continue;
  }
  if (record.kind !== 'event' || record.event !== 'market_batch') {
    if (record.kind === 'event') clearTopic(topic);
    continue;
  }

  const previousEpoch = epochs.get(topic);
  if (previousEpoch !== undefined && previousEpoch !== record.epoch) {
    clearTopic(topic);
    for (const key of revisions.keys()) if (key.startsWith(`${topic}:`)) revisions.delete(key);
  }
  epochs.set(topic, record.epoch);
  const payload = record.payload;
  if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
    clearTopic(topic);
    continue;
  }
  const delivery = payload.delivery;
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) throw Error('ROUNDTRIP_REVISION');
  const revisionKey = `${topic}:${record.epoch}`;
  const priorRevision = revisions.get(revisionKey);
  if (priorRevision !== undefined && delivery.previousRevision !== priorRevision) clearTopic(topic);
  if (priorRevision !== undefined && delivery.revision <= priorRevision) continue;
  revisions.set(revisionKey, delivery.revision);

  const result = engineBookPointsV1(record, scope, record.observedAt);
  rejectedPoints += result.rejected.length;
  acceptedPoints += result.books.length;
  const trades = (payload.trades ?? []).map(scopedTrade).filter(Boolean);
  scopedTrades += trades.length;
  study.observeBatch({ observedAt: record.observedAt, books: result.books, trades });
}

if (replayIndex !== EXPECTED.captureRecords) throw Error('ROUNDTRIP_REPLAY_COUNT');
const finalSource = verifyFixedPrefix(sourceFile, EXPECTED, frozen);
const report = study.report({
  throughAt: EXPECTED.through,
  source: {
    file: EXPECTED.file,
    bytes: EXPECTED.bytes,
    sha256: EXPECTED.sha256,
    fileDev: frozen.dev,
    fileInode: frozen.ino,
    fileSizeAtInitialCheck: frozen.sourceSizeAtCheck,
    fileSizeAtFinalCheck: finalSource.sourceSizeAtCheck,
    captureRecords: audit.records,
    marketBatchRecords: audit.marketBatchRecords,
    rawBookPoints: audit.rawBookPoints,
    topLevelTradeRecords: audit.topLevelTradeRecords,
    from: audit.firstObservedAt,
    through: audit.through,
    chronological: true,
    appendedBytesExcluded: true,
  },
});
report.replay = {
  elapsedMs: Math.round(performance.now() - started),
  acceptedBookPoints: acceptedPoints,
  rejectedEngineBookPoints: rejectedPoints,
  scopedTradeObservations: scopedTrades,
  unscopedTopLevelTradeRecords: unscopedTrades,
  marketsInScope: members.length,
  rule: 'no split, no tuning, full fixed prefix in chronological capture order; lifecycle crossing prefix end is censored/purged',
};

const output = path.resolve('data/passive-roundtrip-20261013.json');
fs.writeFileSync(output, JSON.stringify(report) + '\n');
console.log(JSON.stringify({ output: path.relative(process.cwd(), output), source: report.source, counts: report.counts, aggregate: report.aggregate, replay: report.replay }));
