import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, rmSync, writeFileSync, appendFileSync} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {
  MARKET_EDGE_RIDGE_BANK_PROTOCOL_V1,
  buildFixedRidgeBankV1,
  marketBatchAdmissibleV1,
  marketBatchEpochMatchesStateV1,
  recoveryStartedStateUpdateV1,
  recoveryFinishedStateUpdateV1,
  subscribedStateUpdateV1,
  verifyCaptureSourceAfterV1,
  verifyCaptureSourceV1,
} from './market-edge-ridge-bank-v1.mjs';
import {MARKET_EDGE_RIDGE_PROTOCOL} from './market-edge-ridge-v1.mjs';
import {fitMarketEdgeRidgeV1} from './market-edge-ridge-v1.mjs';

function hash(text) { return createHash('sha256').update(text).digest('hex'); }
function iso(ms) { return new Date(ms).toISOString(); }
function candidate(captureId, startMs, index, shift) {
  const decisionMs = startMs + 20_000 + index * 500;
  const features = Array.from({length: 11}, (_, j) => shift + index * (j + 1) / 1000 + j / 20);
  const segment = 7;
  const provenance = (observedMs, captureIndex) => ({
    captureId, captureIndex, observedMs, sourceMs: observedMs - 20, segment,
  });
  const entryObservedMs = decisionMs + 3_000;
  const exitObservedMs = entryObservedMs + 5_000;
  return {
    marketId: 'm' + String(index % 4), side: index % 2 ? 'YES' : 'NO',
    decisionMs, features, decisionProvenance: provenance(decisionMs, index),
    label: {
      net: 0.01 + shift * 0.02 + (index % 9) * 0.0002,
      entryObservedMs, exitObservedMs,
      decisionProvenance: provenance(decisionMs, index),
      entryProvenance: provenance(entryObservedMs, index + 1),
      exitProvenance: provenance(exitObservedMs, index + 2),
      decisionSegment: segment, entrySegment: segment, exitSegment: segment,
    },
  };
}
function capture(id, startMs, shift) {
  const candidates = Array.from({length: 32}, (_, i) => candidate(id, startMs, i, shift));
  const from = iso(startMs), through = iso(startMs + 60_000);
  return {
    capture: {
      id, role: 'training_only', file: id + '.jsonl', bytes: 100, sha256: id.padEnd(64, '0').slice(0, 64),
      captureRows: 10_000, configRows: 237, markets: 237, exchanges: 237, from, through,
    },
    candidates,
    diagnostics: {
      acceptedBookPoints: 200, validRecoveryCompletions: 100,
      booksBeforeRecovery: 0, lifecycleInvalidations: 5, revisionGaps: 2,
      explicitBreakEvents: 9,
    },
    labelProvenance: {crossSegmentLabelCount: 0, missingCaptureProvenanceCount: 0},
  };
}

test('bank protocol reuses the fixed ridge settings and exactly three training sources', () => {
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.ridgeAlpha, 0.01);
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.decisionLatencyMs, 3_000);
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.holdingHorizonMs, 5_000);
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.feesPerShare, 0);
  assert.equal(MARKET_EDGE_RIDGE_PROTOCOL.positivePredictionThreshold, 0);
  assert.deepEqual(MARKET_EDGE_RIDGE_BANK_PROTOCOL_V1.trainingSourceIds, [
    'train-d53812dd', 'train-0942-14ef0be1', 'train-1328-a7674ae9',
  ]);
});

test('a stale prior-epoch recovery completion cannot overwrite current readiness', () => {
  const current = {epoch: 9, ready: false};
  const stale = recoveryFinishedStateUpdateV1(current, 8, true);
  assert.equal(stale.status, 'stale_or_unmatched');
  assert.deepEqual(stale.state, current);
  const matching = recoveryFinishedStateUpdateV1(current, 9, true);
  assert.equal(matching.status, 'applied');
  assert.deepEqual(matching.state, {epoch: 9, ready: true});
  assert.equal(recoveryFinishedStateUpdateV1(null, 1, true).status, 'stale_or_unmatched');
});

test('subscription and recovery state transitions reject null, missing, or non-integer epochs', () => {
  for (const epoch of [null, undefined, -1, 1.5, Number.NaN, Number.POSITIVE_INFINITY, Number.MAX_SAFE_INTEGER + 1]) {
    const subscription = subscribedStateUpdateV1(epoch);
    assert.equal(subscription.status, 'malformed_epoch');
    assert.deepEqual(subscription.state, {epoch: null, ready: false});

    const prior = {epoch: 9, ready: false};
    const completion = recoveryFinishedStateUpdateV1(prior, epoch, true);
    assert.equal(completion.status, 'malformed_epoch');
    assert.deepEqual(completion.state, prior);
    assert.notEqual(completion.state.ready, true);

    const epochlessPrior = {epoch, ready: false};
    const completionWithoutPriorEpoch = recoveryFinishedStateUpdateV1(epochlessPrior, epoch, true);
    assert.equal(completionWithoutPriorEpoch.status, 'malformed_epoch');
    assert.deepEqual(completionWithoutPriorEpoch.state, epochlessPrior);
    assert.notEqual(completionWithoutPriorEpoch.state.ready, true);

    assert.equal(recoveryStartedStateUpdateV1(prior, epoch).status, 'malformed_epoch');
  }
  assert.equal(subscribedStateUpdateV1(9).status, 'applied');
  assert.equal(recoveryStartedStateUpdateV1({epoch: 9, ready: true}, 9).state.ready, false);
});

