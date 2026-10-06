// research/current-polls-v2/build.mjs — refit the single-poll error family on
// the verified archive, assess the captured primary polls and write
// assessments-20261005-v2.json.
use crate::current_polls_v2__assessment::assess_primary_polls;
use crate::election_baseline_v1__model::{hash, parse_csv};
use crate::election_baseline_v2__model::senate_races_v2;
use crate::files::{read_bytes, read_json, runtime_dir, write_json_pretty_wx};
use crate::js::{date_parse, iso, js_number, now_iso, num, require_that};
use crate::poll_calibration_v1__model::{fit_single_poll_error, poll_observations};
use serde_json::{json, Value as V};
use std::path::Path;

fn eq_err(a: &V, b: &V) -> String {
    format!(
        "{} == {}",
        baal_service_runtime::json_text::js_stringify(a),
        baal_service_runtime::json_text::js_stringify(b)
    )
}

fn poll(pollster: &str, population: &str, d: f64, r: f64, unallocated: f64, field_end_date: &str, file: &str) -> V {
    json!({
        "pollster": pollster,
        "population": population,
        "democratic": num(d),
        "republican": num(r),
        "unallocated": num(unallocated),
        "fieldEndDate": field_end_date,
        "fieldEndAt": format!("{field_end_date}T23:59:59.999Z"),
        "file": file,
    })
}

