// research/current-polls-v1/compare.mjs — exact subsequent-receipt comparison
// of each market-bound v1 assessment against the recorded books (not a
// relabeled live quote; a model unavailable at request start is rejected).
use crate::current_election__assessment::price_decision;
use crate::files::{log_json, read_bytes, read_json, runtime_dir, sha256, write_json_pretty_wx};
use crate::js::{date_parse, js_number, js_string, now_ms, num, require_that};
use baal_execution::core::{decimal_micros, format_micros};
use baal_service_runtime::ledger::payload_hash;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let base = Path::new("research/current-polls-v1");
    let runtime = runtime_dir()?;
    let record = read_json(&base.join("assessments-20261005-v1.json"))?;
    require_that(
        sha256(&read_bytes(&base.join("assessment.mjs"))?)
            == record["assessmentCodeHash"].as_str().unwrap_or(""),
        "ASSESSMENT_CODE_CHANGED",
    )?;
    let activation = read_json(&runtime.join("competition-activation.json"))?;
    let bank = read_json(&runtime.join("production-banks").join(format!(
        "{}.json",
        activation["strategyBankHash"].as_str().unwrap_or("")
    )))?;
    let metadata = runtime
        .join("production-metadata")
        .join("current-poll-post-assessment-20261005-v1");
    let now_ms = now_ms();
    let now = crate::js::iso(now_ms);
    let mut rows: Vec<V> = Vec::new();
    for a in record["assessments"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| crate::js::js_truthy(a.get("marketId")))
    {
        let market_id = a["marketId"].as_str().unwrap_or_default();
        let node = read_json(&metadata.join(format!("{market_id}.json")))?;
        let receipt = read_json(&metadata.join(format!("{market_id}.book.json")))?;
        let book = &receipt["book"];
        let details = &node["root"]["contract_details"];
        let expected = if a["party"].as_str() == Some("republican") {
            "Republican Party"
        } else {
            "Democratic Party"
        };
        require_that(
            payload_hash(&node["root"]).map_err(|e| e.message)?
                == node["rootHash"].as_str().unwrap_or("")
                && node["marketId"] == a["marketId"]
                && details["usState"] == a["state"]
                && details["winnerName"].as_str() == Some(expected)
                && details["raceStage"].as_str() == Some("General")
                && details["electionDate"].as_str() == Some("2026-11-03")
                && details["resolutionType"].as_str() == Some("Party Winner"),
            "CURRENT_CONTRACT_BINDING_CHANGED",
        )?;
        let decision = price_decision(&json!({
            "assessment": a["assessment"],
            "book": book,
            "asOf": book["receivedAt"],
            "policy": bank["policy"],
        }))?;
        let raw = if crate::js::js_truthy(decision.get("maximumAsk")) {
            Some(decimal_micros(&decision["maximumAsk"], "exact")?)
        } else {
            None
        };
        let maximum_executable_ask = match raw {
            None => V::Null,
            Some(x) if x < 0 => V::Null,
            Some(x) => json!(format_micros(x / 5000 * 5000)),
        };
        let variants = a["assessment"]["ordinary"]["variants"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mean = variants
            .iter()
            .map(|v| js_number(v.get("discountedValuation")))
            .sum::<f64>()
            / variants.len() as f64;
        rows.push(json!({
            "assessmentId": a["id"],
            "state": a["state"],
            "party": a["party"],
            "marketId": a["marketId"],
            "exchangeId": book["exchangeId"],
            "rootHash": node["rootHash"],
            "bookResponseHash": receipt["responseHash"],
            "assessmentAt": a["assessment"]["assessedAt"],
            "bookRequestStartedAt": book["requestStartedAt"],
            "bookReceivedAt": book["receivedAt"],
            "quoteAgeAtReportMs": num(now_ms - date_parse(&js_string(book.get("receivedAt").unwrap_or(&V::Null)))),
            "bookAsk": book["asks"][0]["price"],
            "bookBid": book["bids"][0]["price"],
            "ordinary": [a["assessment"]["ordinary"]["lower"], a["assessment"]["ordinary"]["upper"]],
            "stress": [a["assessment"]["stress"]["lower"], a["assessment"]["stress"]["upper"]],
            "uniformScenarioMean": num(mean),
            "meanInterpretation": "Descriptive average over equally enumerated judgment scenarios, not a calibrated forecast or admission rule.",
            "maximumExecutableAsk": maximum_executable_ask,
            "decision": decision,
            "liveQuoteAtReport": false,
            "competitionOrders": 0,
            "executionAuthority": false,
        }));
    }
    let eligible = rows
        .iter()
        .filter(|r| r["decision"]["eligible"] == V::Bool(true))
        .count();
    let out = json!({
        "schemaVersion": 1,
        "id": "subsequent-primary-poll-price-comparison-20261005-v1",
        "computedAt": now,
        "sourceRecordId": record["id"],
        "originalProductionRunId": activation["runId"],
        "originalEntryExpiresAt": activation["entryExpiresAt"],
        "activationChanged": false,
        "productionBankChanged": false,
        "rows": rows,
        "eligiblePriceReceipts": num(eligible as f64),
        "additionalAccountClients": 0,
        "metadataUsesExistingAuxiliarySlots": true,
        "fees": "0.000000",
        "profitabilityEstablished": false,
        "interpretation": "These are exact, dated model/contract/book comparisons. No hypothetical fill or actual net profit is inferred. Execution remains under the original expired entry window.",
    });
    write_json_pretty_wx(&base.join("price-comparison-20261005-v1.json"), &out)?;
    let log = json!({
        "computedAt": now,
        "eligibleReceipts": out["eligiblePriceReceipts"],
        "rows": out["rows"].as_array().cloned().unwrap_or_default().iter().map(|r| json!({
            "state": r["state"],
            "party": r["party"],
            "ask": r["bookAsk"],
            "maximumExecutableAsk": r["maximumExecutableAsk"],
            "reason": r["decision"]["reason"],
            "sourceAfterModel": date_parse(&js_string(r.get("bookRequestStartedAt").unwrap_or(&V::Null)))
                >= date_parse(&js_string(r.get("assessmentAt").unwrap_or(&V::Null))),
        })).collect::<Vec<V>>(),
        "ordersSubmitted": 0,
    });
    log_json(&log);
    Ok(log)
}
