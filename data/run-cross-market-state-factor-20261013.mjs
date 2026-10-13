import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';
import { journalLines } from '../live-paper/journal-lines.mjs';
import { sealedNativePrefix } from '../live-paper/native-dip-host-v1.mjs';
import { engineBookPointsV1 } from '../paper/maker-engine-book-point-v1.mjs';
import {
  aggregateStateClustersV1,
  crossMarketStatePairsV1,
  fitStateFactorV1,
  pairExecutableMarkoutV1,
  stateClusterCoverageV1,
  stateFactorSignalV1,
  synchronizedPairSnapshotV1,
} from '../research/cross-market-state-factor-v1.mjs';

const EXPECTED = Object.freeze({
  tournamentId: 'bda92870-621e-47b0-bc3c-3602c5c26f55',
  captureFile: 'realtime-2026-10-13T02-55-28-953Z-d53812dd-ec75-4852-9267-59c7d5bb7898.jsonl',
  captureBytes: 1_747_459_094,
  captureSha256: 'f9a20ef98e8e9c96d1229dbbc6a8eb17dd669e7c6f3f8d6e49337439945f4418',
  captureRecords: 467_044,
  scopeMarkets: 237,
  from: '2026-10-13T02:55:28.956Z',
  through: '2026-10-13T07:45:51.103Z',
  marketsFile: 'markets.json',
  marketsBytes: 308_497,
  marketsSha256: '41dcb6472ab60302aee6656d3cb75e7baba4b4976671b4419e8334c02189e37c',
  splitAt: '2026-10-13T06:18:44.458Z',
  purgeMs: 5000,
});

const started = performance.now();
const repoRoot = process.cwd();
const here = path.dirname(fileURLToPath(import.meta.url));
const outputPath = path.join(here, 'cross-market-state-factor-20261013.json');
const metaPath = path.join(repoRoot, 'data', EXPECTED.marketsFile);
const analysisModulePath = path.join(repoRoot, 'research', 'cross-market-state-factor-v1.mjs');
const testPath = path.join(repoRoot, 'research', 'cross-market-state-factor-v1.test.mjs');
const runnerPath = fileURLToPath(import.meta.url);
const hashFile = filePath => createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
let priorOlsDiagnostic = null;
let priorSameSourceReplay = null;
let hadPriorOutput = false;
if (fs.existsSync(outputPath)) {
  const previous = JSON.parse(fs.readFileSync(outputPath, 'utf8'));
  if (previous.mode !== 'cross_market_state_factor_relative_value_replay_v1'
    || previous.source?.sha256 !== EXPECTED.captureSha256
    || previous.protocol?.splitAt !== EXPECTED.splitAt) throw new Error('CMSTATE_RESULT_EXISTS');
  hadPriorOutput = true;
  if (previous.protocol?.factor?.includes('OLS hedge beta') && previous.counts?.developmentFitPairs === 0
    && previous.counts?.heldOutSignalCandidates === 0) {
    const failureReasons = Object.create(null);
    for (const pair of previous.pairs ?? []) {
      const reason = pair.model?.reason ?? 'unknown';
      failureReasons[reason] = (failureReasons[reason] ?? 0) + 1;
    }
    priorOlsDiagnostic = {
      mode: previous.mode,
      sourceSha256: previous.source.sha256,
      candidatePairs: previous.universe?.eligibleStatePartyPairs ?? null,
      developmentFitPairs: previous.counts.developmentFitPairs,
      developmentJointFitPoints: previous.counts.developmentJointFitPoints,
      heldOutSignalCandidates: previous.counts.heldOutSignalCandidates,
      failureReasons,
      note: 'Preserved initial OLS-only pass; it had no eligible model or trade marks. It was not used to tune the fixed equal-share comparator.',
    };
  } else if (previous.protocol?.factor?.includes('fixed equal-share beta=1')) {
    priorOlsDiagnostic = previous.priorOlsDiagnostic ?? null;
    priorSameSourceReplay = {
      generatedAt: previous.generatedAt,
      developmentEqualShareScalePairs: previous.counts?.developmentEqualShareScalePairs ?? null,
      heldOutSignalCandidates: previous.counts?.heldOutSignalCandidates ?? null,
      heldOutSummaryByHorizon: previous.heldOutSummaryByHorizon ?? null,
      note: 'Prior exact-source equal-share replay retained as a regeneration diagnostic; not used for model fitting or selection.',
    };
  } else throw new Error('CMSTATE_RESULT_EXISTS');
}
if (!process.env.LOCALAPPDATA) throw new Error('CMSTATE_LOCALAPPDATA');

const baselinePath = path.join(repoRoot, 'data', 'subsecond-microprice-edge-20261013.json');
const baseline = JSON.parse(fs.readFileSync(baselinePath, 'utf8'));
if (baseline.source?.file !== EXPECTED.captureFile || baseline.source?.bytes !== EXPECTED.captureBytes
  || baseline.source?.sha256 !== EXPECTED.captureSha256 || baseline.source?.captureRecords !== EXPECTED.captureRecords
  || baseline.source?.from !== EXPECTED.from || baseline.source?.through !== EXPECTED.through
  || baseline.protocol?.splitAt !== EXPECTED.splitAt || baseline.protocol?.chronologicalPurgeMs !== EXPECTED.purgeMs) {
  throw new Error('CMSTATE_FROZEN_BASELINE');
}

