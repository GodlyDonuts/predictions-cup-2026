import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {journalLines} from '../live-paper/journal-lines.mjs';
import {sealedNativePrefix, hashNativePrefix} from '../live-paper/native-dip-host-v1.mjs';
import {engineBookPointsV1} from '../paper/maker-engine-book-point-v1.mjs';
import {instantNs} from '../feed/contract.mjs';
import {TICK, SCALE} from '../paper/fixed.mjs';
import {
  COMPLEMENT_REPLAY_V1,
  screenComplementBookV1,
  analyzeSameMarketComplementV1,
} from '../research/same-market-complement-arb-v1.mjs';

export const COMPLEMENT_LATER_HOLDOUT_SOURCE_V1 = Object.freeze({
  file: 'realtime-2026-10-14T12-48-27-326Z-58ea4714-2533-43f4-8812-3ff28ff75ab0.jsonl',
  bytes: 382_415_207,
  sha256: '799ea868d21e820860208632d0e6cb4dca96d744da06fe92e0866e06b757f631',
  captureRows: 101_853,
  tournamentId: 'bda92870-621e-47b0-bc3c-3602c5c26f55',
  exchangeCount: 237,
  from: '2026-10-14T12:48:27.327Z',
  through: '2026-10-14T14:01:23.601Z',
});

const OUTPUT = process.env.BAAL_COMPLEMENT_OUTPUT ?? 'data/same-market-complement-arb-later-holdout-20261014.json';
const TOPIC = /^tournament:([0-9a-f-]+):market:(\d+)$/;

function need(ok, code) {
  if (!ok) throw Error(code);
}

export function verifyPinnedPrefixV1({source, before, after, beforeHash, afterHash}) {
  need(source && Number.isSafeInteger(source.bytes) && source.bytes > 0, 'COMPLEMENT_SOURCE_MANIFEST');
  need(before && after && Number.isSafeInteger(before.size) && Number.isSafeInteger(after.size), 'COMPLEMENT_SOURCE_STAT');
  need(before.size >= source.bytes && after.size >= source.bytes, 'COMPLEMENT_SOURCE_PREFIX_SHORT');
  need(before.dev === after.dev && before.ino === after.ino, 'COMPLEMENT_SOURCE_IDENTITY_CHANGED');
  need(beforeHash === source.sha256, 'COMPLEMENT_SOURCE_HASH_BEFORE');
  need(afterHash === source.sha256, 'COMPLEMENT_SOURCE_HASH_AFTER');
  return {beforeSha256:beforeHash,afterSha256:afterHash,beforeBytes:before.size,afterBytes:after.size,
    prefixBytes:source.bytes,identityStable:true,prefixHashStable:true};
}
function topicParts(topic) {
  const match = TOPIC.exec(topic ?? '');
  return match ? {tournamentId: match[1], marketId: match[2]} : null;
}

function iso(ms) {
  const value = new Date(ms);
  need(Number.isFinite(value.getTime()), 'COMPLEMENT_CAPTURE_CLOCK');
  return value.toISOString();
}

function canonicalScopeHash(exchanges) {
  const ordered = exchanges.toSorted((a, b) => a.exchangeId.localeCompare(b.exchangeId));
  return createHash('sha256').update(JSON.stringify(ordered)).digest('hex');
}

function summarizeStress(stats, penalty) {
  if (!stats || stats.count === 0) return {...stats};
  return Object.fromEntries(Object.entries(stats).map(([key, value]) => [
    key,
    typeof value === 'number' && key !== 'count' ? value - penalty : value,
  ]));
}

function addOneTickStress(replay) {
  const perLeg = Number(TICK) / Number(SCALE);
  const twoLegPenalty = 2 * perLeg;
  return {
    perLegAdverseExecution: '0.005000',
    pairAdverseExecution: '0.010000',
    application: 'Subtract one official tick from each leg’s per-share P&L; this is a sensitivity, not a new fill model or selector.',
    sameTimestampTopPairNetPerShare: summarizeStress(replay.atomic.topPairNetPerShare, twoLegPenalty),
    sameTimestampOnePercentDepthNetPerPair: summarizeStress(replay.atomic.onePercentDepthSweepNetPerPair, twoLegPenalty),
    sequentialScenarios: replay.diagnostics.map(row => ({
      period: row.period,
      firstSide: row.firstSide,
      firstLegLatencyMs: row.firstLegLatencyMs,
      secondLegLatencyMs: row.secondLegLatencyMs,
      completedPairs: row.completedPairs,
      pairNetPnlPerShare: row.pairNetPnlPerShare,
      twoLegAdverseStressPnlPerShare: summarizeStress(row.pairNetPnlPerShare, twoLegPenalty),
    })),
  };
}

