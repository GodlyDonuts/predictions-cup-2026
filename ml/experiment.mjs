import { buildSamples, splitSamples, validateConfig, canonical, hash, publicSample, WARNING } from './core.mjs';
import { selectModel, metrics, uncertainty, predict } from './model.mjs';

export function prepare(dataset, config) {
  const c = validateConfig(config), built = buildSamples(dataset, c), split = splitSamples(built.samples, c);
  const counts = Object.fromEntries(Object.entries(split.partitions).map(([p, rows]) => [p, { samples: rows.length, sessions: new Set(rows.map(r => r.sessionId)).size, instruments: new Set(rows.map(r => `${r.tournamentId}/${r.marketId}/${r.exchangeId}`)).size }]));
  const failures = [];
  for (const p of ['train', 'validation', 'test']) {
    const real = dataset.header.dataKind === 'real';
    const minSamples = Math.max(c.minSamples[p], real ? (p === 'train' ? 200 : 100) : 2), minSessions = Math.max(c.minSessions[p], real ? 5 : 1);
    if (counts[p].samples < minSamples || counts[p].sessions < minSessions) failures.push(`${p}: need >=${minSamples} samples and >=${minSessions} sessions; have ${counts[p].samples}/${counts[p].sessions}`);
    const rows = split.partitions[p];
    if (real && rows.length && rows.at(-1).start - rows[0].start < 3600000000000n) failures.push(`${p}: real-data window shorter than one hour`);
  }
  return { dataset, config: c, built, split, counts, failures };
}
export function develop(prepared, runtime) {
  const { dataset, config, built, split, counts, failures } = prepared;
  if (failures.length) { const error = new Error(`INSUFFICIENT_DATA: ${failures.join('; ')}`); error.summary = { status: 'rejected-insufficient-data', dataKind: dataset.header.dataKind, counts, coverage: built.coverage, failures }; throw error; }
  const { train, validation } = split.partitions;
  const selected = selectModel(train, validation, config.alphas);
  const fingerprint = { datasetSha256: dataset.sourceHash, configSha256: hash(canonical(config)), implementationSha256: runtime.implementationSha256 };
  const selection = { schemaVersion: 1, format: 'sig-ridge-selection', ...fingerprint, alpha: selected.model.alpha, modelSha256: hash(canonical(selected.model)), selectionBasis: 'minimum validation MAE; ascending-alpha tie break; test withheld' };
  const report = {
    schemaVersion: 1, status: 'develop-test-withheld', dataKind: dataset.header.dataKind,
    warning: dataset.header.dataKind === 'synthetic' ? WARNING : 'Offline forward-price research only. Passing the data-size gate does not establish representativeness, source truth, profitability or live suitability.',
    source: { ...dataset.header, sha256: dataset.sourceHash }, runtime, config, fingerprint,
    counts, coverage: built.coverage, splitExcluded: split.excluded.length,
    splitRules: 'Global availability-time cuts. Symmetric embargo. Actual future label end is strictly before next cut minus embargo. Past feature starts at or after prior cut plus embargo. No random split.',
    model: selected.model, candidates: selected.candidates,
    train: metrics(train, selected.model), validation: { ...metrics(validation, selected.model), uncertainty: uncertainty(validation, selected.model, config.seed, config.bootstrapReplicates) },
    test: { status: 'withheld', samples: counts.test.samples },
    limitations: ['No capture-to-dataset adapter: normalized quality and source provenance must be audited separately.', 'Source prices include own displayed orders; no aggressor inference, fills, fees, inventory, execution or P&L model.', 'Event-sampled rows overweight busy markets; labels overlap within a session.', 'Only within-instrument past/current book features; no causal or generalization claim.', 'Regression, not a calibrated probability model; Brier score is inapplicable.', 'Configuration and model choices must be fixed before final test access. Repeated holdout tuning invalidates evaluation; this local CLI cannot police future user research.']
  };
  const splitManifest = { schemaVersion: 1, fingerprint, partitions: Object.fromEntries(Object.entries(split.partitions).map(([p, rows]) => [p, rows.map(s => publicSample(s))])), exclusions: { quality: built.excluded, missingLabelRowIds: built.missingLabels, split: split.excluded } };
  return { report, selection, splitManifest, model: selected.model };
}
export function finalEvaluation(prepared, runtime, savedSelection) {
  // Reproduce selection from training/validation only; a forged alpha cannot tune to test.
  const dev = develop(prepared, runtime);
  if (canonical(savedSelection) !== canonical(dev.selection)) throw new Error('SELECTION_MISMATCH');
  const rows = prepared.split.partitions.test;
  const test = { ...metrics(rows, dev.model), uncertainty: uncertainty(rows, dev.model, prepared.config.seed, prepared.config.bootstrapReplicates) };
  const report = { ...dev.report, status: 'final-held-out-evaluation', test, selection: savedSelection };
  const predictions = rows.map(s => ({ ...publicSample(s, true), predictionMidChange: predict(dev.model, s.x), zeroChangePrediction: 0 }));
  return { ...dev, report, predictions };
}
