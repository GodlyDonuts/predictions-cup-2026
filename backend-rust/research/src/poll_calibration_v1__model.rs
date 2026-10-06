// research/poll-calibration-v1/model.mjs — FiveThirtyEight senate-poll archive
// observations, race-level aggregation, error distribution fits, scores.
use crate::election_baseline_v1__model as v1;
use crate::election_baseline_v2__model::SenateRacesV2;
use crate::js::{field_str, iso, js_number, js_string, js_template, js_truthy, num, num_str, require_that, safe_integer};
use indexmap::IndexMap;
use serde_json::{json, Value as V};

const DAY: f64 = 86_400_000.0;

fn name(n: &str) -> String {
    v1::identity(n)
        .split(' ')
        .filter(|t| t.len() > 1)
        .collect::<Vec<_>>()
        .join(" ")
}

fn date(s: &str) -> f64 {
    let b = s.as_bytes();
    let re = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| (i == 4 || i == 7) || c.is_ascii_digit());
    if !re {
        return f64::NAN;
    }
    chrono::DateTime::parse_from_rfc3339(&format!("{s}T12:00:00Z"))
        .map(|d| d.timestamp_millis() as f64)
        .unwrap_or(f64::NAN)
}

// /^\d+$/ on String(v): JS RegExp.test coerces its argument to a string.
fn digits(v: Option<&V>) -> bool {
    let s = js_template(v);
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}

pub struct PollObservations {
    pub rows: Vec<V>,
    pub excluded: IndexMap<String, i64>,
}

