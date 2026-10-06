// research/poll-horizon-v2.mjs — 29-day horizon poll error model with
// within/between-cycle variance decomposition.
use crate::election_baseline_v1__model::probability_from_margin;
use crate::js::{date_parse, finite, js_max, js_number, js_truthy, map_key, num, require_that};
use crate::poll_calibration_v1__model::aggregate_race_polls;
use indexmap::IndexMap;
use serde_json::{json, Value as V};

pub fn definition() -> V {
    json!({
        "forecastHorizonDays": 29,
        "maximumPollAgeDays": 30,
        "recencyHalfLifeDays": 21,
        "minimumRaces": 30,
        "minimumCycles": 3,
        "minimumSd": 3,
    })
}

fn noon(s: &str) -> f64 {
    chrono::DateTime::parse_from_rfc3339(&format!("{s}T12:00:00Z"))
        .map(|d| d.timestamp_millis() as f64)
        .unwrap_or(f64::NAN)
}

pub fn fit_horizon_error(observations: &[V], through_year: f64) -> Result<V, String> {
    let def = definition();
    let filtered: Vec<V> = observations
        .iter()
        .filter(|p| js_number(p.get("year")) <= through_year)
        .cloned()
        .collect();
    let races = aggregate_race_polls(&filtered, &def)?;
    require_that(
        races.len() as f64 >= js_number(def.get("minimumRaces")),
        "races.length >= DEFINITION.minimumRaces",
    )?;
    let mut cycles: IndexMap<String, Vec<&V>> = IndexMap::new();
    let mut cycle_years: Vec<f64> = Vec::new();
    for r in &races {
        let y = js_number(r.get("year"));
        cycles.entry(crate::js::num_str(y)).or_default().push(r);
        if !cycle_years.contains(&y) {
            cycle_years.push(y);
        }
    }
    require_that(
        cycles.len() as f64 >= js_number(def.get("minimumCycles")),
        "cycles.size >= DEFINITION.minimumCycles",
    )?;
    let bias = races.iter().map(|r| js_number(r.get("error"))).sum::<f64>() / races.len() as f64;
    let cycle_rows: Vec<V> = cycles
        .iter()
        .map(|(_, rows)| {
            json!({
                "year": rows[0]["year"],
                "races": rows.len(),
                "mean": num(rows.iter().map(|r| js_number(r.get("error"))).sum::<f64>() / rows.len() as f64),
            })
        })
        .collect();
    let raw_variance = races
        .iter()
        .map(|r| (js_number(r.get("error")) - bias).powi(2))
        .sum::<f64>()
        / races.len() as f64;
    let mut within = 0.0;
    for c in &cycle_rows {
        for r in &cycles[&crate::js::num_str(js_number(c.get("year")))] {
            within += (js_number(r.get("error")) - js_number(c.get("mean"))).powi(2) / races.len() as f64;
        }
    }
    let cycle_center =
        cycle_rows.iter().map(|c| js_number(c.get("mean"))).sum::<f64>() / cycle_rows.len() as f64;
    let mean_cycle_variance = cycle_rows
        .iter()
        .map(|c| (js_number(c.get("mean")) - cycle_center).powi(2))
        .sum::<f64>()
        / cycle_rows.len() as f64;
    let finite_race_mean_noise = within
        * cycle_rows
            .iter()
            .map(|c| 1.0 / js_number(c.get("races")))
            .sum::<f64>()
        / cycle_rows.len() as f64;
    let common = js_max(0.0, mean_cycle_variance - finite_race_mean_noise);
    // Keep at least the full race-level historical variance; repeated polls and
    // larger source counts cannot divide away a common election shock.
    let total = js_max(
        js_max(js_number(def.get("minimumSd")).powi(2), raw_variance),
        within + common,
    );
    let max_year = races
        .iter()
        .map(|r| js_number(r.get("year")))
        .fold(f64::NEG_INFINITY, js_max);
    Ok(json!({
        "schemaVersion": 1,
        "definition": def,
        "trainedThroughYear": num(through_year),
        "maximumTrainingElectionYear": num(max_year),
        "races": races.len(),
        "cycles": cycle_rows,
        "meanBiasPoints": num(bias),
        "withinCycleVariance": num(within),
        "estimatedCommonCycleVariance": num(common),
        "rawRaceVariance": num(raw_variance),
        "totalVariance": num(total),
        "standardDeviationPoints": num(total.sqrt()),
        "commonCorrelationProxy": num(common / total),
        "historicalReleaseClocksAvailable": false,
        "historicalPopulationLabelsAvailable": false,
        "distribution": "fixed_t4",
        "profitabilityEstablished": false,
    }))
}

pub fn value_poll_margin(model: &V, margin: f64, shift: f64) -> Result<f64, String> {
    require_that(
        margin.is_finite() && shift.is_finite(),
        "Number.isFinite(margin) && Number.isFinite(shift)",
    )?;
    probability_from_margin(
        margin + js_number(model.get("meanBiasPoints")) + shift,
        js_number(model.get("standardDeviationPoints")),
    )
}

