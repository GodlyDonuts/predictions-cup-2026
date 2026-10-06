// research/current-polls-v2/compare.mjs — exact-modeled entry price and edge
// diagnostics on the captured auxiliary books (historical price diagnostic
// only; the books preceded assessment).
use crate::files::{read_json, runtime_dir, write_json_pretty_wx};
use crate::js::{date_parse, js_number, js_string, now_iso, now_ms, num, require_that};
use baal_paper::fixed::{big_amount, money, TICK};
use baal_service_runtime::ledger::payload_hash;
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::path::Path;

fn eq_err(a: &V, b: &V) -> String {
    format!(
        "{} == {}",
        baal_service_runtime::json_text::js_stringify(a),
        baal_service_runtime::json_text::js_stringify(b)
    )
}
fn field_s(v: &V, key: &str) -> String {
    crate::js::js_string(v.get(key).unwrap_or(&V::Null))
}

pub fn run() -> Result<V, String> {
    let base = Path::new("research/current-polls-v2");
    let assessment_file = base.join("assessments-20261005-v2.json");
    let data = read_json(&assessment_file)?;
    let dir = runtime_dir()?
        .join("production-metadata")
        .join("current-marist-20261005-v2");
    let now_ms_v = now_ms();
    let mut rows: Vec<V> = Vec::new();
    for a in data["assessments"].as_array().cloned().unwrap_or_default() {
        if a["marketId"].is_null() {
            rows.push(json!({
                "state": a["state"],
                "marketId": V::Null,
                "decision": "not_in_captured_tournament_catalogue",
                "eligible": false,
            }));
            continue;
        }
        let market_id = a["marketId"].as_str().unwrap_or_default();
        let node = read_json(&dir.join(format!("{market_id}.json")))?;
        let receipt = read_json(&dir.join(format!("{market_id}.book.json")))?;
        let b = &receipt["book"];
        let c = &node["root"]["contract_details"];
        require_that(
            payload_hash(&node["root"]).map_err(|e| e.message)?
                == node["rootHash"].as_str().unwrap_or(""),
            eq_err(&json!(payload_hash(&node["root"]).map_err(|e| e.message)?), &node["rootHash"]),
        )?;
        require_that(
            c["usState"] == a["state"]
                && c["raceName"].as_str().is_some_and(|s| s.starts_with("U.S. Senate"))
                && c["raceStage"].as_str() == Some("General")
                && c["electionDate"].as_str() == Some("2026-11-03")
                && c["winnerName"].as_str() == Some("Democratic Party")
                && c["resolutionType"].as_str() == Some("Party Winner")
                && b["exchangeId"] == a["exchangeId"]
                && node["root"].get("settled_with").is_some_and(V::is_null),
            "assert(c.usState===a.state&&c.raceName.startsWith('U.S. Senate')&&c.raceStage==='General'&&c.electionDate==='2026-11-03'&&c.winnerName==='Democratic Party'&&c.resolutionType==='Party Winner'&&b.exchangeId===a.exchangeId&&node.root.settled_with===null)",
        )?;
        let lower = big_amount(&crate::files::to_fixed(
            js_number(a["ordinaryRange"].as_array().and_then(|x| x.first())),
            6,
        ))?;
        let entry = big_amount(&js_string(&b["asks"][0]["price"]))? + BigInt::from(TICK);
        let spread = big_amount(&js_string(&b["asks"][0]["price"]))?
            - big_amount(&js_string(&b["bids"][0]["price"]))?;
        let edge = &lower - &entry;
        let depth = (js_number(b["asks"][0].get("quantity")) / 100.0).floor();
        let admission_edge = big_amount("0.020000")?;
        let admission_spread = big_amount("0.020000")?;
        let economic_admission = edge >= admission_edge && spread <= admission_spread;
        rows.push(json!({
            "state": a["state"],
            "marketId": a["marketId"],
            "exchangeId": a["exchangeId"],
            "rootHash": node["rootHash"],
            "bookResponseHash": receipt["responseHash"],
            "requestStartedAt": b["requestStartedAt"],
            "receivedAt": b["receivedAt"],
            "assessmentAt": a["at"],
            "quoteAgeAtComparisonMs": num(now_ms_v - date_parse(&field_s(b, "receivedAt"))),
            "assessmentKnownBeforeRequest": date_parse(&field_s(&a, "at"))
                <= date_parse(&field_s(b, "requestStartedAt")),
            "bid": b["bids"][0],
            "ask": b["asks"][0],
            "ordinaryRange": a["ordinaryRange"],
            "centralEstimate": a["centralEstimate"],
            "severeOpponentAllocationStress": a["severeOpponentAllocationStress"],
            "modeledEntryPrice": money(&entry),
            "modeledOrdinaryLowerEdge": money(&edge),
            "centralExpectedSurplus": num(js_number(a.get("centralEstimate"))
                - entry.to_string().parse::<f64>().unwrap_or(f64::NAN) / 1e6),
            "fees": "0.000000",
            "slippageTicks": 1,
            "depthOnePercentShares": num(depth),
            "economicAdmission": economic_admission,
            "eligible": false,
            "decision": "historical_price_diagnostic_only",
        }));
    }
    let out = json!({
        "schemaVersion": 1,
        "at": now_iso(),
        "id": "primary-poll-executable-comparison-20261005-v2",
        "assessmentFile": assessment_file.to_string_lossy(),
        "rows": rows,
        "qualifiedReceipts": 0,
        "competitionOrders": 0,
        "accountClientsAdded": 0,
        "executionAuthority": false,
        "profitabilityEstablished": false,
        "limitations": [
            "These exact auxiliary books preceded assessment and are now historical. No causal current entry is inferred.",
            "Central expected surplus is a model estimate, not realized or prospective trading profit.",
            "RV/LV disagreement, severe opponent allocation, third-party outcomes and correlated polling error remain material risks.",
        ],
    });
    write_json_pretty_wx(&base.join("price-comparison-20261005-v2.json"), &out)?;
    let log = V::Array(
        rows.iter()
            .map(|r| {
                json!({
                    "state": r["state"],
                    "ordinaryLowerEdge": r["modeledOrdinaryLowerEdge"],
                    "centralExpectedSurplus": r["centralExpectedSurplus"],
                    "economicAdmission": r["economicAdmission"],
                    "eligible": r["eligible"],
                })
            })
            .collect(),
    );
    println!("{}", serde_json::to_string_pretty(&log).map_err(|e| e.to_string())?);
    Ok(log)
}
