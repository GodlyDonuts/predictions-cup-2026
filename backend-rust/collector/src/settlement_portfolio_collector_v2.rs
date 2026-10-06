// live-paper/settlement-portfolio-collector-v2.mjs — collector-side portfolio host binding.
use baal_execution::core::payload_hash;
use serde_json::Value as V;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn digest(v: &V) -> String { crate::model::digest(v) }
fn hex64(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }

/// openCollectorSettlementPortfolioV2({runtimeDir, universe, canOperate, now, workspaceDir})
/// Ok(None) mirrors the JS null when the pointer file is absent.
/// `live_paper_dir` stands in for import.meta.url (frozen-module + code identity).
pub fn open_collector_settlement_portfolio_v2(runtime_dir: &Path, universe: &V,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>, workspace_dir: &Path,
    live_paper_dir: &Path) -> Result<Option<crate::settlement_portfolio_host_v2::SettlementPortfolioHostV2>, String> {
    let file = runtime_dir.join("settlement-portfolio-v2-active.json");
    if !file.exists() { return Ok(None); }
    let p: V = serde_json::from_slice(&std::fs::read(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let cohort = p["cohort"].as_str().unwrap_or("");
    let cb = cohort.as_bytes();
    if p["schemaVersion"].as_i64() != Some(1)
        || !(cb.len() >= 1 && cb.len() <= 63
            && cb.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-'))
        || !hex64(&p["bankHash"])
    {
        return Err("SETTLEMENT_PORTFOLIO_POINTER".into());
    }
    let dir = runtime_dir.join("settlement-portfolio-v2-cohorts").join(cohort);
    let bank: V = serde_json::from_slice(&std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if digest(&bank) != p["bankHash"].as_str().unwrap_or("") {
        return Err("SETTLEMENT_PORTFOLIO_BANK".into());
    }
    let mut raw = bank["universe"].clone();
    let declared = raw.as_object_mut().and_then(|m| m.shift_remove("hash"));
    let mut live_raw = universe.clone();
    let live_declared = live_raw.as_object_mut().and_then(|m| m.shift_remove("hash"));
    let checked = crate::breadth::validate_universe(&raw, universe["tournamentId"].as_str().unwrap_or(""))?;
    let live = crate::breadth::validate_universe(&live_raw, universe["tournamentId"].as_str().unwrap_or(""))?;
    if (declared.as_ref().is_some_and(|h| baal_service_runtime::json_text::js_truthy(Some(h)))
            && declared.as_ref() != Some(&checked["hash"]))
        || (live_declared.as_ref().is_some_and(|h| baal_service_runtime::json_text::js_truthy(Some(h)))
            && live_declared.as_ref() != Some(&live["hash"]))
        || checked["hash"] != live["hash"]
    {
        return Err("SETTLEMENT_PORTFOLIO_UNIVERSE".into());
    }
    let binding = &bank["appendBackend"];
    if p["binaryHash"] != binding["binaryHash"] || p["engineHash"] != binding["engineHash"]
        || !hex64(&p["binaryHash"]) || !hex64(&p["engineHash"])
    {
        return Err("SETTLEMENT_PORTFOLIO_APPEND_BINDING".into());
    }
    let binary_path = runtime_dir.join("journal-append-native-builds")
        .join(p["binaryHash"].as_str().unwrap_or("")).join("baal-journal-append.exe");
    let source_root = runtime_dir.join("journal-append-native-sources")
        .join(p["engineHash"].as_str().unwrap_or(""));
    let root_resolved = runtime_dir.canonicalize().map_err(|e| e.to_string())?;
    if binding["dependencies"].as_array().is_some_and(|deps| deps.iter().any(|d| {
        PathBuf::from(d["path"].as_str().unwrap_or("")).canonicalize()
            .map(|r| !r.starts_with(&root_resolved)).unwrap_or(true)
    })) {
        return Err("SETTLEMENT_PORTFOLIO_DEPENDENCY_SCOPE".into());
    }
    // Frozen-module check: verify the snapshot, then run the crate's own host.
    let _frozen = crate::frozen_paper_runtime::frozen_paper_module(runtime_dir, &bank,
        "live-paper/settlement-portfolio-host-v2.mjs")?;
    let append = serde_json::json!({"binaryPath": binary_path.to_string_lossy(),
        "sourceRoot": source_root.to_string_lossy(), "binding": binding});
    let mut host = crate::settlement_portfolio_host_v2::SettlementPortfolioHostV2::open(runtime_dir,
        cohort, &append, now, can_operate, workspace_dir, live_paper_dir, 128)?;
    let status = host.status()?;
    if status["bankHash"] != p["bankHash"] || status["canonicalBankHash"].as_str() != Some(payload_hash(&bank)?.as_str()) {
        let _ = host.close();
        return Err("SETTLEMENT_PORTFOLIO_HOST_IDENTITY".into());
    }
    Ok(Some(host))
}
