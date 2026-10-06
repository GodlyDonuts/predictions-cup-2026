// research/election-baseline-v2/model.mjs — affiliation-invariant Senate race
// reconstruction (runoff/override handling) over the v1 feature/ridge stack.
use crate::election_baseline_v1__model as v1;
use crate::js::{field_str, js_number, js_string, js_template, num, or_empty, require_that};
use indexmap::IndexMap;
use serde_json::{json, Value as V};

pub use crate::election_baseline_v1__model::{
    expanding_check, fit_ridge, hash, metrics, parse_csv, predict, presidential_tables, probability_from_margin,
    PresidentTables,
};

fn yes(x: &V) -> bool {
    js_string(x).to_lowercase() == "true"
}
fn count(x: Option<&V>) -> Result<i64, String> {
    let n = crate::js::js_number(x);
    require_that(
        n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 && n >= 0.0,
        "invalid_votes",
    )?;
    Ok(n as i64)
}
fn stage(x: &V) -> String {
    js_string(x).trim().to_lowercase()
}
fn final_runoff(x: &V) -> bool {
    ["gen runoff", "runoff"].contains(&stage(x).as_str())
}
fn pseudo(x: &V) -> bool {
    let s = js_string(x).trim().to_lowercase();
    // /^(overvotes|undervotes|total.*|blank.*|scattered.*|write[ -]?ins?|others?|all others|none of.*)$/i
    s == "overvotes"
        || s == "undervotes"
        || s.starts_with("total")
        || s.starts_with("blank")
        || s.starts_with("scattered")
        || s == "writein"
        || s == "writeins"
        || s == "write-in"
        || s == "write-ins"
        || s == "write ins"
        || s == "other"
        || s == "others"
        || s == "all others"
        || s.starts_with("none of")
}

struct Acc {
    id: String,
    name: String,
    parties: Vec<String>,
    lines: IndexMap<String, IndexMap<String, i64>>,
    party_unknown: bool,
}

fn aggregate(rows: &[&V], allow_uncompetitive_unknown: bool) -> Result<Vec<V>, String> {
    let mut candidates: IndexMap<String, Acc> = IndexMap::new();
    for r in rows {
        let cand = field_str(r, "candidate");
        if cand.is_empty() || pseudo(r.get("candidate").unwrap_or(&V::Null)) {
            continue;
        }
        if !allow_uncompetitive_unknown {
            require_that(!or_empty(r.get("party_detailed")).trim().is_empty(), "missing_party_label")?;
        }
        let id = v1::identity(cand);
        if id.is_empty() {
            continue;
        }
        let entry = candidates.entry(id.clone()).or_insert_with(|| Acc {
            id,
            name: js_string(r.get("candidate").unwrap_or(&V::Null)),
            parties: Vec::new(),
            lines: IndexMap::new(),
            party_unknown: false,
        });
        let line = format!(
            "{}\u{0}{}",
            or_empty(r.get("party_detailed")).to_uppercase(),
            yes(&r["writein"])
        );
        if or_empty(r.get("party_detailed")).trim().is_empty()
            && !["REPUBLICAN", "DEMOCRAT"].contains(&js_template(r.get("party_simplified")).to_uppercase().as_str())
        {
            entry.party_unknown = true;
        }
        let simplified = js_template(r.get("party_simplified")).to_uppercase();
        if !entry.parties.contains(&simplified) {
            entry.parties.push(simplified);
        }
        let modes = entry.lines.entry(line).or_default();
        let mode = {
            let m = js_string(&r["mode"]);
            let m = if m.is_empty() { "total".to_string() } else { m };
            m.to_lowercase()
        };
        require_that(!modes.contains_key(&mode), "duplicate_ballot_line_mode")?;
        modes.insert(mode, count(r.get("candidatevotes"))?);
    }
    candidates
        .into_values()
        .map(|c| {
            let has_r = c.parties.iter().any(|p| p == "REPUBLICAN");
            let has_d = c.parties.iter().any(|p| p == "DEMOCRAT");
            require_that(!(has_r && has_d), "cross_major_fusion")?;
            let votes: i64 = c
                .lines
                .values()
                .map(|modes| {
                    if let Some(t) = modes.get("total") {
                        *t
                    } else {
                        modes.values().sum()
                    }
                })
                .sum();
            Ok(json!({
                "id": c.id,
                "name": c.name,
                "parties": c.parties,
                "republican": has_r,
                "partyUnknown": c.party_unknown && !has_r && !has_d,
                "votes": votes,
            }))
        })
        .collect()
}

