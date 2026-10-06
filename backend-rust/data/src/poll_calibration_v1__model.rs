// research/poll-calibration-v1/model.mjs — poll observation filtering,
// race-level aggregation and fixed-t4 error calibration.
use crate::election_baseline_v1__model::{identity, probability_from_margin};
use indexmap::IndexMap;
use serde_json::{json, Value as V};

const DAY: f64 = 86_400_000.0;

fn need(ok: bool, code: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(code.into()) } }
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => { let t = s.trim(); if t.is_empty() { 0.0 } else { t.parse().unwrap_or(f64::NAN) } }
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        _ => f64::NAN,
    }
}
fn is_safe(n: f64) -> bool { n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 }
fn name(n: &str) -> String {
    identity(n).split(' ').filter(|t| t.len() > 1).collect::<Vec<_>>().join(" ")
}
fn date(s: &V) -> f64 {
    let s = s.as_str().unwrap_or("");
    let ok = s.len() == 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-'
        && s.bytes().enumerate().all(|(i, c)| if i == 4 || i == 7 { true } else { c.is_ascii_digit() });
    if ok { baal_service_runtime::contract::parse_iso_ms(&format!("{}T12:00:00Z", s)).unwrap_or(f64::NAN) } else { f64::NAN }
}
fn digits(s: &V) -> bool { s.as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())) }
fn js_str<'a>(v: &'a V, k: &str) -> &'a str { v[k].as_str().unwrap_or("") }

fn deep_equal(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::Array(x), V::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| deep_equal(a, b)),
        (V::Object(x), V::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| deep_equal(v, w)))
        }
        _ => a == b,
    }
}

/// pollObservations(raw, senate) — senate is the v2 senateRaces result object.
pub fn poll_observations(raw: &[V], senate: &V) -> Result<V, String> {
    let mut rows: Vec<V> = Vec::new();
    let mut excluded: IndexMap<String, i64> = IndexMap::new();
    let mut seen: IndexMap<String, V> = IndexMap::new();
    macro_rules! reject {
        ($reason:expr) => {{ *excluded.entry($reason.to_string()).or_insert(0) += 1; }};
    }
    for p in raw {
        if js_str(p, "type_simple") != "Sen-G" { continue; }
        let year = js_num(&p["cycle"]);
        let key = format!("{}:{}", if is_safe(year) { format!("{}", year as i64) } else { "NaN".to_string() }, js_str(p, "location"));
        let race = &senate["races"][&key];
        if !is_safe(year) || year as i64 % 2 != 0 || race.is_null()
            || race["selectedStage"].as_str() != Some("gen")
            || !race["opponent"]["parties"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some("DEMOCRAT"))) {
            reject!("not_regular_comparable_major_party_final"); continue;
        }
        if js_str(p, "cand1_party") != "DEM" || js_str(p, "cand2_party") != "REP"
            || name(js_str(p, "cand1_name")) != name(race["opponent"]["name"].as_str().unwrap_or(""))
            || name(js_str(p, "cand2_name")) != name(race["republican"]["name"].as_str().unwrap_or("")) {
            reject!("final_candidate_identity_unmatched"); continue;
        }
        if !js_str(p, "partisan").is_empty() && js_str(p, "partisan") != "NA" {
            reject!("partisan_or_internal"); continue;
        }
        let field_at = date(&p["polldate"]);
        let election_at = date(&p["electiondate"]);
        let horizon = (election_at - field_at) / DAY;
        if !(horizon.is_finite() && horizon > 0.0)
            || !js_str(p, "electiondate").starts_with(&format!("{}-11-", year as i64))
            || (horizon - js_num(&p["time_to_election"])).abs() > 1.0 {
            reject!("election_or_field_clock_invalid"); continue;
        }
        let d = js_num(&p["cand1_pct"]);
        let r = js_num(&p["cand2_pct"]);
        let actual_d = js_num(&p["cand1_actual"]);
        let actual_r = js_num(&p["cand2_actual"]);
        if ![d, r, actual_d, actual_r].iter().all(|x| x.is_finite() && *x > 0.0 && *x < 100.0)
            || d + r > 100.001 {
            reject!("invalid_vote_shares"); continue;
        }
        let verified_d = 100.0 * race["opponent"]["votes"].as_f64().unwrap_or(0.0) / race["total"].as_f64().unwrap_or(1.0);
        let verified_r = 100.0 * race["republican"]["votes"].as_f64().unwrap_or(0.0) / race["total"].as_f64().unwrap_or(1.0);
        if (actual_d - verified_d).abs() > 0.25 || (actual_r - verified_r).abs() > 0.25
            || ((actual_d > actual_r) != !(race["winner"]["republican"] == V::Bool(true))) {
            reject!("independent_final_outcome_disagrees"); continue;
        }
        if !digits(&p["question_id"]) || !digits(&p["poll_id"]) || !digits(&p["pollster_rating_id"]) {
            reject!("source_identity_missing"); continue;
        }
        let id = format!("{}:{}", js_str(p, "race_id"), js_str(p, "question_id"));
        let record = json!({"id": id, "key": key, "year": year as i64, "state": js_str(p, "location"),
            "pollId": js_str(p, "poll_id"), "questionId": js_str(p, "question_id"),
            "pollsterId": js_str(p, "pollster_rating_id"), "pollster": js_str(p, "pollster"),
            "fieldDate": js_str(p, "polldate"), "fieldAt": field_at,
            "electionDate": js_str(p, "electiondate"), "electionAt": election_at,
            "horizonDays": horizon, "democratic": d, "republican": r, "remaining": 100.0 - d - r,
            "pollMargin": d - r, "actualMargin": verified_d - verified_r,
            "democraticWon": !(race["winner"]["republican"] == V::Bool(true)),
            "population": "not_in_archive", "sourceFinalMargin": actual_d - actual_r});
        if let Some(old) = seen.get(&id) {
            need(deep_equal(&record, old), "conflicting_source_question")?;
            reject!("identical_duplicate"); continue;
        }
        seen.insert(id, record.clone());
        rows.push(record);
    }
    rows.sort_by(|a, b| a["year"].as_i64().cmp(&b["year"].as_i64())
        .then_with(|| a["key"].as_str().unwrap_or("").cmp(b["key"].as_str().unwrap_or("")))
        .then_with(|| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or(""))));
    Ok(json!({"rows": rows, "excluded": excluded}))
}

