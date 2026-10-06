// live-paper/probability-read-focus.mjs — probability cohort read-focus projection.
use baal_service_runtime::json_text::{digest, js_int};
use serde_json::{json, Value as V};
use std::path::Path;

fn need(ok: bool) -> Result<(), String> { if !ok { return Err("PROBABILITY_FOCUS_INVALID".into()); } Ok(()) }
fn hash(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
fn cohort_ok(s: &str) -> bool {
    // /^[a-z][a-z0-9-]{1,63}$/ — first char + 1..63 more => total 2..64.
    let b = s.as_bytes();
    b.len() >= 2 && b.len() <= 64 && b[0].is_ascii_lowercase()
        && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}
fn parse_ms(v: &V) -> Option<i64> { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).map(|m| m as i64) }
/// probabilityFocusCodeHash() — sha256 over the two frozen source files' bytes.
/// `live_paper_dir` stands in for `import.meta.url`'s directory.
pub fn probability_focus_code_hash(live_paper_dir: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for f in ["executable-edge-observe.mjs", "probability-read-focus.mjs"] {
        let bytes = std::fs::read(live_paper_dir.join(f)).map_err(|e| e.to_string())?;
        h.update(&bytes);
    }
    Ok(format!("{:x}", h.finalize()))
}
/// projectProbabilityReadFocus(stats, lock, observerCohort)
pub fn project_probability_read_focus(stats: &V, lock: &V, observer_cohort: &str, live_paper_dir: &Path) -> Result<V, String> {
    let body = json!({"schemaVersion": 1, "mode": "probability_depth_reads_only", "observerCohort": observer_cohort,
        "pid": lock["pid"], "lockId": lock["id"], "codeHash": probability_focus_code_hash(live_paper_dir)?,
        "statsHash": hash(stats)?, "networkRequests": 0, "competitionOrders": 0});
    let mut out = body.clone();
    out["projectionHash"] = json!(hash(&body)?);
    Ok(out)
}
/// mergeProbabilityReadFocus(focuses)
pub fn merge_probability_read_focus(focuses: &[V]) -> Option<V> {
    let live: Vec<&V> = focuses.iter().filter(|f| baal_service_runtime::json_text::js_truthy(Some(f))).collect();
    if live.is_empty() { return None; }
    let priority = |phase: &str| -> i64 { match phase { "held" => 3, "pending_sell" => 2, "pending_buy" => 1, "depth_required" => 0, _ => 0 } };
    let mut markets: serde_json::Map<String, V> = serde_json::Map::new();
    for focus in &live {
        if focus["executionAuthority"] != V::Bool(false) || !focus["markets"].is_array() { return None; }
        for m in focus["markets"].as_array().unwrap() {
            let mid = m["marketId"].as_str().unwrap_or("").to_string();
            let replace = match markets.get(&mid) {
                None => true,
                Some(old) => priority(m["phase"].as_str().unwrap_or("")) > priority(old["phase"].as_str().unwrap_or("")),
            };
            if replace { markets.insert(mid, m.clone()); }
        }
    }
    Some(json!({
        "bankHash": digest(&V::from(live.iter().map(|f| f["bankHash"].clone()).collect::<Vec<_>>())),
        "markets": markets.values().cloned().collect::<Vec<_>>(),
        "ownedRiskReads": live.iter().any(|f| f["ownedRiskReads"] == V::Bool(true)),
        "executionAuthority": false,
    }))
}
/// readProbabilityReadFocus(runtimeDir, universe, now, activeFile)
/// `live_paper_dir` stands in for the module's own directory for the code hash.
pub fn read_probability_read_focus(runtime_dir: &Path, universe: &V, now: i64, active_file: &str, live_paper_dir: &Path) -> Option<V> {
    (|| -> Result<V, String> {
        need(["probability-receipt-active.json", "calibrated-poll-paper-active.json", "primary-breadth-paper-active.json"].contains(&active_file))?;
        let read_json = |p: &Path| -> Result<V, String> {
            serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
        };
        let active = read_json(&runtime_dir.join(active_file))?;
        need(cohort_ok(active["cohort"].as_str().unwrap_or("")))?;
        let dir = runtime_dir.join("probability-receipt-cohorts").join(active["cohort"].as_str().unwrap());
        let bank = read_json(&dir.join("bank.json"))?;
        let stats = read_json(&dir.join("stats.json"))?;
        let lock = read_json(&dir.join("worker.lock"))?;
        let projection = read_json(&dir.join("read-focus.json"))?;
        let projection_hash = projection["projectionHash"].clone();
        let mut body = projection.clone();
        body.as_object_mut().and_then(|m| m.shift_remove("projectionHash"));
        need(cohort_ok(body["observerCohort"].as_str().unwrap_or("")))?;
        if runtime_dir.join("STOP").exists() || dir.join("STOP").exists() || dir.join("OBSERVER_STOP").exists()
            || runtime_dir.join("executable-edge-cohorts").join(body["observerCohort"].as_str().unwrap()).join("OBSERVER_STOP").exists()
        {
            return Err("focus stopped".into());
        }
        need(body["schemaVersion"].as_i64() == Some(1) && body["mode"].as_str() == Some("probability_depth_reads_only")
            && body["networkRequests"].as_i64() == Some(0) && body["competitionOrders"].as_i64() == Some(0)
            && body["codeHash"].as_str() == probability_focus_code_hash(live_paper_dir).ok().as_deref()
            && projection_hash.as_str() == hash(&body).ok().as_deref()
            && body["statsHash"].as_str() == hash(&stats).ok().as_deref()
            && body["pid"] == lock["pid"] && body["lockId"] == lock["id"]
            && js_int(&lock["pid"]).is_some_and(|p| p > 0) && lock["id"].is_string())?;
        // process.kill(pid, 0): process must exist.
        let pid = js_int(&lock["pid"]).unwrap_or(0);
        need(pid > 0 && Path::new(&format!("/proc/{}", pid)).exists())?;
        let owned_risk = bank["schemaVersion"].as_i64() == Some(3) || bank["schemaVersion"].as_i64() == Some(4);
        if bank["schemaVersion"].as_i64() == Some(4) {
            crate::probability_receipt_v3::validate_probability_receipt_bank(&bank)?;
        } else if owned_risk {
            crate::probability_receipt_v2::validate_probability_receipt_bank(&bank)?;
        } else {
            crate::probability_receipt::validate_probability_receipt_bank(&bank)?;
        }
        let u = crate::breadth::validate_universe(&bank["universe"], universe["tournamentId"].as_str().unwrap_or(""))?;
        let updated = parse_ms(&stats["updatedAt"]);
        need(u["hash"] == universe["hash"] && digest(&u["exchanges"]) == digest(&universe["exchanges"])
            && digest(&bank) == active["bankHash"].as_str().unwrap_or("")
            && stats["bankHash"] == active["bankHash"] && stats["cohort"] == active["cohort"]
            && stats["mode"] == bank["mode"]
            && stats["failed"] == V::Bool(false) && stats["sourceClosed"] == V::Bool(false)
            && stats["competitionOrders"].as_i64() == Some(0) && stats["networkRequests"].as_i64() == Some(0)
            && updated.is_some_and(|t| now >= t && now - t <= 20000))?;
        let file = dir.join("journal.jsonl");
        let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
        use std::io::{Read, Seek, SeekFrom};
        let mut fd = std::fs::File::open(&file).map_err(|e| e.to_string())?;
        let tail = size.min(65536);
        fd.seek(SeekFrom::Start(size - tail)).map_err(|e| e.to_string())?;
        let mut bytes = vec![0u8; tail as usize];
        fd.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        need(!bytes.is_empty() && *bytes.last().unwrap() == b'\n')?;
        let text = String::from_utf8_lossy(&bytes).to_string();
        let lines: Vec<&str> = text.split('\n').collect();
        need(lines.len() >= 2)?;
        let record: V = serde_json::from_str(lines[lines.len() - 2]).map_err(|e| e.to_string())?;
        let journal_hash = record["hash"].clone();
        let mut checkpoint = record.clone();
        checkpoint.as_object_mut().and_then(|m| m.shift_remove("hash"));
        need(digest(&checkpoint) == journal_hash.as_str().unwrap_or("")
            && journal_hash == stats["journalHash"]
            && checkpoint["sequence"] == stats["journalSequence"]
            && checkpoint["stateHash"] == stats["stateHash"]
            && checkpoint["bankHash"] == stats["bankHash"])?;
        let mut markets = crate::bulk_dip_read_focus::anchor_focus_markets(&stats, &u)?;
        let mut seen: std::collections::BTreeSet<String> = markets.iter().map(|x| x["marketId"].as_str().unwrap_or("").to_string()).collect();
        for r in bank["research"].as_array().cloned().unwrap_or_default() {
            let expires = parse_ms(&r["expiresAt"]);
            let source = parse_ms(&r["sourceAt"]);
            // `!r.flags.length`: accessing .length on null/undefined throws;
            // a non-array non-string has .length === undefined -> !undefined.
            let flags_ok = match &r["flags"] {
                V::Null => return Err("flags".into()),
                V::Array(a) => a.is_empty(),
                V::String(s) => s.is_empty(),
                _ => true,
            };
            if r["settlementReviewed"].as_bool().unwrap_or(false)
                && flags_ok
                && expires.is_some_and(|e| now < e)
                && source.is_some_and(|s| now - s <= bank["policy"]["maximumResearchAgeMs"].as_i64().unwrap_or(0))
                && !seen.contains(r["marketId"].as_str().unwrap_or(""))
            {
                seen.insert(r["marketId"].as_str().unwrap().to_string());
                markets.push(json!({"marketId": r["marketId"], "phase": "depth_required"}));
            }
        }
        let mut out = json!({"bankHash": active["bankHash"], "markets": markets});
        if owned_risk { out["ownedRiskReads"] = V::Bool(true); }
        out["executionAuthority"] = V::Bool(false);
        Ok(out)
    })().ok()
}