// Candidate identity, not vote rank, defines this pre-result feature.
fn entrants(rows: &[&V]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for r in rows {
        let cand = field_str(r, "candidate");
        if cand.is_empty() || pseudo(r.get("candidate").unwrap_or(&V::Null)) {
            continue;
        }
        let id = v1::identity(cand);
        if !id.is_empty() && !seen.contains(&id) {
            seen.push(id);
        }
    }
    seen
}

pub struct SenateRacesV2 {
    pub races: IndexMap<String, V>,
    pub exclusions: Vec<V>,
    pub corrections: Vec<V>,
}

pub fn senate_races_v2(rows: &[V], overrides: &[V], allow_uncompetitive_unknown: bool) -> Result<SenateRacesV2, String> {
    let mut grouped: IndexMap<String, Vec<&V>> = IndexMap::new();
    let mut races: IndexMap<String, V> = IndexMap::new();
    let mut exclusions: Vec<V> = Vec::new();
    let mut corrections: Vec<V> = Vec::new();
    let mut override_map: IndexMap<String, &V> = IndexMap::new();
    for x in overrides {
        override_map.insert(field_str(x, "key").to_string(), x);
    }
    require_that(override_map.len() == overrides.len(), "duplicate_override")?;
    for r in rows {
        if field_str(r, "office") != "US SENATE"
            || yes(&r["special"])
            || !["gen", "pre", "gen runoff", "runoff"].contains(&stage(&r["stage"]).as_str())
        {
            continue;
        }
        let y = count(r.get("year"))?;
        let cycle = if field_str(r, "state_po") == "GA" && y % 2 != 0 && final_runoff(&r["stage"]) {
            y - 1
        } else {
            y
        };
        let key = format!("{}:{}", cycle, field_str(r, "state_po"));
        grouped.entry(key).or_default().push(r);
    }
    for (key, group) in &grouped {
        match senate_race_v2(key, group, &override_map, allow_uncompetitive_unknown, &mut corrections) {
            Ok(race) => {
                races.insert(key.clone(), race);
            }
            Err(reason) => exclusions.push(json!({"key": key, "reason": reason})),
        }
    }
    for x in overrides {
        require_that(grouped.contains_key(field_str(x, "key")), "override_missing_source_cycle")?;
    }
    Ok(SenateRacesV2 { races, exclusions, corrections })
}

