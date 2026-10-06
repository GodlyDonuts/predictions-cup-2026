// live-paper/network-budget.mjs — v1 coordinated network budget.
use serde_json::Value as V;
use std::path::Path;

/// validateNetworkBudget(value) — schema v1.
pub fn validate_network_budget(value: &V) -> Result<V, String> {
    let keys = ["schemaVersion","ownerId","enabled","intervalMs","soleAccountClient","formerWriterStopCommentId"];
    let m = value.as_object().ok_or("Invalid coordinated network budget")?;
    if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k))
        || value["schemaVersion"].as_i64() != Some(1) || value["ownerId"].as_str() != Some("Codex")
        || value["enabled"] != V::Bool(true) || value["soleAccountClient"] != V::Bool(true)
        || value["formerWriterStopCommentId"].as_i64() != Some(5947535773)
        || value["intervalMs"].as_i64() != Some(3000) {
        return Err("Invalid coordinated network budget".into());
    }
    Ok(value.clone())
}
pub fn load_network_budget(file: &Path) -> Result<Option<V>, String> {
    if !file.exists() { return Ok(None); }
    validate_network_budget(&serde_json::from_str(&std::fs::read_to_string(file).map_err(|e| e.to_string())?)
        .map_err(|_| "Invalid coordinated network budget".to_string())?).map(Some)
}
pub fn network_interval(interval: i64, budget: Option<&V>) -> Result<i64, String> {
    if interval < 5000 { return Err("Read budget requires >=5 second pacing unless explicitly coordinated".into()); }
    match budget {
        None => Ok(interval),
        Some(b) => Ok(validate_network_budget(b)?["intervalMs"].as_i64().unwrap()),
    }
}
