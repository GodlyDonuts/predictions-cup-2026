import test from 'node:test';
import assert from 'node:assert/strict';
import {
  COMPLEMENT_LATER_HOLDOUT_SOURCE_V1, verifyPinnedPrefixV1,
  replayComplementHoldoutRecordsV1,
} from './run-complement-arb-later-holdout-20261014.mjs';

const tournamentId = 'bda92870-621e-47b0-bc3c-3602c5c26f55';
const topic = `tournament:${tournamentId}:market:1`;
const base = Date.parse('2026-10-13T05:00:00.000Z');
const at = ms => new Date(base + ms).toISOString();

function records({invalidationAt = null, duplicateConfig = false} = {}) {
  let captureIndex = 0;
  const rows = [];
  const push = row => rows.push({format: 'sig-feed-capture', schemaVersion: 1,
    captureIndex: captureIndex++, observedAt: at(0), ...row});
  const config = () => push({kind: 'config', topic, scope: {
    tournamentId, marketId: '1', exchangeIds: ['2'], topic,
  }});
  config();
  if (duplicateConfig) config();
  push({kind: 'subscribed', topic, epoch: 1, observedAt: at(0)});
  push({kind: 'recovery_finished', topic, epoch: 1, booksReady: true, observedAt: at(1)});
  for (const [index, ms] of [2, 1002, 2002, 3002].entries()) {
    if (invalidationAt === ms) push({kind: 'invalidation', topic, observedAt: at(ms), epoch: 1});
    const observedAt = at(ms);
    push({kind: 'event', event: 'market_batch', topic, epoch: 1, observedAt,
      receivedAt: observedAt,
      payload: {
        delivery: {model: 'best-effort-authoritative-resync', revision: index + 1,
          previousRevision: index},
        resyncRequired: false, marketSettled: [], bookDirty: [],
        books: [{exchangeId: '2', asOf: {sequence: index + 1, at: observedAt},
          nextExpiryAt: at(120_000), expiryKnown: true,
          bids: [{price: 0.4, quantity: 1000}],
          asks: [{price: 0.42, quantity: 1000}],
        }],
      },
    });
  }
  return rows;
}

test('sealed source manifest pins the later capture bytes and digest', () => {
  assert.equal(COMPLEMENT_LATER_HOLDOUT_SOURCE_V1.bytes, 382_415_207);
  assert.equal(COMPLEMENT_LATER_HOLDOUT_SOURCE_V1.sha256,
    '799ea868d21e820860208632d0e6cb4dca96d744da06fe92e0866e06b757f631');
  assert.equal(COMPLEMENT_LATER_HOLDOUT_SOURCE_V1.captureRows, 101_853);
});

test('parser glue runs the frozen policy as full-capture holdout and reports one-tick stress', () => {
  const result = replayComplementHoldoutRecordsV1(records(), {expectedExchangeCount: 1});
  assert.equal(result.capture.rows, 7);
  assert.equal(result.scope.markets, 1);
  assert.equal(result.diagnostics.acceptedBookPoints, 4);
  assert.equal(result.replay.diagnostics.length, 32);
  assert.ok(result.replay.diagnostics.every(row => row.period === 'heldout'));
  assert.equal(result.oneTickAdverseStress.perLegAdverseExecution, '0.005000');
  assert.equal(result.oneTickAdverseStress.pairAdverseExecution, '0.010000');
  assert.equal(result.replay.atomic.positiveSameTimestampTopPairCount, 0);
});

test('an invalidation in the entry interval censors the frozen pair scenario', () => {
  const result = replayComplementHoldoutRecordsV1(records({invalidationAt: 1002}), {
    expectedExchangeCount: 1,
  });
  const zeroLatencyYes = result.replay.diagnostics.find(row => row.firstSide === 'YES'
    && row.firstLegLatencyMs === 0 && row.secondLegLatencyMs === 0);
  assert.ok(zeroLatencyYes.breakCensored > 0);
});

test('scope duplication and broken capture indices fail closed', () => {
  assert.throws(() => replayComplementHoldoutRecordsV1(records({duplicateConfig: true}), {
    expectedExchangeCount: 1,
  }), /COMPLEMENT_CAPTURE_CONFIG_DUPLICATE/);
  const broken = records();
  broken[4].captureIndex++;
  assert.throws(() => replayComplementHoldoutRecordsV1(broken, {expectedExchangeCount: 1}),
    /COMPLEMENT_CAPTURE_CHAIN/);
});

test('pinned append-only prefix remains valid as the capture grows', () => {
  const source = {bytes:100,sha256:'a'.repeat(64)};
  const before = {dev:1,ino:2,size:100};
  const after = {dev:1,ino:2,size:140};
  const proof = verifyPinnedPrefixV1({source,before,after,beforeHash:source.sha256,afterHash:source.sha256});
  assert.equal(proof.prefixBytes,100);
  assert.equal(proof.afterBytes,140);
  assert.equal(proof.prefixHashStable,true);
});

test('pinned append-only prefix rejects replacement, truncation, or mutation', () => {
  const source = {bytes:100,sha256:'a'.repeat(64)};
  const before = {dev:1,ino:2,size:100};
  const goodAfter = {dev:1,ino:2,size:140};
  assert.throws(() => verifyPinnedPrefixV1({source,before,after:{...goodAfter,ino:3},beforeHash:source.sha256,afterHash:source.sha256}), /COMPLEMENT_SOURCE_IDENTITY_CHANGED/);
  assert.throws(() => verifyPinnedPrefixV1({source,before,after:{...goodAfter,size:99},beforeHash:source.sha256,afterHash:source.sha256}), /COMPLEMENT_SOURCE_PREFIX_SHORT/);
  assert.throws(() => verifyPinnedPrefixV1({source,before,after:goodAfter,beforeHash:source.sha256,afterHash:'b'.repeat(64)}), /COMPLEMENT_SOURCE_HASH_AFTER/);
});