const metaBytes = fs.readFileSync(metaPath);
const metadataSha256 = createHash('sha256').update(metaBytes).digest('hex');
if (metaBytes.length !== EXPECTED.marketsBytes || metadataSha256 !== EXPECTED.marketsSha256) throw new Error('CMSTATE_MARKET_METADATA_HASH');
const metadata = JSON.parse(metaBytes.toString('utf8'));
const marketRows = Array.isArray(metadata) ? metadata : metadata.data;
const universe = crossMarketStatePairsV1(marketRows);
if (universe.totalMarkets !== EXPECTED.scopeMarkets || universe.pairs.length !== 50 || universe.statewideContracts !== 138) throw new Error('CMSTATE_UNIVERSE_EXPECTATION');

const capturePath = path.join(process.env.LOCALAPPDATA, 'SigTradingCup', 'paper-runtime', EXPECTED.captureFile);
const before = fs.statSync(capturePath);
if (!before.isFile() || before.size < EXPECTED.captureBytes) throw new Error('CMSTATE_CAPTURE_SHORT');
const sealed = sealedNativePrefix(capturePath);
if (sealed.bytes < EXPECTED.captureBytes) throw new Error('CMSTATE_CAPTURE_UNSEALED');

const splitMs = Date.parse(EXPECTED.splitAt);
const fitCutoffMs = splitMs - EXPECTED.purgeMs;
const prefix = `tournament:${EXPECTED.tournamentId}:market:`;
const exchangeToMarket = new Map();
const marketToExchange = new Map();
for (const market of marketRows) {
  if (!Array.isArray(market.exchanges) || market.exchanges.length !== 1) throw new Error('CMSTATE_MARKET_EXCHANGE_CARDINALITY');
  const marketId = String(market.id), exchangeId = String(market.exchanges[0]?.id ?? '');
  if (!/^\d+$/.test(marketId) || !/^\d+$/.test(exchangeId) || exchangeToMarket.has(exchangeId)) throw new Error('CMSTATE_METADATA_MAPPING');
  exchangeToMarket.set(exchangeId, marketId);
  marketToExchange.set(marketId, exchangeId);
}
if (exchangeToMarket.size !== EXPECTED.scopeMarkets) throw new Error('CMSTATE_METADATA_SCOPE_SIZE');
const scope = {
  tournamentId: EXPECTED.tournamentId,
  exchanges: [...exchangeToMarket].map(([exchangeId, marketId]) => ({ exchangeId, marketId })),
};
const canonicalScope = scope.exchanges.map(member => [member.marketId, member.exchangeId])
  .sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));
const scopeSha256 = createHash('sha256').update(JSON.stringify({ tournamentId: scope.tournamentId, pairs: canonicalScope })).digest('hex');

const pairStates = new Map(universe.pairs.map(pair => [pair.id, {
  pair,
  segment: 0,
  lastSnapshotKey: null,
  lastSnapshotAt: -Infinity,
  lastFitPoint: null,
  snapshots: [],
  fitPoints: [],
  developmentSnapshots: 0,
  heldOutSnapshots: 0,
  syncRejected: 0,
  freshnessRejected: 0,
  boundaries: Object.create(null),
}]));
const marketToPairs = new Map();
for (const pair of universe.pairs) {
  for (const marketId of [pair.marketA, pair.marketB]) {
    const entries = marketToPairs.get(marketId) ?? [];
    entries.push(pair.id);
    marketToPairs.set(marketId, entries);
  }
}

const lastAccepted = new Map();
const latest = new Map();
const revisions = new Map();
const epochs = new Map();
const captureDigest = createHash('sha256');
const topicMarkets = new Set();
const capturedExchangeToMarket = new Map();
const rejectionReasons = Object.create(null);
const boundaryReasons = Object.create(null);
const counts = {
  captureRecords: 0,
  marketBatches: 0,
  rawBookPoints: 0,
  acceptedPairBooks: 0,
  duplicateSequences: 0,
  sequenceRollbacks: 0,
  sourceReceiptSkews: 0,
  staleSources: 0,
  observationGaps: 0,
  sourceClockRollbacks: 0,
  receiptClockRollbacks: 0,
  revisionGaps: 0,
  resyncOrSettlementBreaks: 0,
  unexpectedOrMalformedPoints: 0,
  outOfSplitPairSnapshots: 0,
};

let firstObservedAt = null, lastObservedAt = null, previousObservedMs = -1, previousReceivedMs = -1;

function countReason(table, reason) {
  const safe = /^[A-Za-z0-9_]+$/.test(reason) ? reason : 'other';
  table[safe] = (table[safe] ?? 0) + 1;
}

function resetMarket(marketId, reason) {
  const ids = marketToPairs.get(marketId) ?? [];
  for (const id of ids) {
    const state = pairStates.get(id);
    state.segment++;
    state.lastSnapshotKey = null;
    state.lastSnapshotAt = -Infinity;
    state.lastFitPoint = null;
    latest.delete(state.pair.marketA);
    latest.delete(state.pair.marketB);
    lastAccepted.delete(state.pair.marketA);
    lastAccepted.delete(state.pair.marketB);
    state.boundaries[reason] = (state.boundaries[reason] ?? 0) + 1;
    countReason(boundaryReasons, reason);
  }
}

function marketForTopic(topic) {
  if (typeof topic !== 'string' || !topic.startsWith(prefix)) return null;
  const marketId = topic.slice(prefix.length);
  return /^\d+$/.test(marketId) ? marketId : null;
}

