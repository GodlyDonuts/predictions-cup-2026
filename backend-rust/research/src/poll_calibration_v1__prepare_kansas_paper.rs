// research/poll-calibration-v1/prepare-kansas-paper.mjs — bind the Kansas
// Democratic market, fit the single-poll error family plus jackknife, and
// write the Kansas paper assessment + bundle.
use crate::election_baseline_v1__model::{hash, parse_csv};
use crate::election_baseline_v2__model::senate_races_v2;
use crate::files::{
    log_json, read_bytes, read_json, read_text, runtime_dir, to_fixed, write_json_pretty_wx,
};
use crate::js::{
    date_parse, iso, js_max, js_min, js_number, now_iso, now_ms, num, require_that,
};
use crate::poll_calibration_v1__model::{fit_single_poll_error, poll_observations, probability};
use baal_collector::probability_paper::validate_research;
use baal_service_runtime::ledger::payload_hash;
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
    let base = Path::new("research/poll-calibration-v1");
    let runtime = runtime_dir()?;
    let dir = runtime.join("poll-calibration-sources-20261005");
    let definition = read_json(&base.join("definition.json"))?;
    let sources = read_json(&base.join("sources.json"))?;
    for s in sources["captures"].as_array().cloned().unwrap_or_default() {
        let actual = hash(&read_bytes(&dir.join(s["file"].as_str().unwrap_or("")))?);
        require_that(
            actual == s["sha256"].as_str().unwrap_or(""),
            eq_err(&json!(actual), &s["sha256"]),
        )?;
    }
    let senators = read_bytes(Path::new(
        "research/election-baseline-v1/sources/1976-2024-senate-state.csv",
    ))?;
    let overrides = read_bytes(Path::new(
        "research/election-baseline-v2/final-stage-overrides.json",
    ))?;
    let known = read_json(Path::new(
        "research/election-baseline-v2/model-summary-affiliation-invariant.json",
    ))?;
    let want = known["sourceManifests"][0]["files"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .find(|f| f["filename"].as_str() == Some("1976-2024-senate-state.csv"))
        .map(|f| f["sha256"].as_str().unwrap_or("").to_string())
        .unwrap_or_default();
    require_that(
        hash(&senators) == want,
        eq_err(&json!(hash(&senators)), &json!(want)),
    )?;
    require_that(
        hash(&overrides) == known["overridesHash"].as_str().unwrap_or(""),
        eq_err(&json!(hash(&overrides)), &known["overridesHash"]),
    )?;
    let overrides_list: Vec<V> =
        serde_json::from_slice(&overrides).map_err(|e| e.to_string())?;
    let observations = poll_observations(
        &parse_csv(&read_text(&dir.join("raw_polls.csv"))?)?,
        &senate_races_v2(
            &parse_csv(&String::from_utf8_lossy(&senators).to_string())?,
            &overrides_list,
            true,
        )?,
    )?
    .rows;
    let model = fit_single_poll_error(&observations, &definition, None)?;
    let mut jackknife: Vec<V> = Vec::new();
    for cycle in model["cycles"].as_array().cloned().unwrap_or_default() {
        jackknife.push(fit_single_poll_error(
            &observations,
            &definition,
            Some(js_number(Some(&cycle))),
        )?);
    }
    let source_record = read_json(Path::new(
        "research/current-polls-v1/assessments-20261005-v1.json",
    ))?;
    let candidate = source_record["assessments"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|a| {
            a["state"].as_str() == Some("KS") && a["party"].as_str() == Some("democratic")
        })
        .ok_or("candidate".to_string())?;
    let poll = candidate["poll"].clone();
    let field_age =
        (now_ms() - date_parse(&crate::js::js_string(poll.get("fieldEndLatestAt").unwrap_or(&V::Null))))
            / 86_400_000.0;
    require_that(
        poll["population"].as_str() == Some("likely_voters")
            && poll["ballotVerified"] == V::Bool(true)
            && field_age <= 30.0,
        "current_poll_scope_or_age",
    )?;
    let source = candidate["sources"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|s| s["file"].as_str() == Some("kansas-emerson.html"))
        .ok_or("source".to_string())?;
    require_that(
        hash(&read_bytes(
            &runtime
                .join("research-sources-current-polls-20261005")
                .join(source["file"].as_str().unwrap_or("")),
        )?) == source["contentHash"].as_str().unwrap_or(""),
        eq_err(
            &json!(hash(&read_bytes(
                &runtime
                    .join("research-sources-current-polls-20261005")
                    .join(source["file"].as_str().unwrap_or("")),
            )?)),
            &source["contentHash"],
        ),
    )?;
    let root = read_json(
        &runtime
            .join("production-metadata")
            .join("current-poll-post-assessment-20261005-v1")
            .join("356.json"),
    )?;
    require_that(
        root["rootHash"].as_str()
            == Some(payload_hash(&root["root"]).map_err(|e| e.message)?.as_str())
            && root["root"]["contract_details"]["usState"].as_str() == Some("KS")
            && root["root"]["contract_details"]["winnerName"].as_str() == Some("Democratic Party")
            && root["root"]["contract_details"]["resolutionType"].as_str() == Some("Party Winner"),
        "contract_scope",
    )?;
    let margin = 100.0
        * (js_number(poll.get("democratic")) - js_number(poll.get("republican")));
    let mut estimates: Vec<V> = vec![json!({
        "model": model,
        "probability": num(probability(&model, margin)?),
    })];
    for m in &jackknife {
        estimates.push(json!({
            "model": m,
            "probability": num(probability(m, margin)?),
        }));
    }
    let now = now_iso();
    let haircut = 0.02;
    let min_p = estimates
        .iter()
        .map(|e| js_number(e.get("probability")))
        .fold(f64::INFINITY, js_min);
    let max_p = estimates
        .iter()
        .map(|e| js_number(e.get("probability")))
        .fold(f64::NEG_INFINITY, js_max);
    let lower = js_max(0.0, min_p - haircut);
    let upper = js_max(lower, max_p - haircut);
    let research = json!({
        "schemaVersion": 1,
        "id": "kansas-democratic-empirical-poll-20261005-v1",
        "marketId": "356",
        "exchangeId": "1045",
        "side": "YES",
        "predicateHash": root["rootHash"],
        "recordedAt": now,
        "sourceAt": now,
        "expiresAt": iso(date_parse(&now) + 21_600_000.0),
        "lower": to_fixed(lower, 6),
        "upper": to_fixed(upper, 6),
        "method": "subjective_range",
        "settlementReviewed": true,
        "flags": [],
        "sources": candidate["sources"].as_array().cloned().unwrap_or_default().iter().map(|s| json!({
            "url": s["url"], "contentHash": s["contentHash"],
        })).chain(std::iter::once(json!({
            "url": sources["captures"][0]["url"],
            "contentHash": sources["captures"][0]["sha256"],
        }))).collect::<Vec<V>>(),
    });
    validate_research(&research)?;
    let stress: Vec<V> = [-3.0, 0.0, 3.0]
        .iter()
        .map(|shift| {
            Ok(json!({
                "shiftPoints": num(*shift),
                "valuation": num(js_max(
                    0.0,
                    probability(&model, margin + shift)? - haircut,
                )),
            }))
        })
        .collect::<Result<Vec<V>, String>>()?;
    let assessment = json!({
        "schemaVersion": 1,
        "id": "kansas-single-poll-error-assessment-20261005-v1",
        "at": now,
        "modelCodeHash": hash(&read_bytes(&base.join("model.mjs"))?),
        "definitionHash": hash(&read_bytes(&base.join("definition.json"))?),
        "model": model,
        "estimates": estimates,
        "poll": poll,
        "marginPoints": num(margin),
        "componentClocks": {
            "fieldEndDate": poll["fieldEndDate"],
            "originalPollObservedAt": poll["observedAt"],
            "modelTrainedThroughYear": model["trainedThroughYear"],
            "rawArchiveObservedAt": sources["captures"][0]["observedAt"],
            "assessmentAt": now,
        },
        "research": research,
        "rootHash": root["rootHash"],
        "ordinaryRange": [num(lower), num(upper)],
        "turnoutOrPollingShiftStress": stress,
        "limitations": [
            "Cycle-deletion parameter sensitivity is not a confidence bound. A fixed2-point haircut is a judgment reserve.",
            "Historical individual-poll errors give one race equal weight and include within-race poll dispersion.",
            "The archive lacks population labels and original release times; specific likely-voter transport is not empirically established.",
            "Current Kansas evidence is one September15–16 poll, not a fresh polling consensus.",
            "Expected settlement value does not predict a profitable short-term exit or guarantee a positive paper return.",
            "Third-party/undecided dynamics and turnout shocks remain possible; the stress family may contradict the ordinary estimate."
        ],
        "executionAuthority": false,
        "competitionOrders": 0,
        "profitabilityEstablished": false,
    });
    let bundle = json!({
        "identities": {"356": {"exchangeId": "1045", "predicateHash": root["rootHash"]}},
        "research": [research],
        "researchEvidence": [assessment],
    });
    write_json_pretty_wx(
        &base.join("kansas-paper-assessment-20261005-v1.json"),
        &assessment,
    )?;
    write_json_pretty_wx(&base.join("kansas-paper-bundle-20261005-v1.json"), &bundle)?;
    let out = json!({
        "at": now,
        "modelRaces": model["races"],
        "individualPolls": model["pollsterRaceObservations"],
        "errorSdPoints": model["standardDeviationPoints"],
        "errorBiasPoints": model["biasPoints"],
        "ordinary": [num(lower), num(upper)],
        "stresses": assessment["turnoutOrPollingShiftStress"],
        "marketId": "356",
        "productionBankChanged": false,
        "competitionOrders": 0,
    });
    log_json(&out);
    Ok(out)
}
