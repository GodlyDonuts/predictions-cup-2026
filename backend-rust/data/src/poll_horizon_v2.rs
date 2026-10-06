// research/poll-horizon-v2.mjs — 29-day horizon error model with common-cycle
// variance decomposition and current weighted-poll margins.
use crate::election_baseline_v1__model::probability_from_margin;
use crate::poll_calibration_v1__model::aggregate_race_polls;
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(code.into()) } }
fn parse_date12(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(&format!("{}T12:00:00Z", s)).unwrap_or(f64::NAN)
}

/// DEFINITION (frozen).
pub fn definition() -> V {
    json!({"forecastHorizonDays": 29, "maximumPollAgeDays": 30, "recencyHalfLifeDays": 21,
        "minimumRaces": 30, "minimumCycles": 3, "minimumSd": 3})
}

/// fitHorizonError(observations, throughYear).
pub fn fit_horizon_error(observations: &[V], through_year: i64) -> Result<V, String> {
    let def = definition();
    let filtered: Vec<V> = observations.iter()
        .filter(|p| p["year"].as_i64().unwrap_or(0) <= through_year).cloned().collect();
    let races = aggregate_race_polls(&filtered, &def)?;
    need(races.len() >= def["minimumRaces"].as_u64().unwrap_or(30) as usize, "assertion failed")?;
    let mut cycles: indexmap::IndexMap<i64, Vec<V>> = indexmap::IndexMap::new();
    for r in &races {
        cycles.entry(r["year"].as_i64().unwrap_or(0)).or_default().push(r.clone());
    }
    need(cycles.len() >= def["minimumCycles"].as_u64().unwrap_or(3) as usize, "assertion failed")?;
    let bias = races.iter().map(|r| r["error"].as_f64().unwrap_or(0.0)).sum::<f64>() / races.len() as f64;
    let cycle_rows: Vec<V> = cycles.iter().map(|(year, rows)| {
        json!({"year": year, "races": rows.len(),
            "mean": rows.iter().map(|r| r["error"].as_f64().unwrap_or(0.0)).sum::<f64>() / rows.len() as f64})
    }).collect();
    let raw_variance = races.iter().map(|r| {
        let d = r["error"].as_f64().unwrap_or(0.0) - bias; d * d
    }).sum::<f64>() / races.len() as f64;
    let mut within = 0.0f64;
    for c in &cycle_rows {
        let year = c["year"].as_i64().unwrap_or(0);
        let mean = c["mean"].as_f64().unwrap_or(0.0);
        for r in &cycles[&year] {
            let d = r["error"].as_f64().unwrap_or(0.0) - mean;
            within += d * d / races.len() as f64;
        }
    }
    let cycle_center = cycle_rows.iter().map(|c| c["mean"].as_f64().unwrap_or(0.0)).sum::<f64>() / cycle_rows.len() as f64;
    let mean_cycle_variance = cycle_rows.iter().map(|c| {
        let d = c["mean"].as_f64().unwrap_or(0.0) - cycle_center; d * d
    }).sum::<f64>() / cycle_rows.len() as f64;
    let finite_race_mean_noise = within * cycle_rows.iter()
        .map(|c| 1.0 / c["races"].as_f64().unwrap_or(1.0)).sum::<f64>() / cycle_rows.len() as f64;
    let common = (mean_cycle_variance - finite_race_mean_noise).max(0.0);
    // Keep at least the full race-level historical variance; repeated polls and
    // larger source counts cannot divide away a common election shock.
    let total = (def["minimumSd"].as_f64().unwrap_or(3.0) * def["minimumSd"].as_f64().unwrap_or(3.0))
        .max(raw_variance).max(within + common);
    Ok(json!({"schemaVersion": 1, "definition": def, "trainedThroughYear": through_year,
        "maximumTrainingElectionYear": races.iter().map(|r| r["year"].as_i64().unwrap_or(0)).max().unwrap_or(0),
        "races": races.len(), "cycles": cycle_rows, "meanBiasPoints": bias,
        "withinCycleVariance": within, "estimatedCommonCycleVariance": common,
        "rawRaceVariance": raw_variance, "totalVariance": total,
        "standardDeviationPoints": total.sqrt(), "commonCorrelationProxy": common / total,
        "historicalReleaseClocksAvailable": false, "historicalPopulationLabelsAvailable": false,
        "distribution": "fixed_t4", "profitabilityEstablished": false}))
}

/// valuePollMargin(model, margin, {shift=0}).
pub fn value_poll_margin(model: &V, margin: f64, shift: Option<f64>) -> Result<f64, String> {
    let shift = shift.unwrap_or(0.0);
    need(margin.is_finite() && shift.is_finite(), "assertion failed")?;
    probability_from_margin(margin + model["meanBiasPoints"].as_f64().unwrap_or(0.0) + shift,
        model["standardDeviationPoints"].as_f64().unwrap_or(0.0))
}

