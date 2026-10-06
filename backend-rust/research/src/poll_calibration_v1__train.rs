// research/poll-calibration-v1/train.mjs — refit the frozen poll-error model
// family on the verified archive and write report + summary (or verify an
// existing pair with --verify).
use crate::election_baseline_v1__model::{hash, parse_csv};
use crate::election_baseline_v2__model::senate_races_v2;
use crate::files::{log_json, read_bytes, read_json, read_text, runtime_dir, write_json_pretty_wx};
use crate::js::{js_max, js_number, now_iso, num, require_that};
use crate::poll_calibration_v1__frozen_model_baseline_20261005 as frozen;
use serde_json::{json, Value as V};
use std::path::Path;

fn eq_err(a: &V, b: &V) -> String {
    format!(
        "{} == {}",
        baal_service_runtime::json_text::js_stringify(a),
        baal_service_runtime::json_text::js_stringify(b)
    )
}

pub fn run(verify: bool) -> Result<V, String> {
    let base = Path::new("research/poll-calibration-v1");
    let dir = runtime_dir()?.join("poll-calibration-sources-20261005");
    let definition_bytes = read_bytes(&base.join("definition.json"))?;
    let definition: V = serde_json::from_slice(&definition_bytes).map_err(|e| e.to_string())?;
    let sources = read_json(&base.join("sources.json"))?;
    for input in sources["captures"].as_array().cloned().unwrap_or_default() {
        let file = input["file"].as_str().unwrap_or_default();
        let actual = json!(hash(&read_bytes(&dir.join(file))?));
        require_that(
            actual == input["sha256"],
            "immutable_poll_source_changed",
        )?;
    }
    let senate_bytes = read_bytes(Path::new(
        "research/election-baseline-v1/sources/1976-2024-senate-state.csv",
    ))?;
    let overrides_bytes = read_bytes(Path::new(
        "research/election-baseline-v2/final-stage-overrides.json",
    ))?;
    let verified_manifest = read_json(Path::new(
        "research/election-baseline-v1/sources/PEJ5QU-manifest.json",
    ))?;
    let want_hash = verified_manifest["files"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .find(|x| x["filename"].as_str() == Some("1976-2024-senate-state.csv"))
        .map(|x| x["sha256"].as_str().unwrap_or("").to_string())
        .unwrap_or_default();
    require_that(
        hash(&senate_bytes) == want_hash,
        eq_err(&json!(hash(&senate_bytes)), &json!(want_hash)),
    )?;
    let overrides_list: Vec<V> = serde_json::from_slice(&overrides_bytes)
        .map_err(|e| e.to_string())?;
    let senate = senate_races_v2(
        &parse_csv(&String::from_utf8_lossy(&senate_bytes).to_string())?,
        &overrides_list,
        true,
    )?;
    let raw = parse_csv(&read_text(&dir.join("raw_polls.csv"))?)?;
    let observations = frozen::poll_observations(&raw, &senate)?;
    let races = frozen::aggregate_race_polls(&observations.rows, &definition)?;
    let predictions = frozen::chronological_predictions(&races, &definition)?;
    let families: Vec<String> = definition["families"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|f| f.as_str().unwrap_or("").to_string())
        .collect();
    let mut all_by_family = serde_json::Map::new();
    for f in &families {
        let sub: Vec<V> = predictions
            .iter()
            .filter(|r| r["family"].as_str() == Some(f.as_str()))
            .cloned()
            .collect();
        all_by_family.insert(f.clone(), frozen::score(&sub)?);
    }
    let last_year = races
        .iter()
        .map(|r| js_number(r.get("year")))
        .fold(f64::NEG_INFINITY, js_max);
    let minimum_races = js_number(definition.get("minimumTrainingRaces")) as usize;
    let minimum_cycles = js_number(definition.get("minimumCalibrationCycles")) as usize;
    let minimum_sd = js_number(definition.get("minimumMarginSdPoints"));
    let mut models = serde_json::Map::new();
    for f in &families {
        models.insert(
            f.clone(),
            frozen::fit_poll_error(&races, f, minimum_races, minimum_cycles, minimum_sd)?,
        );
    }
    let mut cycles: Vec<V> = Vec::new();
    let mut seen: Vec<f64> = Vec::new();
    for r in &races {
        let y = js_number(r.get("year"));
        if !seen.contains(&y) {
            seen.push(y);
            cycles.push(num(y));
        }
    }
    let mut by_cycle: Vec<V> = Vec::new();
    for year in definition["diagnosticYears"].as_array().cloned().unwrap_or_default() {
        let y = js_number(Some(&year));
        let mut fam = serde_json::Map::new();
        for f in &families {
            let sub: Vec<V> = predictions
                .iter()
                .filter(|r| {
                    r["family"].as_str() == Some(f.as_str())
                        && r.get("year").and_then(V::as_f64) == Some(y)
                })
                .cloned()
                .collect();
            fam.insert(f.clone(), frozen::score(&sub)?);
        }
        by_cycle.push(json!({"year": year, "families": V::Object(fam)}));
    }
    let report = json!({
        "schemaVersion": 1,
        "id": definition["id"],
        "at": now_iso(),
        "definitionHash": hash(&definition_bytes),
        "modelCodeHash": hash(&read_bytes(&base.join("frozen-model-baseline-20261005.mjs"))?),
        "sourceHashes": sources["captures"].as_array().cloned().unwrap_or_default().iter().map(|s| json!({
            "file": s["file"], "sha256": s["sha256"],
        })).collect::<Vec<V>>(),
        "independentFinalSourceHash": hash(&senate_bytes),
        "finalOverridesHash": hash(&overrides_bytes),
        "rowsInSource": num(raw.len() as f64),
        "includedPollObservations": num(observations.rows.len() as f64),
        "exclusions": observations.excluded,
        "races": num(races.len() as f64),
        "cycles": cycles,
        "models": V::Object(models.clone()),
        "diagnostics": V::Object(all_by_family.clone()),
        "byCycle": by_cycle,
        "raceRows": races,
        "diagnosticRows": predictions,
        "qualifiedProductionForecasts": 0,
        "competitionOrders": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "This retrospective collector selects the eventual top two finishers; final identities are independently matched, but coverage cannot represent all originally available races.",
            "Publication dates and LV/RV populations are absent from the archive. Median field dates and observation times are distinct.",
            "One race is one calibration observation; state errors remain correlated within election cycles. Two diagnostic cycles are weak evidence of 2026 transport.",
            "No historical executable market prices, depth, fill queues or net trading returns are supplied by this diagnostic.",
            "No current bank, window, inventory or paper accounting is changed.",
        ],
    });
    let full = base.join("report-20261005-v1.json");
    let summary_file = base.join("summary-20261005-v1.json");
    if verify {
        let summary = read_json(&summary_file)?;
        require_that(
            summary.get("reportHash").and_then(V::as_str) == Some(hash(&read_bytes(&full)?).as_str()),
            eq_err(&summary["reportHash"], &json!(hash(&read_bytes(&full)?))),
        )?;
        require_that(
            summary.get("models") == report.get("models"),
            "assert.deepEqual(summary.models, report.models)",
        )?;
        require_that(
            summary.get("diagnostics") == report.get("diagnostics"),
            "assert.deepEqual(summary.diagnostics, report.diagnostics)",
        )?;
        require_that(
            summary.get("definitionHash") == report.get("definitionHash"),
            eq_err(&summary["definitionHash"], &report["definitionHash"]),
        )?;
        require_that(
            summary.get("modelCodeHash") == report.get("modelCodeHash"),
            eq_err(&summary["modelCodeHash"], &report["modelCodeHash"]),
        )?;
        let out = json!({
            "sourceAndModelReproduced": true,
            "reportImmutable": true,
            "races": num(races.len() as f64),
        });
        log_json(&out);
        return Ok(out);
    }
    write_json_pretty_wx(&full, &report)?;
    let mut summary = report.as_object().cloned().unwrap_or_default();
    summary.insert("reportHash".into(), json!(hash(&read_bytes(&full)?)));
    summary.insert("reportFile".into(), json!("report-20261005-v1.json"));
    summary.shift_remove("raceRows");
    summary.shift_remove("diagnosticRows");
    write_json_pretty_wx(&summary_file, &V::Object(summary))?;
    let out = json!({
        "sourcePolls": num(observations.rows.len() as f64),
        "races": num(races.len() as f64),
        "lastYear": num(last_year),
        "models": V::Object(models),
        "diagnostics": V::Object(all_by_family),
        "exclusions": observations.excluded,
    });
    log_json(&out);
    Ok(out)
}
