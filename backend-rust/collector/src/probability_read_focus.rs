// live-paper/probability-read-focus.mjs — read focus projection for the
// probability-receipt companion host.
use serde_json::{json, Value as V};
use std::path::Path;

fn need(ok: bool) -> Result<(), String> {
    if !ok { return Err("PROBABILITY_FOCUS_INVALID".into()); }
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
fn parse_at(v: &V) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN)
}
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
/// probabilityFocusCodeHash()
pub fn probability_focus_code_hash(workspace_dir: &Path) -> Result<String, String> {
    let mut bytes = std::fs::read(workspace_dir.join("live-paper/executable-edge-observe.mjs"))
        .map_err(|e| e.to_string())?;
    bytes.extend(std::fs::read(workspace_dir.join("live-paper/probability-read-focus.mjs"))
        .map_err(|e| e.to_string())?);
    Ok(baal_service_runtime::json_text::sha256(&bytes))
}
/// projectProbabilityReadFocus(stats, lock, observerCohort)
pub fn project_probability_read_focus(stats: &V, lock: &V, observer_cohort: &V,
    workspace_dir: &Path) -> Result<V, String> {
    let mut body = json!({"schemaVersion":1,"mode":"probability_depth_reads_only",
        "observerCohort":observer_cohort,"pid":lock["pid"],"lockId":lock["id"],
        "codeHash":probability_focus_code_hash(workspace_dir)?,"statsHash":hash(stats)?,
        "networkRequests":0,"competitionOrders":0});
    body["projectionHash"] = json!(hash(&body)?);
    Ok(body)
}
/// mergeProbabilityReadFocus(focuses) — max-priority merge; null when empty.
/// JS: need() throws on invalid focus; V::Null focuses are filtered (falsy).
pub fn merge_probability_read_focus(focuses: &[V]) -> Result<V, String> {
    let live: Vec<&V> = focuses.iter()
        .filter(|f| baal_service_runtime::json_text::js_truthy(Some(*f))).collect();
    if live.is_empty() { return Ok(V::Null); }
    let priority = |phase: &str| -> i64 {
        match phase { "held" => 3, "pending_sell" => 2, "pending_buy" => 1, "depth_required" => 0, _ => 0 }
    };
    let mut markets: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    for focus in &live {
        need(focus["executionAuthority"] == V::Bool(false) && focus["markets"].is_array())?;
        for m in focus["markets"].as_array().unwrap() {
            let mid = m["marketId"].as_str().unwrap_or("").to_string();
            let take = markets.get(&mid).map(|old|
                priority(m["phase"].as_str().unwrap_or(""))
                    > priority(old["phase"].as_str().unwrap_or(""))).unwrap_or(true);
            if take { markets.insert(mid, m.clone()); }
        }
    }
    let mut out = json!({"bankHash": digest(&V::Array(live.iter().map(|f| f["bankHash"].clone()).collect())),
        "markets": markets.into_values().collect::<Vec<V>>()});
    if live.iter().any(|f| f["ownedRiskReads"] == V::Bool(true)) {
        out["ownedRiskReads"] = json!(true);
    }
    out["executionAuthority"] = json!(false);
    Ok(out)
}
/// readProbabilityReadFocus(runtimeDir, universe, now, activeFile) — null when
/// not consumable.
pub fn read_probability_read_focus(runtime_dir: &Path, universe: &V, now: f64,
    active_file: &str, workspace_dir: &Path) -> Option<V> {
    (|| -> Result<V, String> {
        need(["probability-receipt-active.json", "calibrated-poll-paper-active.json",
            "primary-breadth-paper-active.json"].contains(&active_file))?;
        let active = read_json(&runtime_dir.join(active_file))?;
        need(cohort_ok(&active["cohort"]))?;
        let dir = runtime_dir.join("probability-receipt-cohorts")
            .join(active["cohort"].as_str().unwrap());
        let bank = read_json(&dir.join("bank.json"))?;
        let stats = read_json(&dir.join("stats.json"))?;
        let lock = read_json(&dir.join("worker.lock"))?;
        let projection = read_json(&dir.join("read-focus.json"))?;
        let mut body = projection.clone();
        let projection_hash = body.as_object_mut().and_then(|m| m.shift_remove("projectionHash")).unwrap_or(V::Null);
        need(cohort_ok(&body["observerCohort"]))?;
        if runtime_dir.join("STOP").exists() || dir.join("STOP").exists()
            || dir.join("OBSERVER_STOP").exists()
            || runtime_dir.join("executable-edge-cohorts")
                .join(body["observerCohort"].as_str().unwrap_or("")).join("OBSERVER_STOP").exists() {
            return Err("stop".into());
        }
        need(body["schemaVersion"].as_i64() == Some(1)
            && body["mode"].as_str() == Some("probability_depth_reads_only")
            && body["networkRequests"].as_i64() == Some(0)
            && body["competitionOrders"].as_i64() == Some(0)
            && body["codeHash"].as_str() == Some(probability_focus_code_hash(workspace_dir)?.as_str())
            && projection_hash.as_str() == Some(hash(&body)?.as_str())
            && body["statsHash"].as_str() == Some(hash(&stats)?.as_str())
            && body["pid"] == lock["pid"] && body["lockId"] == lock["id"]
            && lock["pid"].as_i64().is_some_and(|p| p > 0) && lock["id"].is_string())?;
        need(lock["pid"].as_i64().is_some_and(|p| std::path::Path::new(&format!("/proc/{}", p)).exists()))?;
        let owned_risk = bank["schemaVersion"].as_i64() == Some(3)
            || bank["schemaVersion"].as_i64() == Some(4);
        if bank["schemaVersion"].as_i64() == Some(4) {
            crate::probability_receipt_v3::validate_probability_receipt_bank(&bank)?;
        } else if owned_risk {
            crate::probability_receipt_v2::validate_probability_receipt_bank(&bank)?;
        } else {
            crate::probability_receipt::validate_probability_receipt_bank(&bank)?;
        }
        let u = crate::breadth::validate_universe(&bank["universe"],
            universe["tournamentId"].as_str().unwrap_or(""))?;
        let updated_at = parse_at(&stats["updatedAt"]);
        need(u["hash"] == universe["hash"]
            && digest(&u["exchanges"]) == digest(&universe["exchanges"])
            && digest(&bank) == active["bankHash"].as_str().unwrap_or("")
            && stats["bankHash"] == active["bankHash"] && stats["cohort"] == active["cohort"]
            && stats["mode"] == bank["mode"]
            && stats["failed"] == V::Bool(false) && stats["sourceClosed"] == V::Bool(false)
            && stats["competitionOrders"].as_i64() == Some(0) && stats["networkRequests"].as_i64() == Some(0)
            && now >= updated_at && now - updated_at <= 20000.0)?;
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
        let mut markets = crate::bulk_dip_read_focus::anchor_focus_markets(&stats, &u)?;
        let mut seen: std::collections::BTreeSet<String> = markets.iter()
            .filter_map(|x| x["marketId"].as_str().map(String::from)).collect();
        for r in bank["research"].as_array().cloned().unwrap_or_default() {
            let expires = parse_at(&r["expiresAt"]);
            let source_at = parse_at(&r["sourceAt"]);
            let mid = r["marketId"].as_str().unwrap_or("").to_string();
            if r["settlementReviewed"] == V::Bool(true)
                && r["flags"].as_array().map(|f| f.is_empty()).unwrap_or(false)
                && now < expires
                && now - source_at <= bank["policy"]["maximumResearchAgeMs"].as_i64().unwrap_or(0) as f64
                && !seen.contains(&mid) {
                seen.insert(mid.clone());
                markets.push(json!({"marketId": mid, "phase": "depth_required"}));
            }
        }
        let mut out = json!({"bankHash": active["bankHash"], "markets": markets});
        if owned_risk { out["ownedRiskReads"] = json!(true); }
        out["executionAuthority"] = json!(false);
        Ok(out)
    })().ok()
}
