// research/governor-error-v1/build.mjs — cross-check the governor poll-error
// family on the verified outcome archive, rescore the frozen Texas valuation,
// and write results-20261005-v1.json.
use crate::current_polls_v2__assessment::assess_primary_polls;
use crate::election_baseline_v1__model::{hash, parse_csv};
use crate::files::{log_json, read_bytes, read_json, runtime_dir, write_json_pretty_wx};
use crate::js::{js_number, now_iso, num, require_that};
use crate::governor_error_v1__model::governor_poll_observations;
use crate::poll_calibration_v1__model::{
    aggregate_race_polls, fit_single_poll_error, probability, score,
};
use serde_json::{json, Value as V};
use std::path::Path;

fn eq_err(a: &V, b: &V) -> String {
    format!(
        "{} == {}",
        baal_service_runtime::json_text::js_stringify(a),
        baal_service_runtime::json_text::js_stringify(b)
    )
}

pub fn run() -> Result<V, String> {
    let rt = runtime_dir()?;
    let source = read_json(Path::new("data/governor-error-source-manifest-20261005.json"))?;
    let definition = read_json(Path::new("research/current-polls-v2/definition.json"))?;
    let frozen = read_json(Path::new(
        "research/current-polls-v3/texas-governor-assessment-20261005-v3.json",
    ))?;
    let polls = read_bytes(&rt.join("poll-calibration-sources-20261005").join("raw_polls.csv"))?;
    let poll_source = read_json(Path::new("research/poll-calibration-v1/sources.json"))?
        ["captures"][0]
        .clone();
    require_that(
        hash(&polls) == poll_source["sha256"].as_str().unwrap_or(""),
        eq_err(&json!(hash(&polls)), &poll_source["sha256"]),
    )?;
    let outcome_source = source["records"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|s| s["file"].as_str() == Some("election_results_gubernatorial.csv"))
        .ok_or("outcomeSource".to_string())?;
    let outcomes = read_bytes(
        &rt.join("governor-error-sources-20261005")
            .join(outcome_source["file"].as_str().unwrap_or("")),
    )?;
    require_that(
        hash(&outcomes) == outcome_source["sha256"].as_str().unwrap_or(""),
        eq_err(&json!(hash(&outcomes)), &outcome_source["sha256"]),
    )?;
    let observations = governor_poll_observations(
        &parse_csv(&String::from_utf8_lossy(&polls).to_string())?,
        &parse_csv(&String::from_utf8_lossy(&outcomes).to_string())?,
    )?;
    let model = fit_single_poll_error(&observations.rows, &definition, None)?;
    let mut models: Vec<V> = vec![model.clone()];
    for y in model["cycles"].as_array().cloned().unwrap_or_default() {
        models.push(fit_single_poll_error(
            &observations.rows,
            &definition,
            Some(js_number(Some(&y))),
        )?);
    }
    let races = aggregate_race_polls(&observations.rows, &definition)?;
    let mut predictions: Vec<V> = Vec::new();
    for year in model["cycles"].as_array().cloned().unwrap_or_default() {
        let y = js_number(Some(&year));
        let prior: Vec<V> = observations
            .rows
            .iter()
            .filter(|p| js_number(p.get("year")) < y)
            .cloned()
            .collect();
        let test: Vec<V> = races
            .iter()
            .filter(|r| r.get("year").and_then(V::as_f64) == Some(y))
            .cloned()
            .collect();
        if test.is_empty() {
            continue;
        }
        let fitted = match fit_single_poll_error(&prior, &definition, None) {
            Ok(f) => f,
            Err(_) => continue,
        };
        require_that(
            js_number(fitted.get("trainedThroughYear")) < y,
            "assert(fitted.trainedThroughYear<year)",
        )?;
        for r in test {
            let mut r = r.as_object().cloned().unwrap_or_default();
            r.insert(
                "trainedThroughYear".into(),
                fitted["trainedThroughYear"].clone(),
            );
            r.insert(
                "democraticWinProbability".into(),
                num(probability(&fitted, js_number(r.get("pollMargin")))?),
            );
            predictions.push(V::Object(r));
        }
    }
    let at = now_iso();
    let valuation = assess_primary_polls(&json!({
        "polls": frozen["polls"],
        "models": models,
        "definition": definition,
        "at": frozen["at"],
    }))?;
    // new URL('./model.mjs', import.meta.url) — hashes the model file next to
    // the running module; the port's analogue is the sibling .rs model.
    let model_code_hash = {
        let own = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("governor_error_v1__model.rs");
        hash(&read_bytes(&own)?)
    };
    let result = json!({
        "schemaVersion": 1,
        "id": "governor-error-20261005-v1",
        "at": at,
        "sourceHashes": {
            "polls": hash(&polls),
            "outcomes": hash(&outcomes),
            "modelCode": model_code_hash,
        },
        "sourceUrls": {
            "polls": poll_source["url"],
            "outcomes": outcome_source["url"],
        },
        "verifiedOutcomeRaces": observations.verified_outcome_races,
        "acceptedPollQuestions": num(observations.rows.len() as f64),
        "excluded": observations.excluded,
        "model": model,
        "models": models,
        "chronological": score(&predictions)?,
        "predictionRows": predictions.iter().map(|r| json!({
            "key": r["key"],
            "year": r["year"],
            "pollMargin": r["pollMargin"],
            "actualMargin": r["actualMargin"],
            "democraticWon": r["democraticWon"],
            "trainedThroughYear": r["trainedThroughYear"],
            "democraticWinProbability": r["democraticWinProbability"],
        })).collect::<Vec<V>>(),
        "texasValuation": valuation,
        "activeProxyBankUnchanged": true,
        "forecastCandidateOnly": true,
        "executionAuthority": false,
        "profitabilityEstablished": false,
        "limitations": [
            "The polling and result archives are separately maintained by the same publisher; this is a cross-check, not independent official certification of every race.",
            "Historical release clocks and LV/RV labels are unavailable. Chronological outcome calibration is not historical market profitability.",
            "Race and cycle errors are correlated; current Governor and Senate forecasts share national risk.",
            "The current three-poll selection and sensitivity are research assumptions; do not replace the active frozen bank or claim its fills use this later model."
        ],
    });
    write_json_pretty_wx(
        Path::new("research/governor-error-v1/results-20261005-v1.json"),
        &result,
    )?;
    let out = json!({
        "at": at,
        "races": model["races"],
        "pollsterRaceObservations": model["pollsterRaceObservations"],
        "bias": model["biasPoints"],
        "sd": model["standardDeviationPoints"],
        "chronological": result["chronological"],
        "texas": {
            "ordinary": valuation["ordinaryRange"],
            "central": valuation["centralEstimate"],
            "severe": valuation["severeOpponentAllocationStress"],
        },
        "activeProxyBankUnchanged": true,
    });
    log_json(&out);
    Ok(out)
}
