import assert from 'node:assert/strict';
import {aggregateRacePolls, fitPollError, probability, score} from './poll-calibration-v1/model.mjs';

const finite = Number.isFinite;

function validateInputs(observations, definition, options) {
  assert(Array.isArray(observations) && observations.length > 0, 'poll_observations_required');
  assert(definition && typeof definition === 'object', 'poll_definition_required');
  assert(Number.isSafeInteger(options.horizonDays) && options.horizonDays > 0, 'invalid_forecast_horizon');
  assert(Number.isSafeInteger(options.maximumPollAgeDays) && options.maximumPollAgeDays >= 0, 'invalid_poll_age_window');
  assert(Number.isSafeInteger(options.cutoffYear), 'invalid_training_cutoff');
  assert(finite(options.currentPollMargin), 'invalid_current_poll_margin');
  assert(Number.isSafeInteger(options.bootstrapReplicates) && options.bootstrapReplicates >= 200, 'insufficient_bootstrap_replicates');
  assert(Number.isSafeInteger(options.seed) && options.seed > 0, 'invalid_bootstrap_seed');
  for (const row of observations) {
    assert(Number.isSafeInteger(row.year) && typeof row.key === 'string' && row.key.length > 0, 'invalid_poll_race_identity');
    assert(finite(row.pollMargin) && finite(row.actualMargin) && typeof row.democraticWon === 'boolean', 'invalid_poll_race_outcome');
  }
}

function quantile(values, p) {
  const sorted = [...values].sort((a, b) => a - b);
  const index = (sorted.length - 1) * p;
  const low = Math.floor(index);
  const high = Math.ceil(index);
  return sorted[low] + (sorted[high] - sorted[low]) * (index - low);
}

function makeRandom(seed) {
  let state = seed >>> 0;
  return () => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) / 0x1_0000_0000;
  };
}

export function bootstrapGovernorPollAverageV1({
  raceRows,
  currentPollMargin,
  minimumRaces,
  minimumCycles,
  minimumSd,
  replicates = 4000,
  seed = 0x6ba4_2026,
}) {
  assert(Array.isArray(raceRows) && raceRows.length >= minimumRaces, 'insufficient_race_rows');
  assert(Number.isSafeInteger(replicates) && replicates >= 200, 'insufficient_bootstrap_replicates');
  assert(Number.isSafeInteger(seed) && seed > 0, 'invalid_bootstrap_seed');
  const byYear = new Map();
  for (const row of raceRows) {
    assert(Number.isSafeInteger(row.year) && finite(row.error), 'invalid_clustered_race_error');
    const rows = byYear.get(row.year) ?? [];
    rows.push(row);
    byYear.set(row.year, rows);
  }
  const years = [...byYear.keys()].sort((a, b) => a - b);
  assert(years.length >= minimumCycles, 'insufficient_independent_election_years');

  const random = makeRandom(seed);
  const samples = [];
  for (let rep = 0; rep < replicates; rep++) {
    const sample = [];
    for (let i = 0; i < years.length; i++) {
      const year = years[Math.floor(random() * years.length)];
      sample.push(...byYear.get(year));
    }
    try {
      const model = fitPollError(sample, {
        family: 'poll_centered', minimumRaces, minimumCycles, minimumSd,
      });
      samples.push(probability(model, currentPollMargin));
    } catch {
      // A resample that fails the predeclared race/cycle floor is discarded.
    }
  }
  assert(samples.length >= Math.floor(replicates * 0.9), 'too_many_invalid_cycle_bootstraps');
  return {
    method: 'calendar_year_cluster_bootstrap_v1',
    independentClusters: years.length,
    bootstrapReplicates: replicates,
    validReplicates: samples.length,
    seed,
    probabilityPercentiles: {
      p025: quantile(samples, 0.025),
      p50: quantile(samples, 0.5),
      p975: quantile(samples, 0.975),
    },
    interpretation: 'historical_parameter_sensitivity_not_a_2026_confidence_interval',
  };
}

export function compareGovernorPollProbabilityToBookV1({
  democraticWinProbability,
  probabilityLow,
  probabilityHigh,
  yesAsk,
  noAsk,
  tick,
}) {
  for (const [name, value] of Object.entries({democraticWinProbability, probabilityLow,
    probabilityHigh, yesAsk, noAsk, tick})) assert(finite(value), `invalid_${name}`);
  assert(democraticWinProbability >= 0 && democraticWinProbability <= 1, 'invalid_democratic_probability');
  assert(probabilityLow >= 0 && probabilityLow <= probabilityHigh && probabilityHigh <= 1, 'invalid_probability_interval');
  assert(yesAsk > 0 && yesAsk < 1 && noAsk > 0 && noAsk < 1 && tick > 0, 'invalid_book_side');
  const yesStressedAsk = yesAsk + tick;
  const noStressedAsk = noAsk + tick;
  assert(yesStressedAsk < 1 && noStressedAsk < 1, 'stressed_ask_out_of_range');
  const yesPointEdge = democraticWinProbability - yesStressedAsk;
  const noPointEdge = (1 - democraticWinProbability) - noStressedAsk;
  const yesLowerEdge = probabilityLow - yesStressedAsk;
  const noLowerEdge = (1 - probabilityHigh) - noStressedAsk;
  return {
    yes: {ask: yesAsk, stressedAsk: yesStressedAsk, pointEdge: yesPointEdge, historicalBootstrapLowerEdge: yesLowerEdge},
    no: {ask: noAsk, stressedAsk: noStressedAsk, pointEdge: noPointEdge, historicalBootstrapLowerEdge: noLowerEdge},
    anyDirectionHasPositiveBootstrapLowerEdge: yesLowerEdge > 0 || noLowerEdge > 0,
    classification: 'sensitivity_comparison_only_not_SIG_fair_value_or_trade',
  };
}