function addSnapshot(state, a, b) {
  const decisionMs = Math.max(Date.parse(a.receivedAt), Date.parse(b.receivedAt));
  const taggedA = { ...a, segment: state.segment };
  const taggedB = { ...b, segment: state.segment };
  const snapshot = synchronizedPairSnapshotV1(taggedA, taggedB, decisionMs, { maxBookAgeMs: 3000, maxCrossMarketSkewMs: 2000 });
  if (!snapshot) {
    state.syncRejected++;
    return;
  }
  snapshot.segment = state.segment;
  snapshot.key = `${snapshot.seqA}:${snapshot.seqB}:${snapshot.segment}`;
  if (snapshot.key === state.lastSnapshotKey) return;
  if (snapshot.atMs - state.lastSnapshotAt < 1000) return;
  state.lastSnapshotKey = snapshot.key;
  state.lastSnapshotAt = snapshot.atMs;
  state.snapshots.push(snapshot);

  const bothBeforeCutoff = snapshot.atMs < fitCutoffMs && snapshot.receivedAMs < fitCutoffMs
    && snapshot.receivedBMs < fitCutoffMs && snapshot.sourceAMs < fitCutoffMs && snapshot.sourceBMs < fitCutoffMs;
  const bothAfterSplit = snapshot.atMs >= splitMs && snapshot.receivedAMs >= splitMs
    && snapshot.receivedBMs >= splitMs && snapshot.sourceAMs >= splitMs && snapshot.sourceBMs >= splitMs;
  if (bothBeforeCutoff) {
    state.developmentSnapshots++;
    const prior = state.lastFitPoint;
    const bothLegsAdvanced = prior && prior.segment === snapshot.segment
      && prior.seqA !== snapshot.seqA && prior.seqB !== snapshot.seqB
      && snapshot.atMs > prior.atMs && snapshot.atMs - prior.atMs <= 10_000;
    if (!prior || bothLegsAdvanced) {
      state.fitPoints.push({ atMs: snapshot.atMs, aMid: snapshot.aMid, bMid: snapshot.bMid });
      state.lastFitPoint = snapshot;
    }
  } else if (bothAfterSplit) {
    state.heldOutSnapshots++;
  } else if (snapshot.atMs >= fitCutoffMs && snapshot.atMs < splitMs || snapshot.atMs >= splitMs) {
    counts.outOfSplitPairSnapshots++;
  }
}

