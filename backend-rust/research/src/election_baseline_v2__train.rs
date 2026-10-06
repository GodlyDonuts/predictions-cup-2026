// research/election-baseline-v2/train.mjs — affiliation-invariant revision of
// the baseline fit with cycle-deletion diagnostics and an optional --verify
// reproduction check (bare `run(verify)` reproduces then exits like the JS).
use crate::election_baseline_v1__model::{
    expanding_check, fit_ridge, hash, metrics, parse_csv, predict,
    presidential_tables, probability_from_margin,
};
use crate::election_baseline_v2__model::{build_rows_v2, senate_races_v2};
use crate::files::{log_json, read_bytes, read_json, read_text, write_wx};
use crate::js::{date_parse, js_max, js_number, js_string, now_iso, now_ms, num, require_that};
use baal_service_runtime::json_text::js_stringify;
use indexmap::IndexMap;
use serde_json::{json, Value as V};
use std::path::Path;

fn eq_err(a: &V, b: &V) -> String {
    format!("{} == {}", js_stringify(a), js_stringify(b))
}

// revision='initial' unless affiliation_invariant; verify mirrors --verify.
pub fn run(affiliation_invariant: bool, verify: bool) -> Result<V, String> {
    let base = Path::new("research/election-baseline-v2");
    let previous = Path::new("research/election-baseline-v1");
    let revision = if affiliation_invariant {
        "affiliation-invariant"
    } else {
        "initial"
    };
    let definition_bytes = read_bytes(&base.join("definition.json"))?;
    let definition: V = serde_json::from_slice(&definition_bytes).map_err(|e| e.to_string())?;
    require_that(
        date_parse(&js_string(&definition["declaredAt"])) <= now_ms(),
        "assert(Date.parse(definition.declaredAt)<=Date.now())",
    )?;
    let mut manifests: Vec<V> = Vec::new();
    for x in ["PEJ5QU", "42MVDX"] {
        manifests.push(read_json(&previous.join("sources").join(format!("{x}-manifest.json")))?);
    }
    for m in &manifests {
        for f in m["files"].as_array().cloned().unwrap_or_default() {
            let filename = f["filename"].as_str().unwrap_or_default();
            let actual = json!(hash(&read_bytes(&previous.join("sources").join(filename))?));
            require_that(actual == f["sha256"], eq_err(&actual, &f["sha256"]))?;
        }
    }
    let overrides_bytes = read_bytes(&base.join("final-stage-overrides.json"))?;
    let overrides: V = serde_json::from_slice(&overrides_bytes).map_err(|e| e.to_string())?;
    let overrides_list = overrides.as_array().cloned().unwrap_or_default();
    let senate = senate_races_v2(
        &parse_csv(&read_text(&previous.join("sources/1976-2024-senate-state.csv"))?)?,
        &overrides_list,
        revision == "affiliation-invariant",
    )?;
    let president = presidential_tables(&parse_csv(&read_text(
        &previous.join("sources/1976-2024-president.csv"),
    )?)?)?;
    let minimum_year = js_number(definition.get("minimumTrainingYear"));
    let rows = build_rows_v2(&senate, &president, minimum_year)?;
    let dev_last_year = js_number(definition.get("developmentLastYear"));
    let mut development: Vec<V> = Vec::new();
    for lambda in definition["ridgeGrid"].as_array().cloned().unwrap_or_default() {
        let lambda_f = js_number(Some(&lambda));
        let predictions = expanding_check(&rows, lambda_f, dev_last_year, 80)?;
        let summary = metrics(&predictions)?;
        let cycles = summary["cycles"].as_array().cloned().unwrap_or_default();
        let mut sum = 0.0f64;
        for y in &cycles {
            let yf = js_number(Some(y));
            let sub: Vec<V> = predictions
                .iter()
                .filter(|r| r.get("year").and_then(V::as_f64) == Some(yf))
                .cloned()
                .collect();
            sum += js_number(metrics(&sub)?.get("rmse")).powi(2);
        }
        development.push(json!({
            "lambda": lambda,
            "metrics": summary,
            "cycleMse": num(sum / cycles.len() as f64),
        }));
    }
    development.sort_by(|a, b| {
        js_number(a.get("cycleMse"))
            .partial_cmp(&js_number(b.get("cycleMse")))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                js_number(a.get("lambda"))
                    .partial_cmp(&js_number(b.get("lambda")))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let selected = development[0].clone();
    let residual_rms = js_max(0.10, js_number(selected["metrics"].get("rmse")));
    let mut diagnostics: Vec<V> = Vec::new();
    for year_v in definition["observedDiagnosticYears"].as_array().cloned().unwrap_or_default() {
        let year = js_number(Some(&year_v));
        let training: Vec<V> = rows
            .iter()
            .filter(|r| js_number(r.get("year")) < year)
            .cloned()
            .collect();
        let model = fit_ridge(&training, js_number(selected.get("lambda")))?;
        require_that(
            js_number(model.get("trainedThroughYear")) < year,
            "assert(model.trainedThroughYear<year)",
        )?;
        for r in rows
            .iter()
            .filter(|r| r.get("year").and_then(V::as_f64) == Some(year))
        {
            let predicted_margin = predict(&model, &x_vec(r)) + js_number(r.get("nationalBaseline"));
            let win_probability = probability_from_margin(predicted_margin, residual_rms)?;
            let outcome = js_number(r.get("winnerRepublican"));
            diagnostics.push(json!({
                "key": r["key"],
                "year": num(year),
                "predictedMargin": num(predicted_margin),
                "actualMargin": r["actualMargin"],
                "residual": num(js_number(r.get("actualMargin")) - predicted_margin),
                "trainedThroughYear": model["trainedThroughYear"],
                "clocks": r["clocks"],
                "outcome": num(outcome),
                "winProbability": num(win_probability),
            }));
        }
    }
    let model = fit_ridge(&rows, js_number(selected.get("lambda")))?;
    let at = now_iso();
    let mut diagnostic_summary = metrics(&diagnostics)?.as_object().cloned().unwrap_or_default();
    diagnostic_summary.insert(
        "correctDirections".to_string(),
        num(diagnostics
            .iter()
            .filter(|r| {
                (js_number(r.get("predictedMargin")) > 0.0)
                    == (js_number(r.get("actualMargin")) > 0.0)
            })
            .count() as f64),
    );
    diagnostic_summary.insert(
        "brier".to_string(),
        num(diagnostics
            .iter()
            .map(|r| (js_number(r.get("winProbability")) - js_number(r.get("outcome"))).powi(2))
            .sum::<f64>()
            / diagnostics.len() as f64),
    );
    diagnostic_summary.insert(
        "logLoss".to_string(),
        num(diagnostics
            .iter()
            .map(|r| {
                let o = js_number(r.get("outcome"));
                let w = js_number(r.get("winProbability"));
                -(o * w.ln() + (1.0 - o) * (1.0 - w).ln())
            })
            .sum::<f64>()
            / diagnostics.len() as f64),
    );
    let mut diag_by_cycle: Vec<V> = Vec::new();
    for year_v in definition["observedDiagnosticYears"].as_array().cloned().unwrap_or_default() {
        let year = js_number(Some(&year_v));
        let sub: Vec<V> = diagnostics
            .iter()
            .filter(|r| r.get("year").and_then(V::as_f64) == Some(year))
            .cloned()
            .collect();
        diag_by_cycle.push(metrics(&sub)?);
    }
    diagnostic_summary.insert("byCycle".to_string(), V::Array(diag_by_cycle));
    diagnostic_summary.insert("untouched".to_string(), json!(false));
    diagnostic_summary.insert(
        "reason".to_string(),
        json!("These cycles were already inspected in V1 and the runoff correction. Outcome diagnostics only; no historical market PnL or original forecast vintage."),
    );
    let report = json!({
        "schemaVersion": 2,
        "id": definition["id"],
        "revision": revision,
        "at": at,
        "definitionHash": hash(&definition_bytes),
        "overridesHash": hash(&overrides_bytes),
        "sourceManifests": manifests,
        "includedRaces": num(senate.races.len() as f64),
        "rows": num(rows.len() as f64),
        "exclusions": senate.exclusions,
        "corrections": senate.corrections,
        "development": development,
        "selectedLambda": selected["lambda"],
        "selectedUsingYearsThrough": num(dev_last_year),
        "residualRms": num(residual_rms),
        "model": model,
        "modelHash": hash(js_stringify(&model).as_bytes()),
        "diagnostics": V::Object(diagnostic_summary.clone()),
        "diagnosticRows": diagnostics,
        "qualifiedProductionForecasts": 0,
        "profitabilityEstablished": false,
        "limitations": definition["limitations"],
    });
    let report_path = base.join(if revision == "initial" {
        "model-report.json".to_string()
    } else {
        format!("model-report-{revision}.json")
    });
    let summary_path = base.join(if revision == "initial" {
        "model-summary.json".to_string()
    } else {
        format!("model-summary-{revision}.json")
    });
    if verify {
        let saved = read_json(&summary_path)?;
        require_that(
            saved.get("modelHash") == report.get("modelHash"),
            eq_err(&saved["modelHash"], &report["modelHash"]),
        )?;
        require_that(
            saved.get("rows") == report.get("rows"),
            eq_err(&saved["rows"], &report["rows"]),
        )?;
        require_that(
            saved.get("diagnostics") == report.get("diagnostics"),
            "assert.deepEqual(saved.diagnostics, report.diagnostics)".to_string(),
        )?;
        require_that(
            saved.get("overridesHash") == report.get("overridesHash"),
            eq_err(&saved["overridesHash"], &report["overridesHash"]),
        )?;
        require_that(
            saved.get("definitionHash") == report.get("definitionHash"),
            eq_err(&saved["definitionHash"], &report["definitionHash"]),
        )?;
        require_that(
            saved.get("fullReportHash").and_then(V::as_str) == Some(hash(&read_bytes(&report_path)?).as_str()),
            eq_err(&saved["fullReportHash"], &json!(hash(&read_bytes(&report_path)?))),
        )?;
        let out = json!({
            "revision": revision,
            "modelFitReproduced": true,
            "immutableReportHashVerified": true,
            "rows": report["rows"],
        });
        log_json(&out);
        return Ok(out);
    }
    write_wx(
        &report_path,
        (serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n").as_bytes(),
    )?;
    let mut summary = report.as_object().cloned().unwrap_or_default();
    summary.shift_remove("diagnosticRows");
    summary.shift_remove("exclusions");
    let exclusions = report["exclusions"].as_array().cloned().unwrap_or_default();
    summary.insert(
        "excludedRaces".to_string(),
        num(exclusions.len() as f64),
    );
    let mut seen: Vec<String> = Vec::new();
    let mut by_reason = IndexMap::new();
    for x in &exclusions {
        let reason = js_string(x.get("reason").unwrap_or(&V::Null));
        if !seen.contains(&reason) {
            seen.push(reason.clone());
            let count = exclusions
                .iter()
                .filter(|e| js_string(e.get("reason").unwrap_or(&V::Null)) == reason)
                .count();
            by_reason.insert(reason, num(count as f64));
        }
    }
    summary.insert(
        "exclusionsByReason".to_string(),
        V::Object(by_reason.into_iter().collect()),
    );
    summary.insert(
        "fullReportHash".to_string(),
        json!(hash(&read_bytes(&report_path)?)),
    );
    summary.insert(
        "fullReportFilename".to_string(),
        json!(report_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()),
    );
    let summary = V::Object(summary);
    write_wx(
        &summary_path,
        (serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())? + "\n").as_bytes(),
    )?;
    let out = json!({
        "rows": report["rows"],
        "includedRaces": report["includedRaces"],
        "excludedRaces": summary["excludedRaces"],
        "selectedLambda": report["selectedLambda"],
        "diagnostics": report["diagnostics"],
        "corrections": report["corrections"],
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
