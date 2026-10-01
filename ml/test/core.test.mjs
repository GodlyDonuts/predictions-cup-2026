import test from 'node:test';
import assert from 'node:assert/strict';
import { parseDataset, buildSamples, splitSamples, instant, validateConfig, canonical } from '../core.mjs';
import { config, values, encode, dataset, shortRows, at, shiftRow, barrier } from './helpers.mjs';
const build = rows => buildSamples(dataset(rows), config);
test('nanosecond calendar parser preserves ordering without float collapse', () => {
  assert.equal(instant('2026-10-01T00:00:00.000000002Z') - instant('2026-10-01T00:00:00.000000001Z'), 1n);
  assert.equal(instant('2026-10-01T00:00:00Z'), instant(at(0)));
  for (const x of ['2026-02-30T00:00:00Z', '2026-01-01T24:00:00Z', '2025-02-29T00:00:00Z', '2026-01-01T00:00:00+01:00', '2026-01-01T00:00:00.0000000001Z', null]) assert.throws(() => instant(x), /TIMESTAMP/);
});
test('strict data/config contracts reject unknown formats, keys and incomplete JSONL', () => {
  assert.throws(() => parseDataset(encode(shortRows()).trimEnd()), /DATASET/);
  assert.throws(() => parseDataset('{bad}\n'), /JSON/);
  for (const mutate of [v => v[0].format = 'sig-feed-capture', v => v[0].provenance.sourceSha256 = 'fake', v => v[1].token = 'unwanted', v => v[1].rowId = 'bad id', v => v[1].quality.expiryKnown = null, v => v[1].sequence = -1, v => v[1].nextExpiryAt = 'tomorrow', v => v[1].bids = [], v => v[1].bids[0].quantity = 0, v => v[1].asks[0].price = -1, v => v[1].bids[0].price = 1, v => v[1].bids.push(v[1].bids[0]), v => v[1].kind = 'trade', v => v[1].receivedAt = at(9999), v => v[2].availableAt = at(0)]) { const rows = shortRows(); mutate(rows); assert.throws(() => dataset(rows)); }
  for (const change of [{ embargoMs: 0 }, { alphas: [1, .1] }, { alphas: [0] }, { toleranceMs: 2001 }, { horizonMs: Infinity }, { seed: -1 }, { validationEnd: config.trainEnd }, { extra: true }]) assert.throws(() => validateConfig({ ...config, ...change }));
});
test('known features use current levels and only prior observations', () => {
  const rows = shortRows(5);
  rows[1].bids = [{ price: .4, quantity: 100 }]; rows[1].asks = [{ price: .6, quantity: 100 }];
  rows[2].bids = [{ price: .45, quantity: 300 }]; rows[2].asks = [{ price: .55, quantity: 100 }];
  const sample = build(rows).samples[0];
  for (const [a, b] of sample.x.map((x, i) => [x, [.1, .5, .025, 0, .5][i]])) assert.ok(Math.abs(a - b) < 1e-12);
  const original = [...sample.x]; rows[4].bids[0].price -= .05; rows[4].asks[0].price -= .05;
  const changed = build(rows).samples[0]; assert.deepEqual(changed.x, original); assert.notEqual(changed.y, sample.y);
});
test('horizon uses first future availability at/after target with bounded tolerance', () => {
  const rows = shortRows(8); shiftRow(rows[4], 3200); const r = build(rows).samples[0];
  assert.equal(r.rowId, rows[2].rowId); assert.equal(r.labelRowId, rows[4].rowId); assert.equal(r.end - r.start, 2200000000n);
  shiftRow(rows[4], 3600); const invalid = build(rows); assert.ok(!invalid.samples.some(s => s.rowId === rows[2].rowId));
});
test('source/arrival/expiry quality exclusions invalidate remainder of the segment', () => {
  for (const mutate of [r => r.sourceAt = at(9000), r => r.sourceAt = at(0), r => r.nextExpiryAt = r.availableAt, r => r.quality.continuous = false, r => r.quality.stale = true, r => r.quality.recovering = true, r => r.quality.expiryKnown = false]) {
    const rows = shortRows(); mutate(rows[5]);
    // Source rollback is a hard dataset error, not a usable row.
    if (rows[5].sourceAt === at(0) || rows[5].sourceAt === at(9000)) { assert.throws(() => build(rows), /VERSION_ROLLBACK/); continue; }
    const out = build(rows); assert.ok(out.excluded.some(x => x.rowId === rows[5].rowId)); assert.ok(!out.samples.some(x => x.end >= instant(rows[5].availableAt)));
  }
});
test('processing delay is part of freshness, and future source clock fails closed', () => {
  let rows = shortRows(1); rows[1].availableAt = at(2000); assert.equal(build(rows).coverage.reasons['source-stale'], 1);
  rows = shortRows(1); rows[1].sourceAt = at(30); assert.equal(build(rows).coverage.reasons['source-clock-future'], 1);
});
test('gap barrier blocks labels and needs a new explicit continuity segment', () => {
  const rows = shortRows(); rows[5] = barrier(rows[5]);
  const blocked = build(rows); assert.ok(blocked.coverage.reasons['segment-blocked'] > 0); assert.ok(!blocked.samples.some(s => s.start < instant(rows[5].availableAt) && s.end >= instant(rows[5].availableAt)));
  for (const r of rows.slice(6)) r.segmentId = 'recovered';
  const recovered = build(rows); assert.ok(recovered.samples.some(s => s.rowId === rows[7].rowId)); assert.ok(!recovered.samples.some(s => s.rowId === rows[6].rowId));
});
test('unmarked timing/expiry gaps never bridge forward labels or past features', () => {
  for (const expiry of [false, true]) {
    const rows = shortRows();
    if (expiry) rows[4].nextExpiryAt = at(3500); else for (let i = 5; i < rows.length; i++) shiftRow(rows[i], (i - 1) * 1000 + 2000);
    const out = build(rows); assert.equal(out.coverage.reasons['unmarked-regime-gap'], 1); assert.ok(!out.samples.some(s => s.labelRowId === rows[5].rowId));
  }
});
test('exact repeated versions are excluded without freshness renewal', () => {
  const rows = shortRows(); const repeated = structuredClone(rows[3]); repeated.rowId = 'repeat'; repeated.receivedAt = at(2510); repeated.availableAt = at(2520); rows.splice(4, 0, repeated);
  const out = build(rows); assert.equal(out.coverage.reasons['duplicate-version'], 1); assert.ok(!out.samples.some(s => s.rowId === 'repeat' || s.labelRowId === 'repeat'));
});
test('duplicate row IDs/equal-version conflicts/rollback fail', () => {
  let rows = shortRows(); rows[2].rowId = rows[1].rowId; assert.throws(() => build(rows), /DUPLICATE_ROW/);
  rows = shortRows(); rows[2].sequence = rows[1].sequence; rows[2].sourceAt = rows[1].sourceAt; assert.throws(() => build(rows), /CONFLICTING_BOOK/);
  rows = shortRows(); rows[4].sequence = 0; assert.throws(() => build(rows), /ROLLBACK/);
  rows = shortRows(); rows[4].sequence = 0; rows[4].segmentId = 'new-segment'; assert.throws(() => build(rows), /ROLLBACK/);
});
test('segment reuse and session interleaving cannot launder invalidations', () => {
  let rows = shortRows(); rows[3].segmentId = 'other'; assert.throws(() => build(rows), /SEGMENT_REUSED/);
  rows = shortRows(); rows[3] = { ...barrier(rows[3]), sessionId: 'other' }; assert.throws(() => build(rows), /SESSION_RESUMED/);
  rows = shortRows(); rows[3].availableAt = rows[2].availableAt; rows[3].receivedAt = rows[2].receivedAt; assert.throws(() => build(rows), /AMBIGUOUS/);
});
test('separate instruments at same time stay distinct and never cross-label', () => {
  const input = shortRows(8), other = input.slice(1).map(r => ({ ...structuredClone(r), rowId: `${r.rowId}-b`, marketId: 'other-market', exchangeId: 'other-exchange' }));
  const rows = [input[0], ...input.slice(1).flatMap((r, i) => [r, other[i]])];
  const out = build(rows); assert.equal(out.samples.length, 10); for (const s of out.samples) assert.equal(s.rowId.endsWith('-b'), s.labelRowId.endsWith('-b'));
});
test('actual label and past feature information intervals are strictly separated', () => {
  const out = build(values()), { partitions, excluded } = splitSamples(out.samples, config), e = BigInt(config.embargoMs) * 1000000n;
  const t = instant(config.trainEnd), v = instant(config.validationEnd);
  assert.ok(partitions.train.every(s => s.end < t - e));
  assert.ok(partitions.validation.every(s => s.featureStart >= t + e && s.end < v - e));
  assert.ok(partitions.test.every(s => s.featureStart >= v + e)); assert.ok(excluded.length > 0);
  assert.equal(out.coverage.acceptedBooks, out.coverage.warmup + out.coverage.labeled + out.coverage.missingLabels);
});
test('purge equality, embargo and feature equality bounds are explicit', () => {
  const t = instant(config.trainEnd), v = instant(config.validationEnd), e = BigInt(config.embargoMs) * 1000000n;
  const make = (rowId, start, featureStart, end) => ({ rowId, start, featureStart, end });
  const rows = [make('label-equal', t - e - 100n, t - e - 200n, t - e), make('embargo-low', t - e, t - e - 1n, t), make('feature-before', t + e + 100n, t + e - 1n, t + e + 200n), make('feature-equal', t + e + 100n, t + e, t + e + 200n), make('test', v + e + 1n, v + e, v + e + 2n)];
  const result = splitSamples(rows, config); assert.deepEqual(result.excluded.map(x => x.reason), ['label-purge', 'embargo', 'feature-purge']); assert.equal(result.partitions.validation[0].rowId, 'feature-equal'); assert.equal(result.partitions.test[0].rowId, 'test');
});
test('canonical hashes are object-key order invariant', () => assert.equal(canonical({ b: 1, a: { d: 3, c: 2 } }), canonical({ a: { c: 2, d: 3 }, b: 1 })));
test('composite versions permit equal sequence with a newer exact source timestamp', () => {
  const rows = shortRows(); for (const r of rows.slice(1)) r.sequence = 100;
  const out = build(rows); assert.equal(out.coverage.acceptedBooks, rows.length - 1); assert.ok(out.samples.length > 0);
});
test('source watermark survives bad quality and subsequent segment changes', () => {
  const rows = shortRows(); rows[4].quality.continuous = false;
  for (const r of rows.slice(5)) r.segmentId = 'recovered'; rows[5].sequence = 1;
  assert.throws(() => build(rows), /VERSION_ROLLBACK/);
});
test('barriers at ambiguous same-instrument timestamps reject the dataset', () => {
  const rows = shortRows(); rows[4] = { ...barrier(rows[4]), availableAt: rows[3].availableAt, receivedAt: rows[3].receivedAt };
  assert.throws(() => build(rows), /AMBIGUOUS_INSTRUMENT_TIME/);
});