pub fn run() -> Result<V, String> {
    let runtime = runtime_dir()?;
    let base = Path::new("research/current-polls-v2");
    let definition = read_json(&base.join("definition.json"))?;
    let at = now_iso();
    let source_files = [
        "research/current-polls-v2/sources.json".to_string(),
        "research/current-polls-v2/comparator-sources.json".to_string(),
        "research/current-polls-v1/sources.json".to_string(),
    ];
    let mut manifests: Vec<V> = Vec::new();
    for f in &source_files {
        manifests.push(read_json(Path::new(f))?);
    }
    for (i, m) in manifests.iter().enumerate() {
        for s in m["captures"].as_array().cloned().unwrap_or_default() {
            let dir = if i == 2 {
                "research-sources-current-polls-20261005"
            } else {
                "research-sources-current-marist-20261005-v2"
            };
            let actual = hash(&read_bytes(&runtime.join(dir).join(s["file"].as_str().unwrap_or("")))?);
            require_that(actual == s["contentHash"].as_str().unwrap_or(""), eq_err(&json!(actual), &s["contentHash"]))?;
        }
    }
    let history = read_json(Path::new("research/poll-calibration-v1/sources.json"))?;
    let historical = read_bytes(&runtime.join("poll-calibration-sources-20261005").join("raw_polls.csv"))?;
    require_that(
        hash(&historical) == history["captures"][0]["sha256"].as_str().unwrap_or(""),
        eq_err(&json!(hash(&historical)), &history["captures"][0]["sha256"]),
    )?;
    let senate_bytes = read_bytes(Path::new(
        "research/election-baseline-v1/sources/1976-2024-senate-state.csv",
    ))?;
    let overrides_bytes = read_bytes(Path::new(
        "research/election-baseline-v2/final-stage-overrides.json",
    ))?;
    let known = read_json(Path::new(
        "research/election-baseline-v2/model-summary-affiliation-invariant.json",
    ))?;
    let want_senate = known["sourceManifests"][0]["files"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .find(|f| f["filename"].as_str() == Some("1976-2024-senate-state.csv"))
        .map(|f| f["sha256"].as_str().unwrap_or("").to_string())
        .unwrap_or_default();
    require_that(
        hash(&senate_bytes) == want_senate,
        eq_err(&json!(hash(&senate_bytes)), &json!(want_senate)),
    )?;
    require_that(
        hash(&overrides_bytes) == known["overridesHash"].as_str().unwrap_or(""),
        eq_err(&json!(hash(&overrides_bytes)), &known["overridesHash"]),
    )?;
    let overrides_list: Vec<V> =
        serde_json::from_slice(&overrides_bytes).map_err(|e| e.to_string())?;
    let senate = senate_races_v2(
        &parse_csv(&String::from_utf8_lossy(&senate_bytes).to_string())?,
        &overrides_list,
        true,
    )?;
    let observations = poll_observations(
        &parse_csv(&String::from_utf8_lossy(&historical).to_string())?,
        &senate,
    )?
    .rows;
    let model = fit_single_poll_error(&observations, &definition, None)?;
    let mut models: Vec<V> = vec![model.clone()];
    for c in model["cycles"].as_array().cloned().unwrap_or_default() {
        models.push(fit_single_poll_error(
            &observations,
            &definition,
            Some(js_number(Some(&c))),
        )?);
    }
    let inputs: Vec<V> = vec![
        json!({"state":"MI","marketId":"358","exchangeId":"1047","candidates":{"democratic":"Abdul El-Sayed","republican":"Mike Rogers"},"ballotSource":"michigan-official.html","polls":[
            poll("Marist","registered_voters",51.0,44.0,4.0,"2026-09-27","michigan-marist.pdf"),
            poll("Emerson","likely_voters",48.0,46.0,6.0,"2026-09-14","michigan-emerson.html")]}),
        json!({"state":"TX","marketId":"293","exchangeId":"982","candidates":{"democratic":"James Talarico","republican":"Ken Paxton","libertarian":"Ted Brown"},"ballotSource":"texas-official.pdf","polls":[
            poll("Marist","registered_voters",50.0,44.0,4.0,"2026-09-20","texas-marist.pdf"),
            poll("Emerson","likely_voters",47.0,46.0,5.0,"2026-09-14","texas-emerson.html")]}),
        json!({"state":"IA","marketId":"260","exchangeId":"949","candidates":{"democratic":"Josh Turek","republican":"Ashley Hinson","libertarian":"Thomas Laehn"},"ballotSource":"iowa-official.html","polls":[
            poll("Marist","registered_voters",50.0,42.0,8.0,"2026-09-20","iowa-marist.html"),
            poll("Emerson","likely_voters",45.0,50.0,5.0,"2026-09-01","iowa-emerson.html")]}),
        json!({"state":"OH","marketId":V::Null,"exchangeId":V::Null,"candidates":{"democratic":"Sherrod Brown","republican":"Jon Husted"},"ballotSource":V::Null,"polls":[
            poll("Marist","registered_voters",51.0,43.0,5.0,"2026-09-27","ohio-marist.pdf")]}),
    ];
    let captures: Vec<V> = manifests
        .iter()
        .flat_map(|m| m["captures"].as_array().cloned().unwrap_or_default())
        .collect();
    let mut assessments: Vec<V> = Vec::new();
    for input in &inputs {
        let valuation = assess_primary_polls(&json!({
            "polls": input["polls"],
            "models": models,
            "definition": definition,
            "at": at,
        }))?;
        let sources: Vec<V> = captures
            .iter()
            .filter(|c| {
                input["polls"].as_array().is_some_and(|ps| {
                    ps.iter().any(|p| p["file"] == c["file"])
                }) || c["file"] == input["ballotSource"]
            })
            .cloned()
            .collect();
        let mut o = input.as_object().cloned().unwrap_or_default();
        o.insert(
            "id".into(),
            json!(format!(
                "{}-democratic-primary-polls-20261005-v2",
                input["state"].as_str().unwrap_or("").to_lowercase()
            )),
        );
        o.insert("at".into(), json!(at));
        o.insert(
            "expiresAt".into(),
            json!(iso(date_parse(&at)
                + js_number(definition.get("assessmentLifetimeMs")))),
        );
        o.insert("sources".into(), V::Array(sources));
        for (k, v) in valuation.as_object().cloned().unwrap_or_default() {
            o.insert(k, v);
        }
        assessments.push(V::Object(o));
    }
    let out = json!({
        "schemaVersion": 1,
        "id": definition["id"],
        "at": at,
        "definitionHash": hash(&read_bytes(&base.join("definition.json"))?),
        "assessmentCodeHash": hash(&read_bytes(&base.join("assessment.mjs"))?),
        "historicalSourceHash": hash(&historical),
        "model": model,
        "models": models,
        "assessments": assessments,
        "accountApiRequests": 0,
        "competitionOrders": 0,
        "executionAuthority": false,
        "profitabilityEstablished": false,
        "limitations": definition["limitations"],
    });
    write_json_pretty_wx(&base.join("assessments-20261005-v2.json"), &out)?;
    let log = json!({
        "races": model["races"],
        "pollsterRaceObservations": model["pollsterRaceObservations"],
        "bias": model["biasPoints"],
        "sd": model["standardDeviationPoints"],
        "states": out["assessments"].as_array().cloned().unwrap_or_default().iter().map(|a| json!({
            "state": a["state"],
            "margin": a["marginPoints"],
            "ordinary": a["ordinaryRange"],
            "central": a["centralEstimate"],
            "severe": a["severeOpponentAllocationStress"],
            "inCatalogue": !a["marketId"].is_null(),
        })).collect::<Vec<V>>(),
    });
    // console.log(JSON.stringify(log, null, 2)) — the JS prints this pretty.
    println!("{}", serde_json::to_string_pretty(&log).map_err(|e| e.to_string())?);
    Ok(log)
}