/**
 * Reconstruct one sealed realtime capture using the same continuity, receipt,
 * scope, book-screening, and pair-analysis code as the original #22 replay.
 * `records` may be parsed objects or a streaming iterable of JSONL strings.
 */
export function replayComplementHoldoutRecordsV1(records, {
  expectedTournamentId = COMPLEMENT_LATER_HOLDOUT_SOURCE_V1.tournamentId,
  expectedExchangeCount = COMPLEMENT_LATER_HOLDOUT_SOURCE_V1.exchangeCount,
} = {}) {
  need(records && typeof records[Symbol.iterator] === 'function', 'COMPLEMENT_INPUT');
  need(typeof expectedTournamentId === 'string' && Number.isSafeInteger(expectedExchangeCount)
    && expectedExchangeCount > 0, 'COMPLEMENT_EXPECTED_SCOPE');

  const config = new Map(), topicMarket = topic => topicParts(topic)?.marketId ?? null;
  const topicStates = new Map(), lastRevision = new Map(), lastBookVersion = new Map();
  const breaks = [], observations = [];
  const diagnostics = {
    captureRows: 0, configRows: 0, marketBatchRows: 0, sourceBookSnapshots: 0,
    tradeObservedRows: 0, nonEventRows: 0,
    duplicateRevisions: 0, revisionGaps: 0, resyncRequired: 0, bookDirtyMarkers: 0,
    dirtyWithoutBook: 0, settlements: 0, lifecycleInvalidations: 0,
    validRecoveryCompletions: 0, booksBeforeRecovery: 0,
    scopeRejectedBooks: 0, duplicateBookVersions: 0, rollbackBookVersions: 0,
    acceptedBookPoints: 0, invalidScreenedBooks: 0,
  };
  let expectedIndex = 0, lastObservedMs = -Infinity, firstObservedAt = null, through = null;
  let dataStarted = false, scope = null, groupAtMs = null, group = [];

  function finishScope() {
    if (scope) return scope;
    const exchanges = [...config.values()].flatMap(({marketId, exchangeIds}) =>
      exchangeIds.map(exchangeId => ({exchangeId, marketId})));
    need(exchanges.length === expectedExchangeCount, 'COMPLEMENT_CAPTURE_SCOPE_COUNT');
    exchanges.sort((a, b) => a.exchangeId.localeCompare(b.exchangeId));
    const distinct = new Set(exchanges.map(row => row.exchangeId));
    need(distinct.size === expectedExchangeCount, 'COMPLEMENT_CAPTURE_SCOPE_DUPLICATE');
    scope = {tournamentId: expectedTournamentId, exchanges};
    return scope;
  }

  function addConfig(row) {
    need(!dataStarted, 'COMPLEMENT_CONFIG_AFTER_DATA');
    const raw = row.scope, parts = topicParts(row.topic);
    need(row.format === 'sig-feed-capture' && row.schemaVersion === 1 && row.kind === 'config'
      && raw && raw.tournamentId === expectedTournamentId && raw.marketId && /^\d+$/.test(raw.marketId)
      && parts?.tournamentId === expectedTournamentId && parts.marketId === raw.marketId
      && raw.topic === row.topic && Array.isArray(raw.exchangeIds) && raw.exchangeIds.length > 0,
    'COMPLEMENT_CAPTURE_CONFIG');
    const exchangeIds = raw.exchangeIds.map(String);
    need(exchangeIds.every(x => /^\d+$/.test(x)) && new Set(exchangeIds).size === exchangeIds.length,
      'COMPLEMENT_CAPTURE_CONFIG_EXCHANGES');
    need(!config.has(raw.marketId), 'COMPLEMENT_CAPTURE_CONFIG_DUPLICATE');
    config.set(raw.marketId, {marketId: raw.marketId, exchangeIds});
    diagnostics.configRows++;
    need([...config.values()].reduce((n, x) => n + x.exchangeIds.length, 0) <= expectedExchangeCount,
      'COMPLEMENT_CAPTURE_SCOPE_OVERFLOW');
  }

  function flushGroup() {
    if (!group.length) return;
    const atIso = iso(groupAtMs), invalidated = new Set(), updates = new Map(), events = [];
    const markInvalid = (topic, kind) => {
      const marketId = topicMarket(topic);
      invalidated.add(marketId);
      breaks.push({marketId, at: atIso});
      const prior = updates.get(topic) ?? topicStates.get(topic) ?? {epoch: null, ready: false};
      updates.set(topic, {...prior, ready: false});
      if (kind === 'lifecycle') diagnostics.lifecycleInvalidations++;
    };

    for (const row of group) {
      const topic = row.topic ?? null, marketId = topicMarket(topic);
      if (row.kind === 'config') continue;
      if (row.kind === 'subscribed') {
        invalidated.add(marketId);
        if (marketId !== null) breaks.push({marketId, at: atIso});
        updates.set(topic, {epoch: row.epoch, ready: false});
        diagnostics.lifecycleInvalidations++;
        continue;
      }
      if (['invalidation', 'disconnected', 'recovery_started'].includes(row.kind)) {
        if (topic === null) {
          invalidated.add(null);
          breaks.push({marketId: null, at: atIso});
          for (const [known, state] of topicStates) updates.set(known, {...state, ready: false});
        } else markInvalid(topic, 'lifecycle');
        continue;
      }
      if (row.kind === 'recovery_finished') {
        const ready = row.booksReady === true;
        const prior = updates.get(topic) ?? topicStates.get(topic) ?? {epoch: null, ready: false};
        updates.set(topic, {...prior, epoch: row.epoch, ready});
        if (ready) diagnostics.validRecoveryCompletions++;
        continue;
      }
      if (row.kind !== 'event' || row.event !== 'market_batch') continue;
      diagnostics.marketBatchRows++;
      const state = topicStates.get(topic), payload = row.payload ?? {};
      diagnostics.sourceBookSnapshots += payload.books?.length ?? 0;
      const revision = payload.delivery?.revision, previous = payload.delivery?.previousRevision;
      if (!state || state.epoch !== row.epoch) { markInvalid(topic, 'revision'); continue; }
      const key = `${topic}#${row.epoch}`;
      if (!Number.isSafeInteger(revision) || revision < 0 || !Number.isSafeInteger(previous) || previous < 0) {
        markInvalid(topic, 'revision'); continue;
      }
      const old = lastRevision.get(key);
      if (old !== undefined && revision <= old) { diagnostics.duplicateRevisions++; continue; }
      const gap = old !== undefined && previous > old;
      lastRevision.set(key, revision);
      if (gap) { diagnostics.revisionGaps++; markInvalid(topic, 'revision'); continue; }
      if (payload.resyncRequired !== false) { diagnostics.resyncRequired++; markInvalid(topic, 'revision'); continue; }
      if (payload.marketSettled?.length) { diagnostics.settlements++; markInvalid(topic, 'revision'); continue; }
      diagnostics.bookDirtyMarkers += payload.bookDirty?.length ?? 0;
      if ((payload.bookDirty ?? []).some(x => !(payload.books ?? []).some(b => b.exchangeId === x.exchangeId))) {
        diagnostics.dirtyWithoutBook++; markInvalid(topic, 'revision'); continue;
      }
      if (!state.ready) diagnostics.booksBeforeRecovery++;
      events.push(row);
    }

    const candidates = new Map();
    for (const row of events) {
      const marketId = topicMarket(row.topic);
      if (invalidated.has(null) || invalidated.has(marketId)) continue;
      const result = engineBookPointsV1(row, scope, row.observedAt);
      diagnostics.scopeRejectedBooks += result.rejected.length;
      for (const book of result.books) {
        const member = scope.exchanges.find(x => x.exchangeId === book.exchangeId);
        if (!member || member.marketId !== book.marketId) { diagnostics.scopeRejectedBooks++; continue; }
        const sequence = BigInt(book.sequence), sourceNs = instantNs(book.sourceAt);
        const old = lastBookVersion.get(book.exchangeId);
        if (old && (sequence < old.sequence || sequence === old.sequence && sourceNs <= old.sourceNs)) {
          diagnostics.duplicateBookVersions++;
          if (sequence < old.sequence) diagnostics.rollbackBookVersions++;
          continue;
        }
        lastBookVersion.set(book.exchangeId, {sequence, sourceNs});
        try {
          const point = screenComplementBookV1({
            marketId: book.marketId, exchangeId: book.exchangeId, sequence: String(book.sequence),
            availableAt: row.observedAt, sourceAt: book.sourceAt,
            nextExpiryAt: book.sourceBinding.nextExpiryAt, bids: book.bids, asks: book.asks,
          });
          const prior = candidates.get(book.exchangeId);
          if (!prior || sequence > BigInt(prior.sequence)
            || sequence === BigInt(prior.sequence) && point.sourceAt > prior.sourceAt) candidates.set(book.exchangeId, point);
        } catch { diagnostics.invalidScreenedBooks++; }
      }
    }
    for (const point of [...candidates.values()].sort((a, b) => a.exchangeId.localeCompare(b.exchangeId))) {
      observations.push(point);
      diagnostics.acceptedBookPoints++;
    }
    for (const [topic, state] of updates) topicStates.set(topic, state);
    if (invalidated.has(null)) {
      for (const [topic, state] of topicStates) topicStates.set(topic, {...state, ready: false});
    }
    group = [];
  }

  for (const raw of records) {
    const row = typeof raw === 'string' ? JSON.parse(raw) : raw;
    need(row && row.format === 'sig-feed-capture' && row.schemaVersion === 1
      && row.captureIndex === expectedIndex++, 'COMPLEMENT_CAPTURE_CHAIN');
    const observedMs = Date.parse(row.observedAt);
    need(Number.isFinite(observedMs) && observedMs >= lastObservedMs, 'COMPLEMENT_CAPTURE_CLOCK');
    lastObservedMs = observedMs;
    firstObservedAt ??= row.observedAt;
    through = row.observedAt;
    diagnostics.captureRows++;
    if (row.kind === 'config') addConfig(row);
    else {
      if (!dataStarted) { dataStarted = true; finishScope(); }
      if (row.kind !== 'event') diagnostics.nonEventRows++;
      if (row.kind === 'trade_observed') diagnostics.tradeObservedRows++;
    }
    if (groupAtMs !== null && observedMs !== groupAtMs) flushGroup();
    groupAtMs = observedMs;
    group.push(row);
  }
  finishScope();
  flushGroup();
  need(diagnostics.configRows === expectedExchangeCount, 'COMPLEMENT_CAPTURE_CONFIG_COUNT');
  need(firstObservedAt && through, 'COMPLEMENT_CAPTURE_EMPTY');

  const splitAt = firstObservedAt;
  const replay = analyzeSameMarketComplementV1({observations, breaks, options: {splitAt}});
  need(replay.diagnostics.length === 32 && replay.diagnostics.every(x => x.period === 'heldout'),
    'COMPLEMENT_CAPTURE_NOT_FULL_HOLDOUT');
  replay.holdout = 'The full sealed later capture is scored as holdout; no fitting, selection, or threshold tuning occurs.';
  return {
    tournamentId: expectedTournamentId,
    scope: {markets: new Set(scope.exchanges.map(x => x.marketId)).size,
      exchanges: scope.exchanges.length, sha256: canonicalScopeHash(scope.exchanges)},
    capture: {rows: diagnostics.captureRows, from: firstObservedAt, through},
    diagnostics,
    replay,
    oneTickAdverseStress: addOneTickStress(replay),
  };
}

