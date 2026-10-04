import test from 'node:test';
import assert from 'node:assert/strict';
import { parseCsv, presidentialTables, senateRaces, raceFeatures, fitRidge, expandingCheck, scenarios, probabilityFromMargin } from './model.mjs';

test('CSV preserves quoted candidate names and multiline text and rejects malformed widths', () => {
  assert.deepEqual(parseCsv('name,value\n"Smith, John","line1\nline2"\n'), [{ name: 'Smith, John', value: 'line1\nline2' }]);
  assert.throws(() => parseCsv('a,b\n1\n'));
  assert.throws(() => parseCsv('a,a\n1,2\n'));
  assert.throws(() => parseCsv('a\n"open'));
});
const senateRow = (candidate, party, votes, extra = {}) => ({ year: '2020', state_po: 'WV', office: 'US SENATE', stage: 'gen',
  special: 'False', unofficial: 'False', candidate, party_simplified: party, party_detailed: party,
  candidatevotes: String(votes), totalvotes: '1000', mode: 'total', ...extra });
test('fusion lines count toward the same person; detailed modes do not duplicate a total', () => {
  const input = [senateRow('Capito', 'REPUBLICAN', 600), senateRow('Capito', 'OTHER', 20, { party_detailed: 'FUSION' }),
    senateRow('Opponent', 'DEMOCRAT', 380), senateRow('Capito', 'REPUBLICAN', 100, { mode: 'absentee' })];
  const result = senateRaces(input);
  assert.equal(result.races.get('2020:WV').republican.votes, 620);
  assert.equal(result.races.get('2020:WV').margin, 0.24);
});
test('ambiguous major fusion, duplicates, independent winners and unofficial results cannot train a binary race', () => {
  const base = [senateRow('R', 'REPUBLICAN', 600), senateRow('D', 'DEMOCRAT', 400)];
  assert.equal(senateRaces([...base, senateRow('R', 'DEMOCRAT', 1)]).races.size, 0);
  assert.equal(senateRaces([...base, base[0]]).races.size, 0);
  assert.equal(senateRaces([senateRow('R', 'REPUBLICAN', 150), senateRow('D', 'DEMOCRAT', 200), senateRow('I', 'OTHER', 650)]).races.size, 0);
  assert.equal(senateRaces(base.map(r => ({ ...r, unofficial: 'True' }))).races.size, 0);
});
test('reported write-in and printed-ballot partitions add without treating either as duplicate', () => {
  const result = senateRaces([senateRow('R', 'REPUBLICAN', 599), senateRow('R', 'REPUBLICAN', 1, { writein: 'True' }), senateRow('D', 'DEMOCRAT', 400)]);
  assert.equal(result.races.get('2020:WV').republican.votes, 600);
});
test('incomplete presidential coverage fails instead of inventing a national lean', () => {
  assert.throws(() => presidentialTables([
    { year: '2024', state_po: 'WV', office: 'US PRESIDENT', candidate: 'R', party_simplified: 'REPUBLICAN', party_detailed: 'REPUBLICAN', candidatevotes: '600' },
    { year: '2024', state_po: 'WV', office: 'US PRESIDENT', candidate: 'D', party_simplified: 'DEMOCRAT', party_detailed: 'DEMOCRAT', candidatevotes: '400' }
  ]), /Incomplete/);
});
test('same-year presidential results never enter election features', () => {
  const president = { years: [2016, 2020, 2024, 2028], national: new Map([2016, 2020, 2024, 2028].map(y => [y, { margin: y === 2020 ? -0.9 : 0.02 }])),
    states: new Map([2016, 2020, 2024, 2028].map(y => [`${y}:WV`, { margin: y === 2020 ? -0.99 : 0.4 }])) };
  const senate = { races: new Map([['2014:WV', { year: 2014, margin: 0.3, winner: { id: 'CAPITO' } }], ['2020:WV', { year: 2020, margin: 0.4, winner: { id: 'CAPITO' } }]]) };
  president.years.unshift(2012); president.national.set(2012, { margin: 0 });
  const f = raceFeatures({ year: 2020, state: 'WV', republican: 'Capito', democratic: 'Opponent' }, senate, president);
  assert.equal(f.clocks.presYear, 2016); assert.equal(f.x[1], 0.38);
  const next = raceFeatures({ year: 2026, state: 'WV', republican: 'Capito', democratic: 'Opponent' }, senate, president);
  assert.equal(next.clocks.presYear, 2024); assert.equal(next.clocks.priorSeatYear, 2020); assert.equal(next.incumbent, 1);
});
test('every expanding-cycle fit excludes that whole cycle and trains normalization on past rows', () => {
  const rows = Array.from({ length: 120 }, (_, i) => ({ year: 1990 + 2 * Math.floor(i / 20), key: String(i), x: [1, (i % 20) / 20],
    y: 0.5 * (i % 20) / 20, actualMargin: 0.5 * (i % 20) / 20, nationalBaseline: 0, clocks: {} }));
  const result = expandingCheck(rows, 1, 2000, 40);
  assert.equal(result.length, 80); assert(result.every(r => r.trainedThroughYear < r.year));
  const m = fitRidge(rows.filter(r => r.year < 1994), 1);
  assert(Math.abs(m.means[1] - 0.475) < 1e-12); assert.equal(m.trainedThroughYear, 1992);
});
test('fixed heavy-tail probabilities are symmetric and do not assert certainty', () => {
  assert.equal(probabilityFromMargin(0, 0.1), 0.5);
  assert(Math.abs(probabilityFromMargin(0.2, 0.1) + probabilityFromMargin(-0.2, 0.1) - 1) < 1e-12);
  assert.equal(probabilityFromMargin(100, 0.1), 0.99);
});
test('assessment clock stays causal and expiry binds the dynamic source, not its retrieval', () => {
  const environment = { sourceAt: '2026-10-04T15:43:00Z', observedAt: '2026-10-04T21:20:00Z', maximumAgeMs: 21600000,
    republicanPercent: 42.2, democraticPercent: 49.9 };
  const args = { model: { coefficients: [0.4], means: [0], scales: [1] }, features: { x: [1] }, environment,
    assessmentAtMs: Date.parse('2026-10-04T21:25:00Z'), residualRms: 0.10,
    definition: { pollingErrorSensitivityPoints: [0, 3, 6], additionalNationalBiasStressPoints: [-5, 0, 5] } };
  const result = scenarios(args);
  assert.equal(result.expiresAt, '2026-10-04T21:43:00.000Z'); assert.equal(result.variants.length, 9);
  assert(result.lower < result.upper);
  assert.throws(() => scenarios({ ...args, assessmentAtMs: Date.parse('2026-10-04T21:44:00Z') }), /Expired/);
  assert.throws(() => scenarios({ ...args, assessmentAtMs: Date.parse('2026-10-04T21:19:00Z') }), /Causal/);
});
