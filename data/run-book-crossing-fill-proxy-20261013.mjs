import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { journalLines } from '../live-paper/journal-lines.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import { createBookCrossingFillProxyV1 } from '../research/book-crossing-fill-proxy-v1.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const manifestPath = path.join(here, 'conditional-fill-markout-20261013-v5.json');
const outputPath = path.join(here, 'book-crossing-fill-proxy-20261013.json');
if (fs.existsSync(outputPath)) throw Error('BOOK_CROSSING_RESULT_EXISTS');

const frozen = Object.freeze({
  file: 'realtime-2026-10-13T21-15-52-245Z-d02873df-952c-4bca-96b2-d52dbef272ba.jsonl',
  bytes: 295072261,
  sha256: '6acfa5d6cb160e6129befe7620f1bcfb3e1e925b7b6d25d497a87061c0497590',
  captureRecords: 81882,
  from: '2026-10-13T21:15:52.247Z',
  through: '2026-10-13T22:04:49.056Z',
  splitAt: '2026-10-13T21:50:08.013Z',
});
const manifestBytes = fs.readFileSync(manifestPath);
const manifest = JSON.parse(manifestBytes.toString('utf8'));
const manifestSource = manifest?.source;
if (!manifestSource || manifestSource.file !== frozen.file || manifestSource.bytes !== frozen.bytes || manifestSource.sha256 !== frozen.sha256
  || manifestSource.captureRecords !== frozen.captureRecords || manifestSource.from !== frozen.from || manifestSource.through !== frozen.through
  || manifest.definition?.chronologicalSplit?.splitAt !== frozen.splitAt
  || manifest.definition?.activationLatencyMs !== 3000 || manifest.definition?.quoteTtlAfterActivationMs !== 10000
  || manifest.definition?.exitResponseLatencyMs !== 3000 || manifest.definition?.fees !== '0.000000'
  || JSON.stringify(manifest.definition?.horizonMs) !== JSON.stringify([1000, 5000, 30000])) {
  throw Error('BOOK_CROSSING_FROZEN_POLICY_MISMATCH');
}
if (!process.env.LOCALAPPDATA) throw Error('BOOK_CROSSING_LOCALDATA_MISSING');
const sourcePath = path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime', frozen.file);
const before = fs.statSync(sourcePath);
if (!before.isFile() || before.size < frozen.bytes) throw Error('BOOK_CROSSING_SOURCE_TOO_SHORT');

const started = performance.now();
const prefixHash = createHash('sha256');
const manifestSha256 = createHash('sha256').update(manifestBytes).digest('hex');
let tournamentId = null;
let configRows = 0;
let duplicatePairs = 0;
let initialScopeFinalized = false;
let scope = null;
let scopeHash = null;
let scopeBreadth = null;
const configuredPairs = new Map();
const marketExchanges = new Map();
const exchangeMarkets = new Map();
const membersByTopic = new Map();
const pairKeys = new Set();
const epochs = new Map();
const revisions = new Map();
let prefixBytesRead = 0;
let recordIndex = 0;
let acceptedPoints = 0;
let rejectedPoints = 0;
let firstObservedAt = null;
let through = null;
let study = null;

function addConfig(record) {
  const configured = record.scope;
  if (!configured || typeof configured.tournamentId !== 'string' || typeof configured.marketId !== 'string'
    || !Array.isArray(configured.exchangeIds) || !configured.exchangeIds.length) throw Error('BOOK_CROSSING_CONFIG_SCOPE');
  if (tournamentId === null) tournamentId = configured.tournamentId;
  if (configured.tournamentId !== tournamentId) throw Error('BOOK_CROSSING_CONFIG_TOURNAMENT');
  const topic = `tournament:${tournamentId}:market:${configured.marketId}`;
  if (record.topic !== topic || configured.topic !== undefined && configured.topic !== topic) throw Error('BOOK_CROSSING_CONFIG_TOPIC');
  configRows++;
  const exchanges = marketExchanges.get(configured.marketId) ?? new Set();
  for (const exchangeId of configured.exchangeIds) {
    if (typeof exchangeId !== 'string' || !exchangeId) throw Error('BOOK_CROSSING_CONFIG_EXCHANGE');
    const key = `${configured.marketId}\u0000${exchangeId}`;
    if (configuredPairs.has(key)) { duplicatePairs++; continue; }
    configuredPairs.set(key, { marketId: configured.marketId, exchangeId, topic });
    exchanges.add(exchangeId);
    const linked = exchangeMarkets.get(exchangeId) ?? new Set();
    linked.add(configured.marketId);
    exchangeMarkets.set(exchangeId, linked);
  }
  marketExchanges.set(configured.marketId, exchanges);
}

