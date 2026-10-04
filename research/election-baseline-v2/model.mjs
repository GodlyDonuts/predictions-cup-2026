import assert from 'node:assert/strict';
import { identity } from '../election-baseline-v1/model.mjs';
export { parseCsv, hash, presidentialTables, fitRidge, predict, expandingCheck, metrics, probabilityFromMargin } from '../election-baseline-v1/model.mjs';

const yes = x => String(x).toLowerCase() === 'true';
const count = x => { const n = Number(x); assert(Number.isSafeInteger(n) && n >= 0, 'invalid_votes'); return n; };
const stage = x => String(x).trim().toLowerCase();
const finalRunoff = x => ['gen runoff', 'runoff'].includes(stage(x));
const pseudo = x => /^(overvotes|undervotes|total.*|blank.*|scattered.*|write[ -]?ins?|others?|all others|none of.*)$/i.test(String(x).trim());

function aggregate(rows, allowUncompetitiveUnknown) {
  const candidates = new Map();
  for (const r of rows) {
    if (!r.candidate || pseudo(r.candidate)) continue;
    if(!allowUncompetitiveUnknown)assert(String(r.party_detailed || '').trim(),'missing_party_label');
    const id = identity(r.candidate); if (!id) continue;
    if (!candidates.has(id)) candidates.set(id, { id, name: r.candidate, parties: new Set(), lines: new Map(), partyUnknown:false });
    const c = candidates.get(id), line = `${String(r.party_detailed || '').toUpperCase()}\0${yes(r.writein)}`;
    if(!String(r.party_detailed || '').trim() && !['REPUBLICAN','DEMOCRAT'].includes(String(r.party_simplified).toUpperCase()))c.partyUnknown=true;
    c.parties.add(String(r.party_simplified).toUpperCase());
    if (!c.lines.has(line)) c.lines.set(line, new Map());
    const modes = c.lines.get(line), mode = String(r.mode || 'total').toLowerCase();
    assert(!modes.has(mode), 'duplicate_ballot_line_mode'); modes.set(mode, count(r.candidatevotes));
  }
  return [...candidates.values()].map(c => {
    assert(!(c.parties.has('REPUBLICAN') && c.parties.has('DEMOCRAT')), 'cross_major_fusion');
    return { id: c.id, name: c.name, parties: [...c.parties], republican: c.parties.has('REPUBLICAN'), partyUnknown:c.partyUnknown && !c.parties.has('REPUBLICAN') && !c.parties.has('DEMOCRAT'),
      votes: [...c.lines.values()].reduce((s, modes) => s + (modes.has('total') ? modes.get('total') : [...modes.values()].reduce((a, b) => a + b, 0)), 0) };
  });
}
function entrants(rows) {
  // Candidate identity, not vote rank, defines this pre-result feature.
  return [...new Set(rows.filter(r => r.candidate && !pseudo(r.candidate)).map(r => identity(r.candidate)))];
}