for (const line of journalLines(capturePath, { endByte: EXPECTED.captureBytes })) {
  captureDigest.update(line + '\n');
  const record = JSON.parse(line);
  if (record.format !== 'sig-feed-capture' || record.captureIndex !== counts.captureRecords) throw new Error('CMSTATE_CAPTURE_CHAIN');
  counts.captureRecords++;
  if (typeof record.observedAt !== 'string') throw new Error('CMSTATE_CAPTURE_CLOCK');
  const observedMs = Date.parse(record.observedAt);
  if (!Number.isFinite(observedMs)) throw new Error('CMSTATE_CAPTURE_CLOCK');
  if (observedMs < previousObservedMs) throw new Error('CMSTATE_OBSERVATION_ROLLBACK');
  previousObservedMs = observedMs;
  firstObservedAt ??= record.observedAt;
  lastObservedAt = record.observedAt;

  const marketId = marketForTopic(record.topic);
  if (!marketId) continue;
  topicMarkets.add(marketId);
  const marketIdInMetadata = marketToExchange.has(marketId);
  if (!marketIdInMetadata) throw new Error('CMSTATE_UNEXPECTED_MARKET_TOPIC');

  const eventBooks = Array.isArray(record.payload?.books) ? record.payload.books : [];
  for (const book of eventBooks) {
    const exchangeId = String(book?.exchangeId ?? '');
    if (!/^\d+$/.test(exchangeId)) throw new Error('CMSTATE_CAPTURE_EXCHANGE_ID');
    const priorMarket = capturedExchangeToMarket.get(exchangeId);
    if (priorMarket !== undefined && priorMarket !== marketId) throw new Error('CMSTATE_CAPTURE_SCOPE_CONFLICT');
    capturedExchangeToMarket.set(exchangeId, marketId);
  }

  const pairIds = marketToPairs.get(marketId) ?? [];
  if (typeof record.receivedAt !== 'string') {
    if (record.kind !== 'config') {
      counts.unexpectedOrMalformedPoints++;
      if (pairIds.length) resetMarket(marketId, 'missing_receipt_clock');
    }
    continue;
  }
  const receivedMs = Date.parse(record.receivedAt);
  if (!Number.isFinite(receivedMs)) {
    counts.unexpectedOrMalformedPoints++;
    if (pairIds.length) resetMarket(marketId, 'invalid_receipt_clock');
    continue;
  }
  if (receivedMs < previousReceivedMs) counts.receiptClockRollbacks++;
  previousReceivedMs = Math.max(previousReceivedMs, receivedMs);
  if (record.kind === 'invalidation' || ['disconnected', 'stopped', 'subscribed', 'rejected'].includes(record.kind)) {
    resetMarket(marketId, `source_${record.kind}`);
    revisions.delete(record.topic);
    epochs.delete(record.topic);
    continue;
  }
  if (record.kind !== 'event' || record.event !== 'market_batch') {
    if (record.kind === 'event' && pairIds.length) resetMarket(marketId, 'unsupported_event');
    continue;
  }
  counts.marketBatches++;
  if (!pairIds.length) continue;

  const payload = record.payload;
  if (payload?.resyncRequired !== false || payload.marketSettled?.length) {
    counts.resyncOrSettlementBreaks++;
    resetMarket(marketId, payload.marketSettled?.length ? 'market_settled' : 'resync_required');
    continue;
  }
  const delivery = payload.delivery;
  if (!Number.isSafeInteger(delivery?.revision) || !Number.isSafeInteger(delivery?.previousRevision)) throw new Error('CMSTATE_DELIVERY_REVISION');
  const previousEpoch = epochs.get(record.topic);
  if (previousEpoch !== undefined && previousEpoch !== record.epoch) resetMarket(marketId, 'epoch_change');
  epochs.set(record.topic, record.epoch);
  const previousRevision = revisions.get(record.topic);
  if (previousRevision !== undefined && delivery.previousRevision !== previousRevision) {
    counts.revisionGaps++;
    resetMarket(marketId, 'delivery_gap');
  }
  if (previousRevision !== undefined && delivery.revision <= previousRevision) continue;
  revisions.set(record.topic, delivery.revision);

  const extracted = engineBookPointsV1(record, scope, record.observedAt);
  counts.rawBookPoints += eventBooks.length;
  for (const rejected of extracted.rejected) {
    countReason(rejectionReasons, rejected.reason ?? 'other');
    if (rejected.marketId && marketToPairs.has(String(rejected.marketId))) resetMarket(String(rejected.marketId), `invalid_${rejected.reason ?? 'book'}`);
  }
  for (const point of extracted.books) {
    const pointMarket = String(point.marketId);
    if (!marketToPairs.has(pointMarket)) continue;
    const pointReceived = Date.parse(point.receivedAt), pointSource = Date.parse(point.sourceAt);
    const seq = Number(point.sequence);
    counts.acceptedPairBooks++;
    if (!Number.isSafeInteger(seq) || !Number.isFinite(pointReceived) || !Number.isFinite(pointSource)) {
      counts.unexpectedOrMalformedPoints++;
      resetMarket(pointMarket, 'malformed_point');
      continue;
    }
    if (pointSource > pointReceived) {
      counts.sourceReceiptSkews++;
      resetMarket(pointMarket, 'source_after_receipt');
      continue;
    }
    if (pointReceived - pointSource > 3000) {
      counts.staleSources++;
      resetMarket(pointMarket, 'source_stale');
      continue;
    }
    const prior = lastAccepted.get(pointMarket);
    if (prior && seq <= prior.seq) {
      if (seq === prior.seq) counts.duplicateSequences++;
      else {
        counts.sequenceRollbacks++;
        resetMarket(pointMarket, 'sequence_rollback');
      }
      continue;
    }
    if (prior && (pointReceived < prior.receivedMs || pointSource < prior.sourceMs)) {
      counts.sourceClockRollbacks++;
      resetMarket(pointMarket, 'book_clock_rollback');
    }
    if (prior && (pointReceived - prior.receivedMs > 10_000 || pointSource - prior.sourceMs > 10_000)) {
      counts.observationGaps++;
      resetMarket(pointMarket, 'observation_gap');
    }
    lastAccepted.set(pointMarket, { seq, receivedMs: pointReceived, sourceMs: pointSource });
    latest.set(pointMarket, point);
    for (const id of pairIds) {
      const state = pairStates.get(id);
      const a = latest.get(state.pair.marketA), b = latest.get(state.pair.marketB);
      if (a && b) addSnapshot(state, a, b);
      else state.freshnessRejected++;
    }
  }
}

const after = fs.statSync(capturePath);
if (after.dev !== before.dev || after.ino !== before.ino || after.size < EXPECTED.captureBytes) throw new Error('CMSTATE_CAPTURE_REPLACED');
if (counts.captureRecords !== EXPECTED.captureRecords || firstObservedAt !== EXPECTED.from || lastObservedAt !== EXPECTED.through) {
  throw new Error('CMSTATE_CAPTURE_EXPECTATION');
}
const captureSha256 = captureDigest.digest('hex');
if (captureSha256 !== EXPECTED.captureSha256) throw new Error('CMSTATE_CAPTURE_HASH');
if (topicMarkets.size !== EXPECTED.scopeMarkets || capturedExchangeToMarket.size !== EXPECTED.scopeMarkets) throw new Error('CMSTATE_CAPTURE_SCOPE_SIZE');
for (const [exchangeId, marketId] of exchangeToMarket) {
  if (capturedExchangeToMarket.get(exchangeId) !== marketId) throw new Error('CMSTATE_CAPTURE_METADATA_MISMATCH');
}

const horizonMs = [1000, 5000, 30_000];
const responseLatencyMs = 3000;
const entryOverrunMs = 15_000;
const markOverrunMs = 15_000;
const thresholdZ = 2;
const cooldownMs = 30_000;
const allPairResults = [];
const aggregate = new Map(horizonMs.map(h => [h, { candidates: 0, marks: 0, profitable: 0, grossPnl: 0, stressedPnl: 0, entryCost: 0, pairs: new Map() }]));

function dueFor(snapshot, targetMs) {
  return snapshot.atMs >= targetMs && snapshot.receivedAMs >= targetMs && snapshot.receivedBMs >= targetMs
    && snapshot.sourceAMs >= targetMs && snapshot.sourceBMs >= targetMs;
}