fn senate_race_v2(
    key: &str,
    group: &[&V],
    override_map: &IndexMap<String, &V>,
    allow_uncompetitive_unknown: bool,
    corrections: &mut Vec<V>,
) -> Result<V, String> {
    let mut it = key.split(':');
    let year = crate::js::js_number_str(it.next().unwrap_or(""));
    let state = it.next().unwrap_or("").to_string();
    let override_row = override_map.get(key).copied();
    require_that(!(state == "AK" && year == 2010.0), "writein_winner_affiliation_unverified")?;
    let certified: Vec<&V> = group.iter().copied().filter(|r| !yes(&r["unofficial"])).collect();
    let first: Vec<&V> = certified.iter().copied().filter(|r| stage(&r["stage"]) == "pre").collect();
    let general: Vec<&V> = certified.iter().copied().filter(|r| stage(&r["stage"]) == "gen").collect();
    let first_entrants = entrants(if !first.is_empty() { &first } else { &general });
    let candidates: Vec<V>;
    let total: i64;
    let label_source: String;
    let selected_stage: String;
    if let Some(o) = override_row {
        require_that(
            field_str(o, "sourceUrl").starts_with("https://www.fec.gov/")
                && o.get("finalElectionDate").is_some_and(|v| js_truthy_nonzero(v))
                && o["candidates"].as_array().map(|a| a.len() > 1).unwrap_or(false),
            "invalid_primary_override",
        )?;
        total = count(o.get("total"))?;
        let mut cands: Vec<V> = Vec::new();
        let mut sum: i64 = 0;
        for c in o["candidates"].as_array().cloned().unwrap_or_default() {
            let name = js_string(c.get("name").unwrap_or(&V::Null));
            let party = js_string(c.get("party").unwrap_or(&V::Null));
            let votes = count(c.get("votes"))?;
            sum += votes;
            cands.push(json!({
                "id": v1::identity(&name),
                "name": name,
                "parties": [party],
                "republican": c["party"].as_str() == Some("REPUBLICAN"),
                "votes": votes,
            }));
        }
        require_that(sum == total, "override_total_mismatch")?;
        candidates = cands;
        label_source = js_string(o.get("sourceUrl").unwrap_or(&V::Null));
        selected_stage = "primary_verified_final_override".to_string();
        corrections.push(json!({
            "key": key,
            "reason": o["reason"],
            "sourceUrl": label_source,
            "finalElectionDate": o["finalElectionDate"],
        }));
    } else {
        let runoffs: Vec<&V> = certified.iter().copied().filter(|r| final_runoff(&r["stage"])).collect();
        let has_runoffs = !runoffs.is_empty();
        let selected = if has_runoffs { runoffs } else { general };
        require_that(!selected.is_empty(), "no_certified_final_stage")?;
        let stage_keys: std::collections::HashSet<String> = selected
            .iter()
            .map(|r| format!("{}:{}", js_template(r.get("year")), stage(&r["stage"])))
            .collect();
        require_that(stage_keys.len() == 1, "multiple_final_stages")?;
        let agg = aggregate(&selected, allow_uncompetitive_unknown)?;
        total = selected
            .iter()
            .map(|r| count(r.get("totalvotes")))
            .collect::<Result<Vec<i64>, _>>()?
            .into_iter()
            .max()
            .unwrap_or(0);
        selected_stage = stage(&selected[0]["stage"]);
        label_source = "MIT Senate source manifest".to_string();
        if ["GA", "LA"].contains(&state.as_str()) && !has_runoffs && first.is_empty() {
            require_that(
                agg.iter().map(|c| c["votes"].as_i64().unwrap_or(0)).max().unwrap_or(0) as f64 > total as f64 / 2.0,
                "majority_runoff_final_unverified",
            )?;
        }
        // The present source does not encode RCV transfers as a comparable party margin.
        require_that(!(state == "AK" && year >= 2022.0), "ranked_choice_final_margin_unverified")?;
        candidates = agg;
    }
    require_that(!first_entrants.is_empty(), "missing_first_round_entrants")?;
    require_that(total >= 100, "invalid_total")?;
    require_that(
        candidates.iter().map(|c| c["votes"].as_i64().unwrap_or(0)).sum::<i64>() as f64 <= total as f64 * 1.001,
        "candidate_total_exceeds_total",
    )?;
    let mut rep: Vec<&V> = candidates.iter().filter(|c| c["republican"].as_bool().unwrap_or(false)).collect();
    rep.sort_by(|a, b| b["votes"].as_i64().cmp(&a["votes"].as_i64()));
    let mut opp: Vec<&V> = candidates
        .iter()
        .filter(|c| !c["republican"].as_bool().unwrap_or(false) && !c["partyUnknown"].as_bool().unwrap_or(false))
        .collect();
    opp.sort_by(|a, b| b["votes"].as_i64().cmp(&a["votes"].as_i64()));
    let r = rep.first().copied();
    let opponent = opp.first().copied();
    require_that(
        r.map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) > 0
            && opponent.map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) > 0,
        "missing_comparable_party_opponent",
    )?;
    let r = r.unwrap();
    let opponent = opponent.unwrap();
    // A minor blank label cannot change the target if either possible affiliation
    // leaves it below both known finalists. Competitive uncertainty is excluded.
    require_that(
        candidates
            .iter()
            .filter(|c| c["partyUnknown"].as_bool().unwrap_or(false))
            .all(|c| {
                c["votes"].as_i64().unwrap_or(0)
                    < r["votes"].as_i64().unwrap_or(0).min(opponent["votes"].as_i64().unwrap_or(0))
            }),
        "competitive_party_affiliation_unverified",
    )?;
    require_that(
        r["votes"].as_i64().unwrap_or(0) != opponent["votes"].as_i64().unwrap_or(0),
        "tied_outcome",
    )?;
    let winner = candidates
        .iter()
        .fold(None::<&V>, |a, b| {
            if a.map(|a| b["votes"].as_i64().unwrap_or(0) > a["votes"].as_i64().unwrap_or(0)).unwrap_or(true) {
                Some(b)
            } else {
                a
            }
        })
        .unwrap();
    let margin = (r["votes"].as_i64().unwrap_or(0) as f64 - opponent["votes"].as_i64().unwrap_or(0) as f64)
        / (r["votes"].as_i64().unwrap_or(0) as f64 + opponent["votes"].as_i64().unwrap_or(0) as f64);
    require_that(
        (margin > 0.0) == winner["republican"].as_bool().unwrap_or(false),
        "party_outcome_inconsistent",
    )?;
    Ok(json!({
        "year": num(year),
        "state": state,
        "republican": r,
        "opponent": opponent,
        "winner": winner,
        "margin": num(margin),
        "total": total,
        "entrants": first_entrants,
        "selectedStage": selected_stage,
        "labelSource": label_source,
    }))
}

