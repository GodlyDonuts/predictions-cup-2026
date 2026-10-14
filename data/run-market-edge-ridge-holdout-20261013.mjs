import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {createReadStream} from 'node:fs';
import readline from 'node:readline';
import {fileURLToPath} from 'node:url';
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
} from '../research/market-edge-ridge-v1.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const RUNTIME = path.join(process.env.LOCALAPPDATA ?? '', 'SigTradingCup', 'paper-runtime');
const REPORT_PATH = path.join(ROOT, 'data', 'market-edge-ridge-holdout-20261013.json');
const EXPECTED_TOURNAMENT = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const EXPECTED_MARKETS = 237;
const SOURCES = [
  {
    id: 'train-d53812dd', role: 'training_only',
    file: 'realtime-2026-10-13T02-55-28-953Z-d53812dd-ec75-4852-9267-59c7d5bb7898.jsonl',
    bytes: 1747459094,
    sha256: 'f9a20ef98e8e9c96d1229dbbc6a8eb17dd669e7c6f3f8d6e49337439945f4418',
    backingBytes: 1747507916,
    backingSha256: 'd321ad760750e0ba6cdbc85afade4fdd7c1874b74aa080df9881810942cfca6e',
    captureRows: 467044, from: '2026-10-13T02:55:28.956Z', through: '2026-10-13T07:45:51.103Z',
  },
  {
    id: 'test-9ecf5d7b', role: 'chronological_test_only',
    file: 'realtime-2026-10-13T19-52-33-253Z-9ecf5d7b-070f-4c01-a991-28a67a9f4a29.jsonl',
    bytes: 279458961,
    sha256: 'fd6a9552a5f70365ee78706fa1efff70e2a36e7f3dd55c1a932cc3795e1979e7',
    backingBytes: 279458961,
    backingSha256: 'fd6a9552a5f70365ee78706fa1efff70e2a36e7f3dd55c1a932cc3795e1979e7',
    captureRows: 76207, from: '2026-10-13T19:52:33.254Z', through: '2026-10-13T20:40:59.415Z',
  },
];

function invariant(ok, code) { if (!ok) throw Error(code); }

function fileIdentity(stat) {
  return {dev: String(stat.dev), ino: String(stat.ino), size: stat.size, mtimeMs: stat.mtimeMs};
}

function sameIdentity(a, b) {
  return a.dev === b.dev && a.ino === b.ino && a.size === b.size && a.mtimeMs === b.mtimeMs;
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
  invariant(prefixHashed === prefixBytes, 'RIDGE_SOURCE_PREFIX_TRUNCATED');
  return {bytes, backingSha256: fullHash.digest('hex'), sha256: prefixHash.digest('hex')};
}

function hasFinalNewline(file, offset) {
  const fd = fs.openSync(file, 'r'), byte = Buffer.alloc(1);
  try { return fs.readSync(fd, byte, 0, 1, offset) === 1 && byte[0] === 0x0a; }
  finally { fs.closeSync(fd); }
}

async function verifyBefore(spec) {
  const file = path.join(RUNTIME, spec.file), beforeStat = fs.statSync(file), beforeIdentity = fileIdentity(beforeStat);
  invariant(beforeStat.isFile() && beforeStat.size === spec.backingBytes && beforeStat.size >= spec.bytes, 'RIDGE_SOURCE_SIZE');
  invariant(hasFinalNewline(file, spec.bytes - 1), 'RIDGE_SOURCE_PREFIX_NOT_LINE_ALIGNED');
  const beforeHash = await hashFileAndPrefix(file, spec.bytes);
  const afterHashStat = fileIdentity(fs.statSync(file));
  invariant(sameIdentity(beforeIdentity, afterHashStat), 'RIDGE_SOURCE_CHANGED_DURING_PREHASH');
  invariant(beforeHash.bytes === spec.backingBytes && beforeHash.backingSha256 === spec.backingSha256, 'RIDGE_BACKING_PREHASH');
  invariant(beforeHash.sha256 === spec.sha256, 'RIDGE_SOURCE_PREFIX_PREHASH');
  return {file, beforeIdentity, beforeSha256: beforeHash.sha256, beforeBackingSha256: beforeHash.backingSha256,
    frozenBytes: spec.bytes, excludedTailBytes: spec.backingBytes - spec.bytes};
}

