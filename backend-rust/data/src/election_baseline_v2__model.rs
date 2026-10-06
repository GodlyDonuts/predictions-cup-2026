// research/election-baseline-v2/model.mjs — revised Senate aggregation with
// primary/runoff overrides, entrant tracking and affiliation gating.
use crate::election_baseline_v1__model as v1;
use indexmap::IndexMap;
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(code.into()) } }
fn yes(x: &V) -> bool { x.as_str().unwrap_or("").eq_ignore_ascii_case("true") }
fn count(x: &V) -> Result<i64, String> {
    let s = match x { V::String(s) => s.clone(), V::Number(n) => n.to_string(), _ => String::new() };
    let n = if s.trim().is_empty() { 0.0 } else { s.trim().parse::<f64>().unwrap_or(f64::NAN) };
    need(n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 && n >= 0.0, "invalid_votes")?;
    Ok(n as i64)
}
fn js_str<'a>(r: &'a V, k: &str) -> &'a str { r[k].as_str().unwrap_or("") }
fn stage(x: &V) -> String { x.as_str().unwrap_or("").trim().to_lowercase() }
fn final_runoff(x: &V) -> bool { ["gen runoff", "runoff"].contains(&stage(x).as_str()) }
fn pseudo(x: &V) -> bool {
    let t = x.as_str().unwrap_or("").trim().to_lowercase();
    if t.is_empty() { return false; }
    if t == "overvotes" || t == "undervotes" || t.starts_with("total") || t.starts_with("blank")
        || t.starts_with("scattered") || t == "other" || t == "others" || t == "all others"
        || t.starts_with("none of") { return true; }
    // /^write[ -]?ins?$/
    let mut s = t.as_str();
    if let Some(rest) = s.strip_prefix("write") {
        let rest = rest.strip_prefix(|c| c == ' ' || c == '-').unwrap_or(rest);
        let _ = s;
        return rest == "in" || rest == "ins";
    }
    false
}

struct Cand { id: String, name: String, parties: Vec<String>, party_unknown: bool,
    lines: IndexMap<String, IndexMap<String, i64>> }

fn aggregate(rows: &[V], allow_uncompetitive_unknown: bool) -> Result<Vec<V>, String> {
    let mut candidates: IndexMap<String, Cand> = IndexMap::new();
    for r in rows {
        if js_str(r, "candidate").is_empty() || pseudo(&r["candidate"]) { continue; }
        if !allow_uncompetitive_unknown {
            need(!js_str(r, "party_detailed").trim().is_empty(), "missing_party_label")?;
        }
        let id = v1::identity(js_str(r, "candidate"));
        if id.is_empty() { continue; }
        let c = candidates.entry(id.clone()).or_insert_with(|| Cand {
            id: id.clone(), name: js_str(r, "candidate").to_string(), parties: Vec::new(),
            lines: IndexMap::new(), party_unknown: false });
        let line = format!("{}\u{0}{}", js_str(r, "party_detailed").to_uppercase(), yes(&r["writein"]));
        let simplified = js_str(r, "party_simplified").to_uppercase();
        if js_str(r, "party_detailed").trim().is_empty() && !["REPUBLICAN", "DEMOCRAT"].contains(&simplified.as_str()) {
            c.party_unknown = true;
        }
        if !c.parties.contains(&simplified) { c.parties.push(simplified); }
        let modes = c.lines.entry(line).or_default();
        let mode = { let m = js_str(r, "mode"); if m.is_empty() { "total".to_string() } else { m.to_lowercase() } };
        need(!modes.contains_key(&mode), "duplicate_ballot_line_mode")?;
        modes.insert(mode, count(&r["candidatevotes"])?);
    }
    let mut out = Vec::new();
    for (_, c) in candidates {
        need(!(c.parties.iter().any(|p| p == "REPUBLICAN") && c.parties.iter().any(|p| p == "DEMOCRAT")), "cross_major_fusion")?;
        let mut votes = 0i64;
        for (_, modes) in &c.lines {
            votes += if let Some(t) = modes.get("total") { *t } else { modes.values().sum() };
        }
        let republican = c.parties.iter().any(|p| p == "REPUBLICAN");
        let has_major = republican || c.parties.iter().any(|p| p == "DEMOCRAT");
        out.push(json!({"id": c.id, "name": c.name, "parties": c.parties, "republican": republican,
            "partyUnknown": c.party_unknown && !has_major, "votes": votes}));
    }
    Ok(out)
}

