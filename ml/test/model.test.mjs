import test from 'node:test';
import assert from 'node:assert/strict';
import { buildSamples, hash, canonical, instant, splitSamples, FEATURE_NAMES } from '../core.mjs';
import { fitRidge, predict, metrics, selectModel, uncertainty, random } from '../model.mjs';
import { prepare, develop, finalEvaluation } from '../experiment.mjs';
import { config, runtime, values, dataset } from './helpers.mjs';
const prepared = () => prepare(dataset(values()), config);
test('regularized fit matches a closed form with constant remaining dimensions', () => {
  const rows = [-1, 1].map(x => ({ rowId: `${x}`, x: [x, 0, 0, 0, 0], y: 2 * x + 3 }));
  const m = fitRidge(rows, 1); assert.equal(m.intercept, 3); assert.equal(m.weights[0], 1); assert.deepEqual(m.scales, [1, 1, 1, 1, 1]); assert.equal(predict(m, [2, 0, 0, 0, 0]), 5);
});
test('constant and collinear features remain finite without unpenalized matrix singularity', () => {
  const rows = Array.from({ length: 30 }, (_, i) => ({ rowId: `${i}`, x: [i, i, 1, i * 2, 0], y: i / 10 }));
  for (const alpha of [.000001, .1, 1e6]) { const m = fitRidge(rows, alpha); for (const r of rows) assert.ok(Number.isFinite(predict(m, r.x))); }
  const m = fitRidge(rows.map(r => ({ ...r, x: [1, 1, 1, 1, 1] })), .1); assert.ok(m.weights.every(w => Math.abs(w) < 1e-12));
});
test('bad fit dimensions, missing samples and nonfinite features fail', () => {
  assert.throws(() => fitRidge([], 1));
  const rows = [{ rowId: '1', x: [1, 2, 3, 4, 5], y: 1 }, { rowId: '2', x: [1, 2, 3, 4, 5], y: 2 }];
  assert.throws(() => fitRidge(rows, 0));
  assert.throws(() => fitRidge([{ ...rows[0], x: [1] }, rows[1]], 1));
  assert.throws(() => fitRidge([{ ...rows[0], y: NaN }, rows[1]], 1));
  assert.throws(() => predict(fitRidge(rows, 1), [1])); assert.throws(() => predict(fitRidge(rows, 1), [NaN, 2, 3, 4, 5]));
  assert.throws(() => metrics([], fitRidge(rows, 1)));
});
test('scaler uses training rows exclusively and zero-change/train-prior metrics are correct', () => {
  const p = prepared(), rows = p.split.partitions.train, model = fitRidge(rows, 1);
  for (let j = 0; j < FEATURE_NAMES.length; j++) assert.ok(Math.abs(model.means[j] - rows.reduce((s, r) => s + r.x[j], 0) / rows.length) < 1e-12);
  const m = metrics([{ x: [0, 0, 0, 0, 0], y: .2 }, { x: [0, 0, 0, 0, 0], y: -.4 }], { weights: [0, 0, 0, 0, 0], means: [0, 0, 0, 0, 0], scales: [1, 1, 1, 1, 1], intercept: .1, trainingTargetMean: .1 });
  assert.ok(Math.abs(m.mae - .3) < 1e-12); assert.ok(Math.abs(m.zeroChangeMae - .3) < 1e-12); assert.equal(m.trainMeanMae, m.mae);
});
test('validation chooses candidate; ties select ascending first alpha', () => {
  const rows = [{ rowId: 'a', x: [0, 0, 0, 0, 0], y: .1 }, { rowId: 'b', x: [0, 0, 0, 0, 0], y: .1 }];
  assert.equal(selectModel(rows, rows, [.1, 1, 10]).model.alpha, .1);
});
test('test-only price poisoning cannot change preprocessing, fitted model or validation candidate scores', () => {
  const before = develop(prepared(), runtime), rows = values();
  for (const r of rows.slice(1)) if (instant(r.availableAt) >= instant(config.validationEnd)) { r.bids[0].price = .001; r.asks[0].price = .999; r.bids[0].quantity = 1e11; }
  const after = develop(prepare(dataset(rows), config), runtime);
  assert.deepEqual(before.model, after.model); assert.deepEqual(before.report.candidates, after.report.candidates); assert.deepEqual(before.report.validation, after.report.validation); assert.notEqual(before.selection.datasetSha256, after.selection.datasetSha256);
  assert.deepEqual(after.report.test, { status: 'withheld', samples: 102 }); assert.ok(!canonical(after.splitManifest.partitions.test).includes('labelMidChange'));
});
test('development and final selection require exact dataset/config/implementation/model identity', () => {
  const p = prepared(), dev = develop(p, runtime), final = finalEvaluation(p, runtime, dev.selection);
  assert.equal(final.report.test.count, 102); assert.equal(final.predictions.length, 102); assert.match(final.report.warning, /SYNTHETIC-ONLY/);
  for (const key of ['alpha', 'datasetSha256', 'configSha256', 'implementationSha256', 'modelSha256']) assert.throws(() => finalEvaluation(p, runtime, { ...dev.selection, [key]: 'tampered' }), /SELECTION_MISMATCH/);
  assert.throws(() => finalEvaluation(p, { implementationSha256: hash('changed') }, dev.selection), /SELECTION_MISMATCH/);
});
test('insufficient datasets reject before training; real mode cannot lower evidence floors', () => {
  let p = prepared(); p.config.minSamples.train = 9999; p = prepare(p.dataset, p.config); assert.throws(() => develop(p, runtime), /INSUFFICIENT_DATA/);
  const rows = values(); rows[0].dataKind = 'real'; p = prepare(dataset(rows), { ...config, minSamples: { train: 2, validation: 2, test: 2 }, minSessions: { train: 1, validation: 1, test: 1 } });
  assert.ok(p.failures.some(x => /200/.test(x))); assert.ok(p.failures.some(x => /one hour/.test(x))); assert.throws(() => develop(p, runtime), /INSUFFICIENT_DATA/);
});
test('bootstrap preserves whole sessions, is deterministic, and avoids weak intervals', () => {
  const p = prepared(), m = fitRidge(p.split.partitions.train, .1), rows = p.split.partitions.test;
  assert.deepEqual(uncertainty(rows, m, 7, 200), uncertainty(rows, m, 7, 200));
  const weak = rows.map(r => ({ ...r, sessionId: 'only-one' })); assert.equal(uncertainty(weak, m, 7, 200).maeDifference95, null);
  const rng1 = random(9), rng2 = random(9); assert.deepEqual(Array.from({ length: 100 }, rng1), Array.from({ length: 100 }, rng2));
});
test('seeded randomized information intervals never overlap across admitted partitions', () => {
  const rng = random(879); let tested = 0;
  for (let run = 0; run < 100; run++) {
    const rows = Array.from({ length: 100 }, (_, i) => {
      const start = instant('2026-10-01T00:00:00Z') + BigInt(Math.floor(rng() * 450000)) * 1000000n;
      return { rowId: `${run}-${i}`, start, featureStart: start - BigInt(Math.floor(rng() * 1500)) * 1000000n, end: start + BigInt(2000 + Math.floor(rng() * 500)) * 1000000n };
    });
    const { partitions: p } = splitSamples(rows, config);
    for (const [left, right] of [[p.train, p.validation], [p.validation, p.test], [p.train, p.test]]) for (const a of left) for (const b of right) { assert.ok(a.end < b.featureStart); tested++; }
  }
  assert.ok(tested > 100000);
});
test('adjacent binary64 features retain a genuinely unpenalized intercept', () => {
  const rows = Array.from({ length: 10 }, (_, i) => ({ rowId: `${i}`, x: [i % 2 ? .10000000000000002 : .1, 0, 0, 0, 0], y: i % 2 ? .01 : -.01 }));
  const m = fitRidge(rows, .01), zs = rows.map(r => (r.x[0] - m.means[0]) / m.scales[0]);
  const zMean = zs.reduce((a, b) => a + b, 0) / rows.length;
  const variance = zs.reduce((sum, z) => sum + (z - zMean) ** 2, 0) / rows.length;
  const covariance = zs.reduce((sum, z, i) => sum + (z - zMean) * rows[i].y, 0) / rows.length;
  const exactWeight = covariance / (variance + .01), exactIntercept = -zMean * exactWeight;
  for (let i = 0; i < rows.length; i++) assert.ok(Math.abs(predict(m, rows[i].x) - (exactIntercept + zs[i] * exactWeight)) < 1e-14);
  const averageResidual = rows.reduce((sum, r) => sum + predict(m, r.x) - r.y, 0) / rows.length;
  assert.ok(Math.abs(averageResidual) < 1e-15); assert.ok(Math.abs(m.intercept) > .005); assert.equal(m.trainingTargetMean, 0);
  const score = metrics(rows, m); assert.ok(Math.abs(score.trainMeanMae - .01) < 1e-15);
});
test('constant near-roundoff feature offsets do not change rank-deficient ridge predictions', () => {
  const rows = Array.from({ length: 10 }, (_, i) => ({ rowId: `${i}`, x: [.1, .1, .1, .1, .1], y: i / 100 }));
  for (const alpha of [.000001, .01, 1e6]) {
    const m = fitRidge(rows, alpha);
    assert.ok(m.weights.every(w => Math.abs(w) < 1e-14));
    assert.ok(rows.every(r => Math.abs(predict(m, r.x) - .045) < 1e-14));
  }
  assert.throws(() => fitRidge(rows, 0), /FIT_INPUT/);
});
