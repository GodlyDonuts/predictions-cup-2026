// Multi-capture fixed-bank builder for the published market-edge ridge protocol.
// Reads only three explicitly pinned training sources. It has no network or order path.
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import readline from 'node:readline';
import {instantNs} from '../feed/contract.mjs';
import {engineBookPointsV1} from '../paper/maker-engine-book-point-v1.mjs';
import {
  MARKET_EDGE_RIDGE_PROTOCOL,
  captureLifecycleCanBeAppliedV1,
  candidatesForSeriesV1,
  fitMarketEdgeRidgeV1,
  marketBatchUsableAfterRecoveryV1,
  scoreMarketEdgeRidgeV1,
  segmentSeriesGapsV1,
} from './market-edge-ridge-v1.mjs';

export const MARKET_EDGE_RIDGE_BANK_PROTOCOL_V1 = Object.freeze({
  alpha: 0.01, decisionLatencyMs: 3000, holdingHorizonMs: 5000,
  feesPerShare: 0, selectionThresholdPerShare: 0,
  trainingSourceIds: Object.freeze(['train-d53812dd', 'train-0942-14ef0be1', 'train-1328-a7674ae9']),
  futureTestStatus: 'pending_separately_frozen_future_capture',
});
const EXPECTED_TOURNAMENT = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const EXPECTED_MARKETS = 237;

export const MARKET_EDGE_RIDGE_BANK_SOURCES_V1 = Object.freeze([
  Object.freeze({
    id: 'train-d53812dd', role: 'training_only',
    file: 'realtime-2026-10-13T02-55-28-953Z-d53812dd-ec75-4852-9267-59c7d5bb7898.jsonl',
    bytes: 1747459094,
    sha256: 'f9a20ef98e8e9c96d1229dbbc6a8eb17dd669e7c6f3f8d6e49337439945f4418',
    backingBytes: 1747507916,
    backingSha256: 'd321ad760750e0ba6cdbc85afade4fdd7c1874b74aa080df9881810942cfca6e',
  }),
  Object.freeze({
    id: 'train-0942-14ef0be1', role: 'training_only',
    file: 'realtime-2026-10-13T09-42-07-994Z-14ef0be1-8e8b-43a2-87ca-66e30c9cde01.jsonl',
    bytes: 819582161,
    sha256: '6129d28da84e08934b7f0ba18c87635b2592491257f6404bc06635de3670ed3c',
    backingBytes: 819582161,
    backingSha256: '6129d28da84e08934b7f0ba18c87635b2592491257f6404bc06635de3670ed3c',
  }),
  Object.freeze({
    id: 'train-1328-a7674ae9', role: 'training_only',
    file: 'realtime-2026-10-13T13-28-06-669Z-a7674ae9-ed0e-41c6-90c8-390b1ce0dc8b.jsonl',
    bytes: 364603159,
    sha256: '83dd7cc069ce221617425c533a6d1938c3056fcac20254c4938cddb7d4c96f61',
    backingBytes: 364603159,
    backingSha256: '83dd7cc069ce221617425c533a6d1938c3056fcac20254c4938cddb7d4c96f61',
  }),
]);

function invariant(ok, code) { if (!ok) throw Error(code); }
function fileIdentity(stat) { return {dev: String(stat.dev), ino: String(stat.ino), size: stat.size, mtimeMs: stat.mtimeMs}; }
function sameIdentity(a, b) { return a.dev === b.dev && a.ino === b.ino && a.size === b.size && a.mtimeMs === b.mtimeMs; }

function validEpochV1(epoch) { return Number.isSafeInteger(epoch) && epoch >= 0; }

export function subscribedStateUpdateV1(eventEpoch) {
  if (!validEpochV1(eventEpoch)) return {status: 'malformed_epoch', state: {epoch: null, ready: false}};
  return {status: 'applied', state: {epoch: eventEpoch, ready: false}};
}

export function recoveryStartedStateUpdateV1(prior, eventEpoch) {
  if (!prior) return {status: 'stale_or_unmatched', state: prior ?? null};
  if (!validEpochV1(prior.epoch) || !validEpochV1(eventEpoch)) return {status: 'malformed_epoch', state: prior};
  if (prior.epoch !== eventEpoch) return {status: 'stale_or_unmatched', state: prior};
  return {status: 'applied', state: {...prior, ready: false}};
}

export function recoveryFinishedStateUpdateV1(prior, eventEpoch, booksReady) {
  if (!prior) return {status: 'stale_or_unmatched', state: null};
  if (!validEpochV1(prior.epoch) || !validEpochV1(eventEpoch)) return {status: 'malformed_epoch', state: prior};
  if (prior.epoch !== eventEpoch) return {status: 'stale_or_unmatched', state: prior};
  return {status: 'applied', state: {...prior, ready: booksReady === true}};
}

export function marketBatchEpochMatchesStateV1(stateEpoch, eventEpoch) {
  return validEpochV1(stateEpoch) && validEpochV1(eventEpoch) && stateEpoch === eventEpoch;
}

export function marketBatchAdmissibleV1({ready, invalidated, stateEpoch, eventEpoch}) {
  return marketBatchEpochMatchesStateV1(stateEpoch, eventEpoch)
    && marketBatchUsableAfterRecoveryV1({ready, invalidated});
}