function locate(snapshots, startIndex, targetMs, maxOverrunMs, segment) {
  const deadline = targetMs + maxOverrunMs;
  for (let i = startIndex + 1; i < snapshots.length; i++) {
    const snapshot = snapshots[i];
    if (snapshot.segment !== segment) return { found: false, reason: 'continuity_boundary' };
    if (snapshot.atMs > deadline) return { found: false, reason: 'clock_overrun' };
    if (dueFor(snapshot, targetMs)) return { found: true, index: i, snapshot };
  }
  return { found: false, reason: 'capture_end' };
}

for (const state of pairStates.values()) {
  const developmentOlsFit = fitStateFactorV1(state.fitPoints, { minObservations: 100 });
  const model = fitStateFactorV1(state.fitPoints, { minObservations: 100, betaOverride: 1 });
  const holdout = state.snapshots.filter(snapshot => snapshot.atMs >= splitMs
    && snapshot.receivedAMs >= splitMs && snapshot.receivedBMs >= splitMs
    && snapshot.sourceAMs >= splitMs && snapshot.sourceBMs >= splitMs);
  const pairResult = {
    id: state.pair.id,
    state: state.pair.state,
    party: state.pair.party,
    marketA: state.pair.marketA,
    exchangeA: state.pair.exchangeA,
    titleA: state.pair.titleA,
    marketB: state.pair.marketB,
    exchangeB: state.pair.exchangeB,
    titleB: state.pair.titleB,
    developmentSynchronizedSnapshots: state.developmentSnapshots,
    developmentJointFitPoints: state.fitPoints.length,
    heldOutSynchronizedSnapshots: state.heldOutSnapshots,
    developmentOlsFit,
    model,
    signal: { thresholdZ, candidates: 0, entries: 0, depthRejected: 0, entryCensored: Object.create(null), marks: Object.create(null) },
    heldOutResidualDiagnostics: null,
    limitations: [],
  };
  for (const horizon of horizonMs) pairResult.signal.marks[String(horizon)] = { candidates: 0, marks: 0, profitable: 0, grossPnl: 0, stressedPnl: 0, entryCost: 0, censored: Object.create(null) };
  if (!model.usable) {
    pairResult.limitations.push(`development_fit_${model.reason}`);
    allPairResults.push(pairResult);
    continue;
  }

  const absZ = [];
  let rawThresholdExceedances = 0;
  let adjacentJointSequenceUpdates = 0;
  let jointThresholdExceedances = 0;
  let priorDiagnosticSnapshot = null;
  const residualValues = [];
  for (const snapshot of holdout) {
    const score = stateFactorSignalV1(model, snapshot.aMid, snapshot.bMid, thresholdZ);
    const magnitude = Math.abs(score.zScore);
    absZ.push(magnitude);
    residualValues.push(score.residual);
    if (score.direction) rawThresholdExceedances++;
    const jointUpdate = priorDiagnosticSnapshot
      && priorDiagnosticSnapshot.seqA !== snapshot.seqA && priorDiagnosticSnapshot.seqB !== snapshot.seqB;
    if (jointUpdate) {
      adjacentJointSequenceUpdates++;
      if (score.direction) jointThresholdExceedances++;
    }
    priorDiagnosticSnapshot = snapshot;
  }
  absZ.sort((a, b) => a - b);
  const residualMin = residualValues.length ? Math.min(...residualValues) : null;
  const residualMax = residualValues.length ? Math.max(...residualValues) : null;
  pairResult.heldOutResidualDiagnostics = {
    snapshots: holdout.length,
    distinctAMids: new Set(holdout.map(snapshot => snapshot.aMid)).size,
    distinctBMids: new Set(holdout.map(snapshot => snapshot.bMid)).size,
    adjacentJointSequenceUpdates,
    rawThresholdExceedanceSnapshots: rawThresholdExceedances,
    adjacentJointThresholdExceedanceSnapshots: jointThresholdExceedances,
    maxAbsZ: absZ.length ? Number(absZ.at(-1).toFixed(6)) : null,
    p95AbsZ: absZ.length ? Number(absZ[Math.floor((absZ.length - 1) * 0.95)].toFixed(6)) : null,
    p99AbsZ: absZ.length ? Number(absZ[Math.floor((absZ.length - 1) * 0.99)].toFixed(6)) : null,
    residualRange: residualValues.length ? [Number(residualMin.toFixed(8)), Number(residualMax.toFixed(8))] : null,
  };

  let priorSignalDirection = 0;
  let lastSignalAt = -Infinity;
  let lastJointSignalSeqA = null, lastJointSignalSeqB = null;
  let busyUntil = -Infinity;
  let blockedSegment = null;
  const trades = [];
  for (let i = 0; i < holdout.length; i++) {
    const signalSnapshot = holdout[i];
    if (blockedSegment !== null && blockedSegment !== signalSnapshot.segment) blockedSegment = null;
    if (signalSnapshot.segment !== (holdout[i - 1]?.segment ?? signalSnapshot.segment)) {
      priorSignalDirection = 0;
      lastJointSignalSeqA = null;
      lastJointSignalSeqB = null;
      busyUntil = -Infinity;
      blockedSegment = null;
    }
    const jointUpdate = lastJointSignalSeqA !== signalSnapshot.seqA && lastJointSignalSeqB !== signalSnapshot.seqB;
    if (!jointUpdate) continue;
    lastJointSignalSeqA = signalSnapshot.seqA;
    lastJointSignalSeqB = signalSnapshot.seqB;
    const classified = stateFactorSignalV1(model, signalSnapshot.aMid, signalSnapshot.bMid, thresholdZ);
    if (!classified.direction) {
      priorSignalDirection = 0;
      continue;
    }
    if (classified.direction === priorSignalDirection) continue;
    priorSignalDirection = classified.direction;
    if (signalSnapshot.atMs < busyUntil || blockedSegment === signalSnapshot.segment || signalSnapshot.atMs - lastSignalAt < cooldownMs) continue;
    lastSignalAt = signalSnapshot.atMs;
    pairResult.signal.candidates++;
    for (const horizon of horizonMs) pairResult.signal.marks[String(horizon)].candidates++;

    const targetEntryMs = signalSnapshot.atMs + responseLatencyMs;
    const entry = locate(holdout, i, targetEntryMs, entryOverrunMs, signalSnapshot.segment);
    if (!entry.found) {
      const reason = entry.reason;
      pairResult.signal.entryCensored[reason] = (pairResult.signal.entryCensored[reason] ?? 0) + 1;
      for (const horizon of horizonMs) {
        const cell = pairResult.signal.marks[String(horizon)];
        cell.censored[`entry_${reason}`] = (cell.censored[`entry_${reason}`] ?? 0) + 1;
      }
      if (reason === 'continuity_boundary' || reason === 'capture_end') blockedSegment = signalSnapshot.segment;
      continue;
    }
    const entryMark = pairExecutableMarkoutV1(classified.direction, model.beta, entry.snapshot, entry.snapshot);
    if (!entryMark.eligible) {
      pairResult.signal.depthRejected++;
      const reason = entryMark.reason;
      pairResult.signal.entryCensored[reason] = (pairResult.signal.entryCensored[reason] ?? 0) + 1;
      continue;
    }
    pairResult.signal.entries++;

    const trade = {
      decisionAt: new Date(signalSnapshot.atMs).toISOString(),
      entryAt: new Date(entry.snapshot.atMs).toISOString(),
      segment: signalSnapshot.segment,
      direction: classified.direction,
      zScore: Number(classified.zScore.toFixed(6)),
      residual: Number(classified.residual.toFixed(8)),
      beta: Number(model.beta.toFixed(6)),
      marks: Object.create(null),
    };
    let fiveSecondExitAt = null;
    for (const horizon of horizonMs) {
      const cell = pairResult.signal.marks[String(horizon)];
      const targetMarkMs = entry.snapshot.atMs + horizon;
      const exit = locate(holdout, entry.index, targetMarkMs, markOverrunMs, signalSnapshot.segment);
      if (!exit.found) {
        cell.censored[exit.reason] = (cell.censored[exit.reason] ?? 0) + 1;
        trade.marks[String(horizon)] = { status: exit.reason };
        continue;
      }
      const mark = pairExecutableMarkoutV1(classified.direction, model.beta, entry.snapshot, exit.snapshot);
      if (!mark.eligible) {
        cell.censored[mark.reason] = (cell.censored[mark.reason] ?? 0) + 1;
        trade.marks[String(horizon)] = { status: mark.reason };
        continue;
      }
      if (horizon === 5000) fiveSecondExitAt = exit.snapshot.atMs;
      cell.marks++;
      cell.grossPnl += mark.grossPnl;
      cell.stressedPnl += mark.oneTickEachFillStressPnl;
      cell.entryCost += mark.entryCost;
      if (mark.grossPnl > 0) cell.profitable++;
      const totalCell = aggregate.get(horizon);
      totalCell.marks++;
      totalCell.grossPnl += mark.grossPnl;
      totalCell.stressedPnl += mark.oneTickEachFillStressPnl;
      totalCell.entryCost += mark.entryCost;
      if (mark.grossPnl > 0) totalCell.profitable++;
      const cluster = totalCell.pairs.get(state.pair.id) ?? { state: state.pair.state, marks: 0, grossPnl: 0, stressedPnl: 0, profitable: 0 };
      cluster.marks++;
      cluster.grossPnl += mark.grossPnl;
      cluster.stressedPnl += mark.oneTickEachFillStressPnl;
      if (mark.grossPnl > 0) cluster.profitable++;
      totalCell.pairs.set(state.pair.id, cluster);
      trade.marks[String(horizon)] = {
        status: 'marked',
        exitAt: new Date(exit.snapshot.atMs).toISOString(),
        entryCost: Number(mark.entryCost.toFixed(8)),
        exitValue: Number(mark.exitValue.toFixed(8)),
        grossPnl: Number(mark.grossPnl.toFixed(8)),
        oneTickEachFillStressPnl: Number(mark.oneTickEachFillStressPnl.toFixed(8)),
      };
    }
    trades.push(trade);
    if (fiveSecondExitAt === null) {
      blockedSegment = signalSnapshot.segment;
    } else {
      busyUntil = fiveSecondExitAt;
    }
  }
  pairResult.signal.trades = trades;
  for (const horizon of horizonMs) {
    const cell = pairResult.signal.marks[String(horizon)];
    cell.meanGrossPnl = cell.marks ? Number((cell.grossPnl / cell.marks).toFixed(8)) : null;
    cell.meanOneTickEachFillStressPnl = cell.marks ? Number((cell.stressedPnl / cell.marks).toFixed(8)) : null;
    cell.meanEntryCost = cell.marks ? Number((cell.entryCost / cell.marks).toFixed(8)) : null;
  }
  allPairResults.push(pairResult);
}

