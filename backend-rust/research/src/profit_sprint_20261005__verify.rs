// research/profit-sprint-20261005/verify.mjs — re-verify every captured
// profit-sprint source (bytes + sha256 + https origin) and the scope/
// sensitivity invariants of the comparison and identity reports.
use crate::files::{log_json, read_bytes, read_json, read_text, runtime_dir, sha256};
use crate::js::{js_number, num, require_that};
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
    let dir = runtime_dir()?.join("research-profit-sprint-20261005");
    let sources = read_json(Path::new("research/profit-sprint-20261005/sources.json"))?;
    let mut checked = 0usize;
    for s in sources["captures"].as_array().cloned().unwrap_or_default() {
        if s["status"].as_str() != Some("captured") {
            continue;
        }
        let b = read_bytes(&dir.join(s["file"].as_str().unwrap_or("")))?;
        require_that(
            b.len() as f64 == js_number(s.get("bytes")),
            eq_err(&json!(b.len()), &s["bytes"]),
        )?;
        let actual = sha256(&b);
        require_that(
            actual == s["contentHash"].as_str().unwrap_or(""),
            eq_err(&json!(actual), &s["contentHash"]),
        )?;
        require_that(
            s["url"].as_str().unwrap_or("").starts_with("https://"),
            eq_err(&json!(s["url"].as_str().unwrap_or("").starts_with("https://")), &json!(true)),
        )?;
        checked += 1;
    }
    let comparison = read_json(Path::new(
        "research/profit-sprint-20261005/texas-governor-comparison.json",
    ))?;
    let primary = comparison["primaryPolls"].as_array().cloned().unwrap_or_default();
    require_that(
        primary.len() == 6,
        eq_err(&json!(primary.len()), &json!(6)),
    )?;
    let mut names: Vec<V> = Vec::new();
    for p in &primary {
        if !names.contains(&p["pollster"]) {
            names.push(p["pollster"].clone());
        }
    }
    require_that(names.len() == 6, eq_err(&json!(names.len()), &json!(6)))?;
    require_that(
        comparison["executionAuthority"] == json!(false),
        eq_err(&comparison["executionAuthority"], &json!(false)),
    )?;
    require_that(
        comparison["activeBanksUnchanged"] == json!(true),
        eq_err(&comparison["activeBanksUnchanged"], &json!(true)),
    )?;
    require_that(
        js_number(comparison["allVerifiedPrimaryPolls"]["ordinaryRange"]
            .as_array()
            .and_then(|a| a.first()))
            < 0.295,
        "assert(comparison.allVerifiedPrimaryPolls.ordinaryRange[0]<0.295)",
    )?;
    require_that(
        js_number(comparison["allVerifiedPrimaryPolls"].get("centralEstimate"))
            < js_number(comparison["previousSelectionAtCurrentClock"].get("centralEstimate")),
        "assert(comparison.allVerifiedPrimaryPolls.centralEstimate<comparison.previousSelectionAtCurrentClock.centralEstimate)",
    )?;
    require_that(
        comparison["unverifiedClockSensitivityPoll"]["primaryClockVerified"] == json!(false),
        eq_err(
            &comparison["unverifiedClockSensitivityPoll"]["primaryClockVerified"],
            &json!(false),
        ),
    )?;
    let loo = comparison["leaveOnePollsterOut"].as_array().cloned().unwrap_or_default();
    require_that(loo.len() == 6, eq_err(&json!(loo.len()), &json!(6)))?;
    let bigdata = read_text(&dir.join("bigdata-texas.html"))?;
    for text in ["793", "698", "September 24", "48.9", "44.4"] {
        require_that(
            bigdata.contains(text),
            "assert(bigdata.includes(text))",
        )?;
    }
    let ids = read_json(Path::new("research/profit-sprint-20261005/market-identities.json"))?;
    let ex293 = ids["selected"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|x| x["marketId"].as_str() == Some("293"))
        .map(|x| x["exchangeId"].clone())
        .unwrap_or(V::Null);
    require_that(ex293 == json!("982"), eq_err(&ex293, &json!("982")))?;
    let ex195 = ids["selected"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|x| x["marketId"].as_str() == Some("195"))
        .map(|x| x["exchangeId"].clone())
        .unwrap_or(V::Null);
    require_that(ex195 == json!("884"), eq_err(&ex195, &json!("884")))?;
    require_that(
        ids["settlementDates"] == json!(["2026-11-04T17:00:00.000Z"]),
        "assert.deepEqual(ids.settlementDates,['2026-11-04T17:00:00.000Z'])",
    )?;
    let out = json!({
        "publicSourceHashesVerified": num(checked as f64),
        "scopeAndSensitivityChecksPassed": true,
        "accountApiRequests": 0,
        "executionAuthority": false,
    });
    log_json(&out);
    Ok(out)
}
