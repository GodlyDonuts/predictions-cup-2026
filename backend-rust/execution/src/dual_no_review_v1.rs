// live-paper/dual-no-review-v1.mjs — contract-predicate binding of a
// conditional no-pair review. Separate party-winner markets are not exhaustive
// complements: their NO/NO floor follows only from normal mutual exclusion;
// third-party wins pay both.
use crate::canonical_no_capacity::validate_capacity_review;
use crate::core::payload_hash;
use baal_service_runtime::fixed::instant;
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("DUAL_NO_REVIEW_{code}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
const FIELDS: [&str; 9] = [
    "raceId", "stageId", "usState", "raceName", "raceStage", "officeLevel",
    "contractType", "electionDate", "resolutionType",
];

// validateDualNoReview(review)
pub fn validate_dual_no_review(review: &V) -> Result<V, String> {
    validate_capacity_review(review)?;
    let legs = get(review, "legs").as_array().cloned().unwrap_or_default();
    let mut roots = Vec::new();
    for leg in &legs {
        let root = get(leg, "root");
        let p = get(root, "contract_details");
        need(
            get(root, "node_type").as_str() == Some("contract")
                && get(root, "contract_type").as_str() == Some("Election Outcome")
                && root.get("settled_with") == Some(&V::Null)
                && baal_service_runtime::json_text::js_truthy(Some(p))
                && get(leg, "captureMarketId") == get(leg, "marketId"),
            "ROOT",
        )?;
        let contract_id = get(root, "contract_id").as_str().unwrap_or("");
        need(
            !contract_id.is_empty() && !contract_id.starts_with('0')
                && contract_id.bytes().all(|b| b.is_ascii_digit())
                && get(root, "node_id") == get(root, "contract_id"),
            "CONTRACT_ID",
        )?;
        need(payload_hash(root)? == get(leg, "rulesHash").as_str().unwrap_or(""), "ROOT_HASH")?;
        need(
            FIELDS.iter().all(|k| get(p, k).as_str().is_some_and(|s| !s.is_empty()))
                && get(p, "raceStage").as_str() == Some("General")
                && get(p, "resolutionType").as_str() == Some("Party Winner")
                && get(p, "contractType").as_str() == Some("Election Outcome"),
            "PREDICATE",
        )?;
        need(
            matches!(get(p, "winnerName").as_str(), Some("Democratic Party" | "Republican Party")),
            "PARTY",
        )?;
        let election_date = get(p, "electionDate").as_str().unwrap_or("");
        need(
            election_date.len() == 10
                && election_date.bytes().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() }
                })
                && instant(get(root, "settlement_date").as_str().unwrap_or(""))?
                    > instant(get(review, "recordedAt").as_str().unwrap_or(""))?,
            "SETTLEMENT",
        )?;
        roots.push(root.clone());
    }
    need(
        get(&roots[0], "contract_details").get("winnerName") != get(&roots[1], "contract_details").get("winnerName"),
        "DIFFERENT_PARTIES",
    )?;
    let p0 = get(&roots[0], "contract_details");
    let p1 = get(&roots[1], "contract_details");
    need(
        FIELDS.iter().all(|k| get(p0, k) == get(p1, k))
            && get(&roots[0], "settlement_date") == get(&roots[1], "settlement_date"),
        "SAME_RACE_STAGE",
    )?;
    need(
        get(review, "normalResolutionOnly") == &json!(true)
            && get(review, "administrativeRefundGapAcknowledged") == &json!(true),
        "EXCEPTION_ACK",
    )?;
    let scenarios = get(review, "scenarios").as_array().cloned().unwrap_or_default();
    for pair in [[0i64, 1i64], [1, 0], [1, 1]] {
        need(
            scenarios.iter().any(|s| {
                get(s, "category").as_str() == Some("ordinary")
                    && get(s, "payouts").as_array().is_some_and(|p| {
                        p.len() == 2
                            && p.iter().enumerate().all(|(i, x)| {
                                x.as_i64() == Some(pair[i])
                            })
                    })
            }),
            "NORMAL_SCENARIOS",
        )?;
    }
    Ok(review.clone())
}
