import test from 'node:test';
import assert from 'node:assert/strict';
import {buildGovernorPollAverageMarketV1, bootstrapGovernorPollAverageV1,
  compareGovernorPollProbabilityToBookV1} from './governor-poll-average-market-v1.mjs';

const definition = {
  forecastHorizonDays: 30,
  maximumPollAgeDays: 30,
  recencyHalfLifeDays: 21,
  minimumTrainingRaces: 30,
  minimumCalibrationCycles: 3,
  minimumMarginSdPoints: 3,
};

function observationsFixture({years = [2000, 2002, 2004, 2006, 2008, 2010, 2012, 2014], racesPerYear = 6} = {}) {
  const rows = [];
  for (const year of years) {
    for (let i = 0; i < racesPerYear; i++) {
      const key = `${year}:S${String(i).padStart(2, '0')}`;
      const actualMargin = [-14, -7, -2, 2, 7, 14][i % 6];
      const commonError = ((i * 7 + year) % 9 - 4) * 1.1;
      const electionAt = Date.parse(`${year}-11-03T12:00:00Z`);
      for (let pollster = 0; pollster < 3; pollster++) {
        const horizonDays = 21 + pollster * 4;
        rows.push({
          id: `${key}:p${pollster}`,
          key,
          year,
          state: `S${i}`,
          pollId: `${key}-${pollster}`,
          questionId: `${key}-${pollster}`,
          pollsterId: `P${pollster}`,
          pollster: `Pollster ${pollster}`,
          fieldDate: new Date(electionAt - horizonDays * 86_400_000).toISOString().slice(0, 10),
          fieldAt: electionAt - horizonDays * 86_400_000,
          electionDate: `${year}-11-03`,
          electionAt,
          horizonDays,
          democratic: 50 + (actualMargin - commonError) / 2,
          republican: 50 - (actualMargin - commonError) / 2,
          remaining: 0,
          pollMargin: actualMargin - commonError,
          actualMargin,
          democraticWon: actualMargin > 0,
        });
      }
    }
  }
  return rows;
}

test('fits one race-level error per governor race instead of shrinking by poll count', () => {
  const result = buildGovernorPollAverageMarketV1({
    observations: observationsFixture(), definition, horizonDays: 21, maximumPollAgeDays: 14,
    cutoffYear: 2014, currentPollMargin: 7.5, bootstrapReplicates: 300,
  });
  assert.equal(result.sample.uniqueGovernorRaces, 48);
  assert.equal(result.sample.independentCalendarYearClusters, 8);
  assert.equal(result.sample.trainedThroughYear, 2014);
  assert.equal(result.model.family, 'poll_centered');
  assert.equal(result.model.distribution, 'fixed_t4');
  assert.equal(result.currentSensitivity.marketCalibrationEstablished, false);
  assert.equal(result.ordersGenerated, 0);
});

test('walk-forward predictions train strictly before each tested election year', () => {
  const result = buildGovernorPollAverageMarketV1({
    observations: observationsFixture(), definition, horizonDays: 21, maximumPollAgeDays: 14,
    cutoffYear: 2014, currentPollMargin: 7.5, bootstrapReplicates: 300,
  });
  assert.ok(result.chronological.predictionCount > 0);
  assert.ok(result.chronological.byYear.every(row => row.trainedThroughYear < row.year));
});

test('calendar-year cluster bootstrap is deterministic and reports historical sensitivity only', () => {
  const raceRows = [];
  for (let year = 2000; year <= 2014; year += 2) {
    for (let i = 0; i < 8; i++) {
      raceRows.push({year, error: ((i * 3 + year) % 11 - 5) * 1.5});
    }
  }
  const args = {raceRows, currentPollMargin: 7.5, minimumRaces: 30, minimumCycles: 3,
    minimumSd: 3, replicates: 500, seed: 12345};
  const a = bootstrapGovernorPollAverageV1(args);
  const b = bootstrapGovernorPollAverageV1(args);
  assert.deepEqual(a, b);
  assert.equal(a.independentClusters, 8);
  assert.equal(a.interpretation, 'historical_parameter_sensitivity_not_a_2026_confidence_interval');
  assert.ok(a.probabilityPercentiles.p025 <= a.probabilityPercentiles.p50);
  assert.ok(a.probabilityPercentiles.p50 <= a.probabilityPercentiles.p975);
});

test('rejects insufficient years rather than bootstrapping race rows as independent', () => {
  const raceRows = Array.from({length: 40}, (_, i) => ({year: 2022, error: i - 20}));
  assert.throws(() => bootstrapGovernorPollAverageV1({raceRows, currentPollMargin: 7,
    minimumRaces: 30, minimumCycles: 3, minimumSd: 3, replicates: 200, seed: 1}),
  /insufficient_independent_election_years/);
});

test('compares both binary asks after the same adverse tick and rejects a point-only edge', () => {
  const comparison = compareGovernorPollProbabilityToBookV1({
    democraticWinProbability: 0.8881763196872845,
    probabilityLow: 0.849997694623553,
    probabilityHigh: 0.9256214554745886,
    yesAsk: 0.9,
    noAsk: 0.105,
    tick: 0.005,
  });
  assert.ok(Math.abs(comparison.yes.pointEdge - (-0.0168236803127155)) < 1e-12);
  assert.ok(Math.abs(comparison.no.pointEdge - 0.0018236803127155) < 1e-12);
  assert.ok(comparison.yes.historicalBootstrapLowerEdge < 0);
  assert.ok(comparison.no.historicalBootstrapLowerEdge < 0);
  assert.equal(comparison.anyDirectionHasPositiveBootstrapLowerEdge, false);
  assert.throws(() => compareGovernorPollProbabilityToBookV1({
    democraticWinProbability: 1.1, probabilityLow: 0.8, probabilityHigh: 0.9,
    yesAsk: 0.9, noAsk: 0.1, tick: 0.01,
  }), /invalid_democratic_probability/);
});

