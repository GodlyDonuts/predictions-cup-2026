import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { journalLines } from '../live-paper/journal-lines.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import { createConditionalFillMarkoutV1 } from '../research/conditional-fill-markout-v1.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const manifestPath = path.join(here, 'conditional-fill-markout-20261013.json');
const outputPath = path.join(here, 'conditional-fill-markout-20261013-broad-prefix-v3.json');
if (fs.existsSync(outputPath)) throw Error('BROAD_MARKOUT_RESULT_EXISTS');

const manifestBytes = fs.readFileSync(manifestPath);
const manifest = JSON.parse(manifestBytes.toString('utf8'));
const expected = manifest?.source;
if (!expected || typeof expected.file !== 'string' || path.basename(expected.file) !== expected.file || !/^realtime-[A-Za-z0-9.-]+\.jsonl$/.test(expected.file)
  || !Number.isSafeInteger(expected.bytes) || expected.bytes < 1 || !Number.isSafeInteger(expected.captureRecords)
  || !/^[a-f0-9]{64}$/.test(expected.sha256) || typeof expected.from !== 'string' || typeof expected.through !== 'string') {
  throw Error('BROAD_MARKOUT_MANIFEST');
}
const originalPolicy = manifest.definition?.chronologicalSplit;
if (manifest.definition?.activationLatencyMs !== 3000 || manifest.definition?.quoteTtlAfterActivationMs !== 10000
  || manifest.definition?.maximumMarkoutDelayMs !== 10000 || manifest.definition?.fees !== '0.000000'
  || JSON.stringify(manifest.definition?.horizonMs) !== JSON.stringify([1000, 5000, 30000])
  || originalPolicy?.splitAt !== '2026-10-13T06:18:44.458Z' || originalPolicy?.purgeMs !== 60000) {
  throw Error('BROAD_MARKOUT_POLICY_MISMATCH');
}
if (!process.env.LOCALAPPDATA) throw Error('BROAD_MARKOUT_LOCALDATA_MISSING');
const runtimeDir = path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime');
const sourcePath = path.join(runtimeDir, expected.file);
const before = fs.statSync(sourcePath);
if (!before.isFile() || before.size < expected.bytes) throw Error('BROAD_MARKOUT_SOURCE_TOO_SHORT');

const started = performance.now();
const prefixHash = createHash('sha256');
const manifestHash = createHash('sha256').update(manifestBytes).digest('hex');
const study = createConditionalFillMarkoutV1({
  latencyMs: manifest.definition.activationLatencyMs,
  quoteTtlMs: manifest.definition.quoteTtlAfterActivationMs,
  sampleEveryMs: 30000,
  markoutHorizonsMs: manifest.definition.horizonMs,
  maximumMarkoutDelayMs: manifest.definition.maximumMarkoutDelayMs,
  exitResponseLatencyMs: 3000,
  purgeMs: originalPolicy.purgeMs,
});

const configuredPairs = new Map();
const marketExchanges = new Map();
const exchangeMarkets = new Map();
const topics = new Set();
let tournamentId = null;
let configRows = 0;
let duplicateConfigPairs = 0;
let initialScopeFinalized = false;
let scope = null;
let scopeHash = null;
let scopeBreadth = null;
const membersByTopic = new Map();
const pairKeys = new Set();