function finalizeScope() {
  if (initialScopeFinalized) return;
  initialScopeFinalized = true;
  if (!configRows || !configuredPairs.size || !tournamentId) throw Error('BOOK_CROSSING_INITIAL_CONFIG_MISSING');
  if ([...exchangeMarkets.values()].some(markets => markets.size !== 1)) throw Error('BOOK_CROSSING_AMBIGUOUS_EXCHANGE');
  const exchanges = [...configuredPairs.values()];
  scope = { tournamentId, exchanges };
  for (const member of exchanges) {
    const list = membersByTopic.get(member.topic) ?? [];
    list.push(member);
    membersByTopic.set(member.topic, list);
    pairKeys.add(`${member.marketId}\u0000${member.exchangeId}`);
  }
  const canonicalPairs = exchanges.map(({ marketId, exchangeId }) => [marketId, exchangeId])
    .sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));
  scopeHash = createHash('sha256').update(JSON.stringify({ tournamentId, pairs: canonicalPairs })).digest('hex');
  const breadth = { oneExchange: 0, twoExchanges: 0, threeOrMoreExchanges: 0 };
  for (const set of marketExchanges.values()) {
    if (set.size === 1) breadth.oneExchange++;
    else if (set.size === 2) breadth.twoExchanges++;
    else breadth.threeOrMoreExchanges++;
  }
  scopeBreadth = {
    initialConfigRows: configRows,
    uniqueMarkets: marketExchanges.size,
    uniqueExchanges: exchangeMarkets.size,
    uniqueMarketExchangePairs: configuredPairs.size,
    duplicateConfigPairs: duplicatePairs,
    breadthByExchangesPerMarket: breadth,
    normalizedMembershipSha256: scopeHash,
  };
  study = createBookCrossingFillProxyV1({ scope });
}

function clearTopic(topic, reason, at) {
  for (const member of membersByTopic.get(topic) ?? []) study.boundary(member.exchangeId, reason, at);
  epochs.delete(topic);
}

function scopedTrade(trade) {
  if (!trade || trade.tournamentId !== scope.tournamentId || typeof trade.marketId !== 'string' || typeof trade.exchangeId !== 'string'
    || !pairKeys.has(`${trade.marketId}\u0000${trade.exchangeId}`)) return null;
  const id = typeof trade.id === 'string' ? trade.id : String(trade.sequence);
  const sequence = Number(trade.sequence), quantity = trade.quantity;
  if (!Number.isSafeInteger(sequence) || sequence < 0 || String(sequence) !== id || !Number.isSafeInteger(quantity) || quantity < 1) return null;
  return { tournamentId: scope.tournamentId, marketId: trade.marketId, exchangeId: trade.exchangeId, id, sequence, price: trade.price, quantity, executedAt: trade.executedAt };
}

