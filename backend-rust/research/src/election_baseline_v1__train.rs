// research/election-baseline-v1/train.mjs — fit the baseline ridge model with
// an expanding-window development check, untouched-cycle checks, and write the
// model report.
use crate::election_baseline_v1__model::{
    build_rows, expanding_check, fit_ridge, hash, metrics, parse_csv, predict,
    presidential_tables, probability_from_margin, race_features, scenarios, senate_races,
};
use crate::files::{log_json, read_bytes, read_json, write_wx};
use crate::js::{date_parse, js_max, js_number, js_string, now_ms, num, require_that};
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

fn eq_err(a: &V, b: &V) -> String {
    format!("{} == {}", js_stringify(a), js_stringify(b))
}

pub fn run() -> Result<V, String> {
    let base = Path::new("research/election-baseline-v1");
    let definition_bytes = read_bytes(&base.join("definition.json"))?;
    let definition: V = serde_json::from_slice(&definition_bytes).map_err(|e| e.to_string())?;
    let declared = definition
        .get("amendedAt")
        .filter(|v| crate::js::js_truthy(Some(v)))
        .or(definition.get("declaredAt"))
        .cloned()
        .unwrap_or(V::Null);
    require_that(
        date_parse(&js_string(&declared)) <= now_ms(),
        "assert(Date.parse(definition.amendedAt || definition.declaredAt) <= Date.now())",
    )?;
    let mut source_manifests: Vec<V> = Vec::new();
    for name in ["PEJ5QU", "42MVDX"] {
        source_manifests.push(read_json(&base.join("sources").join(format!("{name}-manifest.json")))?);
    }
    for m in &source_manifests {
        for f in m["files"].as_array().cloned().unwrap_or_default() {
            let filename = f["filename"].as_str().unwrap_or_default();
            let actual = json!(hash(&read_bytes(&base.join("sources").join(filename))?));
            require_that(
                actual == f["sha256"],
                eq_err(&actual, &f["sha256"]),
            )?;
        }
    }
    let senate = senate_races(&parse_csv(&crate::files::read_text(
        &base.join("sources/1976-2024-senate-state.csv"),
    )?)?);
    let president = presidential_tables(&parse_csv(&crate::files::read_text(
        &base.join("sources/1976-2024-president.csv"),
    )?)?)?;
    let minimum_year = js_number(definition.get("minimumTrainingYear"));
    let rows = build_rows(&senate, &president, minimum_year)?;
    for r in &rows {
        require_that(
            js_number(r["clocks"].get("presYear")) < js_number(r.get("year"))
                && js_number(r["clocks"].get("priorSeatYear")) < js_number(r.get("year"))
                && js_number(r["clocks"].get("priorSeatPresYear"))
                    < js_number(r["clocks"].get("priorSeatYear")),
            "assert(r.clocks.presYear < r.year && r.clocks.priorSeatYear < r.year && r.clocks.priorSeatPresYear < r.clocks.priorSeatYear)",
        )?;
    }
    let dev_last_year = js_number(definition.get("developmentLastYear"));
    let mut development: Vec<V> = Vec::new();
    for lambda in definition["ridgeGrid"].as_array().cloned().unwrap_or_default() {
        let lambda_f = js_number(Some(&lambda));
        let predictions = expanding_check(&rows, lambda_f, dev_last_year, 80)?;
        let metrics_v = metrics(&predictions)?;
        let cycles = metrics_v["cycles"].as_array().cloned().unwrap_or_default();
        let mut sum = 0.0f64;
        for year in &cycles {
            let y = js_number(Some(year));
            let sub: Vec<V> = predictions
                .iter()
                .filter(|r| r.get("year").and_then(V::as_f64) == Some(y))
                .cloned()
                .collect();
            let m = metrics(&sub)?;
            sum += js_number(m.get("rmse")).powi(2);
        }
        let cycle_mse = sum / cycles.len() as f64;
        development.push(json!({
            "lambda": lambda,
            "metrics": metrics_v,
            "cycleMse": num(cycle_mse),
            "predictions": predictions,
        }));
    }
    development.sort_by(|a, b| {
        let am = js_number(a.get("cycleMse"));
        let bm = js_number(b.get("cycleMse"));
        am.partial_cmp(&bm)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                js_number(a.get("lambda"))
                    .partial_cmp(&js_number(b.get("lambda")))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let selected = development[0].clone();
    let residual_rms = js_max(0.10, js_number(selected["metrics"].get("rmse")));
    let mut checks: Vec<V> = Vec::new();
    for year_v in definition["untouchedCheckYears"].as_array().cloned().unwrap_or_default() {
        let year = js_number(Some(&year_v));
        let training: Vec<V> = rows
            .iter()
            .filter(|r| js_number(r.get("year")) < year)
            .cloned()
            .collect();
        let model = fit_ridge(&training, js_number(selected.get("lambda")))?;
        require_that(
            js_number(model.get("trainedThroughYear")) < year,
            "assert(model.trainedThroughYear < year)",
        )?;
        for r in rows
            .iter()
            .filter(|r| r.get("year").and_then(V::as_f64) == Some(year))
        {
            let predicted_margin = predict(&model, &x_vec(r)) + js_number(r.get("nationalBaseline"));
            let win_probability = probability_from_margin(predicted_margin, residual_rms)?;
            let outcome = if js_number(r.get("actualMargin")) > 0.0 { 1.0 } else { 0.0 };
            checks.push(json!({
                "key": r["key"],
                "year": num(year),
                "trainedThroughYear": model["trainedThroughYear"],
                "predictedMargin": num(predicted_margin),
                "actualMargin": r["actualMargin"],
                "residual": num(js_number(r.get("actualMargin")) - predicted_margin),
                "clocks": r["clocks"],
                "winProbability": num(win_probability),
                "outcome": num(outcome),
                "brier": num((win_probability - outcome).powi(2)),
                "logLoss": num(-(outcome * win_probability.ln()
                    + (1.0 - outcome) * (1.0 - win_probability).ln())),
            }));
        }
    }
    require_that(checks.len() > 50, "assert(checks.length > 50)")?;
    let final_model = fit_ridge(&rows, js_number(selected.get("lambda")))?;
    let inputs = read_json(&base.join("current-inputs.json"))?;
    require_that(
        date_parse(&js_string(&inputs["environment"]["observedAt"])) <= now_ms(),
        "assert(Date.parse(inputs.environment.observedAt) <= Date.now())",
    )?;
    let assessment_at_ms = now_ms();
    let mut forecasts: Vec<V> = Vec::new();
    for candidate in inputs["candidates"].as_array().cloned().unwrap_or_default() {
        let features = race_features(&candidate, &senate, &president)?
            .ok_or("assert(features)".to_string())?;
        require_that(
            !features.is_null(),
            "assert(features)",
        )?;
        let assessment = scenarios(&json!({
            "model": final_model,
            "features": features,
            "environment": inputs["environment"],
            "assessmentAtMs": num(assessment_at_ms),
            "residualRms": num(residual_rms),
            "definition": definition,
        }))?;
        let mut c = candidate.as_object().cloned().unwrap_or_default();
        c.insert("features".to_string(), features);
        c.insert("assessment".to_string(), assessment);
        c.insert("currentSourceGateSatisfied".to_string(), json!(false));
        c.insert(
            "eligibilityReason".to_string(),
            json!("Current ballot and complete visible settlement review still required before a production bank."),
        );
        forecasts.push(V::Object(c));
    }
    // includedCycles: first-occurrence year dedupe.
    let mut seen_years: Vec<f64> = Vec::new();
    let mut included_cycles: Vec<V> = Vec::new();
    for r in &rows {
        let y = js_number(r.get("year"));
        if !seen_years.contains(&y) {
            seen_years.push(y);
            included_cycles.push(num(y));
        }
    }
    let mut check_metrics = metrics(&checks)?.as_object().cloned().unwrap_or_default();
    check_metrics.insert(
        "brier".to_string(),
        num(checks.iter().map(|r| js_number(r.get("brier"))).sum::<f64>() / checks.len() as f64),
    );
    check_metrics.insert(
        "logLoss".to_string(),
        num(checks.iter().map(|r| js_number(r.get("logLoss"))).sum::<f64>() / checks.len() as f64),
    );
    check_metrics.insert("noInformationBrier".to_string(), num(0.25));
    let mut by_cycle: Vec<V> = Vec::new();
    for year_v in definition["untouchedCheckYears"].as_array().cloned().unwrap_or_default() {
        let year = js_number(Some(&year_v));
        let sub: Vec<V> = checks
            .iter()
            .filter(|r| r.get("year").and_then(V::as_f64) == Some(year))
            .cloned()
            .collect();
        by_cycle.push(metrics(&sub)?);
    }
    check_metrics.insert("byCycle".to_string(), V::Array(by_cycle));
    let report = json!({
        "schemaVersion": 1,
        "id": definition["id"],
        "at": crate::js::iso(assessment_at_ms),
        "definitionHash": hash(&definition_bytes),
        "sourceManifests": source_manifests,
        "sourceIntegrityVerified": true,
        "rows": num(rows.len() as f64),
        "includedCycles": included_cycles,
        "exclusions": senate.exclusions,
        "development": development.iter().map(|d| json!({
            "lambda": d["lambda"],
            "metrics": d["metrics"],
            "cycleMse": d["cycleMse"],
        })).collect::<Vec<V>>(),
        "selectedLambda": selected["lambda"],
        "selectedUsingYearsThrough": num(dev_last_year),
        "residualRms": num(residual_rms),
        "model": final_model,
        "modelHash": hash(js_stringify(&final_model).as_bytes()),
        "check": V::Object(check_metrics),
        "checkRows": checks,
        "developmentRows": selected["predictions"],
        "forecasts": forecasts,
        "forecastVintagesAvailable": false,
        "pollingTransportValidatedByHistoricalCheck": false,
        "calibrationEstablishedFor2026": false,
        "qualifiedProductionForecasts": 0,
        "ordersSubmitted": 0,
        "paperFills": 0,
        "profitabilityEstablished": false,
        "limitations": definition["limitations"],
    });
    write_wx(
        &base.join("model-report.json"),
        (serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n").as_bytes(),
    )?;
    let out = json!({
        "rows": report["rows"],
        "exclusions": num(report["exclusions"].as_array().map(|a| a.len() as f64).unwrap_or(0.0)),
        "selectedLambda": report["selectedLambda"],
        "development": report["development"],
        "check": report["check"],
        "forecasts": report["forecasts"].as_array().cloned().unwrap_or_default().iter().map(|f| json!({
            "marketId": f["marketId"],
            "meanMargin": f["assessment"]["meanMargin"],
            "lower": f["assessment"]["lower"],
            "upper": f["assessment"]["upper"],
            "expiresAt": f["assessment"]["expiresAt"],
            "incumbentNameMatch": f["features"]["incumbentNameMatch"],
            "currentSourceGateSatisfied": false,
        })).collect::<Vec<V>>(),
        "qualifiedProductionForecasts": 0,
        "profitabilityEstablished": false,
    });
    log_json(&out);
    Ok(out)
}

fn x_vec(r: &V) -> Vec<f64> {
    r["x"]
        .as_array()
        .map(|a| a.iter().map(|v| js_number(Some(v))).collect())
        .unwrap_or_default()
}
