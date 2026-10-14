import test from 'node:test';
import assert from 'node:assert/strict';
import {BOOK_ALPHA_FRESH_CUTOFF, splitBookAlphaLabelsV1, verifyPinnedBookPrefixV1} from './book-alpha-fresh-holdout-v1.mjs';

test('fresh book-alpha cutoff purges labels that mature after the training boundary', () => {
  const cutoff = Date.parse(BOOK_ALPHA_FRESH_CUTOFF);
  const labels = [
    {at: cutoff - 20, labelAt: cutoff - 1, marketId: 'a'},
    {at: cutoff - 10, labelAt: cutoff + 1, marketId: 'b'},
    {at: cutoff, labelAt: cutoff + 10, marketId: 'c'},
  ];
  const split = splitBookAlphaLabelsV1(labels);
  assert.deepEqual(split.training, [labels[0]]);
  assert.deepEqual(split.purged, [labels[1]]);
  assert.deepEqual(split.holdout, [labels[2]]);
});

test('fixed cutoff does not silently move when data grows', () => {
  assert.equal(BOOK_ALPHA_FRESH_CUTOFF, '2026-10-09T00:00:00.000Z');
});

test('pinned source accepts append-only growth with identical prefix', () => {
  const source = {kind: 'sealed_public_books_prefix', bytes: 100, sha256: 'a'.repeat(64)};
  const result = verifyPinnedBookPrefixV1({source, before: {dev: 1, ino: 2, size: 100},
    after: {dev: 1, ino: 2, size: 125}, beforeHash: source.sha256, afterHash: source.sha256});
  assert.equal(result.identityStable, true);
  assert.equal(result.prefixHashStable, true);
  assert.equal(result.bytesAtEnd, 125);
});

test('pinned source rejects replacement, truncation, and prefix mutation', () => {
  const source = {kind: 'sealed_public_books_prefix', bytes: 100, sha256: 'a'.repeat(64)};
  const before = {dev: 1, ino: 2, size: 100}, after = {dev: 1, ino: 2, size: 125};
  assert.throws(() => verifyPinnedBookPrefixV1({source, before, after: {...after, ino: 3}, beforeHash: source.sha256, afterHash: source.sha256}), /SOURCE_IDENTITY/);
  assert.throws(() => verifyPinnedBookPrefixV1({source, before, after: {...after, size: 99}, beforeHash: source.sha256, afterHash: source.sha256}), /SOURCE_PREFIX_SHORT/);
  assert.throws(() => verifyPinnedBookPrefixV1({source, before, after, beforeHash: source.sha256, afterHash: 'b'.repeat(64)}), /SOURCE_PREFIX_HASH/);
});

test('invalid source hash and invalid cutoff fail closed', () => {
  const source = {kind: 'sealed_public_books_prefix', bytes: 100, sha256: 'bad'};
  assert.throws(() => verifyPinnedBookPrefixV1({source, before: {dev: 1, ino: 2, size: 100},
    after: {dev: 1, ino: 2, size: 100}, beforeHash: 'bad', afterHash: 'bad'}), /SOURCE_HASH/);
  assert.throws(() => splitBookAlphaLabelsV1([], 'not-a-date'), /CUTOFF/);
});