/// aggregateRacePolls(observations, definition).
pub fn aggregate_race_polls(observations: &[V], definition: &V) -> Result<Vec<V>, String> {
    let fh = definition["forecastHorizonDays"].as_f64().unwrap_or(f64::NAN);
    let ma = definition["maximumPollAgeDays"].as_f64().unwrap_or(f64::NAN);
    let hl = definition["recencyHalfLifeDays"].as_f64().unwrap_or(f64::NAN);
    need(fh.is_finite() && fh > 0.0 && ma.is_finite() && ma > 0.0 && hl.is_finite() && hl > 0.0, "assertion failed")?;
    let mut grouped: IndexMap<String, IndexMap<String, V>> = IndexMap::new();
    for p in observations {
        let h = p["horizonDays"].as_f64().unwrap_or(f64::NAN);
        if h < fh || h > fh + ma { continue; }
        let pollsters = grouped.entry(p["key"].as_str().unwrap_or("").to_string()).or_default();
        let pid = p["pollsterId"].as_str().unwrap_or("");
        let replace = match pollsters.get(pid) {
            None => true,
            Some(old) => p["fieldAt"].as_f64() > old["fieldAt"].as_f64()
                || (p["fieldAt"].as_f64() == old["fieldAt"].as_f64()
                    && p["id"].as_str().unwrap_or("") < old["id"].as_str().unwrap_or("")),
        };
        if replace { pollsters.insert(pid.to_string(), p.clone()); }
    }
    let mut out = Vec::new();
    for (key, map) in &grouped {
        let polls: Vec<V> = map.values().cloned().collect();
        let first = &polls[0];
        need(polls.iter().all(|p| p["year"] == first["year"] && p["electionAt"] == first["electionAt"]
            && (p["actualMargin"].as_f64().unwrap_or(0.0) - first["actualMargin"].as_f64().unwrap_or(0.0)).abs() < 1e-9
            && p["democraticWon"] == first["democraticWon"]), "race_outcome_disagreement")?;
        let weights: Vec<f64> = polls.iter()
            .map(|p| 2f64.powf(-(p["horizonDays"].as_f64().unwrap_or(0.0) - fh) / hl)).collect();
        let sum: f64 = weights.iter().sum();
        let poll_margin: f64 = polls.iter().zip(&weights)
            .map(|(p, w)| p["pollMargin"].as_f64().unwrap_or(0.0) * w / sum).sum();
        let latest = polls.iter().map(|p| p["fieldAt"].as_f64().unwrap_or(0.0)).fold(f64::NEG_INFINITY, f64::max);
        out.push(json!({"key": key, "year": first["year"], "state": first["state"],
            "forecastHorizonDays": fh, "pollMargin": poll_margin,
            "actualMargin": first["actualMargin"], "democraticWon": first["democraticWon"],
            "error": first["actualMargin"].as_f64().unwrap_or(0.0) - poll_margin,
            "pollCount": polls.len(), "pollIds": polls.iter().map(|p| p["id"].clone()).collect::<Vec<_>>(),
            "latestFieldDate": baal_service_runtime::contract::ms_to_iso(latest),
            "actualHistoricalReleaseTimesAvailable": false, "populationsAvailable": false,
            "remaining": polls.iter().zip(&weights).map(|(p, w)| p["remaining"].as_f64().unwrap_or(0.0) * w / sum).sum::<f64>()}));
    }
    out.sort_by(|a, b| a["year"].as_i64().cmp(&b["year"].as_i64())
        .then_with(|| a["key"].as_str().unwrap_or("").cmp(b["key"].as_str().unwrap_or(""))));
    Ok(out)
}