function addConfig(record) {
  const configured = record.scope;
  if (!configured || typeof configured.tournamentId !== 'string' || typeof configured.marketId !== 'string'
    || !Array.isArray(configured.exchangeIds) || !configured.exchangeIds.length) throw Error('BROAD_MARKOUT_CONFIG_SCOPE');
  if (tournamentId === null) tournamentId = configured.tournamentId;
  if (configured.tournamentId !== tournamentId) throw Error('BROAD_MARKOUT_CONFIG_TOURNAMENT');
  const canonicalTopic = `tournament:${tournamentId}:market:${configured.marketId}`;
  if (record.topic !== canonicalTopic || configured.topic !== undefined && configured.topic !== canonicalTopic) throw Error('BROAD_MARKOUT_CONFIG_TOPIC');
  if (!Array.isArray(configured.exchangeIds) || configured.exchangeIds.some(id => typeof id !== 'string' || !id)) throw Error('BROAD_MARKOUT_CONFIG_EXCHANGE');

  configRows++;
  topics.add(canonicalTopic);
  const marketSet = marketExchanges.get(configured.marketId) ?? new Set();
  for (const exchangeId of configured.exchangeIds) {
    const key = `${configured.marketId}\u0000${exchangeId}`;
    if (configuredPairs.has(key)) {
      duplicateConfigPairs++;
      continue;
    }
    configuredPairs.set(key, { marketId: configured.marketId, exchangeId, topic: canonicalTopic });
    marketSet.add(exchangeId);
    const linkedMarkets = exchangeMarkets.get(exchangeId) ?? new Set();
    linkedMarkets.add(configured.marketId);
    exchangeMarkets.set(exchangeId, linkedMarkets);
  }
  marketExchanges.set(configured.marketId, marketSet);
}

function finalizeInitialScope() {
  if (initialScopeFinalized) return;
  initialScopeFinalized = true;
  if (!configRows || !configuredPairs.size || !tournamentId) throw Error('BROAD_MARKOUT_INITIAL_CONFIG_MISSING');
  if ([...exchangeMarkets.values()].some(markets => markets.size !== 1)) throw Error('BROAD_MARKOUT_AMBIGUOUS_EXCHANGE');
  const exchanges = [...configuredPairs.values()];
  scope = { tournamentId, exchanges };
  for (const member of exchanges) {
    const members = membersByTopic.get(member.topic) ?? [];
    members.push(member);
    membersByTopic.set(member.topic, members);
    pairKeys.add(`${member.marketId}\u0000${member.exchangeId}`);
  }
  const canonicalPairs = exchanges.map(({ marketId, exchangeId }) => [marketId, exchangeId])
    .sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));
  scopeHash = createHash('sha256').update(JSON.stringify({ tournamentId, pairs: canonicalPairs })).digest('hex');
  const breadthCounts = { oneExchange: 0, twoExchanges: 0, threeOrMoreExchanges: 0 };
  for (const set of marketExchanges.values()) {
    if (set.size === 1) breadthCounts.oneExchange++;
    else if (set.size === 2) breadthCounts.twoExchanges++;
    else breadthCounts.threeOrMoreExchanges++;
  }
  scopeBreadth = {
    initialConfigRows: configRows,
    uniqueMarkets: marketExchanges.size,
    uniqueExchanges: exchangeMarkets.size,
    uniqueMarketExchangePairs: configuredPairs.size,
    uniqueTopics: topics.size,
    duplicateConfigPairs,
    breadthByExchangesPerMarket: breadthCounts,
    normalizedMembershipSha256: scopeHash,
  };
}

const sampledAt = new Map(), revisions = new Map(), epochs = new Map();
let recordIndex = 0;
let prefixBytesRead = 0;
let acceptedPoints = 0;
let rejectedPoints = 0;
let firstObservedAt = null;
let through = null;

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
  if (!pairKeys.has(`${trade.marketId}\u0000${trade.exchangeId}`)) return null;
  const id = typeof trade.id === 'string' ? trade.id : String(trade.sequence);
  const sequence = Number(trade.sequence), quantity = trade.quantity;
  if (!Number.isSafeInteger(sequence) || sequence < 0 || String(sequence) !== id || !Number.isSafeInteger(quantity) || quantity < 1) return null;
  return { marketId: trade.marketId, exchangeId: trade.exchangeId, id, sequence, price: trade.price, quantity, executedAt: trade.executedAt };
}

