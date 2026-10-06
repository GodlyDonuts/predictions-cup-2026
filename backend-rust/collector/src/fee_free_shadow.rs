// live-paper/fee-free-shadow.mjs — zero-fee prospective paper baseline shadow.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::ml_shadow::{verify_source, VerifiedSource};

fn iso_now() -> String {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
    ms_to_iso(ms)
}
fn clock_at(e: &V, r: &V) -> String {
    e["clockAt"].as_str().filter(|s| !s.is_empty()).or_else(|| r["at"].as_str().filter(|s| !s.is_empty())).unwrap_or("").to_string()
}
fn orderbook_market(endpoint: &str) -> Option<String> {
    let rest = endpoint.strip_prefix("/markets/")?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if !digits.is_empty() && rest[digits.len()..].starts_with("/orderbook?") { Some(digits) } else { None }
}

/// createFeeFree(raw, c) -> {bank, state}
pub fn create_fee_free(raw: &str, c: &V) -> Result<V, String> {
    if c["feeBps"].as_f64() != Some(0.0) { return Err("Fee-free baseline requires zero fees".into()); }
    let Some(records) = verify_source(raw)? else { return Err("Complete source required".into()); };
    if records.records.is_empty() { return Err("Complete source required".into()); }
    let last = records.records.last().unwrap();
    let cutoff = clock_at(&last["event"], last);
    let bank = json!({"schemaVersion": 1, "name": "fee-free-baseline", "config": c.clone(),
        "sourceRecords": records.records.len(), "sourceHash": records.prefix_hashes.last().cloned().unwrap_or_default(),
        "cutoff": cutoff});
    Ok(json!({"bank": bank.clone(), "state": {"schemaVersion": 1, "bankHash": crate::model::digest(&bank),
        "sourceSequence": records.records.len(), "lastRecordHash": last["hash"].clone(),
        "wallet": crate::model::initial_state(c, "fee-free-baseline", &cutoff)?}}))
}

/// revoke(market) — drop open buy intents, scoped to one market or all.
fn revoke(s: &mut V, market: Option<&str>) {
    let books = s["wallet"]["books"].as_object().cloned().unwrap_or_default();
    if let Some(strategies) = s["wallet"]["strategies"].as_object_mut() {
        for (_, st) in strategies.iter_mut() {
            let remove: Vec<String> = st["pending"].as_object().map(|p| p.iter()
                .filter(|(id, p)| p["action"].as_str() == Some("buy")
                    && (market.is_none()
                        || books.get(*id).and_then(|b| b["marketId"].as_str()) == market))
                .map(|(id, _)| id.clone()).collect()).unwrap_or_default();
            for id in remove {
                if let Some(p) = st["pending"].as_object_mut() { p.remove(&id); }
                let n = st["expiredIntents"].as_i64().unwrap_or(0);
                st["expiredIntents"] = json!(n + 1);
            }
        }
    }
}