pub fn current_known_margin(polls: &[V], election_date: &str, as_of: &str) -> Result<V, String> {
    let def = definition();
    let election = noon(election_date);
    let clock = date_parse(as_of);
    let mut selected: IndexMap<String, V> = IndexMap::new();
    let mut excluded: Vec<V> = Vec::new();
    require_that(
        election.is_finite() && clock.is_finite() && election > clock,
        "Number.isFinite(election) && Number.isFinite(clock) && election > clock",
    )?;
    let fh = js_number(def.get("forecastHorizonDays"));
    let max_age = js_number(def.get("maximumPollAgeDays"));
    let half_life = js_number(def.get("recencyHalfLifeDays"));
    for p in polls {
        if !js_truthy(p.get("methodologyVerified"))
            || !js_truthy(p.get("fieldStart"))
            || !js_truthy(p.get("fieldEnd"))
        {
            excluded.push(json!({"source": p["source"], "reason": "fieldwork_or_methodology_unverified"}));
            continue;
        }
        let from = noon(&crate::js::js_string(p.get("fieldStart").unwrap_or(&V::Null)));
        let to = noon(&crate::js::js_string(p.get("fieldEnd").unwrap_or(&V::Null)));
        let middle = (from + to) / 2.0;
        let horizon = (election - middle) / 86_400_000.0;
        if !horizon.is_finite()
            || from > to
            || to > clock
            || horizon < fh
            || horizon > fh + max_age
            || !finite(p.get("margin"))
        {
            excluded.push(json!({"source": p["source"], "reason": "field_clock_or_horizon"}));
            continue;
        }
        let key = map_key(p.get("source"));
        let replace = match selected.get(&key) {
            Some(old) => middle > js_number(old.get("middle")),
            None => true,
        };
        if replace {
            let mut row = p.clone();
            if let V::Object(ref mut m) = row {
                m.insert("middle".into(), num(middle));
                m.insert("horizon".into(), num(horizon));
                m.insert(
                    "weight".into(),
                    num(2.0f64.powf(-(horizon - fh) / half_life)),
                );
            }
            selected.insert(key, row);
        }
    }
    require_that(!selected.is_empty(), "selected.size > 0")?;
    let rows: Vec<&V> = selected.values().collect();
    let weight: f64 = rows.iter().map(|r| js_number(r.get("weight"))).sum();
    let sources: Vec<V> = rows
        .iter()
        .map(|r| {
            json!({
                "source": r["source"],
                "horizonDays": r["horizon"],
                "weight": num(js_number(r.get("weight")) / weight),
            })
        })
        .collect();
    let margin = rows
        .iter()
        .map(|r| js_number(r.get("margin")) * js_number(r.get("weight")) / weight)
        .sum::<f64>();
    Ok(json!({
        "margin": num(margin),
        "sources": sources,
        "excluded": excluded,
        "precisionGainAssumed": false,
    }))
}

pub fn chronological_horizon_diagnostic(observations: &[V], years: &[f64]) -> Result<V, String> {
    let def = definition();
    let mut rows: Vec<V> = Vec::new();
    for &year in years {
        let model = fit_horizon_error(observations, year - 1.0)?;
        require_that(
            js_number(model.get("maximumTrainingElectionYear")) < year,
            "model.maximumTrainingElectionYear < year",
        )?;
        let filtered: Vec<V> = observations
            .iter()
            .filter(|r| crate::js::js_strict_eq_num(r.get("year"), year))
            .cloned()
            .collect();
        for race in aggregate_race_polls(&filtered, &def)? {
            rows.push(json!({
                "key": race["key"],
                "year": num(year),
                "pollMargin": race["pollMargin"],
                "democraticWon": race["democraticWon"],
                "probability": num(value_poll_margin(&model, js_number(race.get("pollMargin")), 0.0)?),
                "trainingThroughYear": model["maximumTrainingElectionYear"],
            }));
        }
    }
    let won = |r: &V| {
        if r["democraticWon"].as_bool().unwrap_or(false) {
            1.0
        } else {
            0.0
        }
    };
    let brier = rows
        .iter()
        .map(|r| (js_number(r.get("probability")) - won(r)).powi(2))
        .sum::<f64>()
        / rows.len() as f64;
    let by_year: Vec<V> = years
        .iter()
        .map(|&year| {
            let rs: Vec<&V> = rows.iter().filter(|r| js_number(r.get("year")) == year).collect();
            let b = rs
                .iter()
                .map(|r| (js_number(r.get("probability")) - won(r)).powi(2))
                .sum::<f64>()
                / rs.len() as f64;
            json!({"year": num(year), "races": rs.len(), "brier": num(b)})
        })
        .collect();
    Ok(json!({
        "races": rows.len(),
        "years": years.iter().map(|y| num(*y)).collect::<Vec<V>>(),
        "brier": num(brier),
        "byYear": by_year,
        "rows": rows,
        "untouchedOriginalForecastVintage": false,
        "marketPnlAvailable": false,
        "profitabilityEstablished": false,
    }))
}