const stateCoverage = stateClusterCoverageV1(universe.pairs, allPairResults);
if (!stateCoverage.matches || stateCoverage.candidatePairs !== universe.pairs.length
  || stateCoverage.candidateStateClusters !== new Set(universe.pairs.map(pair => pair.state)).size) {
  throw new Error('CMSTATE_REPORT_CLUSTER_COVERAGE');
}

const summaryByHorizon = {};
for (const [horizon, cell] of aggregate) {
  const clusters = [...cell.pairs.values()];
  const stateClusters = aggregateStateClustersV1(clusters);
  const stateMarks = stateClusters.reduce((sum, row) => sum + row.marks, 0);
  const stateGrossPnl = stateClusters.reduce((sum, row) => sum + row.grossPnl, 0);
  const stateStressedPnl = stateClusters.reduce((sum, row) => sum + row.stressedPnl, 0);
  if (stateMarks !== cell.marks || Math.abs(stateGrossPnl - cell.grossPnl) > 1e-10
    || Math.abs(stateStressedPnl - cell.stressedPnl) > 1e-10) throw new Error('CMSTATE_REPORT_STATE_AGGREGATION');
  summaryByHorizon[String(horizon)] = {
    candidateSignals: allPairResults.reduce((sum, pair) => sum + (pair.signal.candidates ?? 0), 0),
    eligibleStateClusters: stateCoverage.candidateStateClusters,
    completedTopOfBookMarkouts: cell.marks,
    positiveMarkouts: cell.profitable,
    meanGrossPairPnl: cell.marks ? Number((cell.grossPnl / cell.marks).toFixed(8)) : null,
    meanOneTickEachFillStressPnl: cell.marks ? Number((cell.stressedPnl / cell.marks).toFixed(8)) : null,
    meanInitialPairCost: cell.marks ? Number((cell.entryCost / cell.marks).toFixed(8)) : null,
    activeStatePartyPairClusters: clusters.length,
    activeStateClusters: stateClusters.filter(row => row.marks > 0).length,
    positivePairMeanClusters: clusters.filter(row => row.grossPnl / row.marks > 0).length,
    negativePairMeanClusters: clusters.filter(row => row.grossPnl / row.marks < 0).length,
    byState: stateClusters.map(row => ({
      state: row.state,
      partyPairClusters: row.pairClusters,
      marks: row.marks,
      positiveMarkouts: row.profitable,
      meanGrossPairPnl: row.marks ? Number((row.grossPnl / row.marks).toFixed(8)) : null,
      meanOneTickEachFillStressPnl: row.marks ? Number((row.stressedPnl / row.marks).toFixed(8)) : null,
    })),
    byStateParty: [...cell.pairs.entries()].map(([id, row]) => ({
      id,
      marks: row.marks,
      positiveMarkouts: row.profitable,
      meanGrossPairPnl: Number((row.grossPnl / row.marks).toFixed(8)),
      meanOneTickEachFillStressPnl: Number((row.stressedPnl / row.marks).toFixed(8)),
    })).sort((a, b) => a.id.localeCompare(b.id)),
  };
}