/// entrants(rows) — candidate identity, not vote rank, defines this pre-result feature.
fn entrants(rows: &[V]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for r in rows {
        let name = js_str(r, "candidate");
        if name.is_empty() || pseudo(&r["candidate"]) { continue; }
        let id = v1::identity(name);
        if !seen.contains(&id) { seen.push(id); }
    }
    seen
}

/// senateRaces(rows, overrides=[], {allowUncompetitiveUnknown=true}).
pub fn senate_races(rows: &[V], overrides: &[V], allow_uncompetitive_unknown: bool) -> Result<V, String> {
    let mut grouped: IndexMap<String, Vec<V>> = IndexMap::new();
    let mut races: IndexMap<String, V> = IndexMap::new();
    let mut exclusions: Vec<V> = Vec::new();
    let mut corrections: Vec<V> = Vec::new();
    let mut override_map: IndexMap<String, V> = IndexMap::new();
    for x in overrides { override_map.insert(x["key"].as_str().unwrap_or("").to_string(), x.clone()); }
    need(override_map.len() == overrides.len(), "duplicate_override")?;
    for r in rows {
        if js_str(r, "office") != "US SENATE" || yes(&r["special"])
            || !["gen", "pre", "gen runoff", "runoff"].contains(&stage(&r["stage"]).as_str()) { continue; }
        let y = match count(&r["year"]) { Ok(y) => y, Err(e) => return Err(e) };
        let cycle = if js_str(r, "state_po") == "GA" && y % 2 != 0 && final_runoff(&r["stage"]) { y - 1 } else { y };
        grouped.entry(format!("{}:{}", cycle, js_str(r, "state_po"))).or_default().push(r.clone());
    }
    for (key, group) in &grouped {
        match senate_race_one(key, group, override_map.get(key), allow_uncompetitive_unknown) {
            Ok((race, correction)) => {
                races.insert(key.clone(), race);
                if let Some(c) = correction { corrections.push(c); }
            }
            Err(reason) => exclusions.push(json!({"key": key, "reason": reason})),
        }
    }
    for x in overrides {
        need(grouped.contains_key(x["key"].as_str().unwrap_or("")), "override_missing_source_cycle")?;
    }
    Ok(json!({"races": races, "exclusions": exclusions, "corrections": corrections}))
}