function hasFinalNewline(file, offset) {
  const fd = fs.openSync(file, 'r'), byte = Buffer.alloc(1);
  try { return fs.readSync(fd, byte, 0, 1, offset) === 1 && byte[0] === 0x0a; }
  finally { fs.closeSync(fd); }
}

async function hashFileAndPrefix(file, prefixBytes) {
  const fullHash = createHash('sha256'), prefixHash = createHash('sha256');
  let bytes = 0, prefixHashed = 0;
  for await (const chunk of createReadStream(file, {highWaterMark: 4 * 1024 * 1024})) {
    fullHash.update(chunk);
    const take = Math.min(chunk.length, Math.max(0, prefixBytes - prefixHashed));
    if (take) { prefixHash.update(chunk.subarray(0, take)); prefixHashed += take; }
    bytes += chunk.length;
  }
  invariant(prefixHashed === prefixBytes, 'RIDGE_BANK_SOURCE_PREFIX_TRUNCATED');
  return {bytes, backingSha256: fullHash.digest('hex'), sha256: prefixHash.digest('hex')};
}

// Verify all frozen inputs before any capture row is parsed. For d538, only
// the pinned prefix is passed to the parser; the appended backing tail is hashed
// solely to bind the source file and is never parsed.
export async function verifyCaptureSourceV1(spec, file) {
  const before = fileIdentity(fs.statSync(file));
  invariant(before.size === spec.backingBytes && before.size >= spec.bytes, 'RIDGE_BANK_SOURCE_SIZE');
  invariant(hasFinalNewline(file, spec.bytes - 1), 'RIDGE_BANK_SOURCE_PREFIX_NOT_LINE_ALIGNED');
  const hashes = await hashFileAndPrefix(file, spec.bytes);
  const after = fileIdentity(fs.statSync(file));
  invariant(sameIdentity(before, after), 'RIDGE_BANK_SOURCE_CHANGED_DURING_PIN');
  invariant(hashes.bytes === spec.backingBytes && hashes.backingSha256 === spec.backingSha256, 'RIDGE_BANK_BACKING_HASH');
  invariant(hashes.sha256 === spec.sha256, 'RIDGE_BANK_PREFIX_HASH');
  return {file, identity: before, frozenBytes: spec.bytes, excludedTailBytes: spec.backingBytes - spec.bytes};
}

export async function verifyCaptureSourceAfterV1(spec, pinned) {
  invariant(sameIdentity(pinned.identity, fileIdentity(fs.statSync(pinned.file))), 'RIDGE_BANK_SOURCE_IDENTITY_CHANGED');
  invariant(hasFinalNewline(pinned.file, spec.bytes - 1), 'RIDGE_BANK_SOURCE_NEWLINE_CHANGED');
  const hashes = await hashFileAndPrefix(pinned.file, spec.bytes);
  invariant(sameIdentity(pinned.identity, fileIdentity(fs.statSync(pinned.file))), 'RIDGE_BANK_SOURCE_CHANGED_DURING_POSTHASH');
  invariant(hashes.bytes === spec.backingBytes && hashes.backingSha256 === spec.backingSha256, 'RIDGE_BANK_BACKING_POSTHASH');
  invariant(hashes.sha256 === spec.sha256, 'RIDGE_BANK_PREFIX_POSTHASH');
  return {
    id: spec.id, file: spec.file, frozenBytes: spec.bytes, frozenSha256: spec.sha256,
    backingBytes: spec.backingBytes, backingSha256: spec.backingSha256,
    excludedTailBytes: spec.backingBytes - spec.bytes, identityStable: true,
  };
}

function stableJson(value) {
  if (Array.isArray(value)) return '[' + value.map(stableJson).join(',') + ']';
  if (value && typeof value === 'object') {
    return '{' + Object.keys(value).sort().map(k => JSON.stringify(k) + ':' + stableJson(value[k])).join(',') + '}';
  }
  return JSON.stringify(value);
}
function sha256Json(value) { return createHash('sha256').update(stableJson(value)).digest('hex'); }