pub fn poll_observations(raw: &[V], senate: &SenateRacesV2) -> Result<PollObservations, String> {
    let mut rows: Vec<V> = Vec::new();
    let mut excluded: IndexMap<String, i64> = IndexMap::new();
    let mut seen: IndexMap<String, V> = IndexMap::new();
    let mut reject = |reason: &str| {
        *excluded.entry(reason.to_string()).or_insert(0) += 1;
    };
    for p in raw {
        if field_str(p, "type_simple") != "Sen-G" {
            continue;
        }
        let year = js_number(p.get("cycle"));
        let key = format!("{}:{}", num_str(year), js_template(p.get("location")));
        let race = senate.races.get(&key);
        if !safe_integer(year)
            || (year as i64) % 2 != 0
            || race.is_none()
            || field_str(race.unwrap(), "selectedStage") != "gen"
            || !race.unwrap()["opponent"]["parties"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x == "DEMOCRAT"))
        {
            reject("not_regular_comparable_major_party_final");
            continue;
        }
        let race = race.unwrap();
        if field_str(p, "cand1_party") != "DEM"
            || field_str(p, "cand2_party") != "REP"
            || name(field_str(p, "cand1_name")) != name(field_str(&race["opponent"], "name"))
            || name(field_str(p, "cand2_name")) != name(field_str(&race["republican"], "name"))
        {
            reject("final_candidate_identity_unmatched");
            continue;
        }
        if js_truthy(p.get("partisan")) && field_str(p, "partisan") != "NA" {
            reject("partisan_or_internal");
            continue;
        }
        let field_at = date(field_str(p, "polldate"));
        let election_at = date(field_str(p, "electiondate"));
        let horizon = (election_at - field_at) / DAY;
        if !horizon.is_finite()
            || horizon <= 0.0
            || !field_str(p, "electiondate").starts_with(&format!("{}-11-", num_str(year)))
            || (horizon - js_number(p.get("time_to_election"))).abs() > 1.0
        {
            reject("election_or_field_clock_invalid");
            continue;
        }
        let d = js_number(p.get("cand1_pct"));
        let r = js_number(p.get("cand2_pct"));
        let actual_d = js_number(p.get("cand1_actual"));
        let actual_r = js_number(p.get("cand2_actual"));
        if ![d, r, actual_d, actual_r]
            .iter()
            .all(|x| x.is_finite() && *x > 0.0 && *x < 100.0)
            || d + r > 100.001
        {
            reject("invalid_vote_shares");
            continue;
        }
        let verified_d = 100.0 * race["opponent"]["votes"].as_f64().unwrap_or(f64::NAN)
            / race["total"].as_f64().unwrap_or(f64::NAN);
        let verified_r = 100.0 * race["republican"]["votes"].as_f64().unwrap_or(f64::NAN)
            / race["total"].as_f64().unwrap_or(f64::NAN);
        if (actual_d - verified_d).abs() > 0.25
            || (actual_r - verified_r).abs() > 0.25
            || ((actual_d > actual_r) != !race["winner"]["republican"].as_bool().unwrap_or(false))
        {
            reject("independent_final_outcome_disagrees");
            continue;
        }
        if !digits(p.get("question_id")) || !digits(p.get("poll_id")) || !digits(p.get("pollster_rating_id")) {
            reject("source_identity_missing");
            continue;
        }
        let id = format!("{}:{}", js_template(p.get("race_id")), js_template(p.get("question_id")));
        let record = json!({
            "id": id,
            "key": key,
            "year": num(year),
            "state": p["location"],
            "pollId": p["poll_id"],
            "questionId": p["question_id"],
            "pollsterId": p["pollster_rating_id"],
            "pollster": p["pollster"],
            "fieldDate": p["polldate"],
            "fieldAt": num(field_at),
            "electionDate": p["electiondate"],
            "electionAt": num(election_at),
            "horizonDays": num(horizon),
            "democratic": num(d),
            "republican": num(r),
            "remaining": num(100.0 - d - r),
            "pollMargin": num(d - r),
            "actualMargin": num(verified_d - verified_r),
            "democraticWon": !race["winner"]["republican"].as_bool().unwrap_or(false),
            "population": "not_in_archive",
            "sourceFinalMargin": num(actual_d - actual_r),
        });
        if let Some(old) = seen.get(&id) {
            require_that(*old == record, "conflicting_source_question")?;
            reject("identical_duplicate");
            continue;
        }
        seen.insert(id, record.clone());
        rows.push(record);
    }
    rows.sort_by(|a, b| {
        js_number(a.get("year"))
            .partial_cmp(&js_number(b.get("year")))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| field_str(a, "key").cmp(field_str(b, "key")))
            .then_with(|| field_str(a, "id").cmp(field_str(b, "id")))
    });
    Ok(PollObservations { rows, excluded })
}

