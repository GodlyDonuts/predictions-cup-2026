import fs from 'node:fs';
import assert from 'node:assert/strict';
import { parseCsv, hash, presidentialTables, senateRaces, buildRows, fitRidge, expandingCheck, metrics, predict, raceFeatures, scenarios, probabilityFromMargin } from './model.mjs';

const base = 'research/election-baseline-v1';
const definitionBytes = fs.readFileSync(`${base}/definition.json`), definition = JSON.parse(definitionBytes);
assert(Date.parse(definition.amendedAt || definition.declaredAt) <= Date.now());
const sourceManifests = ['PEJ5QU', '42MVDX'].map(name => JSON.parse(fs.readFileSync(`${base}/sources/${name}-manifest.json`)));
for (const m of sourceManifests) for (const f of m.files) assert.equal(hash(fs.readFileSync(`${base}/sources/${f.filename}`)), f.sha256);
const senate = senateRaces(parseCsv(fs.readFileSync(`${base}/sources/1976-2024-senate-state.csv`, 'utf8')));
const president = presidentialTables(parseCsv(fs.readFileSync(`${base}/sources/1976-2024-president.csv`, 'utf8')));
const rows = buildRows(senate, president, definition.minimumTrainingYear);
for (const r of rows) assert(r.clocks.presYear < r.year && r.clocks.priorSeatYear < r.year && r.clocks.priorSeatPresYear < r.clocks.priorSeatYear);
const development = definition.ridgeGrid.map(lambda => {
  const predictions = expandingCheck(rows, lambda, definition.developmentLastYear);
  const byCycle = metrics(predictions).cycles.map(year => metrics(predictions.filter(r => r.year === year)));
  return { lambda, metrics: metrics(predictions), cycleMse: byCycle.reduce((sum, m) => sum + m.rmse ** 2, 0) / byCycle.length, predictions };
});
development.sort((a, b) => a.cycleMse - b.cycleMse || a.lambda - b.lambda);
const selected = development[0], residualRms = Math.max(0.10, selected.metrics.rmse);
const checks = [];
for (const year of definition.untouchedCheckYears) {
  const training = rows.filter(r => r.year < year), model = fitRidge(training, selected.lambda);
  assert(model.trainedThroughYear < year);
  for (const r of rows.filter(r => r.year === year)) {
    const predictedMargin = predict(model, r.x) + r.nationalBaseline;
    const winProbability = probabilityFromMargin(predictedMargin, residualRms);
    const outcome = r.actualMargin > 0 ? 1 : 0;
    checks.push({ key: r.key, year, trainedThroughYear: model.trainedThroughYear, predictedMargin, actualMargin: r.actualMargin,
      residual: r.actualMargin - predictedMargin, clocks: r.clocks, winProbability, outcome,
      brier: (winProbability - outcome) ** 2, logLoss: -(outcome * Math.log(winProbability) + (1 - outcome) * Math.log(1 - winProbability)) });
  }
}
assert(checks.length > 50);
const finalModel = fitRidge(rows, selected.lambda), inputs = JSON.parse(fs.readFileSync(`${base}/current-inputs.json`));
assert(Date.parse(inputs.environment.observedAt) <= Date.now());
const assessmentAtMs = Date.now();
const forecasts = inputs.candidates.map(candidate => {
  const features = raceFeatures(candidate, senate, president); assert(features);
  return { ...candidate, features, assessment: scenarios({ model: finalModel, features, environment: inputs.environment,
    assessmentAtMs, residualRms, definition }),
    currentSourceGateSatisfied: false, eligibilityReason: 'Current ballot and complete visible settlement review still required before a production bank.' };
});
const report = {
  schemaVersion: 1, id: definition.id, at: new Date(assessmentAtMs).toISOString(), definitionHash: hash(definitionBytes),
  sourceManifests, sourceIntegrityVerified: true, rows: rows.length,
  includedCycles: [...new Set(rows.map(r => r.year))], exclusions: senate.exclusions,
  development: development.map(({ lambda, metrics, cycleMse }) => ({ lambda, metrics, cycleMse })),
  selectedLambda: selected.lambda, selectedUsingYearsThrough: definition.developmentLastYear,
  residualRms, model: finalModel, modelHash: hash(Buffer.from(JSON.stringify(finalModel))),
  check: { ...metrics(checks), brier: checks.reduce((s, r) => s + r.brier, 0) / checks.length,
    logLoss: checks.reduce((s, r) => s + r.logLoss, 0) / checks.length,
    noInformationBrier: 0.25, byCycle: definition.untouchedCheckYears.map(year => metrics(checks.filter(r => r.year === year))) },
  checkRows: checks, developmentRows: selected.predictions, forecasts,
  forecastVintagesAvailable: false, pollingTransportValidatedByHistoricalCheck: false,
  calibrationEstablishedFor2026: false, qualifiedProductionForecasts: 0,
  ordersSubmitted: 0, paperFills: 0, profitabilityEstablished: false,
  limitations: definition.limitations
};
fs.writeFileSync(`${base}/model-report.json`, `${JSON.stringify(report, null, 2)}\n`, { flag: 'wx' });
console.log(JSON.stringify({ rows: report.rows, exclusions: report.exclusions.length, selectedLambda: report.selectedLambda,
  development: report.development, check: report.check, forecasts: report.forecasts.map(f => ({ marketId: f.marketId,
    meanMargin: f.assessment.meanMargin, lower: f.assessment.lower, upper: f.assessment.upper,
    expiresAt: f.assessment.expiresAt, incumbentNameMatch: f.features.incumbentNameMatch, currentSourceGateSatisfied: false })),
  qualifiedProductionForecasts: 0, profitabilityEstablished: false }));