/// currentKnownMargin(polls, {electionDate, asOf}).
pub fn current_known_margin(polls: &[V], election_date: &str, as_of: &str) -> Result<V, String> {
    let def = definition();
    let election = parse_date12(election_date);
    let clock = baal_service_runtime::contract::parse_iso_ms(as_of).unwrap_or(f64::NAN);
    need(election.is_finite() && clock.is_finite() && election > clock, "assertion failed")?;
    let mut selected: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    let mut excluded: Vec<V> = Vec::new();
    for p in polls {
        if p["methodologyVerified"] != V::Bool(true)
            || p["fieldStart"].as_str().is_none() || p["fieldEnd"].as_str().is_none() {
            excluded.push(json!({"source": p["source"], "reason": "fieldwork_or_methodology_unverified"}));
            continue;
        }
        let from = parse_date12(p["fieldStart"].as_str().unwrap());
        let to = parse_date12(p["fieldEnd"].as_str().unwrap());
        let middle = (from + to) / 2.0;
        let horizon = (election - middle) / 86_400_000.0;
        if !(horizon.is_finite() && from <= to && to <= clock
            && horizon >= def["forecastHorizonDays"].as_f64().unwrap()
            && horizon <= def["forecastHorizonDays"].as_f64().unwrap() + def["maximumPollAgeDays"].as_f64().unwrap()
            && p["margin"].as_f64().is_some_and(|m| m.is_finite())) {
            excluded.push(json!({"source": p["source"], "reason": "field_clock_or_horizon"}));
            continue;
        }
        let source = p["source"].as_str().unwrap_or("").to_string();
        let replace = selected.get(&source)
            .map_or(true, |old| middle > old["middle"].as_f64().unwrap_or(0.0));
        if replace {
            let mut row = p.clone();
            row["middle"] = json!(middle);
            row["horizon"] = json!(horizon);
            row["weight"] = json!(2f64.powf(-(horizon - def["forecastHorizonDays"].as_f64().unwrap())
                / def["recencyHalfLifeDays"].as_f64().unwrap()));
            selected.insert(source, row);
        }
    }
    need(!selected.is_empty(), "assertion failed")?;
    let rows: Vec<V> = selected.values().cloned().collect();
    let weight: f64 = rows.iter().map(|r| r["weight"].as_f64().unwrap_or(0.0)).sum();
    Ok(json!({"margin": rows.iter().map(|r| r["margin"].as_f64().unwrap_or(0.0) * r["weight"].as_f64().unwrap_or(0.0) / weight).sum::<f64>(),
        "sources": rows.iter().map(|r| json!({"source": r["source"],
            "horizonDays": r["horizon"], "weight": r["weight"].as_f64().unwrap_or(0.0) / weight})).collect::<Vec<_>>(),
        "excluded": excluded, "precisionGainAssumed": false}))
}

/// chronologicalHorizonDiagnostic(observations, years=[2020,2022]).
pub fn chronological_horizon_diagnostic(observations: &[V], years: Option<&[i64]>) -> Result<V, String> {
    let years: Vec<i64> = years.map(|y| y.to_vec()).unwrap_or_else(|| vec![2020, 2022]);
    let def = definition();
    let mut rows: Vec<V> = Vec::new();
    for &year in &years {
        let model = fit_horizon_error(observations, year - 1)?;
        need(model["maximumTrainingElectionYear"].as_i64().unwrap_or(0) < year, "assertion failed")?;
        let year_obs: Vec<V> = observations.iter()
            .filter(|r| r["year"].as_i64().unwrap_or(0) == year).cloned().collect();
        for race in aggregate_race_polls(&year_obs, &def)? {
            rows.push(json!({"key": race["key"], "year": year, "pollMargin": race["pollMargin"],
                "democraticWon": race["democraticWon"],
                "probability": value_poll_margin(&model, race["pollMargin"].as_f64().unwrap_or(0.0), None)?,
                "trainingThroughYear": model["maximumTrainingElectionYear"]}));
        }
    }
    let y_num = |r: &V| if r["democraticWon"] == V::Bool(true) { 1.0 } else { 0.0 };
    let brier = |rs: &[V]| -> f64 {
        rs.iter().map(|r| {
            let d = r["probability"].as_f64().unwrap_or(0.0) - y_num(r); d * d
        }).sum::<f64>() / rs.len() as f64
    };
    let by_year: Vec<V> = years.iter().map(|year| {
        let rs: Vec<V> = rows.iter().filter(|r| r["year"].as_i64() == Some(*year)).cloned().collect();
        json!({"year": year, "races": rs.len(), "brier": brier(&rs)})
    }).collect();
    Ok(json!({"races": rows.len(), "years": years, "brier": brier(&rows), "byYear": by_year,
        "rows": rows, "untouchedOriginalForecastVintage": false, "marketPnlAvailable": false,
        "profitabilityEstablished": false}))
}