function labeledRows(candidates, captureId) {
  invariant(Array.isArray(candidates), 'RIDGE_BANK_CANDIDATES');
  for (const row of candidates) {
    invariant(row.decisionProvenance?.captureId === captureId, 'RIDGE_BANK_DECISION_CAPTURE');
    if (Number.isFinite(row.label?.net)) {
      const p = row.label;
      invariant(p.decisionProvenance?.captureId === captureId
        && p.entryProvenance?.captureId === captureId
        && p.exitProvenance?.captureId === captureId, 'RIDGE_BANK_CROSS_CAPTURE_LABEL');
      invariant(p.decisionSegment === p.entrySegment && p.entrySegment === p.exitSegment,
        'RIDGE_BANK_CROSS_SEGMENT_LABEL');
    }
  }
  return candidates.filter(row => Number.isFinite(row.label?.net));
}
function minDecisionMs(candidates) {
  return candidates.reduce((best, x) => Math.min(best, x.decisionMs), Infinity);
}
function maxExitMs(candidates) {
  return candidates.reduce((best, x) => Number.isSafeInteger(x.label?.exitObservedMs)
    ? Math.max(best, x.label.exitObservedMs) : best, -Infinity);
}
function captureSummary(capture) {
  const labels = labeledRows(capture.candidates, capture.capture.id);
  return {
    id: capture.capture.id, role: 'training_only', file: capture.capture.file,
    frozenBytes: capture.capture.bytes, frozenSha256: capture.capture.sha256,
    captureRows: capture.capture.captureRows, configRows: capture.capture.configRows,
    markets: capture.capture.markets, exchanges: capture.capture.exchanges,
    from: capture.capture.from, through: capture.capture.through,
    diagnostics: capture.diagnostics,
    readiness: {
      acceptedBookPoints: capture.diagnostics.acceptedBookPoints,
      validRecoveryCompletions: capture.diagnostics.validRecoveryCompletions,
      booksBeforeRecovery: capture.diagnostics.booksBeforeRecovery,
      lifecycleInvalidations: capture.diagnostics.lifecycleInvalidations,
      revisionGaps: capture.diagnostics.revisionGaps,
      explicitBreakEvents: capture.diagnostics.explicitBreakEvents,
      candidateCount: capture.candidates.length, labeledCount: labels.length,
      censoredCount: capture.candidates.length - labels.length,
      sameCaptureAndSegmentLabelCount: labels.length,
    },
    labelProvenance: capture.labelProvenance,
  };
}
function aggregateSummary(x) {
  return {
    n: x.n, meanPerShare: x.meanPerShare, medianPerShare: x.medianPerShare,
    positiveFraction: x.positiveFraction, oneTickPerLegMeanPerShare: x.oneTickPerLegMeanPerShare,
    oneTickPerLegPositiveFraction: x.oneTickPerLegPositiveFraction, marketClusters: x.marketClusters,
    equalWeightedMarketMean: x.equalWeightedMarketMean, marketClusterStandardError: x.marketClusterStandardError,
    positiveMarketClusterCount: x.positiveMarketClusterCount,
  };
}
function compactScore(score) {
  return {
    candidatePredictions: score.candidatePredictions, labeledCandidates: score.labeledCandidates,
    positivePredictions: score.predictions.positive, selectedCandidateCount: score.selectedCandidateCount,
    selectedWithOutcomeCount: score.selectedWithOutcomeCount, selectedCensoredOutcomeCount: score.selectedCensoredOutcomeCount,
    selected: aggregateSummary(score.selected),
    unconditionalAllLabeledCandidateControl: aggregateSummary(score.unconditionalAllLabeledCandidateControl),
    bySide: Object.fromEntries(Object.entries(score.bySide).map(([side, x]) => [side, {
      candidateCount: x.candidateCount, selectedCandidateCount: x.selectedCandidateCount,
      selectedCensoredOutcomeCount: x.selectedCensoredOutcomeCount,
      labeled: aggregateSummary(x.labeled), selected: aggregateSummary(x.selected),
    }])),
    predictionRmse: score.predictionRmse, trainingMeanControlRmse: score.trainingMeanControlRmse,
    actualFillsProven: false, realizedAccountPnl: null, profitabilityEstablished: false,
  };
}
function serializedModel(model) {
  return {
    featureNames: [...model.featureNames], mean: [...model.mean], scale: [...model.scale],
    coefficients: [...model.coefficients], trainingRows: model.trainingRows,
    trainingMean: model.trainingMean, trainingRmse: model.trainingRmse, alpha: model.alpha,
  };
}

