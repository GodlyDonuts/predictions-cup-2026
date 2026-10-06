// live-paper/profit-sprint-read-plan.mjs
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
use std::path::Path;
const KEYS: [&str; 5] = ["schemaVersion", "owner", "mode", "createdAt", "expiresAt"];
fn is_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0' && s.parse::<i64>().is_ok()
}
/// readProfitSprint(runtimeDir, now)
pub fn read_profit_sprint(runtime_dir: &Path, now: f64) -> Result<Option<V>, String> {
    let file = runtime_dir.join("COLLECTION_PROFIT_SPRINT");
    if !file.exists() { return Ok(None); }
    let v: V = serde_json::from_str(&std::fs::read_to_string(&file).map_err(|_| "INVALID_PROFIT_SPRINT_SETTING")?)
        .map_err(|_| "INVALID_PROFIT_SPRINT_SETTING")?;
    let created = v.get("createdAt").and_then(V::as_str).and_then(parse_iso_ms);
    let expires = v.get("expiresAt").and_then(V::as_str).and_then(parse_iso_ms);
    let keys_ok = v.as_object().is_some_and(|o| o.keys().all(|k| KEYS.contains(&k.as_str())));
    let ok = v.get("schemaVersion").and_then(V::as_i64) == Some(1)
        && v.get("owner").and_then(V::as_str) == Some("Codex")
        && v.get("mode").and_then(V::as_str) == Some("source_bound_and_owned_risk")
        && keys_ok && now.is_finite()
        && created.is_some() && expires.is_some()
        && created.unwrap() <= now && expires.unwrap() > created.unwrap()
        && expires.unwrap() - created.unwrap() <= 3_600_000.0;
    if !ok { return Err("INVALID_PROFIT_SPRINT_SETTING".into()); }
    Ok(if now < expires.unwrap() { Some(v) } else { None })
}
/// passiveHostsCompletelyFlat(statuses)
pub fn passive_hosts_completely_flat(statuses: &V) -> bool {
    statuses.as_array().is_some_and(|a| a.iter().all(|s| {
        s.get("failed") == Some(&V::Bool(false))
            && s.get("riskMarketIds").is_some_and(|r| r.as_array().is_some_and(|a| a.is_empty()))
            && s.get("wallets").and_then(V::as_object).is_some_and(|w| {
                !w.is_empty() && w.values().all(|w| {
                    w.get("positions").is_some_and(|p| p.as_array().is_some_and(|a| a.is_empty()))
                        && w.get("unresolvedQuotes").is_some_and(|q| q.as_array().is_some_and(|a| a.is_empty()))
                        && w.get("reserved").and_then(V::as_f64) == Some(0.0)
                })
            })
    }))
}
/// scopedRiskMarketIds(officialIds, paperFocus)
pub fn scoped_risk_market_ids(official_ids: &[String], paper_focus: Option<&V>) -> Result<Vec<String>, String> {
    let mut paper_ids: Vec<String> = Vec::new();
    if paper_focus.is_some_and(|p| p.get("ownedRiskReads").and_then(V::as_bool) == Some(true)) {
        for x in paper_focus.unwrap().get("markets").and_then(V::as_array).cloned().unwrap_or_default() {
            let phase = x.get("phase").and_then(V::as_str).unwrap_or("");
            if ["held", "exit_pending", "entry_pending", "pending_sell", "pending_buy"].contains(&phase) {
                if let Some(mid) = x.get("marketId").and_then(V::as_str) { paper_ids.push(mid.to_string()); }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<String> = official_ids.iter().cloned().chain(paper_ids).filter(|i| seen.insert(i.clone())).collect();
    if ids.iter().any(|id| !is_id(id)) { return Err("INVALID_PROFIT_SPRINT_RISK".into()); }
    Ok(ids)
}
/// focusProfitSprintReads(plan, {enabled, sourceMarketIds, riskMarketIds})
pub fn focus_profit_sprint_reads(plan: &V, enabled: bool, source_market_ids: &[String], risk_market_ids: &[String]) -> Result<V, String> {
    if !enabled { return Ok(plan.clone()); }
    let mut keep: std::collections::HashSet<String> = source_market_ids.iter().cloned().chain(risk_market_ids.iter().cloned()).collect();
    keep.extend(plan.get("managedMarketIds").and_then(V::as_array).into_iter().flatten().filter_map(|x| x.as_str().map(String::from)));
    if keep.iter().any(|id| !is_id(id)) { return Err("INVALID_PROFIT_SPRINT_SCOPE".into()); }
    let reads: Vec<V> = plan["reads"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|r| keep.contains(r["marketId"].as_str().unwrap_or(""))).collect();
    let removed: Vec<V> = plan["reads"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|r| !keep.contains(r["marketId"].as_str().unwrap_or("")))
        .map(|r| r["marketId"].clone()).collect();
    let mut out = plan.clone();
    let o = out.as_object_mut().unwrap();
    o.insert("reads".into(), json!(reads));
    o.insert("entryMarketIds".into(), json!(plan.get("entryMarketIds").and_then(V::as_array).cloned().unwrap_or_default()
        .into_iter().filter(|id| keep.contains(id.as_str().unwrap_or(""))).collect::<Vec<_>>()));
    o.insert("profitSprint".into(), json!({"mode": "source_bound_and_owned_risk",
        "removedBookMarketIds": removed, "executionAuthority": false}));
    Ok(out)
}