for (const line of journalLines(sourcePath, { endByte: expected.bytes })) {
  prefixHash.update(line);
  prefixHash.update('\n');
  prefixBytesRead += Buffer.byteLength(line, 'utf8') + 1;
  const record = JSON.parse(line);
  if (record.format !== 'sig-feed-capture' || record.captureIndex !== recordIndex++) throw Error('BROAD_MARKOUT_CAPTURE_CHAIN');
  if (typeof record.observedAt !== 'string' || !Number.isFinite(Date.parse(record.observedAt))) throw Error('BROAD_MARKOUT_CAPTURE_CLOCK');
  firstObservedAt ??= record.observedAt;
  through = record.observedAt;
  if (record.kind === 'config') {
    if (initialScopeFinalized) throw Error('BROAD_MARKOUT_LATE_CONFIG');
    addConfig(record);
    continue;
  }
  finalizeInitialScope();

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
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) throw Error('BROAD_MARKOUT_REVISION');
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

finalizeInitialScope();
const after = fs.statSync(sourcePath);
if (after.dev !== before.dev || after.ino !== before.ino || after.size < expected.bytes) throw Error('BROAD_MARKOUT_SOURCE_REPLACED');
const actualSha256 = prefixHash.digest('hex');
if (prefixBytesRead !== expected.bytes || recordIndex !== expected.captureRecords || actualSha256 !== expected.sha256
  || firstObservedAt !== expected.from || through !== expected.through) throw Error('BROAD_MARKOUT_FROZEN_PREFIX_MISMATCH');

const source = {
  file: expected.file,
  bytes: expected.bytes,
  sha256: actualSha256,
  captureRecords: recordIndex,
  from: firstObservedAt,
  through,
};
const analysis = study.report({ splitAt: originalPolicy.splitAt, source });
const foldResults = ['development', 'heldOut'].map(fold => ({
  fold,
  actions: ['buy', 'sell'].map(action => {
    const rows = analysis.tables.filter(row => row.fold === fold && row.action === action);
    const baseline = rows.find(row => row.horizonMs === manifest.definition.horizonMs[0]);
    return {
      action,
      candidateQuotes: baseline?.candidateQuotes ?? 0,
      activatedQuotes: baseline?.activatedQuotes ?? 0,
      strictThroughProxies: baseline?.strictThroughProxies ?? 0,
      markouts: rows.map(row => ({
        horizonMs: row.horizonMs,
        markoutSamples: row.markoutSamples,
        uniqueMarkets: row.overall.markets,
        meanExecutableNetPerUnit: row.overall.meanExecutableNet,
        medianExecutableNetPerUnit: row.overall.medianExecutableNet,
        oneTickEachWayStressMeanPerUnit: row.overall.oneTickEachWayStressMean,
        positiveFraction: row.overall.positiveFraction,
        eventTimeDiagnosticSamples: row.eventTimeDiagnosticSamples,
        eventTimeDiagnostic: row.eventTimeDiagnostic,
      })),
    };
  }),
}));

const result = {
  ...analysis,
  experiment: {
    runner: path.basename(fileURLToPath(import.meta.url)),
    replayMode: 'offline exact frozen-prefix replay; no API, account, order, or service access',
    sourceManifestSha256: manifestHash,
    scopeReconstruction: 'initial contiguous capture config rows only; exchange membership rebuilt per configured market',
    scopeBreadth,
  },
  folds: foldResults,
  runtimeMs: performance.now() - started,
  acceptedPoints,
  rejectedPoints,
};

const fd = fs.openSync(outputPath, 'wx');
try { fs.writeFileSync(fd, JSON.stringify(result) + '\n', 'utf8'); }
finally { fs.closeSync(fd); }
console.log(JSON.stringify({
  output: path.basename(outputPath),
  source: { file: source.file, bytes: source.bytes, sha256: source.sha256, captureRecords: source.captureRecords, from: source.from, through: source.through },
  scopeBreadth,
  counts: analysis.counts,
  folds: foldResults,
  runtimeMs: result.runtimeMs,
}));