async function run() {
  const localAppData = process.env.LOCALAPPDATA;
  need(typeof localAppData === 'string' && localAppData.length > 0, 'COMPLEMENT_LOCALAPPDATA');
  const source = COMPLEMENT_LATER_HOLDOUT_SOURCE_V1;
  const file = path.join(localAppData, 'SigTradingCup', 'paper-runtime', source.file);
  const before = fs.statSync(file);
  need(before.size >= source.bytes, 'COMPLEMENT_SOURCE_PREFIX_SHORT_BEFORE');
  const seal = sealedNativePrefix(file);
  need(seal.dev === before.dev && seal.ino === before.ino && seal.bytes >= source.bytes, 'COMPLEMENT_SOURCE_PREFIX_IDENTITY');
  const beforeHash = (await hashNativePrefix(file, source.bytes, 0)).hash;
  need(beforeHash === source.sha256, 'COMPLEMENT_SOURCE_HASH_BEFORE');

  const result = replayComplementHoldoutRecordsV1(journalLines(file, {endByte: source.bytes}));
  need(result.capture.rows === source.captureRows && result.capture.from === source.from
    && result.capture.through === source.through, 'COMPLEMENT_SOURCE_MANIFEST');
  need(result.scope.exchanges === source.exchangeCount && result.scope.markets === source.exchangeCount,
    'COMPLEMENT_SOURCE_SCOPE');

  const after = fs.statSync(file);
  const afterHash = (await hashNativePrefix(file, source.bytes, 0)).hash;
  const verification = verifyPinnedPrefixV1({source,before,after,beforeHash,afterHash});

  const policy = {
    ...COMPLEMENT_REPLAY_V1,
    firstLegLatenciesMs: [...COMPLEMENT_REPLAY_V1.firstLegLatenciesMs],
    hedgeLatenciesMs: [...COMPLEMENT_REPLAY_V1.hedgeLatenciesMs],
  };
  const report = {
    schemaVersion: 1,
    mode: 'same_market_exact_complement_later_capture_holdout_v1',
    generatedAt: new Date().toISOString(),
    source: {
      kind: 'sealed_local_realtime_capture', file: source.file, bytes: source.bytes,
      sha256: source.sha256, captureRows: result.capture.rows,
      from: result.capture.from, through: result.capture.through,
      tournamentId: result.tournamentId, scopedMarkets: result.scope.markets,
      scopedExchanges: result.scope.exchanges, scopeSha256: result.scope.sha256,
      verification,
    },
    frozenPolicy: {
      source: 'research/same-market-complement-arb-v1.mjs',
      unchangedFromIssue22: true,
      economics: policy,
      holdout: result.replay.holdout,
    },
    captureDiagnostics: result.diagnostics,
    observations: result.replay.observations,
    atomic: result.replay.atomic,
    sameTimestampCandidateAndFillEvidence: {
      positiveTopPairs: result.replay.atomic.positiveSameTimestampTopPairCount,
      positiveOnePercentDepthPairs: result.replay.atomic.positiveSameTimestampOnePercentDepthCount,
      note: 'Visible same-time prices are not actual fills; positive sequential pairs retain first-leg directional exposure.',
    },
    sequentialScenarios: result.replay.diagnostics,
    oneTickAdverseStress: result.oneTickAdverseStress,
    limitations: [
      'Displayed depth and sequential pairing are counterfactual; no contest orders, accepted orders, actual fills, or account data are included.',
      'The capture is one later 73-minute window with 237 related election markets; rows and contracts are not independent samples.',
      'A positive sequential pair reflects directional movement during the unhedged interval, not risk-free arbitrage.',
      'One-tick adverse stress subtracts one official 0.005 tick on each leg; it is a sensitivity, not an observed execution cost.',
    ],
    networkRequests: 0,
    accountStateRead: false,
    runtimeChanges: 0,
    contestOrders: 0,
    profitabilityEstablished: false,
  };
  const runnerBytes = fs.readFileSync(fileURLToPath(import.meta.url));
  const runnerHeader = Buffer.concat([Buffer.from('blob ' + runnerBytes.length), Buffer.from([0])]);
  report.replayCode = {
    baseMainCommit: '8fc4b3dadc5e7a48195a1c1b8e22141cdfa5bf93',
    runnerGitBlobSha1: createHash('sha1').update(runnerHeader).update(runnerBytes).digest('hex'),
    analyzerGitBlobSha1: '539fb63970795f31803ac607ab9e4d09b4b4c3ca',
    priorHoldoutDriverGitBlobSha1: '7c4a795a08d96b6acce7e4f343960b0825d8ec8c',
    prefixValidatorGitBlobSha1: 'ef4ef4c00a4a5421d23c3341711f6273c18a4254',
  };
  const target = path.resolve(OUTPUT);
  fs.writeFileSync(target, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
  const heldout = result.replay.diagnostics.map(row => ({
    firstSide: row.firstSide, firstLegLatencyMs: row.firstLegLatencyMs,
    secondLegLatencyMs: row.secondLegLatencyMs, decisions: row.decisionCount,
    completedPairs: row.completedPairs, profitablePairs: row.profitablePairCount,
    meanPairNet: row.pairNetPnlPerShare.mean,
    meanTwoTickStressNet: row.pairNetPnlPerShare.mean === null ? null : row.pairNetPnlPerShare.mean - 0.01,
    firstDelayCensored: row.firstLegBookDelayCensored,
    secondDelayCensored: row.secondLegBookDelayCensored,
    firstDepthCensored: row.firstLegDepthCensored,
    secondDepthCensored: row.secondLegDepthCensored,
    breakCensored: row.breakCensored,
  }));
  console.log(JSON.stringify({
    report: OUTPUT,
    source: report.source,
    acceptedBookPoints: result.replay.observations.accepted,
    sameTimestamp: report.sameTimestampCandidateAndFillEvidence,
    scenarios: heldout,
    networkRequests: 0,
    contestOrders: 0,
    profitabilityEstablished: false,
  }));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) await run();
