// live-paper/replay.mjs — offline capture-journal replay.
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

/// replayJournal(source, c, {snapshotOrderPolicy}) — source is raw journal text
/// or a verified training source (records + sourceHash already verified).
pub fn replay_journal(raw: Option<&str>, verified: Option<&crate::training_source::VerifiedSource>,
    c: &V, snapshot_order_policy: &str) -> Result<V, String> {
    crate::snapshot_order::validate_snapshot_order(snapshot_order_policy)?;
    crate::model::validate_config(c)?;
    let streamed = verified.is_some();
    let source_hash;
    let records: Vec<V>;
    if let Some(v) = verified {
        source_hash = v.source_hash().to_string();
        records = v.records.clone();
    } else {
        let raw = match raw { Some(r) if !r.is_empty() && r.ends_with('\n') => r, _ => return Err("Incomplete capture journal".into()) };
        source_hash = baal_service_runtime::json_text::sha256(raw.as_bytes());
        records = raw.split('\n').filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).map_err(|_| "Capture integrity failure".to_string())).collect::<Result<_,_>>()?;
    }
    let mut previous = String::new();
    let mut sequence = 0i64;
    let mut state: Option<V> = None;
    let mut cycles = 0i64;
    let mut clock: Option<String> = None;
    for r in records {
        let mut body = r.clone();
        let hash = body.as_object_mut().and_then(|m| m.remove("hash")).and_then(|h| h.as_str().map(String::from)).unwrap_or_default();
        sequence += 1;
        if body["sequence"].as_i64() != Some(sequence)
            || body["previous"].as_str().unwrap_or("") != previous
            || (!streamed && hash != baal_service_runtime::json_text::digest(&body)) {
            return Err("Capture integrity failure".into());
        }
        previous = hash;
        let event_clock = body["event"]["clockAt"].as_str().or(body["at"].as_str()).unwrap_or("").to_string();
        let ec = baal_service_runtime::contract::parse_iso_ms(&event_clock).ok_or("Journal clock rollback")?;
        if let Some(c0) = &clock {
            if ec < baal_service_runtime::contract::parse_iso_ms(c0).unwrap_or(0.0) { return Err("Journal clock rollback".into()); }
        }
        clock = Some(event_clock.clone());
        if body["event"]["type"].as_str() != Some("cycle") {
            if state.is_some() && body["event"]["type"].as_str() == Some("account_read_outage") {
                crate::model::invalidate(state.as_mut().unwrap(), None, &event_clock, body["event"]["reason"].as_str().unwrap_or(""));
            }
            continue;
        }
        let excluded: BTreeSet<String> = body["event"]["accountExclusions"].as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        for e in crate::snapshot_order::ordered_snapshots(&body["event"], c, snapshot_order_policy, Some(&event_clock))? {
            if !e["skipped"].is_null() {
                if let Some(st) = state.as_mut() {
                    let at = if !e["at"].is_null() { e["at"].clone() } else { body["state"]["skips"][e["marketId"].as_str().unwrap_or("")]["at"].clone() };
                    let mid = e["marketId"].as_str().unwrap_or("").to_string();
                    st["skips"][&mid] = json!({"at": at, "reason": e["skipped"]});
                    crate::model::invalidate(st, Some(&mid), at.as_str().unwrap_or(&event_clock), e["skipped"].as_str().unwrap_or(""));
                }
                continue;
            }
            if e["endpoint"].is_null() { continue; }
            let endpoint = e["endpoint"].as_str().unwrap_or("");
            let mid = endpoint.strip_prefix("/markets/").and_then(|r| r.strip_suffix("/orderbook?")).map(|s| s.to_string())
                .or_else(|| endpoint.strip_prefix("/markets/").map(|s| s.split('/').next().unwrap_or("").to_string()));
            let mid = match mid { Some(m) if endpoint.starts_with("/markets/") && endpoint.contains("/orderbook?") => m, _ => continue };
            if !c["marketIds"].as_array().map(|a| a.iter().any(|x| x.as_str() == Some(&mid))).unwrap_or(false) { continue; }
            if state.is_none() {
                state = Some(crate::model::initial_state(c, &format!("replay-{}", &source_hash[..16.min(source_hash.len())]),
                    e["requestStartedAt"].as_str().unwrap_or(""))?);
            }
            let b = match crate::model::normalize_book(&e["data"], &mid, c["tournamentId"].as_str().unwrap_or(""),
                e["receivedAt"].as_str().unwrap_or(""), e["requestStartedAt"].as_str().unwrap_or(""),
                c["maxDataAgeMs"].as_i64().unwrap_or(0), &excluded, "authoritative_receipt_v1") {
                Ok(b) => b,
                Err(err) => {
                    let st = state.as_mut().unwrap();
                    st["skips"][&mid] = json!({"at": e["receivedAt"], "reason": err});
                    crate::model::invalidate(st, Some(&mid), e["receivedAt"].as_str().unwrap_or(""), &err);
                    continue;
                }
            };
            let st = state.take().unwrap();
            state = Some(crate::model::observe(&st, &b, c)?);
        }
        if let Some(st) = state.as_mut() { crate::model::sample_equity(st, c, &event_clock); }
        cycles += 1;
    }
    let state = state.ok_or("No accepted real source observations")?;
    if state["observations"].is_null() { return Err("No accepted real source observations".into()); }
    let mut out = crate::model::report(&state, c, clock.as_deref().unwrap_or(""));
    out["replay"] = json!({"sourceHash": source_hash, "sourceRecords": sequence, "cycles": cycles,
        "finalEventClock": clock, "scope": "Whole captured path; parameter exploration, not held-out profitability",
        "networkRequests": 0});
    Ok(out)
}