// JS truthy check on a field that may be a string/date; used only for presence.
fn js_truthy_nonzero(v: &V) -> bool {
    crate::js::js_truthy(Some(v))
}

pub fn race_features_v2(args: &V, senate: &SenateRacesV2, president: &PresidentTables) -> Result<Option<V>, String> {
    let entrants_field = args.get("entrants").and_then(V::as_array);
    require_that(
        entrants_field.is_some_and(|a| !a.is_empty()),
        "explicit_entrant_list_required",
    )?;
    let year = js_number(args.get("year"));
    let state = field_str(args, "state");
    let py = match president.years.iter().copied().filter(|y| (*y as f64) < year).last() {
        Some(y) => y,
        None => return Ok(v1_race_features_null()),
    };
    let nat = president.national.get(&py).cloned().unwrap_or(V::Null);
    let sp = president.states.get(&format!("{}:{}", py, state));
    let prior = senate.races.get(&format!("{}:{}", year as i64 - 6, state));
    if sp.is_none() || nat.is_null() || prior.is_none() {
        return Ok(None);
    }
    let sp = sp.unwrap();
    let prior = prior.unwrap();
    let old_py = president
        .years
        .iter()
        .copied()
        .filter(|y| (*y as f64) < js_number(prior.get("year")))
        .last();
    let old_nat = old_py.and_then(|y| president.national.get(&y));
    require_that(old_nat.is_some(), "missing_prior_national")?;
    let old_py = old_py.unwrap();
    let old_nat = old_nat.unwrap();
    let running = entrants_field
        .unwrap()
        .iter()
        .map(|e| v1::identity(&js_string(e)))
        .any(|id| id == field_str(&prior["winner"], "id"));
    let incumbent = if running {
        if prior["winner"]["republican"].as_bool().unwrap_or(false) {
            1
        } else {
            -1
        }
    } else {
        0
    };
    Ok(Some(json!({
        "x": [
            V::from(1),
            num(sp["margin"].as_f64().unwrap_or(f64::NAN) - nat["margin"].as_f64().unwrap_or(f64::NAN)),
            num(prior["margin"].as_f64().unwrap_or(f64::NAN) - old_nat["margin"].as_f64().unwrap_or(f64::NAN)),
            V::from(incumbent),
        ],
        "nationalBaseline": nat["margin"],
        "incumbent": incumbent,
        "clocks": {
            "presYear": py,
            "priorSeatYear": prior["year"],
            "priorSeatPresYear": old_py,
        },
    })))
}

fn v1_race_features_null() -> Option<V> {
    None
}

pub fn build_rows_v2(senate: &SenateRacesV2, president: &PresidentTables, minimum_year: f64) -> Result<Vec<V>, String> {
    let mut out: Vec<V> = Vec::new();
    for r in senate.races.values() {
        if js_number(r.get("year")) < minimum_year {
            continue;
        }
        let f = race_features_v2(r, senate, president)?;
        if let Some(f) = f {
            out.push(json!({
                "key": format!("{}:{}", js_template(r.get("year")), js_template(r.get("state"))),
                "year": r["year"],
                "state": r["state"],
                "x": f["x"],
                "y": num(r["margin"].as_f64().unwrap_or(f64::NAN) - f["nationalBaseline"].as_f64().unwrap_or(f64::NAN)),
                "actualMargin": r["margin"],
                "nationalBaseline": f["nationalBaseline"],
                "clocks": f["clocks"],
                "winnerRepublican": r["winner"]["republican"],
            }));
        }
    }
    out.sort_by(|a, b| {
        js_number(a.get("year"))
            .partial_cmp(&js_number(b.get("year")))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| field_str(a, "state").cmp(field_str(b, "state")))
    });
    Ok(out)
}