const fittedResidualPairs = allPairResults.filter(pair => pair.heldOutResidualDiagnostics);
const highestHeldOutZ = fittedResidualPairs
  .map(pair => ({ id: pair.id, maxAbsZ: pair.heldOutResidualDiagnostics.maxAbsZ }))
  .filter(pair => Number.isFinite(pair.maxAbsZ))
  .sort((a, b) => b.maxAbsZ - a.maxAbsZ);
const heldOutResidualSummary = {
  pairsWithFit: fittedResidualPairs.length,
  pairSnapshots: fittedResidualPairs.reduce((sum, pair) => sum + pair.heldOutResidualDiagnostics.snapshots, 0),
  rawThresholdExceedanceSnapshots: fittedResidualPairs.reduce((sum, pair) => sum + pair.heldOutResidualDiagnostics.rawThresholdExceedanceSnapshots, 0),
  pairsWithAnyThresholdExceedance: fittedResidualPairs.filter(pair => pair.heldOutResidualDiagnostics.rawThresholdExceedanceSnapshots > 0).length,
  maximumAbsoluteZ: highestHeldOutZ[0] ?? null,
  interpretation: 'The fixed |z|>=2 rule produced no signal if the threshold-exceedance count is zero; no trade markout is defined for an empty strategy.',
};

const afterReplay = fs.statSync(capturePath);
if (afterReplay.dev !== before.dev || afterReplay.ino !== before.ino || afterReplay.size < EXPECTED.captureBytes) throw new Error('CMSTATE_CAPTURE_CHANGED_DURING_REPLAY');

