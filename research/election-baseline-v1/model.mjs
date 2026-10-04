import assert from 'node:assert/strict';
import crypto from 'node:crypto';

export const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
export function parseCsv(text) {
  const rows = []; let row = [], field = '', quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') { field += '"'; i++; }
      else if (c === '"') quoted = false;
      else field += c;
    } else if (c === '"') { if (field !== '') throw Error('Quote inside unquoted field'); quoted = true; }
    else if (c === ',') { row.push(field); field = ''; }
    else if (c === '\n') { row.push(field.replace(/\r$/, '')); if (row.some(x => x !== '')) rows.push(row); row = []; field = ''; }
    else field += c;
  }
  if (quoted) throw Error('Unterminated CSV quote');
  if (field !== '' || row.length) { row.push(field.replace(/\r$/, '')); rows.push(row); }
  const header = rows.shift();
  if (!header?.length || new Set(header).size !== header.length) throw Error('Invalid CSV header');
  return rows.map((values, i) => { if (values.length !== header.length) throw Error(`CSV width ${i}`); return Object.fromEntries(header.map((h, j) => [h, values[j]])); });
}
export const identity = name => name.toUpperCase().replace(/[^A-Z0-9 ]/g, ' ').split(/\s+/).filter(x => x && !['JR', 'SR', 'II', 'III', 'IV'].includes(x)).sort().join(' ');
const boolean = v => String(v).toLowerCase() === 'true';
const voteCount = value => { const n = Number(value); if (!Number.isSafeInteger(n) || n < 0) throw Error('Invalid vote count'); return n; };
const major = value => ['REPUBLICAN', 'DEMOCRAT'].includes(value) ? value : null;

// Ballot lines of the same person belong to that person. A fusion line is not a separate voter.
function candidateTotals(rows) {
  const candidates = new Map();
  for (const row of rows) {
    const id = identity(row.candidate);
    if (!id) continue;
    if (!candidates.has(id)) candidates.set(id, { id, name: row.candidate, parties: new Set(), lines: new Map() });
    const c = candidates.get(id), p = major(row.party_simplified);
    if (p) c.parties.add(p);
    const line = `${row.party_detailed}\0writein=${boolean(row.writein)}`, mode = row.mode || 'total';
    if (!c.lines.has(line)) c.lines.set(line, new Map());
    const modes = c.lines.get(line);
    if (modes.has(mode)) throw Error('Duplicate candidate ballot-line/mode');
    modes.set(mode, voteCount(row.candidatevotes));
  }
  return [...candidates.values()].map(c => ({ id: c.id, name: c.name, parties: [...c.parties],
    votes: [...c.lines.values()].reduce((sum, modes) => sum + (modes.has('total') ? modes.get('total') : [...modes.values()].reduce((a, b) => a + b, 0)), 0) }));
}

export function presidentialTables(rows) {
  const grouped = new Map();
  for (const r of rows) {
    if (r.office !== 'US PRESIDENT' || !/^[A-Z]{2}$/.test(r.state_po)) continue;
    const key = `${r.year}:${r.state_po}`;
    if (!grouped.has(key)) grouped.set(key, []); grouped.get(key).push(r);
  }
  const states = new Map(), nationalVotes = new Map();
  for (const [key, group] of grouped) {
    const candidates = candidateTotals(group);
    if (candidates.some(c => c.parties.length > 1)) throw Error('Ambiguous presidential candidate party');
    const republican = candidates.filter(c => c.parties.includes('REPUBLICAN')).reduce((a, c) => a + c.votes, 0);
    const democratic = candidates.filter(c => c.parties.includes('DEMOCRAT')).reduce((a, c) => a + c.votes, 0);
    if (!republican || !democratic) throw Error('Missing presidential major party');
    const [yearText, state] = key.split(':'), year = Number(yearText);
    states.set(key, { year, state, republican, democratic, margin: (republican - democratic) / (republican + democratic) });
    if (!nationalVotes.has(year)) nationalVotes.set(year, { republican: 0, democratic: 0, states: 0 });
    const n = nationalVotes.get(year); n.republican += republican; n.democratic += democratic; n.states++;
  }
  const national = new Map([...nationalVotes].map(([year, n]) => {
    if (n.states !== 51) throw Error(`Incomplete presidential states ${year}:${n.states}`);
    return [year, { ...n, year, margin: (n.republican - n.democratic) / (n.republican + n.democratic) }];
  }));
  return { states, national, years: [...national.keys()].sort((a, b) => a - b) };
}