pub fn aggregate_race_polls(observations: &[V], definition: &V) -> Result<Vec<V>, String> {
    require_that(
        js_number(definition.get("forecastHorizonDays")).is_finite()
            && js_number(definition.get("forecastHorizonDays")) > 0.0
            && js_number(definition.get("maximumPollAgeDays")).is_finite()
            && js_number(definition.get("maximumPollAgeDays")) > 0.0
            && js_number(definition.get("recencyHalfLifeDays")).is_finite()
            && js_number(definition.get("recencyHalfLifeDays")) > 0.0,
        "Number.isFinite(definition.forecastHorizonDays) && definition.forecastHorizonDays > 0 && Number.isFinite(definition.maximumPollAgeDays) && definition.maximumPollAgeDays > 0 && Number.isFinite(definition.recencyHalfLifeDays) && definition.recencyHalfLifeDays > 0",
    )?;
    let fh = js_number(definition.get("forecastHorizonDays"));
    let max_age = js_number(definition.get("maximumPollAgeDays"));
    let half_life = js_number(definition.get("recencyHalfLifeDays"));
    let mut grouped: IndexMap<String, IndexMap<String, &V>> = IndexMap::new();
    for p in observations {
        let horizon = js_number(p.get("horizonDays"));
        if horizon < fh || horizon > fh + max_age {
            continue;
        }
        let pollsters = grouped.entry(field_str(p, "key").to_string()).or_default();
        let pid = crate::js::map_key(p.get("pollsterId"));
        match pollsters.get(&pid) {
            Some(old)
                if !(js_number(p.get("fieldAt")) > js_number(old.get("fieldAt"))
                    || (js_number(p.get("fieldAt")) == js_number(old.get("fieldAt"))
                        && field_str(p, "id") < field_str(old, "id"))) => {}
            _ => {
                pollsters.insert(pid, p);
            }
        }
    }
    let mut out: Vec<V> = grouped
        .iter()
        .map(|(key, map)| -> Result<V, String> {
            let polls: Vec<&V> = map.values().copied().collect();
            let first = polls[0];
            require_that(
                polls.iter().all(|p| {
                    js_number(p.get("year")) == js_number(first.get("year"))
                        && js_number(p.get("electionAt")) == js_number(first.get("electionAt"))
                        && (js_number(p.get("actualMargin")) - js_number(first.get("actualMargin"))).abs() < 1e-9
                        && p["democraticWon"].as_bool() == first["democraticWon"].as_bool()
                }),
                "race_outcome_disagreement",
            )?;
            let weights: Vec<f64> = polls
                .iter()
                .map(|p| 2.0f64.powf(-(js_number(p.get("horizonDays")) - fh) / half_life))
                .collect();
            let sum: f64 = weights.iter().sum();
            let poll_margin: f64 = polls
                .iter()
                .zip(&weights)
                .map(|(p, w)| js_number(p.get("pollMargin")) * w / sum)
                .sum();
            let remaining: f64 = polls
                .iter()
                .zip(&weights)
                .map(|(p, w)| js_number(p.get("remaining")) * w / sum)
                .sum();
            let latest = polls.iter().map(|p| js_number(p.get("fieldAt"))).fold(f64::NEG_INFINITY, f64::max);
            Ok(json!({
                "key": key,
                "year": first["year"],
                "state": first["state"],
                "forecastHorizonDays": num(fh),
                "pollMargin": num(poll_margin),
                "actualMargin": first["actualMargin"],
                "democraticWon": first["democraticWon"],
                "error": num(js_number(first.get("actualMargin")) - poll_margin),
                "pollCount": polls.len(),
                "pollIds": polls.iter().map(|p| p["id"].clone()).collect::<Vec<V>>(),
                "latestFieldDate": iso(latest),
                "actualHistoricalReleaseTimesAvailable": false,
                "populationsAvailable": false,
                "remaining": num(remaining),
            }))
        })
        .collect::<Result<_, _>>()?;
    out.sort_by(|a, b| {
        js_number(a.get("year"))
            .partial_cmp(&js_number(b.get("year")))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| field_str(a, "key").cmp(field_str(b, "key")))
    });
    Ok(out)
}