async function verifyAfter(spec, verified) {
  const beforePostHash = fileIdentity(fs.statSync(verified.file));
  invariant(sameIdentity(verified.beforeIdentity, beforePostHash), 'RIDGE_SOURCE_IDENTITY_CHANGED');
  invariant(hasFinalNewline(verified.file, spec.bytes - 1), 'RIDGE_SOURCE_PREFIX_NOT_LINE_ALIGNED_AFTER');
  const afterHash = await hashFileAndPrefix(verified.file, spec.bytes);
  const afterIdentity = fileIdentity(fs.statSync(verified.file));
  invariant(sameIdentity(verified.beforeIdentity, afterIdentity), 'RIDGE_SOURCE_CHANGED_DURING_POSTHASH');
  invariant(afterHash.bytes === spec.backingBytes && afterHash.backingSha256 === spec.backingSha256
    && afterHash.backingSha256 === verified.beforeBackingSha256, 'RIDGE_BACKING_POSTHASH');
  invariant(afterHash.sha256 === spec.sha256 && afterHash.sha256 === verified.beforeSha256, 'RIDGE_SOURCE_PREFIX_POSTHASH');
  return {file: spec.file, frozenBytes: spec.bytes, frozenSha256: spec.sha256,
    excludedTailBytes: verified.excludedTailBytes, backingFileBytes: spec.backingBytes,
    backingFileSha256: spec.backingSha256, beforeSha256: verified.beforeSha256,
    afterSha256: afterHash.sha256, beforeIdentity: verified.beforeIdentity,
    afterIdentity, identityStable: true};
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
    booksBeforeRecovery: 0, scopeRejectedBooks: 0, duplicateBookVersions: 0,
    rollbackBookVersions: 0, acceptedBookPoints: 0, explicitBreakEvents: 0,
    explicitSegmentIncrements: 0, explicitBreakReasons: {}, interBookGapBreaks: 0,
    observationGapBreaks: 0, sourceGapBreaks: 0, sourceRollbackBreaks: 0,
    invalidRows: 0, staleForFeatures: 0, missingStateRows: 0, epochMismatchRows: 0,
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

async function readCapture(spec, verified) {
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
        invalidated.add(parsedTopic.marketId); bumpSegment(parsedTopic.marketId, 'subscribed');
        updates.set(topic, {epoch: row.epoch, ready: false}); diagnostics.lifecycleInvalidations++;
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
        updates.set(topic, {...prior, epoch: row.epoch, ready: row.booksReady === true});
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
      if (state.epoch !== row.epoch) {
        diagnostics.epochMismatchRows++;
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
      if (!state.ready) { diagnostics.booksBeforeRecovery++; continue; }
      usableEvents.push(row);
    }

    const candidates = new Map();
    for (const row of usableEvents) {
      const marketId = marketFromTopic(row.topic).marketId;
      if (!marketBatchUsableAfterRecoveryV1({ready: true, invalidated: invalidated.has(null) || invalidated.has(marketId)})) continue;
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
  invariant(diagnostics.captureRows === spec.captureRows, 'RIDGE_CAPTURE_ROW_COUNT');
  invariant(diagnostics.configRows === EXPECTED_MARKETS && scopeByExchange.size === EXPECTED_MARKETS, 'RIDGE_CAPTURE_MARKET_COUNT');
  invariant(firstObservedAt && throughObservedAt, 'RIDGE_CAPTURE_TIME_RANGE');
  invariant(instantNs(firstObservedAt) === instantNs(spec.from) && instantNs(throughObservedAt) === instantNs(spec.through), 'RIDGE_CAPTURE_TIME_BOUND');

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

function finiteSummary(candidateRows) {
  const labels = candidateRows.filter(x => Number.isFinite(x.label?.net));
  return {candidateCount: candidateRows.length, labeledCount: labels.length, censoredCount: candidateRows.length - labels.length};
}

async function main() {
  invariant(process.env.LOCALAPPDATA, 'RIDGE_LOCALAPPDATA_MISSING');
  const diagnoseTrainingOnly = process.argv.includes('--diagnose-training-only');
  if (!diagnoseTrainingOnly) invariant(!fs.existsSync(REPORT_PATH), 'RIDGE_REPORT_ALREADY_EXISTS');
  const verified = [];
  const selectedSources = diagnoseTrainingOnly ? [SOURCES[0]] : SOURCES;
  for (const spec of selectedSources) verified.push(await verifyBefore(spec));

  const training = await readCapture(SOURCES[0], verified[0]);
  const trainingSummary = finiteSummary(training.candidates);
  if (diagnoseTrainingOnly) {
    const post = await verifyAfter(SOURCES[0], verified[0]);
    console.log(JSON.stringify({diagnosticMode: 'training_only_no_model_no_holdout', source: post,
      candidateCoverage: trainingSummary, labelProvenance: training.labelProvenance, diagnostics: training.diagnostics}));
    return;
  }
  const model = fitMarketEdgeRidgeV1(training.candidates);
  invariant(model.trainingRows === trainingSummary.labeledCount, 'RIDGE_TRAINING_SCOPE');
  training.candidates = null;
  if (global.gc) global.gc();

  // The model, feature order, scaler and action threshold are frozen in memory
  // before any holdout book row is parsed.
  const holdout = await readCapture(SOURCES[1], verified[1]);
  const score = scoreMarketEdgeRidgeV1(model, holdout.candidates);
  const holdoutSummary = finiteSummary(holdout.candidates);
  invariant(score.candidatePredictions === holdoutSummary.candidateCount, 'RIDGE_SCORE_COVERAGE');
  invariant(score.labeledCandidates === holdoutSummary.labeledCount, 'RIDGE_SCORE_LABEL_COUNT');

  const post = [];
  for (let i = 0; i < SOURCES.length; i++) post.push(await verifyAfter(SOURCES[i], verified[i]));
  const sourceChecks = SOURCES.map((spec, i) => ({...spec, frozenBytes: spec.bytes,
    excludedTailBytes: verified[i].excludedTailBytes,
    beforeFrozenSha256: verified[i].beforeSha256, afterFrozenSha256: post[i].afterSha256,
    beforeBackingSha256: verified[i].beforeBackingSha256,
    afterBackingSha256: post[i].backingFileSha256,
    beforeBackingBytes: verified[i].beforeIdentity.size, afterBackingBytes: post[i].afterIdentity.size,
    identityStable: post[i].identityStable}));
  invariant(sourceChecks.every(x => x.identityStable && x.sha256 === x.beforeFrozenSha256
    && x.sha256 === x.afterFrozenSha256 && x.backingSha256 === x.beforeBackingSha256
    && x.backingSha256 === x.afterBackingSha256), 'RIDGE_SOURCE_BINDING');

  const report = {
    schemaVersion: 1,
    experimentId: 'market-edge-ridge-fixed-executable-return-20261013-v1',
    generatedAt: new Date().toISOString(),
    method: 'one frozen pooled ridge model; training capture only for fit/scaling; later capture read after fit only for chronological scoring',
    sources: sourceChecks,
    sourceScope: {trainingFrozenPrefixBytes: SOURCES[0].bytes, trainingBackingFileBytes: SOURCES[0].backingBytes,
      excludedAppendedTrainingTailBytes: SOURCES[0].backingBytes - SOURCES[0].bytes,
      tailRowsParsed: false, frozenPrefixesHashVerifiedBeforeAndAfter: true,
      backingFileSha256VerifiedBeforeAndAfter: true, completeFinalJsonlNewlineRequired: true},
    marketScope: {tournamentId: EXPECTED_TOURNAMENT, trainingMarkets: training.capture.markets,
      holdoutMarkets: holdout.capture.markets, trainingExchanges: training.capture.exchanges,
      holdoutExchanges: holdout.capture.exchanges, contractIdentifiersUsedAsFeatures: false},
    protocol: {
      model: 'ordinary squared-error linear regression with fixed ridge slope penalty; no model-family or alpha selection',
      featureNames: model.featureNames,
      featureDefinitions: {
        sidePrice: 'YES mid; NO 1−YES mid',
        lagReturns: 'side-oriented mid-price change from the last prior same-segment point at/before 1s, 5s, and 15s lag targets; maximum lag-point distance 2s',
        spread: 'current YES best ask minus best bid',
        imbalance: 'side-oriented (bid quantity−ask quantity)/(bid quantity+ask quantity), cumulative top 1, 5, and 20 levels',
        displayedDepth: 'log1p(cumulative bid+ask quantity) at top 1, 5, and 20 levels',
      },
      scaler: {fitCapture: SOURCES[0].id, rows: model.trainingRows, mean: model.mean, scale: model.scale,
        constantFeatureScale: 1, holdoutUsedForScaling: false},
      ridge: {alpha: model.alpha, objective: 'mean((target−prediction)^2) + alpha*sum(standardized slopes^2)', interceptPenalized: false},
      training: {sourceCapture: SOURCES[0].id, rows: model.trainingRows, targetMeanPerShare: model.trainingMean,
        residualRmsePerShareUnpenalized: model.trainingRmse, fitUsesOnlyTrainingCapture: true},
      timing: {
        decisionAvailability: 'capture observedAt is the actionable observation clock; sourceAt is the exchange book as-of/event clock; receivedAt is the engine packet receipt',
        decisionLatencyMs: MARKET_EDGE_RIDGE_PROTOCOL.decisionLatencyMs,
        entry: 'first same-segment point with both observedAt and sourceAt >= decision observedAt + 3000ms, within 10000ms additional observed-time wait',
        holdingHorizonMs: MARKET_EDGE_RIDGE_PROTOCOL.holdingHorizonMs,
        holdingClock: 'entry observedAt + 5000ms; both exit observedAt and sourceAt must be >= that target, within 10000ms additional observed-time wait',
        sourceAgeMaximumMs: MARKET_EDGE_RIDGE_PROTOCOL.maximumSourceAgeMs,
        zeroFeesPerShare: MARKET_EDGE_RIDGE_PROTOCOL.feesPerShare,
      },
      labels: {
        yes: 'exit YES bid − entry YES ask', no: 'entry YES bid − exit YES ask',
        minimumDisplayedQuantityAtEntryAndExitPerRelevantSide: 1,
        missingOrBoundaryCrossing: 'censored; decision, entry, and exit must have the same per-market segment; capture files are processed independently',
        trainingBoundary: 'no training labels use any holdout row; labels require in-capture future observations',
      },
      action: {threshold: 'predicted executable net return strictly > 0 SUSQies/share',
        thresholdTunedOnHoldout: false, allHoldoutCandidatesScoredOnce: true,
        rowLevelOutput: 'predictionsAndOutcomes.predictionRows contains one row per holdout candidate in deterministic market-id, observed-time, YES-then-NO order; rowIndex is contiguous and each outcome is labeled or censored',
        oneTickPerLegStress: {tick: MARKET_EDGE_RIDGE_PROTOCOL.officialTick, deductionPerShare: 2 * MARKET_EDGE_RIDGE_PROTOCOL.officialTick}},
      segmentation: {perMarketInputMustBeChronological: true,
        explicitLifecycleRevisionSettlementBoundaries: 'increment per-market segment before accepting post-boundary observations',
        inferredBreak: 'new segment after >10s observed-time or source-time gap, or source-clock rollback; propagated through all later rows until another boundary',
        noSortingOrReordering: true},
    },
    training: {
      capture: training.capture, diagnostics: training.diagnostics,
      candidates: trainingSummary, labelProvenance: training.labelProvenance,
    },
    holdout: {
      status: 'scored once after model/scaler/threshold freeze; later capture had prior use in other fixed-policy studies',
      capture: holdout.capture, diagnostics: holdout.diagnostics,
      candidateCoverage: holdoutSummary, labelProvenance: holdout.labelProvenance,
      predictionsAndOutcomes: score,
    },
    limitations: [
      'This is one model-specific chronological later-capture test, not a globally untouched sample and not proof of persistent profitability.',
      'Labels are overlapping one-share displayed-quote counterfactuals; market-cluster summaries address grouping but do not establish independent trials or portfolio returns.',
      'Public depth does not prove order acceptance, queue priority, achievable fills, or size beyond one displayed share; no fees are modeled and a one-tick-per-leg stress is reported separately.',
      'No actual orders, fills, account state, P&L, market impact, or production policy are tested.',
      'The zero predicted-return threshold was fixed before holdout scoring; no threshold or feature tuning was performed on the later capture.',
    ],
    noNetworkRequests: true,
    noAccountOrRuntimeServiceAccess: true,
    contestOrders: 0,
    actualFillsProven: false,
    profitabilityEstablished: false,
  };
  fs.writeFileSync(REPORT_PATH, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
  console.log(JSON.stringify({reportPath: path.relative(ROOT, REPORT_PATH), training: trainingSummary,
    selected: score.selectedCandidateCount, selectedWithOutcome: score.selectedWithOutcomeCount,
    selectedCensored: score.selectedCensoredOutcomeCount, selectedMean: score.selected.meanPerShare,
    selectedOneTickStressMean: score.selected.oneTickPerLegMeanPerShare,
    unconditionalMean: score.unconditionalAllLabeledCandidateControl.meanPerShare,
    predictionRmse: score.predictionRmse, trainingMeanControlRmse: score.trainingMeanControlRmse,
    sourcesVerifiedBeforeAfter: sourceChecks.every(x => x.identityStable), actualFillsProven: false,
    profitabilityEstablished: false}));
}

main().catch(error => { console.error(error.stack ?? error.message); process.exitCode = 1; });

