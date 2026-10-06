// live-paper/canonical-no-capacity-v2.mjs — refund-scenario capacity extension.
use baal_paper::fixed::{big_amount, money, SCALE, TICK};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("CANONICAL_REFUND_{}", code)); } Ok(()) }
/// validateCapacityReview(review) — base review + refund policy + scenarios.
pub fn validate_capacity_review(review: &V) -> Result<V, String> {
    crate::canonical_no_capacity::validate_capacity_review(review).map_err(|e| {
        if e.starts_with("CANONICAL_CAPACITY_") { format!("CANONICAL_REFUND_{}", &e["CANONICAL_CAPACITY_".len()..]) } else { e }
    })?;
    need(review["refundPolicy"]["schemaVersion"].as_i64() == Some(1)
        && review["refundPolicy"]["representation"].as_str() == Some("held_purchase_principal_not_face_value")
        && review["refundPolicy"]["feeRefundAssumed"] == V::Bool(false)
        && review["refundPolicy"]["sourceHash"].as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())), "POLICY")?;
    need(review["refundScenarios"].as_array().is_some_and(|s| s.len() >= 3
            && s.iter().all(|x| x["id"].as_str().is_some_and(|i| !i.is_empty())
                && x["outcomes"].as_array().is_some_and(|o| o.len() == 2
                    && o.iter().any(|v| v.as_str() == Some("refund"))
                    && o.iter().all(|v| v.as_i64() == Some(0) || v.as_i64() == Some(1) || v.as_str() == Some("refund"))))
            && s.iter().map(|x| x["id"].as_str()).collect::<BTreeSet<_>>().len() == s.len()), "SCENARIOS")?;
    for pair in [["refund","refund"],["refund","0"],["0","refund"]] {
        need(review["refundScenarios"].as_array().unwrap().iter().any(|s| {
            s["outcomes"].as_array().unwrap().iter().enumerate().all(|(i, v)| {
                if pair[i] == "refund" { v.as_str() == Some("refund") } else { v.as_i64().map(|n| n.to_string()) == Some(pair[i].into()) || v.as_i64() == Some(0) }
            })
        }), "ASYMMETRIC_STRESS")?;
    }
    Ok(review.clone())
}
/// evaluateCanonicalNoCapacity(options)
pub fn evaluate_canonical_no_capacity(review: &V, books: &[V], account: Option<&V>, now: &str, policy: &V) -> Result<V, String> {
    validate_capacity_review(review)?;
    let result = crate::canonical_no_capacity::evaluate_canonical_no_capacity(review, books, account, now, policy)?;
    let quantity: BigInt = result["matchedQuantity"].as_str().unwrap_or("0").parse().unwrap_or(BigInt::from(0));
    let mut principal = [BigInt::from(0), BigInt::from(0)];
    let mut fees = [BigInt::from(0), BigInt::from(0)];
    for segment in result["segments"].as_array().cloned().unwrap_or_default() {
        let n: BigInt = segment["quantity"].as_str().unwrap_or("0").parse().unwrap_or(BigInt::from(0));
        for i in 0..2 {
            principal[i] += (big_amount(segment["noPrices"][i].as_str().unwrap_or(""))?
                + BigInt::from(policy["adverseTicksPerLeg"].as_i64().unwrap_or(0)) * BigInt::from(TICK)) * &n;
            fees[i] += big_amount(policy["feePerShare"].as_str().unwrap_or(""))? * &n;
        }
    }
    if quantity > BigInt::from(0) {
        for i in 0..2 { fees[i] += big_amount(policy["fixedFeePerLeg"].as_str().unwrap_or(""))?; }
    }
    need(&principal[0] + &principal[1] + &fees[0] + &fees[1]
        == big_amount(result["acquisitionCost"].as_str().unwrap_or(""))?, "COST_RECONCILIATION")?;
    let scenarios: Vec<V> = review["refundScenarios"].as_array().unwrap().iter().map(|s| {
        let proceeds: BigInt = s["outcomes"].as_array().unwrap().iter().enumerate().map(|(i, v)| {
            if v.as_str() == Some("refund") { principal[i].clone() }
            else { BigInt::from(v.as_i64().unwrap_or(0)) * BigInt::from(SCALE) * &quantity }
        }).sum();
        let mut s = s.clone();
        s["proceeds"] = json!(money(&proceeds));
        s["profit"] = json!(money(&(proceeds - big_amount(result["acquisitionCost"].as_str().unwrap()).unwrap_or(BigInt::from(0)))));
        s
    }).collect();
    let mut out = result;
    out["mode"] = json!("canonical_no_conditional_capacity_refund_v2");
    out["purchasePrincipalByLeg"] = json!(principal.iter().map(money).collect::<Vec<_>>());
    out["nonrefundableModeledFeesByLeg"] = json!(fees.iter().map(money).collect::<Vec<_>>());
    out["refundRepresentation"] = review["refundPolicy"]["representation"].clone();
    out["cancellationScenarios"] = json!(scenarios);
    let mut limitations: Vec<V> = out["limitations"].as_array().cloned().unwrap_or_default();
    limitations.extend([json!("Cancellation refunds modeled held purchase principal, never face value; separate fees are not assumed refundable. Actual platform refundable-cost allocation remains unverified."),
        json!("Partial cancellation can lose the other leg principal. Both legs refunded produce no principal gain. No paired cancellation or scenario completeness is guaranteed.")]);
    out["limitations"] = json!(limitations);
    Ok(out)
}
