// live-paper/coupled-rust-transport-config.mjs — coupled bulk native-transport config.
use baal_execution::core::payload_hash;
use serde_json::{json, Value as V};
use std::fs;
use std::path::PathBuf;

use crate::native_journal_restore::{hex64, journal_engine_identity, native_journal_binding};

fn workspace_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }
/// /^[a-z][a-z0-9-]{1,70}$/ .test(v)
fn cohort_ok(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() >= 2 && s.len() <= 71
        && s.as_bytes()[0].is_ascii_lowercase()
        && s[1..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}
/// /^[1-9]\d*$/ .test(String(v)) — knownRiskMarketIds filter keeps string ids only.
fn id_string(v: &V) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && !s.starts_with('0')
        && s.bytes().all(|b| b.is_ascii_digit()))
}
/// coupledRustCollectorCodeIdentity()
pub fn coupled_rust_collector_code_identity() -> Result<V, String> {
    let files = crate::frozen_paper_graph::collect_frozen_paper_files(&workspace_dir(), &[
        "live-paper/coupled-rust-transport-config.mjs".to_string(),
        "live-paper/coupled-maker-host-rust.mjs".to_string(),
    ])?;
    Ok(json!(files.into_iter().map(|(k, v)| (k, V::String(v))).collect::<serde_json::Map<String, V>>()))
}
/// readCoupledRustTransport(runtimeDir, active)
pub fn read_coupled_rust_transport(runtime_dir: &std::path::Path, active: &V) -> Result<Option<V>, String> {
    let file = runtime_dir.join("coupled-bulk-rust-transport.json");
    if !file.exists() { return Ok(None); }
    let c: V = serde_json::from_str(&fs::read_to_string(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    const KEYS: [&str; 7] = ["schemaVersion", "mode", "cohort", "bankHash", "codeIdentity", "sourceSnapshot", "nativeBackend"];
    let keys_ok = c.as_object().is_some_and(|m| m.keys().all(|k| KEYS.contains(&k.as_str())));
    if !(keys_ok
        && c["schemaVersion"].as_i64() == Some(1)
        && c["mode"].as_str() == Some("original_coupled_wallet_rust_transport_v1")
        && c["cohort"] == active["cohort"]
        && c["bankHash"] == active["bankHash"]
        && hex64(&c["bankHash"])
        && payload_hash(&c["codeIdentity"]).ok() == payload_hash(&coupled_rust_collector_code_identity()?).ok()
        && payload_hash(&c["codeIdentity"]).ok().as_deref() == c["sourceSnapshot"].as_str()) {
        return Err("COUPLED_RUST_TRANSPORT_CONFIG".into());
    }
    native_journal_binding(runtime_dir, &c["nativeBackend"])?;
    if journal_engine_identity(&runtime_dir.join("journal-native-sources")
            .join(c["nativeBackend"]["engineHash"].as_str().unwrap_or("")))?
        != c["nativeBackend"]["engineHash"].as_str().unwrap_or("") {
        return Err("COUPLED_RUST_TRANSPORT_NATIVE_SOURCE".into());
    }
    Ok(Some(c))
}
/// coupledBulkStartupFailure(runtimeDir) — tolerant read that never throws.
pub fn coupled_bulk_startup_failure(runtime_dir: &std::path::Path) -> V {
    let mut prior = V::Null;
    let mut load = || -> Result<(), ()> {
        let active: V = serde_json::from_str(
            &fs::read_to_string(runtime_dir.join("coupled-bulk-active.json")).map_err(|_| ())?)
            .map_err(|_| ())?;
        if !cohort_ok(&active["cohort"]) { return Ok(()); }
        let stats: V = serde_json::from_str(
            &fs::read_to_string(runtime_dir.join("coupled-bulk-cohorts")
                .join(active["cohort"].as_str().unwrap()).join("stats.json")).map_err(|_| ())?)
            .map_err(|_| ())?;
        prior = stats;
        Ok(())
    };
    let _ = load();
    let known_risk: Vec<V> = prior["riskMarketIds"].as_array().cloned().unwrap_or_default()
        .into_iter().filter(id_string).collect();
    json!({"status": "failed", "failed": true, "executionAuthority": false,
        "accountingPreserved": true, "accountingVerified": false, "liabilitiesUnverified": true,
        "lastKnownAccounting": prior, "knownRiskMarketIds": known_risk,
        "reason": "COUPLED_STARTUP_FAILED_RECONCILIATION_REQUIRED"})
}