/// fitPollError(races, {family, minimumRaces=30, minimumCycles=3, minimumSd=3}).
pub fn fit_poll_error(races: &[V], family: &str, minimum_races: Option<usize>,
    minimum_cycles: Option<usize>, minimum_sd: Option<f64>) -> Result<V, String> {
    need(["poll_centered", "historical_bias_corrected"].contains(&family), "unknown_family")?;
    let minimum_races = minimum_races.unwrap_or(30);
    let minimum_cycles = minimum_cycles.unwrap_or(3);
    let minimum_sd = minimum_sd.unwrap_or(3.0);
    let mut cycles: Vec<i64> = Vec::new();
    for r in races {
        let y = r["year"].as_i64().unwrap_or(0);
        if !cycles.contains(&y) { cycles.push(y); }
    }
    need(races.len() >= minimum_races && cycles.len() >= minimum_cycles, "insufficient_independent_history")?;
    need(races.iter().all(|r| r["error"].as_f64().is_some_and(|e| e.is_finite())), "invalid_error")?;
    let bias = if family == "historical_bias_corrected" {
        races.iter().map(|r| r["error"].as_f64().unwrap_or(0.0)).sum::<f64>() / races.len() as f64
    } else { 0.0 };
    let sd = minimum_sd.max((races.iter().map(|r| {
        let d = r["error"].as_f64().unwrap_or(0.0) - bias; d * d
    }).sum::<f64>() / races.len() as f64).sqrt());
    Ok(json!({"family": family, "races": races.len(), "cycles": cycles,
        "trainedThroughYear": races.iter().map(|r| r["year"].as_i64().unwrap_or(0)).max().unwrap_or(0),
        "biasPoints": bias, "standardDeviationPoints": sd, "distribution": "fixed_t4",
        "effectiveUnit": "one_state_race", "historicalPopulationTransportMeasured": false}))
}

/// probability(model, pollMargin).
pub fn probability(model: &V, poll_margin: f64) -> Result<f64, String> {
    need(poll_margin.is_finite(), "invalid_poll_margin")?;
    probability_from_margin(poll_margin + model["biasPoints"].as_f64().unwrap_or(0.0),
        model["standardDeviationPoints"].as_f64().unwrap_or(0.0))
}

// A current single poll must use individual-poll uncertainty, not the smaller
// error of a historical average. Each race retains total weight one.
/// fitSinglePollError(observations, definition, omitCycle=null).
pub fn fit_single_poll_error(observations: &[V], definition: &V, omit_cycle: Option<i64>) -> Result<V, String> {
    let fh = definition["forecastHorizonDays"].as_f64().unwrap_or(f64::NAN);
    let ma = definition["maximumPollAgeDays"].as_f64().unwrap_or(f64::NAN);
    let mut groups: IndexMap<String, IndexMap<String, V>> = IndexMap::new();
    for p in observations {
        if Some(p["year"].as_i64().unwrap_or(0)) == omit_cycle { continue; }
        let h = p["horizonDays"].as_f64().unwrap_or(f64::NAN);
        if h < fh || h > fh + ma { continue; }
        let map = groups.entry(p["key"].as_str().unwrap_or("").to_string()).or_default();
        let pid = p["pollsterId"].as_str().unwrap_or("");
        let replace = match map.get(pid) {
            None => true,
            Some(old) => p["fieldAt"].as_f64() > old["fieldAt"].as_f64()
                || (p["fieldAt"].as_f64() == old["fieldAt"].as_f64()
                    && p["id"].as_str().unwrap_or("") < old["id"].as_str().unwrap_or("")),
        };
        if replace { map.insert(pid.to_string(), p.clone()); }
    }
    need(groups.len() >= definition["minimumTrainingRaces"].as_u64().unwrap_or(0) as usize,
        "insufficient_single_poll_history")?;
    let races: Vec<Vec<V>> = groups.values().map(|m| m.values().cloned().collect()).collect();
    let mut cycles: Vec<i64> = Vec::new();
    for ps in &races {
        let y = ps[0]["year"].as_i64().unwrap_or(0);
        if !cycles.contains(&y) { cycles.push(y); }
    }
    need(cycles.len() >= definition["minimumCalibrationCycles"].as_u64().unwrap_or(0) as usize,
        "insufficient_single_poll_cycles")?;
    let average = |f: &dyn Fn(&V) -> f64| -> f64 {
        races.iter().map(|ps| ps.iter().map(|p| f(p)).sum::<f64>() / ps.len() as f64).sum::<f64>() / races.len() as f64
    };
    let bias = average(&|p| p["actualMargin"].as_f64().unwrap_or(0.0) - p["pollMargin"].as_f64().unwrap_or(0.0));
    let sd = definition["minimumMarginSdPoints"].as_f64().unwrap_or(0.0)
        .max(average(&|p| {
            let d = p["actualMargin"].as_f64().unwrap_or(0.0) - p["pollMargin"].as_f64().unwrap_or(0.0) - bias;
            d * d
        }).sqrt());
    Ok(json!({"family": "single_poll_race_weighted", "races": races.len(),
        "pollsterRaceObservations": races.iter().map(|ps| ps.len()).sum::<usize>(),
        "cycles": cycles, "trainedThroughYear": cycles.iter().max().copied().unwrap_or(0),
        "biasPoints": bias, "standardDeviationPoints": sd, "distribution": "fixed_t4",
        "effectiveUnit": "equal_race_weight_with_individual_poll_dispersion",
        "omittedCycle": omit_cycle, "historicalPopulationTransportMeasured": false}))
}

