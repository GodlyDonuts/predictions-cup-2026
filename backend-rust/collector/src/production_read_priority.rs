// live-paper/production-read-priority.mjs
use serde_json::Value as V;
use std::collections::HashSet;
/// prioritizeProductionReads(plan, {candidateMarketIds, ownedMarketIds})
pub fn prioritize_production_reads(plan: &V, candidate_market_ids: &[String], owned_market_ids: &[String]) -> V {
    let owned: HashSet<&str> = owned_market_ids.iter().map(String::as_str).collect();
    let candidate: HashSet<&str> = candidate_market_ids.iter().map(String::as_str).collect();
    let rank = |id: &str| if owned.contains(id) { 0 } else if candidate.contains(id) { 1 } else { 2 };
    let mut indexed: Vec<(usize, V)> = plan["reads"].as_array().cloned().unwrap_or_default().into_iter().enumerate().collect();
    indexed.sort_by_key(|(i, r)| (rank(r["marketId"].as_str().unwrap_or("")), *i));
    let mut out = plan.clone();
    out["reads"] = V::Array(indexed.into_iter().map(|(_, r)| r).collect());
    out
}
