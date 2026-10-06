// live-paper/maker-read-plan.mjs — maker pair read substitution.
// External collection policy: keep the frozen evaluator and maker banks intact.
use baal_service_runtime::json_text::{js_stringify, js_truthy};
use serde_json::{json, Value as V};
use std::collections::HashSet;

const OPTIONAL: [&str; 4] = ["discovery", "comparison", "edge_observation", "conditional_capacity_pair"];

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("MAKER_READ_PLAN_{}", code)); } Ok(()) }
fn instant(v: &V) -> Result<i64, String> { baal_service_runtime::fixed::instant(v.as_str().unwrap_or("")) }
fn safe_int(x: f64) -> bool { x.is_finite() && x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_991.0 }
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(_), V::Number(_)) => a.as_f64() == b.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false, // objects/arrays: JS === is identity; separate parses never match
    }
}
fn dedup_key(v: &V) -> String { js_stringify(v) }

/// {...plan, reads, entryMarketIds: filtered, makerPairFocus}
fn result(plan: &V, reads: &[V], ids: &[V], reason: &str, paired: bool) -> V {
    let mut out = plan.clone();
    out["entryMarketIds"] = json!(plan["entryMarketIds"].as_array().cloned().unwrap_or_default().iter()
        .filter(|id| reads.iter().any(|r| strict_eq(&r["marketId"], id)) && (!paired || !ids.iter().any(|i| strict_eq(i, id))))
        .cloned().collect::<Vec<_>>());
    out["reads"] = json!(reads);
    out["makerPairFocus"] = json!({"marketIds": if paired { json!(ids) } else { json!([]) },
        "reason": reason, "requestsAdded": 0, "executionAuthority": false});
    out
}

/// planMakerPairReads(plan, review, {now, excludedExchangeIds=[]})
pub fn plan_maker_pair_reads(plan: &V, review: &V, now: f64, excluded_exchange_ids: &[V]) -> Result<V, String> {
    crate::canonical_no_capacity::validate_capacity_review(review)?;
    let base_reads = plan["reads"].as_array().cloned().unwrap_or_default();
    need(safe_int(now) && plan["reads"].is_array() && base_reads.len() <= 4, "INPUT")?;
    let legs = review["legs"].as_array().cloned().unwrap_or_default();
    let ids: Vec<V> = legs.iter().map(|l| l["marketId"].clone()).collect();
    need(ids.len() == 2
        && base_reads.iter().map(|r| dedup_key(&r["marketId"])).collect::<HashSet<_>>().len() == base_reads.len(),
        "SCOPE")?;
    if now < instant(&review["recordedAt"])? as f64 || now >= instant(&review["expiresAt"])? as f64 {
        return Ok(result(plan, &base_reads, &ids, "review_not_current", false));
    }
    if legs.iter().any(|l| excluded_exchange_ids.iter().any(|x| strict_eq(x, &l["exchangeId"]))) {
        return Ok(result(plan, &base_reads, &ids, "own_account_exposure", false));
    }
    if js_truthy(plan.get("comparisonSampling"))
        || plan["basketMeasurement"]["marketIds"].as_array().is_some_and(|a| !a.is_empty())
        || plan["paperPackageFocus"]["marketIds"].as_array().is_some_and(|a| !a.is_empty())
    {
        return Ok(result(plan, &base_reads, &ids, "other_pair_obligation", false));
    }
    let protected_reads: Vec<V> = base_reads.iter()
        .filter(|r| !OPTIONAL.contains(&r["reason"].as_str().unwrap_or(""))).cloned().collect();
    let protected_ids: Vec<V> = protected_reads.iter().map(|r| r["marketId"].clone()).collect();
    if protected_ids.iter().chain(ids.iter()).map(dedup_key).collect::<HashSet<_>>().len() > base_reads.len() {
        return Ok(result(plan, &base_reads, &ids, "protected_budget_full", false));
    }
    let protected_leg_idx: Vec<usize> = protected_reads.iter().enumerate()
        .filter(|(_, r)| ids.iter().any(|i| strict_eq(i, &r["marketId"]))).map(|(i, _)| i).collect();
    if protected_leg_idx.len() == 2 && (protected_leg_idx[0] as i64 - protected_leg_idx[1] as i64).abs() != 1 {
        return Ok(result(plan, &base_reads, &ids, "separated_protected_legs", false));
    }
    let spec = |id: &V| -> V {
        let leg = legs.iter().find(|l| strict_eq(&l["marketId"], id)).cloned().unwrap_or(V::Null);
        json!({"marketId": id, "exchangeId": leg["exchangeId"].clone(), "option": "YES",
            "path": "supplementalDepth", "reason": "maker_pair_partner"})
    };
    let mut reads: Vec<V> = Vec::new();
    for r in &protected_reads {
        reads.push(r.clone());
        if protected_leg_idx.len() == 1 && strict_eq(&r["marketId"], &protected_reads[protected_leg_idx[0]]["marketId"]) {
            reads.push(spec(ids.iter().find(|id| !strict_eq(*id, &r["marketId"])).unwrap()));
        }
    }
    if protected_leg_idx.is_empty() {
        for id in &ids {
            reads.push(spec(id));
        }
    }
    for r in &base_reads {
        if reads.len() < base_reads.len() && !reads.iter().any(|x| strict_eq(&x["marketId"], &r["marketId"])) {
            reads.push(r.clone());
        }
    }
    need(reads.len() == base_reads.len()
        && reads.iter().map(|r| dedup_key(&r["marketId"])).collect::<HashSet<_>>().len() == reads.len(), "BUDGET")?;
    let filtered: Vec<&V> = reads.iter().filter(|x| protected_ids.iter().any(|p| strict_eq(p, &x["marketId"]))).collect();
    need(protected_reads.iter().enumerate().all(|(i, r)| filtered.get(i).is_some_and(|x| *x == r)), "PROTECTED_ORDER")?;
    let pos = |id: &V| reads.iter().position(|r| strict_eq(&r["marketId"], id)).map(|i| i as i64).unwrap_or(-1);
    need((pos(&ids[0]) - pos(&ids[1])).abs() == 1, "PAIR_ADJACENCY")?;
    Ok(result(plan, &reads, &ids, "coherent_maker_pair", true))
}