export function buildFixedRidgeBankV1(captures) {
  invariant(Array.isArray(captures) && captures.length === 3, 'RIDGE_BANK_CAPTURE_COUNT');
  invariant(JSON.stringify(captures.map(x => x.capture.id)) === JSON.stringify(MARKET_EDGE_RIDGE_BANK_PROTOCOL_V1.trainingSourceIds),
    'RIDGE_BANK_SOURCE_ORDER');
  invariant(MARKET_EDGE_RIDGE_PROTOCOL.ridgeAlpha === 0.01
    && MARKET_EDGE_RIDGE_PROTOCOL.decisionLatencyMs === 3000
    && MARKET_EDGE_RIDGE_PROTOCOL.holdingHorizonMs === 5000
    && MARKET_EDGE_RIDGE_PROTOCOL.feesPerShare === 0
    && MARKET_EDGE_RIDGE_PROTOCOL.positivePredictionThreshold === 0, 'RIDGE_BANK_PROTOCOL_MISMATCH');

  const summaries = captures.map(captureSummary);
  for (let i = 1; i < captures.length; i++) {
    invariant(instantNs(captures[i - 1].capture.through) < instantNs(captures[i].capture.from), 'RIDGE_BANK_CAPTURE_ORDER');
    invariant(instantNs(captures[i].capture.from) - instantNs(captures[i - 1].capture.through) > 30_000_000_000n,
      'RIDGE_BANK_CAPTURE_GAP_TOO_SHORT');
    invariant(maxExitMs(captures[i - 1].candidates) < minDecisionMs(captures[i].candidates), 'RIDGE_BANK_LABEL_BOUNDARY');
  }

  const folds = [
    {training: captures.slice(0, 1), scored: captures[1]},
    {training: captures.slice(0, 2), scored: captures[2]},
  ].map(({training, scored}) => {
    const rows = training.flatMap(c => labeledRows(c.candidates, c.capture.id));
    const model = fitMarketEdgeRidgeV1(rows);
    invariant(model.alpha === 0.01 && model.trainingRows === rows.length, 'RIDGE_BANK_FOLD_TRAINING');
    const scoredLabels = labeledRows(scored.candidates, scored.capture.id);
    const result = scoreMarketEdgeRidgeV1(model, scored.candidates);
    invariant(result.candidatePredictions === scored.candidates.length && result.labeledCandidates === scoredLabels.length,
      'RIDGE_BANK_FOLD_SCORE_COVERAGE');
    return {
      trainingCaptureIds: training.map(x => x.capture.id), trainingRows: model.trainingRows,
      trainingTargetMeanPerShare: model.trainingMean, trainingRmsePerShare: model.trainingRmse,
      scoredCaptureId: scored.capture.id, scoredSourceSha256: scored.capture.sha256,
      scoredCandidateCount: scored.candidates.length, scoredLabeledCount: scoredLabels.length,
      diagnosticsOnly: true, score: compactScore(result),
    };
  });

  const allTraining = captures.flatMap(c => labeledRows(c.candidates, c.capture.id));
  const model = fitMarketEdgeRidgeV1(allTraining);
  invariant(model.alpha === 0.01 && model.trainingRows === allTraining.length, 'RIDGE_BANK_FINAL_TRAINING');
  const serialized = serializedModel(model);
  const trainingSources = captures.map(c => ({
    id: c.capture.id, file: c.capture.file, frozenBytes: c.capture.bytes,
    sha256: c.capture.sha256, captureRows: c.capture.captureRows,
    from: c.capture.from, through: c.capture.through,
    candidateCount: c.candidates.length, labeledCount: labeledRows(c.candidates, c.capture.id).length,
  }));
  const modelHash = sha256Json({
    protocol: MARKET_EDGE_RIDGE_BANK_PROTOCOL_V1,
    featureNames: [...MARKET_EDGE_RIDGE_PROTOCOL.featureNames], trainingSources, model: serialized,
  });
  const payload = {
    schemaVersion: 1, experimentId: 'market-edge-ridge-fixed-multicapture-bank-20261013-v1',
    modelIdentityHash: modelHash,
    policy: {
      features: [...MARKET_EDGE_RIDGE_PROTOCOL.featureNames],
      ridgeAlpha: MARKET_EDGE_RIDGE_PROTOCOL.ridgeAlpha,
      decisionLatencyMs: MARKET_EDGE_RIDGE_PROTOCOL.decisionLatencyMs,
      holdingHorizonMs: MARKET_EDGE_RIDGE_PROTOCOL.holdingHorizonMs,
      feesPerShare: MARKET_EDGE_RIDGE_PROTOCOL.feesPerShare,
      fixedPositiveThreshold: MARKET_EDGE_RIDGE_PROTOCOL.positivePredictionThreshold,
    },
    trainingSources,
    perCaptureReadiness: summaries.map(x => ({
      id: x.id, readiness: x.readiness, captureRows: x.captureRows,
      configRows: x.configRows, from: x.from, through: x.through,
    })),
    expandingWalkForward: folds,
    finalModel: {...serialized, frozenModelSha256: modelHash},
    futureTest: {
      status: MARKET_EDGE_RIDGE_BANK_PROTOCOL_V1.futureTestStatus,
      frozenCutoffObservedAt: captures.at(-1).capture.through,
      minimumFutureObservedAtExclusive: captures.at(-1).capture.through,
      requiresSeparateFrozenSourceIdentity: true, noFutureCaptureRowsUsedForFitOrScaling: true,
      performanceConclusion: 'none',
    },
    limitations: [
      'Walk-forward values are overlapping one-share displayed-book counterfactual labels, not portfolio returns or actual fills.',
      'These development captures and prior fixed-policy research are not confirmatory evidence.',
      'Displayed size does not establish queue priority, achievable fills, market impact, or scalable capacity.',
      'Zero fees are modeled; one tick per leg is shown as stress and may not bound real execution costs.',
      'No profitability, contest execution, production readiness, or scale conclusion is established.',
    ],
    noNetworkRequests: true, noAccountOrRuntimeServiceAccess: true,
    contestOrders: 0, actualFillsProven: false, profitabilityEstablished: false,
  };
  return {...payload, frozenBankSha256: sha256Json(payload)};
}
function marketFromTopic(topic) {
  const m = /^tournament:([0-9a-f-]+):market:(\d+)$/.exec(topic ?? '');
  return m ? {tournamentId: m[1], marketId: m[2]} : null;
}

function newDiagnostics() {
  return {
    captureRows: 0, configRows: 0, eventRows: 0, marketBatchRows: 0, nonEventRows: 0,
    duplicateRevisions: 0, revisionGaps: 0, resyncRequired: 0, bookDirtyMarkers: 0,
    dirtyWithoutBook: 0, settlements: 0, lifecycleInvalidations: 0, validRecoveryCompletions: 0,
    staleRecoveryCompletions: 0, malformedRecoveryCompletions: 0,
    staleRecoveryStarts: 0, malformedRecoveryStarts: 0,
    booksBeforeRecovery: 0, scopeRejectedBooks: 0, duplicateBookVersions: 0,
    rollbackBookVersions: 0, acceptedBookPoints: 0, explicitBreakEvents: 0,
    explicitSegmentIncrements: 0, explicitBreakReasons: {}, interBookGapBreaks: 0,
    observationGapBreaks: 0, sourceGapBreaks: 0, sourceRollbackBreaks: 0,
    invalidRows: 0, staleForFeatures: 0, missingStateRows: 0, epochMismatchRows: 0, malformedEpochRows: 0,
    stateMismatchExamples: [], epochMismatchExamples: [],
  };
}

