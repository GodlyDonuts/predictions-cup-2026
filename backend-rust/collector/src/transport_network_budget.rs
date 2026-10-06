// live-paper/transport-network-budget.mjs — schema v1-4 coordinated budgets.
use serde_json::Value as V;
use std::path::Path;

/// validateNetworkBudget(value) — schema v1/v2/v3/v4.
pub fn validate_network_budget(value: &V) -> Result<V, String> {
    if value["schemaVersion"].as_i64() == Some(4) {
        let human = value["authorityKind"].as_str() == Some("direct_human_assignment");
        let mut keys = vec!["schemaVersion","ownerId","enabled","intervalMs","writeIntervalMs","soleAccountClient","executionOwner","handoffId","ackCommentId","writesEnabled"];
        keys.push(if human { "authorityKind" } else { "releaseCommentId" });
        if human { keys.push("assignmentCommentId"); }
        let mut keys = keys;
        if human { keys = vec!["schemaVersion","ownerId","enabled","intervalMs","writeIntervalMs","soleAccountClient","executionOwner","handoffId","ackCommentId","writesEnabled","authorityKind","assignmentCommentId"]; }
        let m = value.as_object().ok_or("Invalid production network budget")?;
        let handoff_ok = value["handoffId"].as_str().is_some_and(|s| {
            let prefix = if human { "human-to-baal-" } else { "daenerys-to-baal-" };
            s.starts_with(prefix) && s.len() - prefix.len() >= 1 && s.len() - prefix.len() <= 70
                && s[prefix.len()..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
        let ack = if human { value["assignmentCommentId"].clone() } else { value["releaseCommentId"].clone() };
        if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k))
            || value["ownerId"].as_str() != Some("Codex") || value["enabled"] != V::Bool(true)
            || value["intervalMs"].as_i64() != Some(1000) || value["writeIntervalMs"].as_i64() != Some(2500)
            || value["soleAccountClient"] != V::Bool(true) || value["executionOwner"].as_str() != Some("Codex")
            || value["writesEnabled"] != V::Bool(true) || !handoff_ok
            || ![ack, value["ackCommentId"].clone()].iter().all(|n| n.as_i64().is_some_and(|x| x > 0)) {
            return Err("Invalid production network budget".into());
        }
        return Ok(value.clone());
    }
    if value["schemaVersion"].as_i64() == Some(3) {
        let keys = ["schemaVersion","ownerId","enabled","intervalMs","soleAccountClient","executionOwner","ackCommentId","maxReadsPerMinute","writesEnabled"];
        let m = value.as_object().ok_or("Invalid shared read-only network budget")?;
        if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k))
            || value["ownerId"].as_str() != Some("Codex") || value["enabled"] != V::Bool(true)
            || value["intervalMs"].as_i64() != Some(3000) || value["soleAccountClient"] != V::Bool(false)
            || value["executionOwner"].as_str() != Some("Daenerys") || value["ackCommentId"].as_i64() != Some(5966761902)
            || value["maxReadsPerMinute"].as_i64() != Some(20) || value["writesEnabled"] != V::Bool(false) {
            return Err("Invalid shared read-only network budget".into());
        }
        return Ok(value.clone());
    }
    let base = ["schemaVersion","ownerId","enabled","intervalMs","soleAccountClient","formerWriterStopCommentId"];
    let v2 = value["schemaVersion"].as_i64() == Some(2);
    let keys: Vec<&str> = if v2 { [&base[..], &["writeIntervalMs"]].concat() } else { base.to_vec() };
    let m = value.as_object().ok_or("Invalid coordinated network budget")?;
    if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k))
        || ![1, 2].contains(&value["schemaVersion"].as_i64().unwrap_or(0))
        || value["ownerId"].as_str() != Some("Codex") || value["enabled"] != V::Bool(true)
        || value["soleAccountClient"] != V::Bool(true)
        || value["formerWriterStopCommentId"].as_i64() != Some(5947535773)
        || (!v2 && value["intervalMs"].as_i64() != Some(3000))
        || (v2 && (value["intervalMs"].as_i64() != Some(1000) || value["writeIntervalMs"].as_i64() != Some(2500))) {
        return Err("Invalid coordinated network budget".into());
    }
    Ok(value.clone())
}
pub fn network_interval(interval: i64, budget: Option<&V>) -> Result<i64, String> {
    if interval < 5000 { return Err("Read budget requires >=5 second pacing unless explicitly coordinated".into()); }
    match budget {
        None => Ok(interval),
        Some(b) => Ok(validate_network_budget(b)?["intervalMs"].as_i64().unwrap()),
    }
}
pub fn network_write_interval(interval: i64, budget: Option<&V>) -> Result<i64, String> {
    let read = network_interval(interval, budget)?;
    match budget {
        Some(b) if [2, 4].contains(&b["schemaVersion"].as_i64().unwrap_or(0)) => {
            Ok(validate_network_budget(b)?["writeIntervalMs"].as_i64().unwrap())
        }
        _ => Ok(read),
    }
}
pub fn load_network_budget(file: &Path) -> Result<Option<V>, String> {
    if !file.exists() { return Ok(None); }
    validate_network_budget(&serde_json::from_str(&std::fs::read_to_string(file).map_err(|e| e.to_string())?)
        .map_err(|_| "Invalid coordinated network budget".to_string())?).map(Some)
}
