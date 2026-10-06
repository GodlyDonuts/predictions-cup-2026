// live-paper/paired-package-read-plan.mjs — bounded observation allocation (no
// trade authority).
use serde_json::{json, Value as V};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const PACKAGE_OBSERVATION_INTERVAL_MS: i64 = 60_000;

fn name_ok(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && s.len() <= 64
        && !["con","prn","aux","nul","com1","com2","com3","com4","com5","com6","com7","com8","com9","lpt1","lpt2","lpt3","lpt4","lpt5","lpt6","lpt7","lpt8","lpt9"].contains(&s)
}
fn hash_js(v: &V) -> String {
    baal_service_runtime::json_text::digest(v)
}
/// readPairedPackageFocus(runtimeDir, universe, now=Date.now())
pub fn read_paired_package_focus(runtime_dir: &Path, universe: &V, now: f64) -> V {
    let file: PathBuf = runtime_dir.join("paired-package-active.json");
    if !file.exists() { return V::Null; }
    let inner = || -> Result<V, String> {
        let manifest: V = serde_json::from_str(&fs::read_to_string(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let dir = runtime_dir.join("paired-package-cohorts").join(manifest["cohort"].as_str().unwrap_or(""));
        if manifest["schemaVersion"].as_i64() != Some(1)
            || !manifest["cohort"].as_str().is_some_and(name_ok)
            || !manifest["bankHash"].as_str().unwrap_or("").chars().all(|c| c.is_ascii_hexdigit())
            || manifest["bankHash"].as_str().unwrap_or("").len() != 64
            || dir.join("STOP").exists() {
            return Err("bad manifest".into());
        }
        let bank: V = serde_json::from_str(&fs::read_to_string(dir.join("bank.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let stats: V = serde_json::from_str(&fs::read_to_string(dir.join("stats.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let reported = stats["reportedAt"].as_str().and_then(baal_service_runtime::contract::parse_iso_ms);
        if hash_js(&bank) != manifest["bankHash"].as_str().unwrap_or("")
            || bank["tournamentId"] != universe["tournamentId"]
            || bank["cohort"] != manifest["cohort"]
            || stats["bankHash"] != manifest["bankHash"]
            || stats["cohort"] != manifest["cohort"]
            || stats["mode"].as_str() != Some("prospective_sequential_dual_no_paper")
            || stats["status"].as_str() != Some("running")
            || stats["networkRequests"].as_i64() != Some(0)
            || stats["competitionOrders"].as_i64() != Some(0)
            || !stats["pid"].as_i64().is_some_and(|p| p > 0)
            || reported.is_none()
            || now - reported.unwrap() < 0.0
            || now - reported.unwrap() > 10000.0 {
            return Err("bad stats".into());
        }
        let pid = stats["pid"].as_i64().unwrap() as u32;
        if !Path::new(&format!("/proc/{}", pid)).exists() { return Err("dead pid".into()); }
        let markets = bank["markets"].as_array().cloned().unwrap_or_default();
        if markets.len() != 2 || !["381", "382"].iter().all(|id| {
            markets.iter().any(|m| m["marketId"].as_str() == Some(id)
                && universe["exchanges"].as_array().is_some_and(|xs| xs.iter().any(|x| x["marketId"].as_str() == Some(id) && x["exchangeId"] == m["exchangeId"])))
        }) {
            return Err("bad markets".into());
        }
        let wallet = stats.get("wallets").and_then(|w| w.get("sequential_dual_no")).cloned();
        let Some(wallet) = wallet.filter(|w| !w.is_null()) else { return Err("bad wallet".into()); };
        let needed = !wallet["pending"].is_null() || wallet["activePackage"]["stage"].as_str() == Some("orphan_exit_blocked");
        let ids: Vec<String> = markets.iter().filter_map(|m| m["marketId"].as_str().map(String::from)).collect();
        if !needed {
            return Ok(json!({"available": true, "needed": false, "marketIds": ids, "cohort": manifest["cohort"], "bankHash": manifest["bankHash"], "reason": "scoped_paper_pair_periodic_observation"}));
        }
        if !wallet["pending"].is_null()
            && !markets.iter().any(|m| m["marketId"] == wallet["pending"]["marketId"] && m["exchangeId"] == wallet["pending"]["exchangeId"]) {
            return Err("bad pending".into());
        }
        Ok(json!({"available": true, "needed": true, "marketIds": ids, "cohort": manifest["cohort"], "bankHash": manifest["bankHash"], "reason": "scoped_counterfactual_pending_or_orphan_observation"}))
    };
    inner().unwrap_or_else(|_| json!({"available": false, "needed": false, "marketIds": [], "reason": "paper_package_focus_unavailable"}))
}
/// focusPairedPackageReads(plan, focus, {now, lastAttempts})
pub fn focus_paired_package_reads(plan: &V, focus: &V, now: Option<f64>, last_attempts: Option<&V>) -> Result<V, String> {
    if focus.get("available") != Some(&V::Bool(true)) { return Ok(plan.clone()); }
    if focus.get("needed") != Some(&V::Bool(true)) {
        let (Some(now), Some(la)) = (now, last_attempts) else { return Ok(plan.clone()); };
        if now < 0.0 || now.floor() != now || !la.is_object() { return Err("INVALID_PACKAGE_OBSERVATION_CLOCK".into()); }
        let mut clocks = Vec::new();
        for id in focus["marketIds"].as_array().cloned().unwrap_or_default() {
            match la.get(id.as_str().unwrap_or("")) {
                None => clocks.push(0.0),
                Some(v) => {
                    let at = v.as_str().and_then(baal_service_runtime::contract::parse_iso_ms);
                    match at { Some(a) if a <= now => clocks.push(a), _ => return Err("INVALID_PACKAGE_ATTEMPT_CLOCK".into()) }
                }
            }
        }
        if clocks.len() != 2 || now - clocks.iter().cloned().fold(f64::INFINITY, f64::min) < PACKAGE_OBSERVATION_INTERVAL_MS as f64 {
            return Ok(plan.clone());
        }
    }
    let wanted: Vec<String> = focus["marketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|v| v.as_str().map(String::from)).collect();
    if wanted.len() != 2 || BTreeSet::from_iter(wanted.iter()).len() != 2 || wanted.iter().any(|id| !["381", "382"].contains(&id.as_str())) {
        return Err("INVALID_PACKAGE_READ_SCOPE".into());
    }
    let mut reads: Vec<V> = plan["reads"].as_array().cloned().unwrap_or_default();
    let present: BTreeSet<String> = reads.iter().filter_map(|r| r["marketId"].as_str().map(String::from)).collect();
    let missing: Vec<&String> = wanted.iter().filter(|id| !present.contains(*id)).collect();
    let basket_ids: BTreeSet<String> = plan["basketMeasurement"]["marketIds"].as_array().cloned().unwrap_or_default().iter().filter_map(|v| v.as_str().map(String::from)).collect();
    let slots: Vec<usize> = reads.iter().enumerate().filter(|(_, x)| {
        let reason = x["reason"].as_str().unwrap_or("");
        ["comparison", "discovery"].contains(&reason)
            && !wanted.contains(&x["marketId"].as_str().unwrap_or("").to_string())
            && !basket_ids.contains(x["marketId"].as_str().unwrap_or(""))
    }).map(|(i, _)| i).collect();
    if slots.len() < missing.len() {
        let mut out = plan.clone();
        out["paperPackageFocus"] = json!({"reason": "managed_risk_retains_capacity", "marketIds": []});
        return Ok(out);
    }
    for (i, m) in missing.iter().enumerate() {
        reads[slots[i]] = json!({"marketId": m, "reason": "paper_package", "path": "supplementalDepth"});
    }
    let admitted: BTreeSet<String> = reads.iter().filter_map(|r| r["marketId"].as_str().map(String::from)).collect();
    let mut out = plan.clone();
    out["reads"] = json!(reads);
    out["entryMarketIds"] = json!(plan["entryMarketIds"].as_array().cloned().unwrap_or_default().iter().filter(|id| admitted.contains(id.as_str().unwrap_or(""))).cloned().collect::<Vec<_>>());
    out["paperPackageFocus"] = json!({"reason": focus["reason"], "marketIds": wanted, "requestsAdded": 0, "replacedSlots": missing.len(), "executionAuthority": false});
    Ok(out)
}