/// chronologicalPredictions(races, definition).
pub fn chronological_predictions(races: &[V], definition: &V) -> Result<Vec<V>, String> {
    let mut out = Vec::new();
    for year_v in definition["diagnosticYears"].as_array().cloned().unwrap_or_default() {
        let year = year_v.as_i64().unwrap_or(0);
        let training: Vec<V> = races.iter().filter(|r| r["year"].as_i64().unwrap_or(0) < year).cloned().collect();
        let testing: Vec<V> = races.iter().filter(|r| r["year"].as_i64().unwrap_or(0) == year).cloned().collect();
        for family_v in definition["families"].as_array().cloned().unwrap_or_default() {
            let family = family_v.as_str().unwrap_or("");
            let model = fit_poll_error(&training, family,
                Some(definition["minimumTrainingRaces"].as_u64().unwrap_or(0) as usize),
                Some(definition["minimumCalibrationCycles"].as_u64().unwrap_or(0) as usize),
                definition["minimumMarginSdPoints"].as_f64())?;
            need(model["trainedThroughYear"].as_i64().unwrap_or(0) < year, "future_calibration_leak")?;
            for r in &testing {
                let mut row = r.clone();
                row["family"] = json!(family);
                row["trainedThroughYear"] = model["trainedThroughYear"].clone();
                row["model"] = model.clone();
                row["democraticWinProbability"] = json!(probability(&model, r["pollMargin"].as_f64().unwrap_or(0.0))?);
                out.push(row);
            }
        }
    }
    Ok(out)
}

/// score(rows) — Brier/log-loss plus calibration bins.
pub fn score(rows: &[V]) -> Result<V, String> {
    need(!rows.is_empty(), "no_predictions")?;
    let n = rows.len() as f64;
    let prob = |r: &V| r["democraticWinProbability"].as_f64().unwrap_or(0.0);
    let y = |r: &V| if r["democraticWon"] == V::Bool(true) { 1.0 } else { 0.0 };
    let brier = rows.iter().map(|r| { let d = prob(r) - y(r); d * d }).sum::<f64>() / n;
    let log_loss = rows.iter()
        .map(|r| -y(r) * prob(r).ln() - (1.0 - y(r)) * (1.0 - prob(r)).ln()).sum::<f64>() / n;
    let mut cycles: Vec<i64> = Vec::new();
    for r in rows {
        let yv = r["year"].as_i64().unwrap_or(0);
        if !cycles.contains(&yv) { cycles.push(yv); }
    }
    let bins: Vec<V> = [(0.0, 0.2), (0.2, 0.4), (0.4, 0.6), (0.6, 0.8), (0.8, 1.000001)].iter()
        .map(|(low, high)| {
            let group: Vec<&V> = rows.iter().filter(|r| prob(r) >= *low && prob(r) < *high).collect();
            json!({"low": low, "high": high, "n": group.len(),
                "meanForecast": if group.is_empty() { V::Null } else { json!(group.iter().map(|r| prob(r)).sum::<f64>() / group.len() as f64) },
                "observedFrequency": if group.is_empty() { V::Null } else { json!(group.iter().map(|r| y(r)).sum::<f64>() / group.len() as f64) }})
        }).collect();
    Ok(json!({"races": rows.len(), "cycles": cycles, "brier": brier, "logLoss": log_loss,
        "correctDirections": rows.iter().filter(|r| (prob(r) > 0.5) == (r["democraticWon"] == V::Bool(true))).count(),
        "bins": bins,
        "independentElectionCycles": cycles.len(),
        "untouchedOriginalForecastVintage": false, "historicalMarketPnlAvailable": false,
        "profitabilityEstablished": false}))
}