export function senateRaces(rows) {
  const grouped = new Map(), exclusions = [];
  for (const r of rows) {
    if (r.office !== 'US SENATE' || r.stage.toLowerCase() !== 'gen' || boolean(r.special) || boolean(r.unofficial)) continue;
    const key = `${r.year}:${r.state_po}`;
    if (!grouped.has(key)) grouped.set(key, []); grouped.get(key).push(r);
  }
  const races = new Map();
  for (const [key, group] of grouped) {
    try {
      const candidates = candidateTotals(group);
      if (candidates.some(c => c.parties.length > 1)) throw Error('cross_major_party_fusion');
      const total = Math.max(...group.map(r => voteCount(r.totalvotes)));
      if (total < 100) throw Error('uncontested_or_invalid_total');
      const ranked = party => candidates.filter(c => c.parties.includes(party)).sort((a, b) => b.votes - a.votes);
      const r = ranked('REPUBLICAN'), d = ranked('DEMOCRAT');
      if (!r[0]?.votes || !d[0]?.votes) throw Error('missing_major_party');
      if ((r[1]?.votes || 0) / total >= 0.01 || (d[1]?.votes || 0) / total >= 0.01) throw Error('multiple_material_major_candidates');
      const outside = candidates.filter(c => !c.parties.length);
      if (outside.some(c => c.votes >= Math.max(r[0].votes, d[0].votes)) || outside.reduce((a, c) => a + c.votes, 0) / total >= 0.20) throw Error('material_independent_competition');
      if (candidates.reduce((sum, c) => sum + c.votes, 0) > total * 1.001) throw Error('candidate_total_exceeds_reported_total');
      const [year, state] = key.split(':');
      races.set(key, { year: Number(year), state, republican: r[0], democratic: d[0],
        margin: (r[0].votes - d[0].votes) / (r[0].votes + d[0].votes), total,
        winner: r[0].votes > d[0].votes ? r[0] : d[0] });
    } catch (error) { exclusions.push({ key, reason: error.message }); }
  }
  return { races, exclusions };
}

function priorPresident(tables, year) {
  const prior = tables.years.filter(y => y < year).at(-1);
  if (prior === undefined) throw Error('Missing strictly prior president');
  return prior;
}
export function raceFeatures({ year, state, republican, democratic }, senate, president) {
  const presYear = priorPresident(president, year), nat = president.national.get(presYear);
  const statePres = president.states.get(`${presYear}:${state}`), prior = senate.races.get(`${year - 6}:${state}`);
  if (!statePres || !prior) return null;
  const oldPresYear = priorPresident(president, prior.year), oldNational = president.national.get(oldPresYear);
  const rId = identity(typeof republican === 'string' ? republican : republican.name), dId = identity(typeof democratic === 'string' ? democratic : democratic.name);
  const incumbent = prior.winner.id === rId ? 1 : prior.winner.id === dId ? -1 : 0;
  return { x: [1, statePres.margin - nat.margin, prior.margin - oldNational.margin, incumbent],
    clocks: { presYear, priorSeatYear: prior.year, priorSeatPresYear: oldPresYear },
    nationalBaseline: nat.margin, stateMargin: statePres.margin, priorSeatMargin: prior.margin, incumbent,
    incumbentNameMatch: incumbent !== 0 };
}
export function buildRows(senate, president, minimumYear = 1990) {
  return [...senate.races.values()].filter(r => r.year >= minimumYear).flatMap(r => {
    const f = raceFeatures(r, senate, president);
    return f ? [{ key: `${r.year}:${r.state}`, year: r.year, state: r.state, x: f.x,
      y: r.margin - f.nationalBaseline, actualMargin: r.margin, nationalBaseline: f.nationalBaseline, clocks: f.clocks }] : [];
  }).sort((a, b) => a.year - b.year || a.state.localeCompare(b.state));
}