/// advanceFeeFree(input, records, bank) — records is a verify_source() output
/// (or the equivalent readIncrementalSource rows + prefix_hashes).
pub fn advance_fee_free(input: &V, source: &VerifiedSource, bank: &V) -> Result<V, String> {
    let mut s = input.clone();
    let c = &bank["config"];
    let seq = s["sourceSequence"].as_i64().unwrap_or(0);
    let bank_n = bank["sourceRecords"].as_i64().unwrap_or(-1);
    let prefix_bad = match (if bank_n >= 1 { source.prefix_hashes.get(bank_n as usize - 1) } else { None },
                            bank.get("sourceHash")) {
        (Some(a), Some(b)) => b.as_str() != Some(a.as_str()),
        (None, None) => false,
        _ => true,
    };
    // records[s.sourceSequence-1]?.hash !== s.lastRecordHash — no short-circuit:
    // a missing key in `lastRecordHash` is JS `undefined !== undefined` -> not bad.
    let prior_bad = match (if seq >= 1 { source.records.get(seq as usize - 1) } else { None },
                           input.get("lastRecordHash")) {
        (Some(r), Some(lh)) => r.get("hash") != Some(lh),
        (Some(_), None) => true,
        (None, None) => false,
        (None, Some(_)) => true,
    };
    if c["feeBps"].as_f64() != Some(0.0)
        || s["bankHash"].as_str() != Some(crate::model::digest(bank).as_str())
        || prefix_bad || prior_bad {
        return Err("Fee-free source/bank changed".into());
    }
    crate::model::assert_state(&s["wallet"], c)?;
    for r in &source.records {
        if r["sequence"].as_i64().unwrap_or(0) <= s["sourceSequence"].as_i64().unwrap_or(0) { continue; }
        s["sourceSequence"] = r["sequence"].clone();
        s["lastRecordHash"] = r["hash"].clone();
        let e = &r["event"];
        let at = clock_at(e, r);
        if e["type"].as_str() == Some("account_read_outage") {
            s["wallet"]["skips"]["account"] = json!({"at": at, "reason": e["reason"].as_str().filter(|x| !x.is_empty()).unwrap_or("account_outage")});
            crate::model::invalidate(&mut s["wallet"], None, &at, "account_outage");
            revoke(&mut s, None);
            continue;
        }
        if e["type"].as_str() != Some("cycle") { continue; }
        for snap in crate::depth_scope::scoped_snapshots(e, c)? {
            let snap_at = snap["at"].as_str().filter(|x| !x.is_empty()).unwrap_or(&at).to_string();
            if js_truthy(snap.get("skipped")) {
                let mid = snap["marketId"].as_str().unwrap_or("").to_string();
                s["wallet"]["skips"][&mid] = json!({"at": snap_at, "reason": snap["skipped"]});
                // JS invalidate stores the raw reason value (skipped may be non-string).
                {
                    let w = &mut s["wallet"];
                    if w.get("invalidated").is_none() { w["invalidated"] = json!({}); }
                    let ids: Vec<String> = w["books"].as_object().map(|o| o.iter()
                        .filter(|(_, b)| b["marketId"].as_str() == Some(mid.as_str()))
                        .map(|(k, _)| k.clone()).collect()).unwrap_or_default();
                    for id in ids { w["invalidated"][&id] = json!({"at": snap_at, "reason": snap["skipped"]}); }
                }
                revoke(&mut s, Some(&mid));
                continue;
            }
            let Some(mid) = orderbook_market(snap["endpoint"].as_str().unwrap_or("")) else { continue; };
            if !c["marketIds"].as_array().map(|a| a.iter().any(|x| x.as_str() == Some(mid.as_str()))).unwrap_or(false) { continue; }
            if parse_iso_ms(snap["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
                <= parse_iso_ms(bank["cutoff"].as_str().unwrap_or("")).unwrap_or(f64::NAN) { continue; }
            let book = (|| -> Result<V, String> {
                let excluded: BTreeSet<String> = match &e["accountExclusions"] {
                    V::Null => BTreeSet::new(),
                    V::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                    V::String(t) => t.chars().map(|ch| ch.to_string()).collect(),
                    _ => return Err("invalid exclusions".into()),
                };
                crate::model::normalize_book(&snap["data"], &mid, c["tournamentId"].as_str().unwrap_or(""),
                    snap["receivedAt"].as_str().unwrap_or(""), snap["requestStartedAt"].as_str().unwrap_or(""),
                    c["maxDataAgeMs"].as_i64().unwrap_or(0), &excluded, "event_age_v1")
            })();
            let b = match book {
                Ok(b) => b,
                Err(_) => {
                    s["wallet"]["skips"][&mid] = json!({"at": snap["receivedAt"], "reason": "invalid_book"});
                    crate::model::invalidate(&mut s["wallet"], Some(&mid), snap["receivedAt"].as_str().unwrap_or(""), "invalid_book");
                    revoke(&mut s, Some(&mid));
                    continue;
                }
            };
            s["wallet"] = crate::model::observe(&s["wallet"], &b, c)?;
        }
        crate::model::sample_equity(&mut s["wallet"], c, &at);
    }
    crate::model::assert_state(&s["wallet"], c)?;
    Ok(s)
}

/// feeFreeReport(s, bank, at)
pub fn fee_free_report(s: &V, bank: &V, at: &str) -> V {
    let mut r = crate::model::report(&s["wallet"], &bank["config"], at);
    let data_health = crate::worker_health::market_data_health(&s["wallet"], &bank["config"], &json!(at));
    let mut assumptions = r["assumptions"].as_object().cloned().unwrap_or_default();
    assumptions.insert("feeStatus".into(), json!("human_requested_zero_additional_explicit_fee_baseline"));
    if let Some(o) = r.as_object_mut() {
        o.insert("dataHealth".into(), data_health);
        o.insert("assumptions".into(), V::Object(assumptions));
        o.insert("mode".into(), json!("prospective_fee_free_paper_baseline"));
        o.insert("feeScenario".into(), json!("zero_additional_explicit_fee_baseline"));
        o.insert("cutoff".into(), bank["cutoff"].clone());
        o.insert("bankHash".into(), s["bankHash"].clone());
        o.insert("sourceSequence".into(), s["sourceSequence"].clone());
        o.insert("lastRecordHash".into(), s["lastRecordHash"].clone());
        o.insert("networkRequests".into(), json!(0));
        o.insert("competitionOrders".into(), json!(0));
    }
    r
}

/// CLI entry shared by fee-free-shadow.mjs (full verifySource) and
/// fee-free-shadow-v2.mjs (readIncrementalSource + sourceReadMetrics).
pub fn run_fee_free_with_source<F, M>(dir: &Path, workspace_dir: &Path, once: bool,
    mut read_source: F, metrics: M) -> Result<(), String>
where F: FnMut() -> Result<Option<VerifiedSource>, String>, M: Fn() -> Option<V> {
    let _release = crate::paper_runtime::lock(&dir.join("fee-free-shadow.lock"))?;
    let bank: V = serde_json::from_str(&std::fs::read_to_string(dir.join("fee-free-bank.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let state_path = dir.join("fee-free-state.json");
    let mut s: V = serde_json::from_str(&std::fs::read_to_string(&state_path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let stopped = || ["STOP", "FEE_FREE_STOP"].iter().any(|f| dir.join(f).exists()) || workspace_dir.join("STOP").exists();
    let publish = |s: &V, bank: &V, status: &str| -> Result<(), String> {
        let mut r = fee_free_report(s, bank, &iso_now());
        if let Some(o) = r.as_object_mut() {
            o.insert("status".into(), json!(status));
            if let Some(m) = metrics() { o.insert("sourceReadMetrics".into(), m); }
        }
        baal_service_runtime::atomic::atomic_json(&dir.join("fee-free-stats.json"), &r)
    };
    let mut partial = 0;
    let inner = (|| -> Result<(), String> {
        loop {
            if stopped() { break; }
            match read_source()? {
                None => {
                    partial += 1;
                    if partial >= 4 { return Err("Incomplete source".into()); }
                    publish(&s, &bank, "awaiting_capture")?;
                }
                Some(records) => {
                    partial = 0;
                    s = advance_fee_free(&s, &records, &bank)?;
                    baal_service_runtime::atomic::atomic_json(&state_path, &s)?;
                    publish(&s, &bank, "running")?;
                }
            }
            if once { break; }
            let mut i = 0;
            while i < 75 && !stopped() { std::thread::sleep(Duration::from_millis(200)); i += 1; }
            if stopped() { break; }
        }
        if stopped() { publish(&s, &bank, "stopped")?; }
        Ok(())
    })();
    if let Err(e) = inner { publish(&s, &bank, "failed")?; return Err(e); }
    Ok(())
}

/// fee-free-shadow.mjs CLI: verifySource over the full journal each poll.
pub fn run_fee_free(dir: &Path, workspace_dir: &Path, once: bool) -> Result<(), String> {
    let source_file = dir.join("journal.jsonl");
    run_fee_free_with_source(dir, workspace_dir, once,
        || verify_source(&std::fs::read_to_string(&source_file).map_err(|e| e.to_string())?), || None)
}
