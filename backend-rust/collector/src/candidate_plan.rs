// live-paper/candidate-plan.mjs
use crate::execution_metrics::PAPER_EDGE_MARGINS;
use serde_json::Value as V;
pub const PORTFOLIO_PLAN: &str = "sequential-portfolio-selection-v1";
pub fn validate_candidate_plan(bank: &V) -> Result<(), String> {
    let Some(p) = bank.get("candidatePlan") else { return Ok(()); };
    if p.get("version").and_then(V::as_str) != Some(PORTFOLIO_PLAN)
        || !p.get("candidates").is_some_and(|c| c.as_array().is_some_and(|a| a.len() <= 1))
        || !bank.get("selected").is_some_and(|s| s.as_array().map(|a| a.len()) == p.get("candidates").and_then(V::as_array).map(|c| c.len()))
        || bank.get("targetPolicy").and_then(V::as_str) != Some("delayed_fifo_roundtrip_net_v1")
        || bank.get("forecastLifecycle").and_then(V::as_str) != Some("revoke-on-invalidation-v2") {
        return Err("Invalid frozen candidate plan".into());
    }
    let candidates = p["candidates"].as_array().unwrap();
    if !candidates.is_empty() {
        let c = &candidates[0];
        let margin_ok = c.get("margin").and_then(V::as_f64).is_some_and(|m| PAPER_EDGE_MARGINS.contains(&m));
        if !c.get("modelId").is_some_and(V::is_string)
            || c.get("modelId") != bank["selected"][0].get("id")
            || !margin_ok {
            return Err("Invalid frozen candidate margin".into());
        }
    }
    Ok(())
}
pub fn model_margins(bank: &V, model_id: &str) -> Vec<f64> {
    match bank.get("candidatePlan") {
        None => PAPER_EDGE_MARGINS.to_vec(),
        Some(p) => p["candidates"].as_array().map(|a| a.iter()
            .filter(|c| c["modelId"].as_str() == Some(model_id))
            .filter_map(|c| c["margin"].as_f64()).collect()).unwrap_or_default(),
    }
}