function incrementReason(diagnostics, reason, increments = 1) {
  diagnostics.explicitBreakReasons[reason] = (diagnostics.explicitBreakReasons[reason] ?? 0) + increments;
}

function summarizeLabels(candidates, captureId) {
  const labels = candidates.filter(row => Number.isFinite(row.label?.net)).map(row => row.label);
  const sameSegment = labels.filter(x => x.decisionSegment === x.entrySegment && x.entrySegment === x.exitSegment).length;
  const noProvenance = labels.filter(x => !x.decisionProvenance || !x.entryProvenance || !x.exitProvenance).length;
  const observedHolds = labels.map(x => x.realizedObservedHoldingMs).filter(Number.isFinite);
  const sourceHolds = labels.map(x => x.exitSourceMs - x.entrySourceMs).filter(Number.isFinite);
  const decisionToEntry = labels.map(x => x.entryObservedMs - x.decisionProvenance.observedMs).filter(Number.isFinite);
  const entryToExitSourceAge = labels.map(x => x.realizedObservedHoldingMs - (x.exitSourceMs - x.entrySourceMs)).filter(Number.isFinite);
  const min = values => values.length ? values.reduce((best, value) => Math.min(best, value), Infinity) : null;
  const max = values => values.length ? values.reduce((best, value) => Math.max(best, value), -Infinity) : null;
  return {
    labeledCandidateCount: labels.length,
    decisionEntryExitSameSegmentCount: sameSegment,
    crossSegmentLabelCount: labels.length - sameSegment,
    missingCaptureProvenanceCount: noProvenance,
    captureId: captureId,
    uniqueCaptureIds: [...new Set(labels.flatMap(x => [x.decisionProvenance?.captureId, x.entryProvenance?.captureId, x.exitProvenance?.captureId]).filter(Boolean))],
    decisionEntryObservedDelayMs: {min: min(decisionToEntry), max: max(decisionToEntry)},
    observedHoldingMs: {min: min(observedHolds), max: max(observedHolds)},
    sourceHoldingMs: {min: min(sourceHolds), max: max(sourceHolds)},
    observedMinusSourceHoldingMs: {min: min(entryToExitSourceAge), max: max(entryToExitSourceAge)},
    provenanceExamples: labels.slice(0, 3).map(x => ({
      decision: x.decisionProvenance, entry: x.entryProvenance, exit: x.exitProvenance,
      decisionSegment: x.decisionSegment, entrySegment: x.entrySegment, exitSegment: x.exitSegment,
    })),
  };
}

