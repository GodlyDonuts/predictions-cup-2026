import { FEATURE_NAMES, hash, canonical } from './core.mjs';
const finite = xs => { if (xs.some(x => !Number.isFinite(x))) throw new Error('NONFINITE_MODEL'); };
const mean = xs => {
  let sum = 0, correction = 0;
  for (const x of xs) { const adjusted = x - correction, next = sum + adjusted; correction = (next - sum) - adjusted; sum = next; }
  return sum / xs.length;
};
/** Train-only standardization, centered target, unpenalized intercept. Objective:
 * mean((y - intercept - z*w)^2) + alpha * sum(w^2). */
export function fitRidge(samples, alpha) {
  if (!Array.isArray(samples) || samples.length < 2 || !Number.isFinite(alpha) || alpha <= 0) throw new Error('FIT_INPUT');
  const d = FEATURE_NAMES.length;
  for (const s of samples) { if (s.x.length !== d) throw new Error('FEATURE_DIMENSION'); finite([...s.x, s.y]); }
  const means = Array.from({ length: d }, (_, j) => mean(samples.map(s => s.x[j])));
  const scales = means.map((m, j) => Math.sqrt(mean(samples.map(s => (s.x[j] - m) ** 2))) || 1);
  const trainingTargetMean = mean(samples.map(s => s.y));
  const normalized = samples.map(s => s.x.map((x, j) => (x - means[j]) / scales[j]));
  // Rounded feature means need not make floating-point standardized columns
  // zero-mean, particularly for adjacent representable near-constant prices.
  // Center the actual transformed data and restore its offset in the intercept.
  const standardizedMeans = means.map((_, j) => mean(normalized.map(z => z[j])));
  const matrix = Array.from({ length: d }, () => Array(d + 1).fill(0));
  for (let i = 0; i < samples.length; i++) {
    const s = samples[i], z = normalized[i].map((x, j) => x - standardizedMeans[j]);
    for (let j = 0; j < d; j++) {
      matrix[j][d] += z[j] * (s.y - trainingTargetMean) / samples.length;
      for (let k = 0; k < d; k++) matrix[j][k] += z[j] * z[k] / samples.length;
    }
  }
  for (let j = 0; j < d; j++) matrix[j][j] += alpha;
  // Partial-pivot Gaussian elimination of tiny (5x5) positive regularized system.
  for (let j = 0; j < d; j++) {
    let pivot = j;
    for (let k = j + 1; k < d; k++) if (Math.abs(matrix[k][j]) > Math.abs(matrix[pivot][j])) pivot = k;
    [matrix[j], matrix[pivot]] = [matrix[pivot], matrix[j]];
    if (Math.abs(matrix[j][j]) < 1e-14) throw new Error('ILL_CONDITIONED_MODEL');
    const v = matrix[j][j];
    for (let k = j; k <= d; k++) matrix[j][k] /= v;
    for (let row = 0; row < d; row++) if (row !== j) {
      const f = matrix[row][j];
      for (let k = j; k <= d; k++) matrix[row][k] -= f * matrix[j][k];
    }
  }
  const weights = matrix.map(row => row[d]);
  const intercept = trainingTargetMean - standardizedMeans.reduce((sum, m, j) => sum + m * weights[j], 0);
  finite([...means, ...scales, ...standardizedMeans, trainingTargetMean, intercept, ...weights]);
  return { family: 'ridge-forward-mid-change', featureNames: FEATURE_NAMES, alpha, means, scales, standardizedMeans, trainingTargetMean, intercept, weights, trainCount: samples.length, trainingRowsSha256: hash(canonical(samples.map(s => ({ rowId: s.rowId, x: s.x, y: s.y })))) };
}
export function predict(model, x) {
  if (x.length !== model.weights.length) throw new Error('FEATURE_DIMENSION');
  const y = model.intercept + x.reduce((s, v, j) => s + ((v - model.means[j]) / model.scales[j]) * model.weights[j], 0);
  finite([y]); return y;
}
export function metrics(samples, model) {
  if (!samples.length) throw new Error('NO_EVALUATION_SAMPLES');
  finite([model.trainingTargetMean]);
  const pairs = samples.map(s => [predict(model, s.x), s.y]);
  return { count: pairs.length, mae: mean(pairs.map(([p, y]) => Math.abs(p - y))), zeroChangeMae: mean(pairs.map(([, y]) => Math.abs(y))), trainMeanMae: mean(pairs.map(([, y]) => Math.abs(model.trainingTargetMean - y))), rmse: Math.sqrt(mean(pairs.map(([p, y]) => (p - y) ** 2))), meanTarget: mean(pairs.map(([, y]) => y)), units: 'YES price change, range of book midprice [0,1]' };
}
export function selectModel(train, validation, alphas) {
  const candidates = alphas.map(alpha => { const model = fitRidge(train, alpha); return { alpha, validation: metrics(validation, model), model }; });
  let chosen = candidates[0];
  // Deterministic tie: smaller alpha because config requires ascending candidates.
  for (const candidate of candidates.slice(1)) if (candidate.validation.mae < chosen.validation.mae) chosen = candidate;
  return { model: chosen.model, candidates: candidates.map(({ model, ...rest }) => rest) };
}
export function random(seed) {
  let state = seed >>> 0;
  return () => { state = (Math.imul(1664525, state) + 1013904223) >>> 0; return state / 4294967296; };
}
/** Resample whole sessions, preserving within-session serial/cross-market dependence.
 * Descriptive only: different sessions can still be dependent. */
export function uncertainty(samples, model, seed, replicates) {
  const sessions = new Map();
  for (const s of samples) {
    if (!sessions.has(s.sessionId)) sessions.set(s.sessionId, []);
    sessions.get(s.sessionId).push(Math.abs(predict(model, s.x) - s.y) - Math.abs(s.y));
  }
  const clusters = [...sessions.values()];
  if (clusters.length < 5) return { method: 'whole-session cluster bootstrap', sessionCount: clusters.length, maeDifference95: null, reason: 'Fewer than five sessions; no defensible uncertainty interval supplied' };
  const rng = random(seed), draws = [];
  for (let b = 0; b < replicates; b++) {
    let sum = 0, n = 0;
    for (let j = 0; j < clusters.length; j++) { const xs = clusters[Math.floor(rng() * clusters.length)]; sum += xs.reduce((a, x) => a + x, 0); n += xs.length; }
    draws.push(sum / n);
  }
  draws.sort((a, b) => a - b);
  return { method: 'whole-session cluster bootstrap, percentile, ridge MAE minus zero-change MAE', sessionCount: clusters.length, seed, replicates, maeDifference95: [draws[Math.floor(replicates * .025)], draws[Math.min(replicates - 1, Math.floor(replicates * .975))]], caveat: 'Descriptive only; assumes sessions represent independent blocks. Overlapping labels and common regimes reduce effective sample size. Not a significance or profitability claim.' };
}