fn senate_race_one(key: &str, group: &[V], override_def: Option<&V>, allow_uncompetitive_unknown: bool)
    -> Result<(V, Option<V>), String> {
    let mut split = key.splitn(2, ':');
    let year: i64 = split.next().unwrap_or("").parse().unwrap_or(0);
    let state = split.next().unwrap_or("").to_string();
    need(!(state == "AK" && year == 2010), "writein_winner_affiliation_unverified")?;
    let certified: Vec<V> = group.iter().filter(|r| !yes(&r["unofficial"])).cloned().collect();
    let first: Vec<V> = certified.iter().filter(|r| stage(&r["stage"]) == "pre").cloned().collect();
    let general: Vec<V> = certified.iter().filter(|r| stage(&r["stage"]) == "gen").cloned().collect();
    let first_entrants = entrants(if !first.is_empty() { &first } else { &general });
    let candidates: Vec<V>;
    let total: i64;
    let label_source: String;
    let selected_stage: String;
    let mut correction = None;
    if let Some(ov) = override_def {
        need(ov["sourceUrl"].as_str().is_some_and(|s| s.starts_with("https://www.fec.gov/"))
            && ov["finalElectionDate"].as_str().is_some()
            && ov["candidates"].as_array().is_some_and(|a| a.len() > 1), "invalid_primary_override")?;
        total = count(&ov["total"])?;
        let mut cs = Vec::new();
        for c in ov["candidates"].as_array().unwrap() {
            let party = c["party"].as_str().unwrap_or("");
            cs.push(json!({"id": v1::identity(c["name"].as_str().unwrap_or("")),
                "name": c["name"], "parties": [party], "republican": party == "REPUBLICAN",
                "votes": count(&c["votes"])?}));
        }
        need(cs.iter().map(|c| c["votes"].as_i64().unwrap_or(0)).sum::<i64>() == total, "override_total_mismatch")?;
        label_source = ov["sourceUrl"].as_str().unwrap().to_string();
        selected_stage = "primary_verified_final_override".to_string();
        candidates = cs;
        correction = Some(json!({"key": key, "reason": ov["reason"], "sourceUrl": label_source,
            "finalElectionDate": ov["finalElectionDate"]}));
    } else {
        let runoffs: Vec<V> = certified.iter().filter(|r| final_runoff(&r["stage"])).cloned().collect();
        let runoffs_present = !runoffs.is_empty();
        let selected = if runoffs_present { runoffs } else { general };
        need(!selected.is_empty(), "no_certified_final_stage")?;
        let mut stage_keys: Vec<String> = Vec::new();
        for r in &selected {
            let k = format!("{}:{}", js_str(r, "year"), stage(&r["stage"]));
            if !stage_keys.contains(&k) { stage_keys.push(k); }
        }
        need(stage_keys.len() == 1, "multiple_final_stages")?;
        candidates = aggregate(&selected, allow_uncompetitive_unknown)?;
        total = selected.iter().map(|r| count(&r["totalvotes"]).unwrap_or(0)).max().unwrap_or(0);
        selected_stage = stage(&selected[0]["stage"]);
        label_source = "MIT Senate source manifest".to_string();
        if ["GA", "LA"].contains(&state.as_str()) && !runoffs_present && first.is_empty() {
            need(candidates.iter().map(|c| c["votes"].as_i64().unwrap_or(0)).max().unwrap_or(0) as f64
                > total as f64 / 2.0, "majority_runoff_final_unverified")?;
        }
        // The present source does not encode RCV transfers as a comparable party margin.
        need(!(state == "AK" && year >= 2022), "ranked_choice_final_margin_unverified")?;
    }
    need(!first_entrants.is_empty(), "missing_first_round_entrants")?;
    need(total >= 100, "invalid_total")?;
    need(candidates.iter().map(|c| c["votes"].as_i64().unwrap_or(0)).sum::<i64>() as f64 <= total as f64 * 1.001,
        "candidate_total_exceeds_total")?;
    let mut r_sorted: Vec<V> = candidates.iter().filter(|c| c["republican"] == V::Bool(true)).cloned().collect();
    r_sorted.sort_by(|a, b| b["votes"].as_i64().cmp(&a["votes"].as_i64()));
    let r = r_sorted.first().cloned();
    let mut o_sorted: Vec<V> = candidates.iter()
        .filter(|c| c["republican"] != V::Bool(true) && c["partyUnknown"] != V::Bool(true)).cloned().collect();
    o_sorted.sort_by(|a, b| b["votes"].as_i64().cmp(&a["votes"].as_i64()));
    let opponent = o_sorted.first().cloned();
    need(r.as_ref().map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) > 0
        && opponent.as_ref().map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) > 0,
        "missing_comparable_party_opponent")?;
    let (r, opponent) = (r.unwrap(), opponent.unwrap());
    // A minor blank label cannot change the target if either possible affiliation
    // leaves it below both known finalists. Competitive uncertainty is excluded.
    let floor = r["votes"].as_i64().unwrap_or(0).min(opponent["votes"].as_i64().unwrap_or(0));
    need(candidates.iter().filter(|c| c["partyUnknown"] == V::Bool(true))
        .all(|c| c["votes"].as_i64().unwrap_or(0) < floor), "competitive_party_affiliation_unverified")?;
    need(r["votes"] != opponent["votes"], "tied_outcome")?;
    let mut winner = candidates[0].clone();
    for c in &candidates[1..] {
        if c["votes"].as_i64() > winner["votes"].as_i64() { winner = c.clone(); }
    }
    let margin = (r["votes"].as_f64().unwrap_or(0.0) - opponent["votes"].as_f64().unwrap_or(0.0))
        / (r["votes"].as_f64().unwrap_or(0.0) + opponent["votes"].as_f64().unwrap_or(0.0));
    need((margin > 0.0) == (winner["republican"] == V::Bool(true)), "party_outcome_inconsistent")?;
    Ok((json!({"year": year, "state": state, "republican": r, "opponent": opponent, "winner": winner,
        "margin": margin, "total": total, "entrants": first_entrants,
        "selectedStage": selected_stage, "labelSource": label_source}), correction))
}