for (const line of journalLines(sourcePath, { endByte: frozen.bytes })) {
  prefixHash.update(line);
  prefixHash.update('\n');
  prefixBytesRead += Buffer.byteLength(line, 'utf8') + 1;
  const record = JSON.parse(line);
  if (record.format !== 'sig-feed-capture' || record.captureIndex !== recordIndex++) throw Error('BOOK_CROSSING_CAPTURE_CHAIN');
  if (typeof record.observedAt !== 'string' || !Number.isFinite(Date.parse(record.observedAt))) throw Error('BOOK_CROSSING_CAPTURE_CLOCK');
  firstObservedAt ??= record.observedAt;
  through = record.observedAt;
  if (record.kind === 'config') {
    if (initialScopeFinalized) throw Error('BOOK_CROSSING_LATE_CONFIG');
    addConfig(record);
    continue;
  }
  finalizeScope();

  const envelopeReceivedAt = typeof record.receivedAt === 'string' ? record.receivedAt : record.observedAt;
  if (record.kind === 'trade_observed') {
    const trade = scopedTrade(record.trade);
    if (trade) study.observeBatch({ observedAt: record.observedAt, receivedAt: envelopeReceivedAt, trades: [trade] });
    continue;
  }

  const topic = record.topic;
  if (typeof topic !== 'string' || !membersByTopic.has(topic)) continue;
  if (['disconnected', 'stopped', 'subscribed', 'rejected'].includes(record.kind)) {
    clearTopic(topic, String(record.kind).toUpperCase(), record.observedAt);
    continue;
  }
  if (record.kind === 'invalidation') {
    const reason = String(record.reason ?? 'UNKNOWN');
    const only = record.exchangeId ? [String(record.exchangeId)] : (membersByTopic.get(topic) ?? []).map(x => x.exchangeId);
    // Every invalidation conservatively ends any live hypothetical quote; unknown
    // reasons are boundaries too, not permission to assume continuity.
    for (const exchangeId of only) study.boundary(exchangeId, reason, record.observedAt);
    continue;
  }
  if (record.kind !== 'event' || record.event !== 'market_batch') {
    if (record.kind === 'event') clearTopic(topic, `UNHANDLED_${String(record.event ?? 'EVENT')}`, record.observedAt);
    continue;
  }

  const previousEpoch = epochs.get(topic);
  if (previousEpoch !== undefined && previousEpoch !== record.epoch) clearTopic(topic, 'EPOCH_CHANGE', record.observedAt);
  epochs.set(topic, record.epoch);
  const payload = record.payload;
  if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
    clearTopic(topic, payload.marketSettled?.length ? 'SETTLEMENT' : 'RESYNC_REQUIRED', record.observedAt);
    continue;
  }
  const delivery = payload.delivery;
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) throw Error('BOOK_CROSSING_REVISION');
  const revisionKey = `${topic}:${record.epoch}`;
  const previousRevision = revisions.get(revisionKey);
  if (previousRevision !== undefined && delivery.previousRevision !== previousRevision) {
    clearTopic(topic, 'DELIVERY_GAP', record.observedAt);
  }
  if (previousRevision !== undefined && delivery.revision <= previousRevision) continue;
  revisions.set(revisionKey, delivery.revision);

  const result = engineBookPointsV1(record, scope, record.observedAt);
  rejectedPoints += result.rejected.length;
  acceptedPoints += result.books.length;
  const trades = (payload.trades ?? []).map(scopedTrade).filter(Boolean);
  study.observeBatch({ observedAt: record.observedAt, receivedAt: envelopeReceivedAt, books: result.books, trades });
}

finalizeScope();
const after = fs.statSync(sourcePath);
if (after.dev !== before.dev || after.ino !== before.ino || after.size < frozen.bytes) throw Error('BOOK_CROSSING_SOURCE_REPLACED');
const actualSha256 = prefixHash.digest('hex');
if (prefixBytesRead !== frozen.bytes || recordIndex !== frozen.captureRecords || actualSha256 !== frozen.sha256
  || firstObservedAt !== frozen.from || through !== frozen.through) throw Error('BOOK_CROSSING_FROZEN_PREFIX_MISMATCH');

const source = {
  file: frozen.file, bytes: frozen.bytes, sha256: actualSha256,
  captureRecords: recordIndex, from: firstObservedAt, through,
};
const result = {
  ...study.report({ splitAt: frozen.splitAt, purgeMs: 60000, source }),
  experiment: {
    runner: path.basename(fileURLToPath(import.meta.url)),
    replayMode: 'offline exact frozen-prefix replay; zero API or account requests',
    sourceManifestSha256: manifestSha256,
    scopeReconstruction: 'initial capture config membership; every later book is rechecked against tournament, market and exchange',
    scopeBreadth,
    acceptedScopedBooks: acceptedPoints,
    rejectedEngineBookPoints: rejectedPoints,
    runtimeMs: performance.now() - started,
  },
};
const fd = fs.openSync(outputPath, 'wx');
try { fs.writeFileSync(fd, JSON.stringify(result) + '\n', 'utf8'); }
finally { fs.closeSync(fd); }
console.log(JSON.stringify({
  output: path.basename(outputPath), source,
  scopeBreadth, counts: result.counts, proxyEvidence: result.proxyEvidence,
  folds: result.folds, runtimeMs: result.experiment.runtimeMs,
}));
