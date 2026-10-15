import test from 'node:test';
import assert from 'node:assert/strict';
import {calibrateGovernorPollForecastsV1, screenGovernorPollForecastV1,
  summarizeGovernorPollValueScreenV1} from './all-governor-poll-value-screen-v1.mjs';

const t0 = Date.parse('2026-10-14T17:05:00.000Z');
const forecast = {state: 'MI', fetchedAt: new Date(t0).toISOString(), contentHash: 'a'.repeat(64),
  pollingMarginForDPoints: 2, modelMarginForDPoints: 6, probabilityCalibratedForSIG: false};
const contract = {state: 'MI', stateName: 'Michigan', party: 'D', marketId: '305', exchangeId: '994',
  marketTitle: 'Will the Democratic Party win the Michigan Governor?'};
const calibration = {state: 'MI', pointProbability: 0.6, bootstrap: {p025: 0.55, p50: 0.6, p975: 0.65}};

function book({requestMs, sequence, bid = 0.545, ask = 0.55, sourceOffsetMs = -50, depth = 20} = {}) {
  const request = t0 + requestMs;
  const received = request + 100;
  const source = received + sourceOffsetMs;
  const sourceAt = new Date(source).toISOString();
  return {
    marketId: contract.marketId,
    exchangeId: contract.exchangeId,
    instrument: `${contract.exchangeId}:YES`,
    requestStartedAt: new Date(request).toISOString(),
    receivedAt: new Date(received).toISOString(),
    publishedAt: new Date(received + 100).toISOString(),
    sourceAt,
    sourceEventAgeMs: received - source,
    freshnessPolicy: 'authoritative_receipt_v1',
    versionKey: `${sequence}:${sourceAt}`,
    publicationSequence: sequence,
    publicationHash: String(sequence).padStart(64, '0'),
    bids: Array.from({length: depth}, (_, i) => ({price: (bid - i * 0.005).toFixed(3), quantity: 100})),
    asks: Array.from({length: depth}, (_, i) => ({price: (ask + i * 0.005).toFixed(3), quantity: 100})),
  };
}

test('shared cycle bootstrap is deterministic, uses the fixed cutoff, and omits missing polls', () => {
  const raceRows = [];
  for (const year of [2010, 2012, 2014, 2016, 2018, 2020]) {
    for (let race = 0; race < 10; race += 1) {
      raceRows.push({year, error: ((race * 3 + year / 2) % 17 - 8) * 1.25});
    }
  }
  const forecasts = [
    {state: 'MI', pollingMarginForDPoints: 2},
    {state: 'WI', pollingMarginForDPoints: -3},
    {state: 'WY', pollingMarginForDPoints: null},
  ];
  const options = {raceRows, forecasts, definition: {minimumTrainingRaces: 30,
    minimumCalibrationCycles: 3, minimumMarginSdPoints: 3}, replicates: 200, seed: 1234};
  const first = calibrateGovernorPollForecastsV1(options);
  const again = calibrateGovernorPollForecastsV1(options);
  assert.deepEqual(first, again);
  assert.equal(first.model.trainedThroughYear, 2020);
  assert.equal(first.bootstrap.method, 'shared_calendar_year_cluster_bootstrap_v1');
  assert.equal(first.forecasts[2].reason, 'NO_POLL_MARGIN');
  assert(first.bootstrap.validReplicates >= 180);
});

test('poll calibration rejects any training race beyond the declared historical cutoff', () => {
  assert.throws(() => calibrateGovernorPollForecastsV1({
    raceRows: [{year: 2024, error: 1}],
    forecasts: [{state: 'MI', pollingMarginForDPoints: 1}],
    definition: {minimumTrainingRaces: 1, minimumCalibrationCycles: 1, minimumMarginSdPoints: 3},
    cutoffYear: 2022,
  }), /POLL_SCREEN_CUTOFF/);
});

