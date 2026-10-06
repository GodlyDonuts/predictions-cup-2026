// live-paper/coupled-maker-replay-v2.mjs — bulk replay over coupled-maker-paper-v2.
// Unlike v1, this also seals runtimeDir/market-prices.jsonl and splices 'prices'
// events into the stream, and its causal gate binds eligibility to `request`
// (the book's requestStartedAt) rather than the event's availableAt.
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};
use crate::stable_maker_replay_v2 as shared;

/// replayCoupledBulkEvents(events, bank)
pub fn replay_coupled_bulk_events(events: &[V], bank: &V) -> Result<V, String> {
    let mut state = crate::coupled_maker_paper_v2::create_coupled_bulk_state(bank)?;
    let mut accounts: Vec<V> = Vec::new();
    let mut processed = 0i64;
    let cutoff = shared::dp(&bank["cutoff"]);
    for e in events {
        if e["kind"].as_str() == Some("eligibility") { accounts.push(e.clone()); continue; }
        if shared::dp(&e["availableAt"]) < cutoff { continue; }
        if e["kind"].as_str() == Some("book") {
            match e.get("book") {
                v @ (None | Some(V::Null)) =>
                    return Err(shared::prop_read_err(v, "requestStartedAt")),
                _ => {}
            }
            let request = shared::dp(&e["book"]["requestStartedAt"]);
            // v2: Date.parse(a.availableAt) <= request
            let causal = shared::causal_account(&accounts, request, 0.0, true).cloned();
            state = crate::coupled_maker_paper_v2::advance_coupled_bulk(
                &state, &shared::attach_account(e, causal.as_ref()), bank)?;
        } else {
            state = crate::coupled_maker_paper_v2::advance_coupled_bulk(&state, e, bank)?;
        }
        processed += 1;
    }
    Ok(json!({"state": state, "processed": processed,
        "report": crate::coupled_maker_paper_v2::coupled_bulk_report(
            &state, bank, state["lastAt"].as_str().unwrap_or(""))?}))
}

/// runCoupledBulkReplay({runtimeDir, bank, executable, binaryHash, out, workingDir})
pub fn run_coupled_bulk_replay(runtime_dir: &Path, bank: &V, executable: &Path, binary_hash: &str,
    out: &Path, working_dir: Option<&Path>) -> Result<V, String> {
    let owned_dir: PathBuf;
    let working_dir = match working_dir {
        Some(w) => w,
        None => {
            owned_dir = runtime_dir.join("coupled-bulk-diagnostics").join(shared::uuid_v4());
            &owned_dir
        }
    };
    let started = std::time::Instant::now();
    let mut sources = shared::load_stable_maker_sources(runtime_dir, working_dir, executable, binary_hash)?;
    let price_file = runtime_dir.join("market-prices.jsonl");
    let prefix = crate::native_dip_host_v1::sealed_native_prefix(&price_file)?;
    let data = fs::read(&price_file).map_err(|e| e.to_string())?;
    // Buffer.subarray(0, prefix.bytes) — silent truncation, no TRUNCATED check here.
    let bytes = data[..(prefix.bytes as usize).min(data.len())].to_vec();
    let price_rows = shared::verify_chained_rows(&bytes)?;
    if let Some(b) = sources["bindings"].as_object_mut() {
        b.insert("prices".into(), json!({"bytes": prefix.bytes, "hash": shared::sha(&bytes),
            "dev": prefix.dev, "ino": prefix.ino}));
    }
    let stat = fs::metadata(&price_file).map_err(|e| e.to_string())?;
    let (dev, ino) = crate::native_dip_host_v1::file_dev_ino(&stat);
    if dev != prefix.dev || ino != prefix.ino || stat.len() < prefix.bytes {
        return Err("COUPLED_BULK_REPLAY_PRICE_REPLACED".into());
    }
    let mut events = shared::stable_maker_events(&sources, bank)?;
    for row in &price_rows {
        // row.event?.type!=='market_price_observation' -> throw
        if row["event"]["type"].as_str() != Some("market_price_observation") {
            return Err("COUPLED_BULK_REPLAY_PRICE_TYPE".into());
        }
        let mut ev = serde_json::Map::new();
        ev.insert("kind".into(), json!("prices"));
        if let Some(at) = row.get("at") { ev.insert("availableAt".into(), at.clone()); }
        // snapshot:row.event.snapshot — a missing key is dropped by JSON.stringify;
        // an explicit null still serializes.
        if let Some(snap) = row["event"].get("snapshot") {
            ev.insert("snapshot".into(), snap.clone());
        }
        events.push(V::Object(ev));
    }
    shared::sort_events(&mut events);
    let loaded = std::time::Instant::now();
    let replay = replay_coupled_bulk_events(&events, bank)?;
    let finished = std::time::Instant::now();
    let mut record = shared::base_record("coupled_bulk_actual_capture_development_replay_v2",
        bank, &sources, &events, &replay, started, loaded, finished);
    record.insert("fills".into(), replay["state"]["wallets"]["COUPLED"]["fills"].clone());
    record.insert("redemptions".into(), replay["state"]["wallets"]["COUPLED"]["redemptions"].clone());
    record.insert("networkRequests".into(), json!(0));
    record.insert("competitionOrders".into(), json!(0));
    record.insert("fees".into(), json!(0));
    record.insert("profitabilityEstablished".into(), json!(false));
    record.insert("limitation".into(), json!("Historical development diagnostic on original sequential receipts. The strategy was implemented after these captures; strict-through fills are counterfactual and cannot prove production profitability."));
    shared::write_record(out, record)
}
