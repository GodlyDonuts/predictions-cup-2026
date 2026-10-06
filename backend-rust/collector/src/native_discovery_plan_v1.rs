// live-paper/native-discovery-plan-v1.mjs
use crate::book_scheduler::plan_book_reads;
use serde_json::{json, Value as V};
use std::collections::{HashMap, HashSet};
fn need(ok: bool, c: &str) -> Result<(), String> { if !ok { return Err(format!("NATIVE_DISCOVERY_{}", c)); } Ok(()) }
fn is_id(s: &str) -> bool { !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0' }
pub fn record_native_discovery_attempt(state: &V, market_id: &str) -> Result<V, String> {
    need(is_id(market_id), "MARKET")?;
    let n = state.get("nativeDiscoveryAttempts").and_then(V::as_i64).unwrap_or(0);
    need(n >= 0, "LEDGER")?;
    let mut out = state.clone();
    out["nativeDiscoveryAttempts"] = json!(n + 1);
    Ok(out)
}
/// planNativeDiscoveryBooks(state, scope, inputs, {screenedMarketIds, excludedMarketIds, ownedRiskMarketIds})
pub fn plan_native_discovery_books(state: &V, scope: &V, inputs: &V,
    screened_market_ids: &[String], excluded_market_ids: &[String], owned_risk_market_ids: &[String]) -> Result<V, String> {
    let members: HashMap<&str, &V> = scope["universe"]["exchanges"].as_array().map(|a| {
        a.iter().filter_map(|x| Some((x["marketId"].as_str()?, x))).collect()
    }).unwrap_or_default();
    let blocked: HashSet<&str> = excluded_market_ids.iter().map(String::as_str).collect();
    need(screened_market_ids.iter().all(|x| members.contains_key(x.as_str()))
        && excluded_market_ids.iter().all(|x| members.contains_key(x.as_str())), "SCOPE")?;
    let ordinal = state.get("nativeDiscoveryAttempts").and_then(V::as_i64).unwrap_or(0);
    need(ordinal >= 0, "LEDGER")?;
    let held: HashSet<&str> = inputs.get("heldMarketIds").and_then(V::as_array).into_iter().flatten()
        .chain(inputs.get("priorityManagedMarketIds").and_then(V::as_array).into_iter().flatten())
        .filter_map(V::as_str).collect();
    need(owned_risk_market_ids.iter().all(|x| members.contains_key(x.as_str()) && held.contains(x.as_str())), "OWNED_RISK")?;
    let reduce_only: HashSet<&str> = owned_risk_market_ids.iter().map(String::as_str).collect();
    let mut read_inputs = inputs.clone();
    read_inputs["excludedMarketIds"] = json!(inputs.get("excludedMarketIds").and_then(V::as_array).cloned().unwrap_or_default()
        .into_iter().filter(|x| x.as_str().is_some_and(|s| !reduce_only.contains(s))).collect::<Vec<_>>());
    let plan = plan_book_reads(state, scope, &read_inputs, 4, 3, 0)?;
    let mut reads: Vec<V> = plan["reads"].as_array().cloned().unwrap_or_default();
    let optional = reads.iter().position(|x| x["reason"].as_str() == Some("discovery"));
    let Some(optional) = optional else {
        let mut out = plan.clone();
        out["nativeDiscovery"] = json!({"mode": "native_market_discovery_v1",
            "reason": "protected_budget_full", "requestsAdded": 0, "executionAuthority": false});
        return Ok(out);
    };
    let reserved_fair = (ordinal + 1) % 4 == 0;
    let others: HashSet<&str> = reads.iter().enumerate().filter(|(i, _)| *i != optional)
        .filter_map(|(_, r)| r["marketId"].as_str()).collect();
    let mut seen = HashSet::new();
    let mut eligible: Vec<String> = screened_market_ids.iter()
        .filter(|x| seen.insert((*x).clone()) && !blocked.contains(x.as_str()) && !others.contains(x.as_str())).cloned().collect();
    let last = state.get("bookLastAttempt").and_then(V::as_object).cloned().unwrap_or_default();
    eligible.sort_by_key(|a| (last.get(a).and_then(V::as_i64).unwrap_or(0), screened_market_ids.iter().position(|s| s == a).unwrap_or(usize::MAX)));
    if !reserved_fair && !eligible.is_empty() {
        let mid = &eligible[0];
        let spec = members[mid.as_str()];
        let core: Vec<String> = scope.get("coreMarketIds").and_then(V::as_array).into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
        reads[optional] = json!({"marketId": spec["marketId"], "exchangeId": spec["exchangeId"], "option": spec["option"],
            "path": if core.contains(mid) { "sourceSnapshots" } else { "supplementalDepth" }, "reason": "discovery"});
    }
    reads[optional]["nativeDiscovery"] = json!(true);
    let mut out = plan.clone();
    let o = out.as_object_mut().unwrap();
    o.insert("reads".into(), json!(reads.clone()));
    o.insert("nativeDiscovery".into(), json!({"mode": "native_market_discovery_v1",
        "reason": if !reserved_fair && !eligible.is_empty() { "screened_depth_candidate" } else { "persisted_fair_discovery" },
        "ordinal": ordinal + 1, "marketId": reads[optional]["marketId"], "screenedCandidates": eligible.len(),
        "reservedFair": reserved_fair, "requestsAdded": 0, "executionAuthority": false}));
    Ok(out)
}