test('entry selection waits for latency and skips future-source-clock and shallow books', () => {
  const tooEarly = book({requestMs: 2500, sequence: 1});
  const sourceAhead = book({requestMs: 3100, sequence: 2, sourceOffsetMs: 2500});
  const shallow = book({requestMs: 3200, sequence: 3, depth: 19});
  const accepted = book({requestMs: 3300, sequence: 4});
  const result = screenGovernorPollForecastV1({forecast, calibration, contract,
    books: [accepted, shallow, sourceAhead, tooEarly], markoutHorizonsMs: []});
  assert.equal(result.status, 'SCREENED');
  assert.equal(result.entryBook.sequence, 4);
  assert.equal(Date.parse(result.entryBook.requestStartedAt) - t0, 3300);
  assert(Math.abs(result.sideValues.find(row => row.side === 'YES').pointSettlementEdgePerShare - 0.045) < 1e-12);
  assert(Math.abs(result.sideValues.find(row => row.side === 'NO').pointSettlementEdgePerShare + 0.06) < 1e-12);
  assert.equal(result.sideValues.find(row => row.side === 'YES').fees, 0);
});

test('receipt chronology tolerates bounded feed clock lead and reports the skew', () => {
  const slightlyAhead = book({requestMs: 3300, sequence: 5, sourceOffsetMs: 1000});
  const result = screenGovernorPollForecastV1({forecast, calibration, contract,
    books: [slightlyAhead], markoutHorizonsMs: []});
  assert.equal(result.status, 'SCREENED');
  assert.equal(result.entryBook.sourceClockLeadAtReceiptMs, 1000);
  assert.equal(result.entryBook.sourceAgeAtReceiptMs, -1000);
});

test('no later valid full-depth quote fails closed and exact contract identity is required', () => {
  const result = screenGovernorPollForecastV1({forecast, calibration, contract,
    books: [book({requestMs: 2000, sequence: 1}), book({requestMs: 3100, sequence: 2, sourceOffsetMs: 2500})],
    maxEntryWaitMs: 1000, markoutHorizonsMs: []});
  assert.equal(result.status, 'NO_FRESH_FULL_DEPTH_ENTRY_BOOK');
  assert.throws(() => screenGovernorPollForecastV1({forecast, calibration,
    contract: {...contract, marketTitle: 'Will the Democratic Party win the Wisconsin Governor?'}, books: []}),
  /POLL_SCREEN_CONTRACT_MAPPING/);
});

test('NO side uses the YES complement and markout charges one adverse tick on both sides', () => {
  const entry = book({requestMs: 3300, sequence: 10, bid: 0.545, ask: 0.55});
  const exitRequest = Date.parse(entry.publishedAt) + 60000 + 3000;
  const exit = book({requestMs: exitRequest - t0, sequence: 11, bid: 0.56, ask: 0.565});
  const result = screenGovernorPollForecastV1({forecast, calibration, contract, books: [entry, exit],
    markoutHorizonsMs: [60000]});
  const yes = result.sideValues.find(row => row.side === 'YES');
  const no = result.sideValues.find(row => row.side === 'NO');
  assert.equal(yes.markouts[0].rawExitBid, 0.56);
  assert.equal(yes.markouts[0].stressedEntryCost, 0.555);
  assert.equal(yes.markouts[0].stressedExitBid, 0.555);
  assert.equal(yes.markouts[0].netPerShare, 0);
  assert.equal(no.rawAsk, 1 - 0.545);
  assert.equal(no.markouts[0].rawExitBid, 1 - 0.565);
  assert.equal(no.markouts[0].actualFill, false);
});

test('summary labels proxy marks separately from fills and realized P&L', () => {
  const row = screenGovernorPollForecastV1({forecast, calibration, contract,
    books: [book({requestMs: 3300, sequence: 10})], markoutHorizonsMs: []});
  const summary = summarizeGovernorPollValueScreenV1([row]);
  assert.equal(summary.withEntryBook, 1);
  assert.equal(summary.actualFills, 0);
  assert.equal(summary.realizedPnl, 0);
  assert.equal(summary.profitabilityEstablished, false);
});