export function senateRaces(rows, overrides = [], {allowUncompetitiveUnknown=true} = {}) {
  const grouped = new Map(), races = new Map(), exclusions = [], corrections = [];
  const overrideMap = new Map(overrides.map(x => [x.key, x])); assert(overrideMap.size === overrides.length, 'duplicate_override');
  for (const r of rows) {
    if (r.office !== 'US SENATE' || yes(r.special) || !['gen', 'pre', 'gen runoff', 'runoff'].includes(stage(r.stage))) continue;
    const y = count(r.year), cycle = r.state_po === 'GA' && y % 2 && finalRunoff(r.stage) ? y - 1 : y;
    const key = `${cycle}:${r.state_po}`;
    if (!grouped.has(key)) grouped.set(key, []); grouped.get(key).push(r);
  }
  for (const [key, group] of grouped) {
    try {
      const [yearText, state] = key.split(':'), year = Number(yearText), override = overrideMap.get(key);
      assert(!(state === 'AK' && year === 2010), 'writein_winner_affiliation_unverified');
      const certified = group.filter(r => !yes(r.unofficial));
      const first = certified.filter(r => stage(r.stage) === 'pre');
      const general = certified.filter(r => stage(r.stage) === 'gen');
      const firstEntrants = entrants(first.length ? first : general);
      let candidates, total, labelSource, selectedStage;
      if (override) {
        assert(override.sourceUrl?.startsWith('https://www.fec.gov/') && override.finalElectionDate && override.candidates.length > 1, 'invalid_primary_override');
        total = count(override.total);
        candidates = override.candidates.map(c => ({ id: identity(c.name), name: c.name, parties: [c.party], republican: c.party === 'REPUBLICAN', votes: count(c.votes) }));
        assert(candidates.reduce((s, c) => s + c.votes, 0) === total, 'override_total_mismatch');
        labelSource = override.sourceUrl; selectedStage = 'primary_verified_final_override';
        corrections.push({ key, reason: override.reason, sourceUrl: labelSource, finalElectionDate: override.finalElectionDate });
      } else {
        const runoffs = certified.filter(r => finalRunoff(r.stage));
        const selected = runoffs.length ? runoffs : general;
        assert(selected.length, 'no_certified_final_stage');
        assert(new Set(selected.map(r => `${r.year}:${stage(r.stage)}`)).size === 1, 'multiple_final_stages');
        candidates = aggregate(selected,allowUncompetitiveUnknown); total = Math.max(...selected.map(r => count(r.totalvotes)));
        selectedStage = stage(selected[0].stage); labelSource = 'MIT Senate source manifest';
        if (['GA', 'LA'].includes(state) && !runoffs.length && !first.length) {
          assert(Math.max(...candidates.map(c => c.votes)) > total / 2, 'majority_runoff_final_unverified');
        }
        // The present source does not encode RCV transfers as a comparable party margin.
        assert(!(state === 'AK' && year >= 2022), 'ranked_choice_final_margin_unverified');
      }
      assert(firstEntrants.length > 0, 'missing_first_round_entrants');
      assert(total >= 100, 'invalid_total');
      assert(candidates.reduce((s, c) => s + c.votes, 0) <= total * 1.001, 'candidate_total_exceeds_total');
      const r = candidates.filter(c => c.republican).sort((a, b) => b.votes - a.votes)[0];
      const opponent = candidates.filter(c => !c.republican && !c.partyUnknown).sort((a, b) => b.votes - a.votes)[0];
      assert(r?.votes > 0 && opponent?.votes > 0, 'missing_comparable_party_opponent');
      // A minor blank label cannot change the target if either possible affiliation
      // leaves it below both known finalists. Competitive uncertainty is excluded.
      assert(candidates.filter(c=>c.partyUnknown).every(c=>c.votes<Math.min(r.votes,opponent.votes)), 'competitive_party_affiliation_unverified');
      assert(r.votes !== opponent.votes, 'tied_outcome');
      const winner = candidates.reduce((a, b) => b.votes > a.votes ? b : a);
      const margin = (r.votes - opponent.votes) / (r.votes + opponent.votes);
      assert((margin > 0) === winner.republican, 'party_outcome_inconsistent');
      races.set(key, { year, state, republican: r, opponent, winner, margin, total, entrants: firstEntrants, selectedStage, labelSource });
    } catch (e) { exclusions.push({ key, reason: e.message }); }
  }
  for (const x of overrides) assert(grouped.has(x.key), 'override_missing_source_cycle');
  return { races, exclusions, corrections };
}

export function raceFeatures({ year, state, entrants }, senate, president) {
  assert(Array.isArray(entrants) && entrants.length, 'explicit_entrant_list_required');
  const py = president.years.filter(y => y < year).at(-1), nat = president.national.get(py);
  const sp = president.states.get(`${py}:${state}`), prior = senate.races.get(`${year - 6}:${state}`);
  if (!sp || !nat || !prior) return null;
  const oldPy = president.years.filter(y => y < prior.year).at(-1), oldNat = president.national.get(oldPy);
  assert(oldNat, 'missing_prior_national');
  const running = entrants.map(identity).includes(prior.winner.id), incumbent = running ? (prior.winner.republican ? 1 : -1) : 0;
  return { x: [1, sp.margin - nat.margin, prior.margin - oldNat.margin, incumbent],
    nationalBaseline: nat.margin, incumbent, clocks: { presYear: py, priorSeatYear: prior.year, priorSeatPresYear: oldPy } };
}
export function buildRows(senate, president, minimumYear = 1990) {
  return [...senate.races.values()].filter(r => r.year >= minimumYear).flatMap(r => {
    const f = raceFeatures(r, senate, president);
    return f ? [{ key: `${r.year}:${r.state}`, year: r.year, state: r.state, x: f.x, y: r.margin - f.nationalBaseline,
      actualMargin: r.margin, nationalBaseline: f.nationalBaseline, clocks: f.clocks, winnerRepublican: r.winner.republican }] : [];
  }).sort((a, b) => a.year - b.year || a.state.localeCompare(b.state));
}