export async function readCaptureV1(spec, verified) {
  const diagnostics = newDiagnostics();
  const scopeByExchange = new Map(), marketToExchange = new Map(), topicByMarket = new Map();
  const topicStates = new Map(), lastRevision = new Map(), lastBookVersion = new Map();
  const segmentByMarket = new Map(), seriesByMarket = new Map();
  let expectedIndex = 0, lastObservedNs = null, firstObservedAt = null, throughObservedAt = null;
  let groupMs = null, group = [], sawEvent = false;
  let engineScope = null;

  const bumpSegment = (marketId, reason) => {
    diagnostics.explicitBreakEvents++;
    incrementReason(diagnostics, reason);
    if (marketId === null) {
      for (const id of marketToExchange.keys()) {
        segmentByMarket.set(id, (segmentByMarket.get(id) ?? 1) + 1);
        diagnostics.explicitSegmentIncrements++;
      }
    } else {
      segmentByMarket.set(marketId, (segmentByMarket.get(marketId) ?? 1) + 1);
      diagnostics.explicitSegmentIncrements++;
    }
  };

  function flushGroup() {
    if (!group.length) return;
    const invalidated = new Set(), updates = new Map(), usableEvents = [];
    const setInvalid = (topic, reason) => {
      const parsed = marketFromTopic(topic);
      if (!parsed || parsed.tournamentId !== EXPECTED_TOURNAMENT) throw Error('RIDGE_INVALIDATION_TOPIC');
      invalidated.add(parsed.marketId);
      bumpSegment(parsed.marketId, reason);
      const prior = updates.get(topic) ?? topicStates.get(topic) ?? {epoch: null, ready: false};
      updates.set(topic, {...prior, ready: false});
      if (reason === 'lifecycle') diagnostics.lifecycleInvalidations++;
    };

    for (const row of group) {
      const topic = row.topic ?? null;
      const parsedTopic = topic === null ? null : marketFromTopic(topic);
      if (topic !== null && (!parsedTopic || parsedTopic.tournamentId !== EXPECTED_TOURNAMENT || !topicByMarket.has(parsedTopic.marketId)))
        throw Error('RIDGE_CAPTURE_TOPIC_SCOPE');
      if (row.kind === 'subscribed') {
        if (!parsedTopic) throw Error('RIDGE_SUBSCRIBE_TOPIC');
        const transition = subscribedStateUpdateV1(row.epoch);
        invalidated.add(parsedTopic.marketId); bumpSegment(parsedTopic.marketId, 'subscribed');
        updates.set(topic, transition.state); diagnostics.lifecycleInvalidations++;
        if (transition.status !== 'applied') diagnostics.malformedEpochRows++;
        continue;
      }
      if (row.kind === 'recovery_started' && topic !== null) {
        const prior = updates.get(topic) ?? topicStates.get(topic) ?? null;
        const transition = recoveryStartedStateUpdateV1(prior, row.epoch);
        if (transition.status === 'malformed_epoch') {
          diagnostics.malformedEpochRows++;
          diagnostics.malformedRecoveryStarts++;
          setInvalid(topic, 'lifecycle');
          continue;
        }
        if (transition.status !== 'applied') {
          diagnostics.staleRecoveryStarts++;
          continue;
        }
        invalidated.add(parsedTopic.marketId); bumpSegment(parsedTopic.marketId, 'lifecycle');
        updates.set(topic, transition.state); diagnostics.lifecycleInvalidations++;
        continue;
      }
      if (['invalidation', 'disconnected', 'recovery_started'].includes(row.kind)) {
        if (topic === null) {
          invalidated.add(null); bumpSegment(null, row.kind);
          for (const known of topicByMarket.values()) {
            const prior = updates.get(known) ?? topicStates.get(known) ?? {epoch: null, ready: false};
            updates.set(known, {...prior, ready: false});
          }
          diagnostics.lifecycleInvalidations++;
        } else setInvalid(topic, 'lifecycle');
        continue;
      }
      if (row.kind === 'recovery_finished') {
        if (!topic || !parsedTopic) throw Error('RIDGE_RECOVERY_TOPIC');
        const prior = updates.get(topic) ?? topicStates.get(topic) ?? {epoch: null, ready: false};
        const transition = recoveryFinishedStateUpdateV1(prior, row.epoch, row.booksReady);
        if (transition.status !== 'applied') {
          diagnostics.staleRecoveryCompletions++;
          if (transition.status === 'malformed_epoch') {
            diagnostics.malformedRecoveryCompletions++;
            diagnostics.malformedEpochRows++;
          }
          continue;
        }
        updates.set(topic, transition.state);
        if (row.booksReady === true) diagnostics.validRecoveryCompletions++;
        else setInvalid(topic, 'recovery_not_ready');
        continue;
      }
      if (row.kind !== 'event') { diagnostics.nonEventRows++; continue; }
      diagnostics.eventRows++;
      if (row.event !== 'market_batch') continue;
      diagnostics.marketBatchRows++;
      const state = updates.get(topic) ?? topicStates.get(topic), payload = row.payload ?? {}, delivery = payload.delivery;
      const revision = delivery?.revision, previous = delivery?.previousRevision;
      if (!state) {
        diagnostics.missingStateRows++;
        if (diagnostics.stateMismatchExamples.length < 12) diagnostics.stateMismatchExamples.push({
          captureIndex: row.captureIndex, topic, rowEpoch: row.epoch, stateEpoch: null, stateReady: false, reason: 'missing_state'});
        setInvalid(topic, 'missing_state'); continue;
      }
      if (!marketBatchEpochMatchesStateV1(state.epoch, row.epoch)) {
        diagnostics.epochMismatchRows++;
        if (!validEpochV1(row.epoch)) diagnostics.malformedEpochRows++;
        if (diagnostics.epochMismatchExamples.length < 12) diagnostics.epochMismatchExamples.push({
          captureIndex: row.captureIndex, topic, rowEpoch: row.epoch, stateEpoch: state.epoch, stateReady: state.ready, reason: 'epoch_mismatch'});
        setInvalid(topic, 'epoch_mismatch'); continue;
      }
      const revKey = `${topic}#${row.epoch}`, oldRevision = lastRevision.get(revKey);
      if (!Number.isSafeInteger(revision) || revision < 0 || !Number.isSafeInteger(previous) || previous < 0) {
        setInvalid(topic, 'invalid_revision'); continue;
      }
      if (oldRevision !== undefined && revision <= oldRevision) { diagnostics.duplicateRevisions++; continue; }
      const revisionGap = oldRevision !== undefined && previous > oldRevision;
      lastRevision.set(revKey, revision);
      if (revisionGap) { diagnostics.revisionGaps++; setInvalid(topic, 'revision_gap'); continue; }
      if (payload.resyncRequired !== false) { diagnostics.resyncRequired++; setInvalid(topic, 'resync_required'); continue; }
      if (payload.marketSettled?.length) { diagnostics.settlements++; setInvalid(topic, 'market_settled'); continue; }
      diagnostics.bookDirtyMarkers += payload.bookDirty?.length ?? 0;
      if ((payload.bookDirty ?? []).some(dirty => !(payload.books ?? []).some(book => String(book.exchangeId) === String(dirty.exchangeId)))) {
        diagnostics.dirtyWithoutBook++; setInvalid(topic, 'dirty_without_book'); continue;
      }
      if (!marketBatchAdmissibleV1({ready: state.ready, invalidated: false,
        stateEpoch: state.epoch, eventEpoch: row.epoch})) {
        diagnostics.booksBeforeRecovery++; continue;
      }
      usableEvents.push(row);
    }

    const candidates = new Map();
    for (const row of usableEvents) {
      const marketId = marketFromTopic(row.topic).marketId;
      const state = updates.get(row.topic) ?? topicStates.get(row.topic);
      if (!marketBatchAdmissibleV1({ready: state?.ready,
        invalidated: invalidated.has(null) || invalidated.has(marketId),
        stateEpoch: state?.epoch, eventEpoch: row.epoch})) continue;
      const result = engineBookPointsV1(row, engineScope, row.observedAt);
      diagnostics.scopeRejectedBooks += result.rejected.length;
      for (const book of result.books) {
        const member = scopeByExchange.get(String(book.exchangeId));
        if (!member || member.marketId !== book.marketId) { diagnostics.scopeRejectedBooks++; continue; }
        const sequence = BigInt(book.sequence), sourceNs = instantNs(book.sourceAt), old = lastBookVersion.get(book.exchangeId);
        if (old && (sequence < old.sequence || sequence === old.sequence && sourceNs <= old.sourceNs)) {
          diagnostics.duplicateBookVersions++;
          if (sequence < old.sequence) {
            diagnostics.rollbackBookVersions++;
            bumpSegment(member.marketId, 'book_version_rollback');
          }
          continue;
        }
        lastBookVersion.set(book.exchangeId, {sequence, sourceNs});
        try {
          const observedNs = instantNs(row.observedAt);
          const observedMs = Number(observedNs / 1000000n), sourceMs = Number(sourceNs / 1000000n);
          const bids = book.bids, asks = book.asks;
          if (!bids.length || !asks.length) throw Error('RIDGE_EMPTY_BOOK');
          const sum = (levels, n) => levels.slice(0, n).reduce((total, level) => total + level.quantity, 0);
          const point = {
            marketId: member.marketId, exchangeId: member.exchangeId, observedMs, sourceMs,
            sourceAgeMs: observedMs - sourceMs, segment: segmentByMarket.get(member.marketId) ?? 1,
            bid: Number(bids[0].price), ask: Number(asks[0].price),
            bidDepth1: sum(bids, 1), askDepth1: sum(asks, 1),
            bidDepth5: sum(bids, 5), askDepth5: sum(asks, 5),
            bidDepth20: sum(bids, 20), askDepth20: sum(asks, 20),
            provenance: {
              captureId: spec.id, captureIndex: book.sourceBinding.captureIndex, epoch: book.sourceBinding.epoch,
              topic: book.sourceBinding.topic, observedAt: row.observedAt, receivedAt: book.receivedAt,
              sourceAt: book.sourceAt, rawBookHash: book.sourceBinding.rawBookHash,
              recordHash: book.sourceBinding.recordHash,
            },
          };
          if (!Number.isFinite(point.bid) || !Number.isFinite(point.ask) || point.bid <= 0 || point.ask >= 1 || point.bid >= point.ask
            || point.bidDepth1 < 1 || point.askDepth1 < 1) throw Error('RIDGE_BOOK_POINT');
          const prior = candidates.get(member.exchangeId);
          if (!prior || sequence > prior.sequence || sequence === prior.sequence && sourceNs > prior.sourceNs)
            candidates.set(member.exchangeId, {point, sequence, sourceNs});
        } catch {
          diagnostics.invalidRows++;
        }
      }
    }
    for (const {point} of [...candidates.values()].sort((a, b) => a.point.marketId < b.point.marketId ? -1 : a.point.marketId > b.point.marketId ? 1 : 0)) {
      const rows = seriesByMarket.get(point.marketId) ?? [];
      if (rows.length && point.observedMs < rows.at(-1).observedMs) throw Error('RIDGE_PER_MARKET_ORDER');
      if (rows.length && point.observedMs === rows.at(-1).observedMs) throw Error('RIDGE_DUPLICATE_OBSERVATION_TIME');
      rows.push(point); seriesByMarket.set(point.marketId, rows); diagnostics.acceptedBookPoints++;
      if (point.sourceAgeMs < 0 || point.sourceAgeMs > MARKET_EDGE_RIDGE_PROTOCOL.maximumSourceAgeMs) diagnostics.staleForFeatures++;
    }
    for (const [topic, state] of updates) topicStates.set(topic, state);
    if (invalidated.has(null)) for (const [topic, state] of topicStates) topicStates.set(topic, {...state, ready: false});
    group = [];
  }

  const input = createReadStream(verified.file, {encoding: 'utf8', start: 0, end: spec.bytes - 1, highWaterMark: 4 * 1024 * 1024});
  const lines = readline.createInterface({input, crlfDelay: Infinity});
  for await (const line of lines) {
    if (!line) throw Error('RIDGE_BLANK_CAPTURE_ROW');
    const row = JSON.parse(line);
    if (row.format !== 'sig-feed-capture' || row.schemaVersion !== 1 || row.captureIndex !== expectedIndex++) throw Error('RIDGE_CAPTURE_CHAIN');
    const observedNs = instantNs(row.observedAt);
    if (lastObservedNs !== null && observedNs < lastObservedNs) throw Error('RIDGE_CAPTURE_CLOCK');
    lastObservedNs = observedNs; firstObservedAt ??= row.observedAt; throughObservedAt = row.observedAt;
    diagnostics.captureRows++;
    if (row.kind === 'config') {
      if (sawEvent) throw Error('RIDGE_CONFIG_AFTER_EVENT');
      diagnostics.configRows++;
      diagnostics.nonEventRows++;
      const parsed = marketFromTopic(row.topic), scope = row.scope;
      if (!parsed || parsed.tournamentId !== EXPECTED_TOURNAMENT || scope?.tournamentId !== EXPECTED_TOURNAMENT
        || scope?.marketId !== parsed.marketId || !Array.isArray(scope.exchangeIds) || !scope.exchangeIds.length) throw Error('RIDGE_CONFIG_SCOPE');
      if (topicByMarket.has(parsed.marketId)) throw Error('RIDGE_DUPLICATE_MARKET_CONFIG');
      topicByMarket.set(parsed.marketId, row.topic); marketToExchange.set(parsed.marketId, null);
      for (const exchangeIdRaw of scope.exchangeIds) {
        const exchangeId = String(exchangeIdRaw);
        if (!/^\d+$/.test(exchangeId) || scopeByExchange.has(exchangeId)) throw Error('RIDGE_DUPLICATE_EXCHANGE_SCOPE');
        scopeByExchange.set(exchangeId, {marketId: parsed.marketId, exchangeId});
        if (marketToExchange.get(parsed.marketId) !== null) throw Error('RIDGE_MULTIPLE_EXCHANGES_PER_MARKET');
        marketToExchange.set(parsed.marketId, exchangeId);
      }
    } else if (row.kind === 'event') {
      sawEvent = true;
      if (diagnostics.configRows !== EXPECTED_MARKETS || scopeByExchange.size !== EXPECTED_MARKETS) throw Error('RIDGE_CONFIG_INCOMPLETE');
      const parsed = marketFromTopic(row.topic);
      if (!parsed || parsed.tournamentId !== EXPECTED_TOURNAMENT || !topicByMarket.has(parsed.marketId)) throw Error('RIDGE_EVENT_SCOPE');
      const observedMs = Number(observedNs / 1000000n);
      if (groupMs !== null && observedMs !== groupMs) flushGroup();
      groupMs = observedMs; group.push(row);
    } else {
      diagnostics.nonEventRows++;
      if (captureLifecycleCanBeAppliedV1({configRows: diagnostics.configRows, expectedMarkets: EXPECTED_MARKETS})) {
        const observedMs = Number(observedNs / 1000000n);
        if (groupMs !== null && observedMs !== groupMs) flushGroup();
        groupMs = observedMs; group.push(row);
      }
    }
    if (sawEvent && diagnostics.configRows === EXPECTED_MARKETS && engineScope === null) {
      engineScope = {tournamentId: EXPECTED_TOURNAMENT,
        exchanges: [...scopeByExchange.values()].map(x => ({marketId: x.marketId, exchangeId: x.exchangeId}))};
    }
  }
  flushGroup();
  if (Number.isSafeInteger(spec.captureRows)) invariant(diagnostics.captureRows === spec.captureRows, 'RIDGE_CAPTURE_ROW_COUNT');
  invariant(diagnostics.configRows === EXPECTED_MARKETS && scopeByExchange.size === EXPECTED_MARKETS, 'RIDGE_CAPTURE_MARKET_COUNT');
  invariant(firstObservedAt && throughObservedAt, 'RIDGE_CAPTURE_TIME_RANGE');
  if (spec.from) invariant(instantNs(firstObservedAt) === instantNs(spec.from), 'RIDGE_CAPTURE_FROM_BOUND');
  if (spec.through) invariant(instantNs(throughObservedAt) === instantNs(spec.through), 'RIDGE_CAPTURE_THROUGH_BOUND');

  const allCandidates = [];
  for (const [marketId, rows] of [...seriesByMarket.entries()].sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)) {
    const segmented = segmentSeriesGapsV1(rows);
    diagnostics.interBookGapBreaks += segmented.gapBreaks;
    diagnostics.observationGapBreaks += segmented.observationGapBreaks;
    diagnostics.sourceGapBreaks += segmented.sourceGapBreaks;
    diagnostics.sourceRollbackBreaks += segmented.sourceRollbackBreaks;
    const candidates = candidatesForSeriesV1(segmented.rows);
    allCandidates.push(...candidates);
  }
  const labeled = allCandidates.filter(row => Number.isFinite(row.label?.net));
  const provenance = summarizeLabels(allCandidates, spec.id);
  invariant(provenance.crossSegmentLabelCount === 0 && provenance.missingCaptureProvenanceCount === 0, 'RIDGE_LABEL_PROVENANCE');
  invariant(provenance.labeledCandidateCount === labeled.length, 'RIDGE_LABEL_COUNT');
  return {
    capture: {id: spec.id, role: spec.role, file: spec.file, bytes: spec.bytes, sha256: spec.sha256,
      captureRows: diagnostics.captureRows, configRows: diagnostics.configRows, markets: topicByMarket.size,
      exchanges: scopeByExchange.size, from: firstObservedAt, through: throughObservedAt},
    diagnostics, candidates: allCandidates, labelProvenance: provenance,
  };
}

export const RIDGE_BANK_CANONICAL_JSON_V1 = stableJson;

