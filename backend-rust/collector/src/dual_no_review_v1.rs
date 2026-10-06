// live-paper/dual-no-review-v1.mjs — dual-NO pair review validation.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::js_truthy;
use serde_json::Value as V;

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("DUAL_NO_REVIEW_{}", code)); } Ok(()) }
fn date(s: &V) -> bool { s.as_str().is_some_and(|x| x.len() == 10 && x.as_bytes()[4] == b'-' && x.as_bytes()[7] == b'-' && x.bytes().enumerate().all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())) }
fn instant(v: &V) -> Result<i64, String> { crate::model::instant(v.as_str().unwrap_or("")) }
const FIELDS: [&str; 9] = ["raceId","stageId","usState","raceName","raceStage","officeLevel","contractType","electionDate","resolutionType"];

/// validateDualNoReview(review)
pub fn validate_dual_no_review(review: &V) -> Result<V, String> {
    crate::canonical_no_capacity::validate_capacity_review(review)?;
    let mut roots = Vec::new();
    for leg in review["legs"].as_array().cloned().unwrap_or_default() {
        let root = leg["root"].clone();
        let p = root["contract_details"].clone();
        need(root["node_type"].as_str() == Some("contract") && root["contract_type"].as_str() == Some("Election Outcome")
            && root.get("settled_with").is_some_and(|v| v.is_null()) && js_truthy(Some(&p))
            && leg["captureMarketId"] == leg["marketId"], "ROOT")?;
        need(root["contract_id"].as_str().is_some_and(|s| !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit()))
            && root["node_id"] == root["contract_id"], "CONTRACT_ID")?;
        need(payload_hash(&root)? == leg["rulesHash"].as_str().unwrap_or(""), "ROOT_HASH")?;
        need(FIELDS.iter().all(|k| p[*k].as_str().is_some_and(|s| !s.is_empty()))
            && p["raceStage"].as_str() == Some("General") && p["resolutionType"].as_str() == Some("Party Winner")
            && p["contractType"].as_str() == Some("Election Outcome"), "PREDICATE")?;
        need(["Democratic Party","Republican Party"].contains(&p["winnerName"].as_str().unwrap_or("")), "PARTY")?;
        need(date(&p["electionDate"]) && instant(&root["settlement_date"])? > instant(&review["recordedAt"])?, "SETTLEMENT")?;
        roots.push(root);
    }
    need(roots[0]["contract_details"]["winnerName"] != roots[1]["contract_details"]["winnerName"], "DIFFERENT_PARTIES")?;
    need(FIELDS.iter().all(|k| roots[0]["contract_details"][*k] == roots[1]["contract_details"][*k])
        && roots[0]["settlement_date"] == roots[1]["settlement_date"], "SAME_RACE_STAGE")?;
    need(review["normalResolutionOnly"].as_bool() == Some(true) && review["administrativeRefundGapAcknowledged"].as_bool() == Some(true), "EXCEPTION_ACK")?;
    // Separate party-winner markets are not exhaustive complements. Their NO/NO
    // floor follows only from normal mutual exclusion; third-party wins pay both.
    for pair in [[0i64, 1i64], [1, 0], [1, 1]] {
        need(review["scenarios"].as_array().is_some_and(|ss| ss.iter().any(|s| s["category"].as_str() == Some("ordinary")
            && s["payouts"].as_array().is_some_and(|pa| pa.iter().enumerate().all(|(i, x)| pair.get(i).is_some_and(|&p| x.as_i64() == Some(p)))))), "NORMAL_SCENARIOS")?;
    }
    Ok(review.clone())
}