test('epochless or malformed market batches cannot pass the recovery admission gate', () => {
  for (const epoch of [null, undefined, 1.5, Number.NaN, Number.POSITIVE_INFINITY]) {
    assert.equal(marketBatchEpochMatchesStateV1(epoch, epoch), false);
    assert.equal(marketBatchAdmissibleV1({ready: true, invalidated: false,
      stateEpoch: epoch, eventEpoch: epoch}), false);
  }
  assert.equal(marketBatchAdmissibleV1({ready: true, invalidated: false, stateEpoch: 9, eventEpoch: 9}), true);
  assert.equal(marketBatchAdmissibleV1({ready: false, invalidated: false, stateEpoch: 9, eventEpoch: 9}), false);
  assert.equal(marketBatchAdmissibleV1({ready: true, invalidated: true, stateEpoch: 9, eventEpoch: 9}), false);
  assert.equal(marketBatchAdmissibleV1({ready: true, invalidated: false, stateEpoch: 9, eventEpoch: 8}), false);
});

test('prefix is pinned separately from an appended tail and verified after parsing', async () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'ridge-bank-'));
  try {
    const file = path.join(dir, 'capture.jsonl');
    const prefix = 'row-one\n', tail = 'later-tail';
    const contents = prefix + tail;
    writeFileSync(file, contents);
    const spec = {
      id: 'synthetic', file: 'capture.jsonl', bytes: Buffer.byteLength(prefix),
      sha256: hash(prefix), backingBytes: Buffer.byteLength(contents), backingSha256: hash(contents),
    };
    const pinned = await verifyCaptureSourceV1(spec, file);
    assert.equal(pinned.frozenBytes, Buffer.byteLength(prefix));
    assert.equal(pinned.excludedTailBytes, Buffer.byteLength(tail));
    const after = await verifyCaptureSourceAfterV1(spec, pinned);
    assert.equal(after.identityStable, true);
    assert.equal(after.frozenSha256, hash(prefix));
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});

test('source mutation between pin and postcheck fails closed', async () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'ridge-bank-mut-'));
  try {
    const file = path.join(dir, 'capture.jsonl');
    writeFileSync(file, 'stable\n');
    const contents = 'stable\n';
    const spec = {
      id: 'synthetic', file: 'capture.jsonl', bytes: Buffer.byteLength(contents),
      sha256: hash(contents), backingBytes: Buffer.byteLength(contents), backingSha256: hash(contents),
    };
    const pinned = await verifyCaptureSourceV1(spec, file);
    appendFileSync(file, 'changed');
    await assert.rejects(verifyCaptureSourceAfterV1(spec, pinned), /RIDGE_BANK_SOURCE_IDENTITY_CHANGED/);
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});

test('fixed bank fits only labeled, capture-local data and freezes a deterministic final identity', () => {
  const origin = Date.parse('2026-10-13T02:55:28.956Z');
  const captures = [
    capture('train-d53812dd', origin, 0),
    capture('train-0942-14ef0be1', origin + 7 * 60 * 60_000, 0.4),
    capture('train-1328-a7674ae9', origin + 11 * 60 * 60_000, -0.2),
  ];
  const first = buildFixedRidgeBankV1(captures);
  const second = buildFixedRidgeBankV1(captures);
  assert.equal(first.finalModel.trainingRows, 96);
  assert.equal(first.expandingWalkForward.length, 2);
  assert.equal(first.expandingWalkForward[0].trainingRows, 32);
  assert.equal(first.expandingWalkForward[0].scoredCaptureId, 'train-0942-14ef0be1');
  assert.equal(first.expandingWalkForward[1].trainingRows, 64);
  assert.equal(first.expandingWalkForward[1].scoredCaptureId, 'train-1328-a7674ae9');
  assert.equal(first.futureTest.frozenCutoffObservedAt, captures[2].capture.through);
  assert.equal(first.profitabilityEstablished, false);
  assert.equal(first.frozenBankSha256, second.frozenBankSha256);
  assert.equal(first.modelIdentityHash, second.modelIdentityHash);
});

test('ridge normalization is computed from the explicit training rows only', () => {
  const origin = Date.parse('2026-10-13T02:55:28.956Z');
  const training = capture('train-d53812dd', origin, 0).candidates;
  const later = capture('train-0942-14ef0be1', origin + 7 * 60 * 60_000, 50).candidates;
  const trainModel = fitMarketEdgeRidgeV1(training);
  const pooledModel = fitMarketEdgeRidgeV1([...training, ...later]);
  const expectedMean = Array.from({length: 11}, (_, j) =>
    training.reduce((sum, row) => sum + row.features[j], 0) / training.length);
  assert.equal(trainModel.mean.length, expectedMean.length);
  assert.ok(trainModel.mean.every((value, i) => Math.abs(value - expectedMean[i]) < 1e-12));
  assert.notDeepEqual(trainModel.mean, pooledModel.mean);
});

test('labels whose decision, entry, and exit do not share one capture are rejected', () => {
  const origin = Date.parse('2026-10-13T02:55:28.956Z');
  const captures = [
    capture('train-d53812dd', origin, 0),
    capture('train-0942-14ef0be1', origin + 7 * 60 * 60_000, 0.4),
    capture('train-1328-a7674ae9', origin + 11 * 60 * 60_000, -0.2),
  ];
  captures[0].candidates[0].label.exitProvenance.captureId = captures[1].capture.id;
  assert.throws(() => buildFixedRidgeBankV1(captures), /RIDGE_BANK_CROSS_CAPTURE_LABEL/);
});