/// raceFeatures({year,state,entrants}, senate, president).
/// `senate` is the v2 senateRaces result object ({races,...}); `president` is the v1 tables.
pub fn race_features(race: &V, senate: &V, president: &v1::PresidentialTables) -> Result<Option<V>, String> {
    let entrants_v = &race["entrants"];
    need(entrants_v.as_array().is_some_and(|a| !a.is_empty()), "explicit_entrant_list_required")?;
    let year = race["year"].as_i64().unwrap_or(0);
    let state = race["state"].as_str().unwrap_or("");
    let py = president.years.iter().copied().filter(|y| *y < year).last();
    let Some(py) = py else { return Ok(None) };
    let nat = president.national.get(&py).cloned().unwrap_or(V::Null);
    let sp = president.states.get(&format!("{}:{}", py, state)).cloned().unwrap_or(V::Null);
    let prior = senate["races"][&format!("{}:{}", year - 6, state)].clone();
    if sp.is_null() || nat.is_null() || prior.is_null() { return Ok(None); }
    let old_py = president.years.iter().copied().filter(|y| *y < prior["year"].as_i64().unwrap_or(0)).last();
    need(old_py.is_some(), "missing_prior_national")?;
    let old_py = old_py.unwrap();
    let old_nat = president.national.get(&old_py).cloned().unwrap_or(V::Null);
    need(!old_nat.is_null(), "missing_prior_national")?;
    let entrant_ids: Vec<String> = entrants_v.as_array().unwrap().iter()
        .map(|e| v1::identity(e.as_str().unwrap_or(""))).collect();
    let running = entrant_ids.iter().any(|e| e == prior["winner"]["id"].as_str().unwrap_or(""));
    let incumbent = if running { if prior["winner"]["republican"] == V::Bool(true) { 1 } else { -1 } } else { 0 };
    Ok(Some(json!({
        "x": [1.0, sp["margin"].as_f64().unwrap_or(0.0) - nat["margin"].as_f64().unwrap_or(0.0),
            prior["margin"].as_f64().unwrap_or(0.0) - old_nat["margin"].as_f64().unwrap_or(0.0), incumbent],
        "nationalBaseline": nat["margin"], "incumbent": incumbent,
        "clocks": {"presYear": py, "priorSeatYear": prior["year"], "priorSeatPresYear": old_py}})))
}

/// buildRows(senate, president, minimumYear=1990) — v2 row shape adds winnerRepublican.
pub fn build_rows(senate: &V, president: &v1::PresidentialTables, minimum_year: Option<i64>) -> Result<Vec<V>, String> {
    let minimum_year = minimum_year.unwrap_or(1990);
    let mut out = Vec::new();
    if let V::Object(races) = &senate["races"] {
        for (_, r) in races {
            if r["year"].as_i64().unwrap_or(0) < minimum_year { continue; }
            if let Some(f) = race_features(r, senate, president)? {
                out.push(json!({"key": format!("{}:{}", r["year"].as_i64().unwrap_or(0), r["state"].as_str().unwrap_or("")),
                    "year": r["year"], "state": r["state"], "x": f["x"],
                    "y": r["margin"].as_f64().unwrap_or(0.0) - f["nationalBaseline"].as_f64().unwrap_or(0.0),
                    "actualMargin": r["margin"], "nationalBaseline": f["nationalBaseline"],
                    "clocks": f["clocks"], "winnerRepublican": r["winner"]["republican"]}));
            }
        }
    }
    out.sort_by(|a, b| a["year"].as_i64().cmp(&b["year"].as_i64())
        .then_with(|| a["state"].as_str().unwrap_or("").cmp(b["state"].as_str().unwrap_or(""))));
    Ok(out)
}