function solve(matrix, vector) {
  const n = vector.length, a = matrix.map((row, i) => [...row, vector[i]]);
  for (let col = 0; col < n; col++) {
    let pivot = col; for (let i = col + 1; i < n; i++) if (Math.abs(a[i][col]) > Math.abs(a[pivot][col])) pivot = i;
    [a[pivot], a[col]] = [a[col], a[pivot]];
    if (Math.abs(a[col][col]) < 1e-12) throw Error('Singular regression');
    const scale = a[col][col]; for (let j = col; j <= n; j++) a[col][j] /= scale;
    for (let i = 0; i < n; i++) if (i !== col) { const factor = a[i][col]; for (let j = col; j <= n; j++) a[i][j] -= factor * a[col][j]; }
  }
  return a.map(row => row[n]);
}
export function fitRidge(rows, lambda) {
  assert(rows.length >= 20); assert(Number.isFinite(lambda) && lambda > 0);
  const width = rows[0].x.length;
  const means = [0], scales = [1];
  for (let j = 1; j < width; j++) {
    const mean = rows.reduce((sum, r) => sum + r.x[j], 0) / rows.length;
    const sd = Math.sqrt(rows.reduce((sum, r) => sum + (r.x[j] - mean) ** 2, 0) / rows.length);
    means.push(mean); scales.push(sd > 1e-9 ? sd : 1);
  }
  const matrix = Array.from({ length: width }, () => new Array(width).fill(0)), vector = new Array(width).fill(0);
  for (const r of rows) {
    const x = r.x.map((value, j) => (value - means[j]) / scales[j]);
    for (let j = 0; j < width; j++) { vector[j] += x[j] * r.y; for (let k = 0; k < width; k++) matrix[j][k] += x[j] * x[k]; }
  }
  for (let j = 1; j < width; j++) matrix[j][j] += lambda;
  return { lambda, n: rows.length, trainedThroughYear: Math.max(...rows.map(r => r.year)), means, scales,
    coefficients: solve(matrix, vector) };
}
export function predict(model, x) { return x.reduce((sum, value, j) => sum + model.coefficients[j] * (value - model.means[j]) / model.scales[j], 0); }
export function expandingCheck(rows, lambda, lastYear, minimumTrainRows = 80) {
  const predictions = [];
  for (const year of [...new Set(rows.map(r => r.year))].filter(y => y <= lastYear)) {
    const train = rows.filter(r => r.year < year); if (train.length < minimumTrainRows) continue;
    const model = fitRidge(train, lambda);
    for (const r of rows.filter(r => r.year === year)) predictions.push({ key: r.key, year, trainedThroughYear: model.trainedThroughYear,
      predictedMargin: predict(model, r.x) + r.nationalBaseline, actualMargin: r.actualMargin,
      residual: r.y - predict(model, r.x), clocks: r.clocks });
  }
  return predictions;
}
export function metrics(predictions) {
  assert(predictions.length > 0);
  const n = predictions.length, mse = predictions.reduce((sum, r) => sum + r.residual ** 2, 0) / n;
  return { n, rmse: Math.sqrt(mse), mae: predictions.reduce((sum, r) => sum + Math.abs(r.residual), 0) / n,
    directionAccuracy: predictions.filter(r => (r.predictedMargin > 0) === (r.actualMargin > 0)).length / n,
    cycles: [...new Set(predictions.map(r => r.year))] };
}
// Fixed t4 distribution: CDF(t) = 1/2 + 3z/4 - z^3/4, z=t/sqrt(4+t^2).
export function probabilityFromMargin(margin, standardDeviation) {
  assert(Number.isFinite(margin) && Number.isFinite(standardDeviation) && standardDeviation > 0);
  const t = margin / (standardDeviation / Math.sqrt(2)), z = t / Math.sqrt(4 + t * t);
  return Math.min(0.99, Math.max(0.01, 0.5 + 0.75 * z - 0.25 * z ** 3));
}
export function scenarios({ model, features, environment, assessmentAtMs, residualRms, definition }) {
  assert(Number.isFinite(assessmentAtMs));
  const sourceAtMs = Date.parse(environment.sourceAt), observedAtMs = Date.parse(environment.observedAt);
  assert(Number.isFinite(sourceAtMs) && Number.isFinite(observedAtMs));
  assert(sourceAtMs <= observedAtMs && observedAtMs <= assessmentAtMs, 'Causal polling clocks');
  assert(assessmentAtMs - sourceAtMs <= environment.maximumAgeMs, 'Expired current environment');
  assert(environment.republicanPercent > 0 && environment.democraticPercent > 0);
  const nationalMargin = (environment.republicanPercent - environment.democraticPercent) / (environment.republicanPercent + environment.democraticPercent);
  const center = predict(model, features.x) + nationalMargin, residualSd = Math.max(0.10, residualRms);
  const variants = definition.pollingErrorSensitivityPoints.flatMap(error => definition.additionalNationalBiasStressPoints.map(bias => {
    const sd = Math.sqrt(residualSd ** 2 + (error / 100) ** 2), margin = center + bias / 100;
    return { nationalBiasPoints: bias, pollingRmsPoints: error, margin, standardDeviation: sd, winProbability: probabilityFromMargin(margin, sd) };
  }));
  return { meanMargin: center, nationalMargin, residualSd, variants,
    lower: Math.min(...variants.map(v => v.winProbability)), upper: Math.max(...variants.map(v => v.winProbability)),
    recordedAt: new Date(assessmentAtMs).toISOString(), sourceAt: environment.sourceAt,
    expiresAt: new Date(sourceAtMs + environment.maximumAgeMs).toISOString(),
    limitations: ['Scenario range is not a calibrated confidence interval.', 'Uniform national swing transport and residual distribution are assumptions.', 'Current polling has market dependence only if its stated sources do; this model does not import DDHQ win-probability outputs.'] };
}
