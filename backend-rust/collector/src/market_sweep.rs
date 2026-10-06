// live-paper/market-sweep.mjs — sync port; caller supplies the request closure.
use crate::breadth::{normalize_prices, price_batch, price_endpoint, update_coverage};
use baal_service_runtime::budget::validate_network_budget;
use serde_json::{json, Value as V};
use std::path::Path;
const FIELDS: [&str; 5] = ["schemaVersion", "ownerId", "enabled", "universeHash", "cycleIntervalMs"];
pub fn validate_market_sweep(value: &V, universe: &V, network_budget: &V) -> Result<V, String> {
    let err = "INVALID_MARKET_SWEEP_SETTING";
    let keys_ok = value.as_object().is_some_and(|o| o.len() == FIELDS.len() && o.keys().all(|k| FIELDS.contains(&k.as_str())));
    let budget_schema = validate_network_budget(network_budget).ok()
        .and_then(|b| b.get("schemaVersion").and_then(V::as_i64)).unwrap_or(-1);
    if !keys_ok || value["schemaVersion"].as_i64() != Some(1) || value["ownerId"].as_str() != Some("Codex")
        || value["enabled"].as_bool() != Some(true) || value["universeHash"] != universe["hash"]
        || value["cycleIntervalMs"].as_i64() != Some(10000)
        || !universe["exchanges"].as_array().is_some_and(|e| (1..=300).contains(&e.len()))
        || ![2, 3, 4].contains(&budget_schema) {
        return Err(err.into());
    }
    Ok(value.clone())
}
pub fn load_market_sweep(file: &Path, universe: &V, network_budget: &V) -> Result<Option<V>, String> {
    if !file.exists() { return Ok(None); }
    let v: V = serde_json::from_str(&std::fs::read_to_string(file).map_err(|_| "INVALID_MARKET_SWEEP_SETTING")?)
        .map_err(|_| "INVALID_MARKET_SWEEP_SETTING")?;
    validate_market_sweep(&v, universe, network_budget).map(Some)
}
pub struct SweepHooks<'a> {
    pub request: &'a dyn Fn(&str) -> Result<V, (Option<i64>, bool)>, // (http status, manual_review)
    pub is_stopped: &'a dyn Fn() -> bool,
    pub cooldown_remaining_ms: &'a dyn Fn() -> f64,
    pub now: &'a dyn Fn() -> f64,
}
/// collectMarketSweep — durable-recording failures abort instead of hiding.
pub fn collect_market_sweep(universe: &V, coverage: &V, enabled: bool, managed_market_ids: &[String],
    h: &SweepHooks, on_snapshot: &mut dyn FnMut(&V) -> Result<(), String>,
    on_coverage: &mut dyn FnMut(&V) -> Result<(), String>) -> Result<V, String> {
    let count = ((universe["exchanges"].as_array().map(|a| a.len()).unwrap_or(0)) + 99) / 100;
    let start = coverage.get("nextBatch").and_then(V::as_i64)
        .filter(|n| *n >= 0 && (*n as usize) < count).map(|n| n as usize).unwrap_or(0);
    let requested = if enabled && managed_market_ids.is_empty() { count } else { 1 };
    let mut next = coverage.clone();
    let mut primary: Option<V> = None;
    let mut snapshots: Vec<V> = Vec::new();
    let mut error: V = V::Null;
    for i in 0..requested {
        if (h.is_stopped)() || (h.cooldown_remaining_ms)() > (h.now)() { break; }
        let index = (start + i) % count.max(1);
        let batch = price_batch(universe, index);
        let endpoint = price_endpoint(&batch, universe["tournamentId"].as_str().unwrap_or(""));
        let snapshot = match (h.request)(&endpoint) {
            Ok(resp) => match normalize_prices(&resp, &batch, universe) {
                Ok(s) => s,
                Err(e) => return Err(e),
            },
            Err((status, manual)) => {
                if manual || status == Some(401) || status == Some(403) { return Err("market_sweep_unauthorized".into()); }
                error = json!(if (h.is_stopped)() { "stopped".to_string() } else {
                    status.map(|s| format!("http_{}", s)).unwrap_or_else(|| "bulk_price_unavailable".to_string()) });
                break;
            }
        };
        if (h.is_stopped)() { break; }
        on_snapshot(&snapshot).map_err(|_| "Market sweep durable recording failed".to_string())?;
        next = update_coverage(&next, &snapshot, universe, index);
        snapshots.push(snapshot.clone());
        if primary.is_none() { primary = Some(snapshot); }
        next["nextBatch"] = json!((start + 1) % count.max(1));
        on_coverage(&next).map_err(|_| "Market sweep durable recording failed".to_string())?;
    }
    Ok(json!({"coverage": next, "primarySnapshot": primary.unwrap_or(V::Null), "snapshots": snapshots,
        "error": error, "requestedBatches": requested, "completedBatches": snapshots.len(),
        "complete": snapshots.len() == requested, "riskPriority": enabled && !managed_market_ids.is_empty()}))
}
