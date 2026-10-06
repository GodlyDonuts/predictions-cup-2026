// live-paper/bid-anchor-read-focus.mjs — read focus projection for the
// bid-anchor live companion host.
use serde_json::{json, Value as V};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn need(ok: bool) -> Result<(), String> {
    if !ok { return Err("INVALID_ANCHOR_READ_FOCUS".into()); }
    Ok(())
}
fn hash(x: &V) -> Result<String, String> {
    Ok(baal_service_runtime::json_text::sha256(
        baal_service_runtime::json_text::canonical_json(x)?.as_bytes()))
}
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn cohort_ok(x: &V) -> bool {
    x.as_str().is_some_and(|s| s.len() >= 2 && s.len() <= 64
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
/// anchorFocusCodeHash() — sha256 of the observer wrapper + this module.
pub fn anchor_focus_code_hash(workspace_dir: &Path) -> Result<String, String> {
    let mut bytes = std::fs::read(workspace_dir.join("live-paper/executable-edge-observe.mjs"))
        .map_err(|e| e.to_string())?;
    bytes.extend(std::fs::read(workspace_dir.join("live-paper/bid-anchor-read-focus.mjs"))
        .map_err(|e| e.to_string())?);
    Ok(baal_service_runtime::json_text::sha256(&bytes))
}
/// projectAnchorReadFocus(stats, lock, observerCohort)
pub fn project_anchor_read_focus(stats: &V, lock: &V, observer_cohort: &V,
    workspace_dir: &Path) -> Result<V, String> {
    let mut body = json!({"schemaVersion":1,"mode":"bid_anchor_read_focus_only",
        "observerCohort":observer_cohort,"pid":lock["pid"],"lockId":lock["id"],
        "codeHash":anchor_focus_code_hash(workspace_dir)?,"statsHash":hash(stats)?,
        "networkRequests":0,"competitionOrders":0});
    body["projectionHash"] = json!(hash(&body)?);
    Ok(body)
}
/// anchorFocusMarkets(stats, universe) — held/entry_pending markets, sorted.
pub fn anchor_focus_markets(stats: &V, universe: &V) -> Result<Vec<V>, String> {
    let valid: BTreeSet<String> = universe["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().filter_map(|x| x["marketId"].as_str().map(String::from)).collect();
    let by_instrument: BTreeMap<String, String> = universe["exchanges"].as_array().cloned().unwrap_or_default()
        .iter().filter_map(|x| {
            Some((format!("{}:YES", x["exchangeId"].as_str().unwrap_or("")),
                x["marketId"].as_str().unwrap_or("").to_string()))
        }).collect();
    let mut markets: BTreeMap<String, String> = BTreeMap::new();
    need(stats["wallets"].as_object().is_some_and(|w| w.len() == 2
        && w.contains_key("1000") && w.contains_key("3000")))?;
    for wallet in stats["wallets"].as_object().cloned().unwrap_or_default().values() {
        need(wallet["positions"].is_array() && wallet["pending"].is_array())?;
        for position in wallet["positions"].as_array().unwrap() {
            let market_id = by_instrument.get(position["instrument"].as_str().unwrap_or(""));
            need(market_id.is_some()
                && valid.contains(market_id.unwrap())
                && position["quantity"].as_i64().is_some_and(|q| q > 0))?;
            markets.insert(market_id.unwrap().clone(), "held".to_string());
        }
        for intent in wallet["pending"].as_array().unwrap() {
            need(["buy", "sell"].contains(&intent["action"].as_str().unwrap_or("")))?;
            if intent["action"].as_str() == Some("sell") {
                need(!wallet["positions"].as_array().unwrap().is_empty())?;
                continue;
            }
            let candidates: Vec<V> = stats["signals"].as_array().cloned().unwrap_or_default()
                .into_iter().filter(|s| s["actualAvailableAt"] == intent["createdAt"]).collect();
            need(!candidates.is_empty())?;
            for signal in candidates {
                let mid = signal["marketId"].as_str().unwrap_or("");
                need(valid.contains(mid))?;
                if !markets.contains_key(mid) {
                    markets.insert(mid.to_string(), "entry_pending".to_string());
                }
            }
        }
    }
    let mut out: Vec<V> = markets.into_iter()
        .map(|(market_id, phase)| json!({"marketId": market_id, "phase": phase})).collect();
    out.sort_by(|a, b| {
        let ra = (a["phase"].as_str() == Some("entry_pending")) as i64;
        let rb = (b["phase"].as_str() == Some("entry_pending")) as i64;
        ra.cmp(&rb).then(a["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)
            .cmp(&b["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)))
    });
    Ok(out)
}
/// readAnchorReadFocus(runtimeDir, universe, now) — null when not consumable.
pub fn read_anchor_read_focus(runtime_dir: &Path, universe: &V, now: f64,
    workspace_dir: &Path) -> Option<V> {
    (|| -> Result<V, String> {
        let active = read_json(&runtime_dir.join("bid-anchor-live-active.json"))?;
        need(cohort_ok(&active["cohort"]))?;
        let dir = runtime_dir.join("bid-anchor-live-cohorts")
            .join(active["cohort"].as_str().unwrap());
        let bank = read_json(&dir.join("bank.json"))?;
        let stats = read_json(&dir.join("stats.json"))?;
        let lock = read_json(&dir.join("worker.lock"))?;
        let projection = read_json(&dir.join("read-focus.json"))?;
        let mut body = projection.clone();
        let projection_hash = body.as_object_mut().and_then(|m| m.shift_remove("projectionHash")).unwrap_or(V::Null);
        need(cohort_ok(&body["observerCohort"]))?;
        if ["STOP", "BID_ANCHOR_LIVE_STOP"].iter().any(|n| runtime_dir.join(n).exists())
            || dir.join("STOP").exists()
            || runtime_dir.join("executable-edge-cohorts")
                .join(body["observerCohort"].as_str().unwrap_or("")).join("OBSERVER_STOP").exists() {
            return Err("stop".into());
        }
        need(body["schemaVersion"].as_i64() == Some(1)
            && body["mode"].as_str() == Some("bid_anchor_read_focus_only")
            && body["networkRequests"].as_i64() == Some(0)
            && body["competitionOrders"].as_i64() == Some(0)
            && body["codeHash"].as_str() == Some(anchor_focus_code_hash(workspace_dir)?.as_str())
            && projection_hash.as_str() == Some(hash(&body)?.as_str())
            && body["statsHash"].as_str() == Some(hash(&stats)?.as_str())
            && body["pid"] == lock["pid"] && body["lockId"] == lock["id"]
            && lock["pid"].as_i64().is_some_and(|p| p > 0)
            && lock["id"].is_string())?;
        // process.kill(lock.pid, 0) — pid liveness check (no signal sent).
        need(lock["pid"].as_i64().is_some_and(|p| std::path::Path::new(&format!("/proc/{}", p)).exists()))?;
        let canonical = crate::breadth::validate_universe(&bank["universe"],
            universe["tournamentId"].as_str().unwrap_or(""))?;
        need(canonical["hash"] == universe["hash"]
            && digest(&canonical["exchanges"]) == digest(&universe["exchanges"]))?;
        let updated_at = baal_service_runtime::contract::parse_iso_ms(stats["updatedAt"].as_str().unwrap_or(""))
            .unwrap_or(f64::NAN);
        need(digest(&bank) == active["bankHash"].as_str().unwrap_or("")
            && stats["bankHash"] == active["bankHash"]
            && stats["cohort"] == active["cohort"]
            && stats["mode"].as_str() == Some("bid_anchor_actual_receipt_paper")
            && stats["failed"] == V::Bool(false) && stats["sourceClosed"] == V::Bool(false)
            && stats["competitionOrders"].as_i64() == Some(0) && stats["networkRequests"].as_i64() == Some(0)
            && updated_at.is_finite() && now >= updated_at && now - updated_at <= 20000.0)?;
        let file = dir.join("journal.jsonl");
        let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
        let tail = {
            use std::io::{Read, Seek, SeekFrom};
            let mut fd = std::fs::File::open(&file).map_err(|e| e.to_string())?;
            let take = size.min(65536);
            let mut bytes = vec![0u8; take as usize];
            fd.seek(SeekFrom::Start(size - take)).map_err(|e| e.to_string())?;
            let n = fd.read(&mut bytes).map_err(|e| e.to_string())?;
            bytes.truncate(n);
            bytes
        };
        need(!tail.is_empty() && *tail.last().unwrap() == b'\n')?;
        let text = String::from_utf8(tail).map_err(|e| e.to_string())?;
        let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
        let record: V = serde_json::from_str(lines.last().ok_or("empty")?).map_err(|e| e.to_string())?;
        let mut checkpoint = record.clone();
        let journal_hash = checkpoint.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
        need(digest(&checkpoint) == journal_hash.as_str().unwrap_or("")
            && journal_hash == stats["journalHash"]
            && checkpoint["sequence"] == stats["journalSequence"]
            && checkpoint["stateHash"] == stats["stateHash"]
            && checkpoint["bankHash"] == stats["bankHash"])?;
        Ok(json!({"bankHash": stats["bankHash"],
            "markets": anchor_focus_markets(&stats, universe)?}))
    })().ok()
}
/// prioritizeAnchorReads(plan, focus, {excludedMarketIds, ownedRiskMarketIds, lastAttempts})
pub fn prioritize_anchor_reads(plan: &V, focus: Option<&V>, excluded_market_ids: &[String],
    owned_risk_market_ids: &[String], last_attempts: &std::collections::BTreeMap<String, i64>) -> V {
    let focus = match focus {
        Some(f) => f,
        None => return plan.clone(),
    };
    if focus["markets"].as_array().map(|m| m.is_empty()).unwrap_or(true)
        || baal_service_runtime::json_text::js_truthy(Some(&plan["comparisonSampling"]))
        || plan["basketMeasurement"]["marketIds"].as_array().is_some_and(|m| !m.is_empty())
        || plan["paperPackageFocus"]["marketIds"].as_array().is_some_and(|m| !m.is_empty()) {
        return plan.clone();
    }
    let rank = |phase: &str| -> i64 {
        match phase { "held" | "exit_pending" => 0, "entry_pending" => 1, "depth_required" => 2, _ => 0 }
    };
    let excluded: BTreeSet<&String> = excluded_market_ids.iter().collect();
    let owned_risk: BTreeSet<&String> = owned_risk_market_ids.iter().collect();
    let mut eligible: Vec<V> = focus["markets"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|x| {
            let mid = x["marketId"].as_str().unwrap_or("").to_string();
            !excluded.contains(&mid)
                || (owned_risk.contains(&mid) && ["held", "exit_pending"].contains(&x["phase"].as_str().unwrap_or("")))
        }).collect();
    eligible.sort_by(|a, b| {
        rank(a["phase"].as_str().unwrap_or("")).cmp(&rank(b["phase"].as_str().unwrap_or("")))
            .then(last_attempts.get(a["marketId"].as_str().unwrap_or("")).copied().unwrap_or(0)
                .cmp(&last_attempts.get(b["marketId"].as_str().unwrap_or("")).copied().unwrap_or(0)))
            .then(a["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)
                .cmp(&b["marketId"].as_str().unwrap_or("0").parse::<i64>().unwrap_or(0)))
    });
    let mut reads: Vec<V> = plan["reads"].as_array().cloned().unwrap_or_default();
    let mut observed: Vec<String> = Vec::new();
    let mut replacements = 0i64;
    for market in &eligible {
        let mid = market["marketId"].as_str().unwrap_or("").to_string();
        if reads.iter().any(|x| x["marketId"].as_str() == Some(mid.as_str())) {
            observed.push(mid);
            continue;
        }
        if replacements == 2 { break; }
        let index = reads.iter().position(|x|
            ["discovery", "comparison", "edge_observation"].contains(&x["reason"].as_str().unwrap_or(""))
            && !eligible.iter().any(|y| y["marketId"] == x["marketId"]));
        let Some(index) = index else { continue };
        reads[index] = json!({"marketId": mid, "path": "supplementalDepth",
            "reason": "bid_anchor_paper_risk"});
        replacements += 1;
        observed.push(mid);
    }
    if observed.is_empty() { return plan.clone(); }
    let present: BTreeSet<String> = reads.iter().map(|x| x["marketId"].as_str().unwrap_or("").to_string()).collect();
    let mut out = plan.clone();
    out["reads"] = json!(reads);
    out["entryMarketIds"] = json!(plan["entryMarketIds"].as_array().cloned().unwrap_or_default()
        .into_iter().filter(|id| present.contains(id.as_str().unwrap_or(""))
            && !observed.contains(&id.as_str().unwrap_or("").to_string())).collect::<Vec<V>>());
    out["bidAnchorReadFocus"] = json!({"marketIds": observed, "replacedSlots": replacements,
        "requestsAdded": 0, "executionAuthority": false});
    out
}