pub fn fit_poll_error(
    races: &[V],
    family: &str,
    minimum_races: usize,
    minimum_cycles: usize,
    minimum_sd: f64,
) -> Result<V, String> {
    require_that(
        ["poll_centered", "historical_bias_corrected"].contains(&family),
        "unknown_family",
    )?;
    let mut cycles: Vec<f64> = Vec::new();
    for r in races {
        let y = js_number(r.get("year"));
        if !cycles.contains(&y) {
            cycles.push(y);
        }
    }
    require_that(
        races.len() >= minimum_races && cycles.len() >= minimum_cycles,
        "insufficient_independent_history",
    )?;
    require_that(
        races.iter().all(|r| js_number(r.get("error")).is_finite()),
        "invalid_error",
    )?;
    let bias = if family == "historical_bias_corrected" {
        races.iter().map(|r| js_number(r.get("error"))).sum::<f64>() / races.len() as f64
    } else {
        0.0
    };
    let sd = minimum_sd.max(
        (races
            .iter()
            .map(|r| (js_number(r.get("error")) - bias).powi(2))
            .sum::<f64>()
            / races.len() as f64)
            .sqrt(),
    );
    Ok(json!({
        "family": family,
        "races": races.len(),
        "cycles": cycles.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "trainedThroughYear": num(cycles.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "biasPoints": num(bias),
        "standardDeviationPoints": num(sd),
        "distribution": "fixed_t4",
        "effectiveUnit": "one_state_race",
        "historicalPopulationTransportMeasured": false,
    }))
}

pub fn probability(model: &V, poll_margin: f64) -> Result<f64, String> {
    require_that(poll_margin.is_finite(), "invalid_poll_margin")?;
    v1::probability_from_margin(
        poll_margin + js_number(model.get("biasPoints")),
        js_number(model.get("standardDeviationPoints")),
    )
}

// A current single poll must use individual-poll uncertainty, not the smaller
// error of a historical average. Each race retains total weight one.
pub fn fit_single_poll_error(observations: &[V], definition: &V, omit_cycle: Option<f64>) -> Result<V, String> {
    let fh = js_number(definition.get("forecastHorizonDays"));
    let max_age = js_number(definition.get("maximumPollAgeDays"));
    let mut groups: IndexMap<String, IndexMap<String, &V>> = IndexMap::new();
    for p in observations {
        if omit_cycle.is_some_and(|oc| js_number(p.get("year")) == oc) {
            continue;
        }
        let horizon = js_number(p.get("horizonDays"));
        if horizon < fh || horizon > fh + max_age {
            continue;
        }
        let map = groups.entry(field_str(p, "key").to_string()).or_default();
        let pid = crate::js::map_key(p.get("pollsterId"));
        match map.get(&pid) {
            Some(old)
                if !(js_number(p.get("fieldAt")) > js_number(old.get("fieldAt"))
                    || (js_number(p.get("fieldAt")) == js_number(old.get("fieldAt"))
                        && field_str(p, "id") < field_str(old, "id"))) => {}
            _ => {
                map.insert(pid, p);
            }
        }
    }
    require_that(
        groups.len() as f64 >= js_number(definition.get("minimumTrainingRaces")),
        "insufficient_single_poll_history",
    )?;
    let races: Vec<Vec<&V>> = groups.values().map(|m| m.values().copied().collect()).collect();
    let mut cycles: Vec<f64> = Vec::new();
    for ps in &races {
        let y = js_number(ps[0].get("year"));
        if !cycles.contains(&y) {
            cycles.push(y);
        }
    }
    require_that(
        cycles.len() as f64 >= js_number(definition.get("minimumCalibrationCycles")),
        "insufficient_single_poll_cycles",
    )?;
    let average = |f: &dyn Fn(&V) -> f64| -> f64 {
        races
            .iter()
            .map(|ps| ps.iter().map(|p| f(p)).sum::<f64>() / ps.len() as f64)
            .sum::<f64>()
            / races.len() as f64
    };
    let bias = average(&|p| js_number(p.get("actualMargin")) - js_number(p.get("pollMargin")));
    let sd = js_number(definition.get("minimumMarginSdPoints"))
        .max(average(&|p| (js_number(p.get("actualMargin")) - js_number(p.get("pollMargin")) - bias).powi(2)).sqrt());
    let pollster_obs: usize = races.iter().map(|ps| ps.len()).sum();
    Ok(json!({
        "family": "single_poll_race_weighted",
        "races": races.len(),
        "pollsterRaceObservations": pollster_obs,
        "cycles": cycles.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "trainedThroughYear": num(cycles.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "biasPoints": num(bias),
        "standardDeviationPoints": num(sd),
        "distribution": "fixed_t4",
        "effectiveUnit": "equal_race_weight_with_individual_poll_dispersion",
        "omittedCycle": omit_cycle.map(num).unwrap_or(V::Null),
        "historicalPopulationTransportMeasured": false,
    }))
}

pub fn chronological_predictions(races: &[V], definition: &V) -> Result<Vec<V>, String> {
    let mut out: Vec<V> = Vec::new();
    for year_v in definition["diagnosticYears"].as_array().cloned().unwrap_or_default() {
        let year = js_number(Some(&year_v));
        let training: Vec<V> = races.iter().filter(|r| js_number(r.get("year")) < year).cloned().collect();
        let testing: Vec<&V> = races.iter().filter(|r| js_number(r.get("year")) == year).collect();
        for family_v in definition["families"].as_array().cloned().unwrap_or_default() {
            let family = js_string(&family_v);
            let model = fit_poll_error(
                &training,
                &family,
                js_number(definition.get("minimumTrainingRaces")) as usize,
                js_number(definition.get("minimumCalibrationCycles")) as usize,
                js_number(definition.get("minimumMarginSdPoints")),
            )?;
            require_that(
                js_number(model.get("trainedThroughYear")) < year,
                "future_calibration_leak",
            )?;
            for r in &testing {
                let mut row = (*r).clone();
                let prob = probability(&model, js_number(r.get("pollMargin")))?;
                if let V::Object(ref mut m) = row {
                    m.insert("family".into(), family_v.clone());
                    m.insert("trainedThroughYear".into(), model["trainedThroughYear"].clone());
                    m.insert("model".into(), model.clone());
                    m.insert("democraticWinProbability".into(), num(prob));
                }
                out.push(row);
            }
        }
    }
    Ok(out)
}

pub fn score(rows: &[V]) -> Result<V, String> {
    require_that(!rows.is_empty(), "no_predictions")?;
    let n = rows.len() as f64;
    let prob = |r: &V| js_number(r.get("democraticWinProbability"));
    let y = |r: &V| {
        if r["democraticWon"].as_bool().unwrap_or(false) {
            1.0
        } else {
            0.0
        }
    };
    let brier = rows.iter().map(|r| (prob(r) - y(r)).powi(2)).sum::<f64>() / n;
    let log_loss = rows
        .iter()
        .map(|r| -y(r) * prob(r).ln() - (1.0 - y(r)) * (1.0 - prob(r)).ln())
        .sum::<f64>()
        / n;
    let mut cycles: Vec<f64> = Vec::new();
    for r in rows {
        let cy = js_number(r.get("year"));
        if !cycles.contains(&cy) {
            cycles.push(cy);
        }
    }
    let correct = rows
        .iter()
        .filter(|r| (prob(r) > 0.5) == r["democraticWon"].as_bool().unwrap_or(false))
        .count();
    let bins: Vec<V> = [[0.0, 0.2], [0.2, 0.4], [0.4, 0.6], [0.6, 0.8], [0.8, 1.000001]]
        .iter()
        .map(|pair| {
            let (low, high) = (pair[0], pair[1]);
            let group: Vec<&V> = rows.iter().filter(|r| prob(r) >= low && prob(r) < high).collect();
            let mean_forecast = if group.is_empty() {
                V::Null
            } else {
                num(group.iter().map(|r| prob(r)).sum::<f64>() / group.len() as f64)
            };
            let observed = if group.is_empty() {
                V::Null
            } else {
                num(group.iter().map(|r| y(r)).sum::<f64>() / group.len() as f64)
            };
            json!({
                "low": num(low),
                "high": num(high),
                "n": group.len(),
                "meanForecast": mean_forecast,
                "observedFrequency": observed,
            })
        })
        .collect();
    Ok(json!({
        "races": n as i64,
        "cycles": cycles.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "brier": num(brier),
        "logLoss": num(log_loss),
        "correctDirections": correct,
        "bins": bins,
        "independentElectionCycles": cycles.len(),
        "untouchedOriginalForecastVintage": false,
        "historicalMarketPnlAvailable": false,
        "profitabilityEstablished": false,
    }))
}