const output = {
  schemaVersion: 1,
  mode: 'cross_market_state_factor_relative_value_replay_v1',
  source: {
    kind: 'existing_sealed_local_realtime_capture_prefix',
    file: EXPECTED.captureFile,
    bytes: EXPECTED.captureBytes,
    sha256: captureSha256,
    captureRecords: counts.captureRecords,
    from: firstObservedAt,
    through: lastObservedAt,
    tournamentId: EXPECTED.tournamentId,
    scopeMarkets: topicMarkets.size,
    scopeSha256,
    metadataFile: `data/${EXPECTED.marketsFile}`,
    metadataBytes: metaBytes.length,
    metadataSha256,
  },
  protocol: {
    splitAt: EXPECTED.splitAt,
    purgeMs: EXPECTED.purgeMs,
    developmentLabelsMustBeBefore: new Date(fitCutoffMs).toISOString(),
    holdout: 'chronological later segment; all four underlying source/receipt clocks must be on or after the split; no holdout fitting or threshold search',
    eligiblePairs: 'strict same-state, same-party statewide Senate and Governor contracts from frozen market metadata; no candidate/party substitution',
    factor: 'fixed equal-share beta=1 across every state/party pair; only the residual center and scale are fitted from development-only synchronized snapshots; development OLS hedge fits are recorded as diagnostics and never select or size trades',
    signal: `fixed residual threshold |z| >= ${thresholdZ}; long the cheap YES and buy one share of the rich contract NO; only threshold transitions are candidates`,
    responseLatencyMs,
    freshness: { maxBookSourceAgeMs: 3000, maxLocalReceiptAgeMs: 3000, maxCrossMarketClockSkewMs: 2000, maxObservationGapMs: 10000 },
    horizonMs,
    primaryHoldMs: 5000,
    entryAndExit: 'both legs priced at their post-latency top-of-book sides; one displayed share must be available on each required level; equal one-share sizing; zero fees; no actual order or fill inference',
    adverseStress: 'one tick adverse per leg on entry and exit; no tuning or selection on held-out results',
    signalOverlap: 'one pair position at a time; block new signals until the 5s close mark, and censor the rest of a continuity segment if that close cannot be established',
  },
  universe: {
    totalMetadataMarkets: universe.totalMarkets,
    statewidePartyContracts: universe.statewideContracts,
    eligibleStatePartyPairs: universe.pairs.length,
    eligibleStates: new Set(universe.pairs.map(pair => pair.state)).size,
    candidatePairIds: universe.pairs.map(pair => pair.id),
    excluded: 'House and other nonstatewide contracts, state/party groups missing either office, and any contract not matching exact statewide title grammar',
  },
  coverage: {
    ...stateCoverage,
    pairRowsReconciled: stateCoverage.matches,
  },
  counts: {
    ...counts,
    topicMarkets: topicMarkets.size,
    capturedExchangeMappings: capturedExchangeToMarket.size,
    pairSnapshots: allPairResults.reduce((sum, pair) => sum + pair.developmentSynchronizedSnapshots + pair.heldOutSynchronizedSnapshots, 0),
    developmentJointFitPoints: allPairResults.reduce((sum, pair) => sum + pair.developmentJointFitPoints, 0),
    developmentOlsUsablePairs: allPairResults.filter(pair => pair.developmentOlsFit.usable).length,
    developmentEqualShareScalePairs: allPairResults.filter(pair => pair.model.usable).length,
    heldOutSignalCandidates: allPairResults.reduce((sum, pair) => sum + (pair.signal.candidates ?? 0), 0),
    heldOutTwoLegEntryDepthProxies: allPairResults.reduce((sum, pair) => sum + (pair.signal.entries ?? 0), 0),
    heldOutEntryDepthRejects: allPairResults.reduce((sum, pair) => sum + (pair.signal.depthRejected ?? 0), 0),
    boundaryReasons,
    pointRejectionReasons: rejectionReasons,
  },
  heldOutResidualSummary,
  heldOutSummaryByHorizon: summaryByHorizon,
  priorOlsDiagnostic,
  priorSameSourceReplay,
  pairs: allPairResults,
  analysisSourceSha256: {
    module: hashFile(analysisModulePath),
    runner: hashFile(runnerPath),
    tests: hashFile(testPath),
  },
  limitations: [
    'A same-state party YES contract in two different offices is a correlation hedge, not an arbitrage identity or guaranteed payout relation.',
    'The continuous regression hedge ratio may not be an accepted live order quantity; reported paired P&L is a normalized counterfactual quote markout, not account return.',
    'Both legs must be independently marketable; no actual fills, atomic two-market execution, queue position, partial-fill hedging, or synchronized order acceptance are proven.',
    `There are ${stateCoverage.candidateStateClusters} state-level clusters, grouping party-specific pairs by state, and one captured session. Per-pair and aggregate markouts are diagnostic, serially dependent, and do not establish persistent alpha or profitability; state outcomes can also share nationwide shocks.`,
    'Only top-of-book displayed quantity is used; deeper capacity, portfolio funding, market impact, settlement exceptions, and competition orders are not modeled.',
  ],
  actualFillsProven: false,
  profitabilityEstablished: false,
  contestOrders: 0,
  accountApiRequests: 0,
  replayRuntimeMs: Number((performance.now() - started).toFixed(3)),
  generatedAt: new Date().toISOString(),
};

fs.writeFileSync(outputPath, `${JSON.stringify(output, null, 2)}\n`, { flag: hadPriorOutput ? 'w' : 'wx' });
console.log(JSON.stringify({
  report: path.relative(repoRoot, outputPath),
  sourceSha256: captureSha256,
  candidatePairs: universe.pairs.length,
  candidateStateClusters: stateCoverage.candidateStateClusters,
  analyzedStateClusters: stateCoverage.analyzedStateClusters,
  pairRowsReconciled: stateCoverage.matches,
  olsFitPairs: output.counts.developmentOlsUsablePairs,
  equalShareScalePairs: output.counts.developmentEqualShareScalePairs,
  heldOutSignals: output.counts.heldOutSignalCandidates,
  markouts: summaryByHorizon,
  runtimeMs: output.replayRuntimeMs,
}, null, 2));