export function buildGovernorPollAverageMarketV1({
  observations,
  definition,
  horizonDays = 21,
  maximumPollAgeDays = 14,
  cutoffYear = 2022,
  currentPollMargin,
  bootstrapReplicates = 4000,
  bootstrapSeed = 0x6ba4_2026,
}) {
  const options = {horizonDays, maximumPollAgeDays, cutoffYear, currentPollMargin,
    bootstrapReplicates, seed: bootstrapSeed};
  validateInputs(observations, definition, options);

  const fixedDefinition = {...definition, forecastHorizonDays: horizonDays, maximumPollAgeDays};
  const allRaceRows = aggregateRacePolls(observations, fixedDefinition);
  const raceRows = allRaceRows.filter(row => row.year <= cutoffYear);
  const fitOptions = {
    family: 'poll_centered',
    minimumRaces: definition.minimumTrainingRaces,
    minimumCycles: definition.minimumCalibrationCycles,
    minimumSd: definition.minimumMarginSdPoints,
  };
  const finalModel = fitPollError(raceRows, fitOptions);
  assert(finalModel.trainedThroughYear <= cutoffYear, 'future_race_in_final_fit');

  const walkForwardRows = [];
  for (const year of [...new Set(raceRows.map(row => row.year))].sort((a, b) => a - b)) {
    const training = raceRows.filter(row => row.year < year);
    const testing = raceRows.filter(row => row.year === year);
    let model;
    try {
      model = fitPollError(training, fitOptions);
    } catch {
      continue;
    }
    assert(model.trainedThroughYear < year, 'walk_forward_future_leak');
    for (const row of testing) {
      walkForwardRows.push({...row, trainedThroughYear: model.trainedThroughYear,
        democraticWinProbability: probability(model, row.pollMargin)});
    }
  }
  assert(walkForwardRows.length > 0, 'no_chronological_predictions');
  const walkForward = score(walkForwardRows);
  const byYear = [...new Set(walkForwardRows.map(row => row.year))].sort((a, b) => a - b).map(year => {
    const rows = walkForwardRows.filter(row => row.year === year);
    const metrics = score(rows);
    return {year, races: rows.length, trainedThroughYear: Math.max(...rows.map(row => row.trainedThroughYear)),
      brier: metrics.brier, logLoss: metrics.logLoss, correctDirections: metrics.correctDirections};
  });
  const midtermRows = walkForwardRows.filter(row => row.year % 4 === 2);
  const midtermDiagnostics = midtermRows.length ? score(midtermRows) : null;
  const currentProbability = probability(finalModel, currentPollMargin);
  const bootstrap = bootstrapGovernorPollAverageV1({
    raceRows, currentPollMargin, ...fitOptions,
    replicates: bootstrapReplicates, seed: bootstrapSeed,
  });
  return {
    schemaVersion: 1,
    method: 'governor_race_level_poll_average_error_v1',
    fixedDesign: {horizonDays, maximumPollAgeDays, recencyHalfLifeDays: definition.recencyHalfLifeDays,
      cutoffYear, family: 'poll_centered', distribution: 'fixed_t4', minimumSdPoints: definition.minimumMarginSdPoints},
    sample: {
      sourcePollObservations: observations.filter(row => row.year <= cutoffYear
        && row.horizonDays >= horizonDays && row.horizonDays <= horizonDays + maximumPollAgeDays).length,
      uniqueGovernorRaces: raceRows.length,
      independentCalendarYearClusters: [...new Set(raceRows.map(row => row.year))].length,
      trainedThroughYear: finalModel.trainedThroughYear,
      raceYears: finalModel.cycles,
    },
    model: finalModel,
    chronological: {predictionCount: walkForwardRows.length, metrics: walkForward,
      midtermCycleMetrics: midtermDiagnostics, byYear},
    currentSensitivity: {
      inputPollMarginPoints: currentPollMargin,
      democraticWinProbability: currentProbability,
      bootstrap,
      yesFairValueSensitivity: currentProbability,
      noFairValueSensitivity: 1 - currentProbability,
      marketCalibrationEstablished: false,
      classification: 'external_polling_sensitivity_not_SIG_fair_value_or_trade',
    },
    limitations: [
      'Historical source rows are grouped into one recency-weighted poll average per governor race; this empirically preserves shared race-level error instead of dividing single-poll dispersion by sqrt(n).',
      'The historical archive does not identify likely-voter population, original publication time, or the exact publisher aggregation for the current 41-poll average.',
      'Election-year clusters are few and outcomes within a cycle are correlated; the bootstrap interval is historical parameter sensitivity, not a 2026 confidence interval.',
      'The archive ends in 2022. Model transport to the 2026 Michigan race is unvalidated.',
      'No historical market prices, fills, market calibration, or trading P&L are used.',
    ],
    profitabilityEstablished: false,
    executionAuthority: false,
    ordersGenerated: 0,
  };
}

