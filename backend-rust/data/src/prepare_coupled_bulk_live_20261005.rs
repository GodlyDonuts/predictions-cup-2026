// data/prepare-coupled-bulk-live-20261005.mjs — declare the coupled-bulk live
// cohort bank over the whole validated universe and freeze its code snapshot.
use baal_collector::breadth::validate_universe;
use baal_collector::coupled_maker_host_v2::{coupled_bulk_code_identity, CoupledBulkHost};
use baal_collector::coupled_maker_paper_v2::coupled_bulk_policy;
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::stable_maker_replay_v2::verify_chained_rows;
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn sha(b: &[u8]) -> String { sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64
}
fn parse_iso_ms(s: &str) -> Option<f64> {
    baal_service_runtime::contract::parse_iso_ms(s)
}
fn write_wx(p: &Path, b: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600); }
    let mut f = o.open(p).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, b).map_err(|e| e.to_string())
}
fn mkdir_700(p: &Path) -> Result<(), String> {
    #[cfg(unix)] {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))] { std::fs::create_dir_all(p).map_err(|e| e.to_string()) }
}

pub fn run() -> Result<V, String> {
    let runtime_dir = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let workspace = std::env::current_dir().map_err(|e| e.to_string())?;
    let marker = read_json(&runtime_dir.join("COLLECTOR_STOP"))?;
    if marker["source"].as_str() != Some("coupled_bulk_paper_deployment_20261005") {
        return Err("Deployment provenance missing".into());
    }
    let name = "baal-coupled-bulk-20261005-v2c-live";
    let dir = runtime_dir.join("coupled-bulk-cohorts").join(name);
    if dir.exists() || runtime_dir.join("coupled-bulk-active.json").exists() {
        return Err("Existing cohort must be preserved".into());
    }
    let raw = read_json(Path::new("live-paper/price-universe.json"))?;
    let universe = validate_universe(&raw,
        raw["tournamentId"].as_str().unwrap_or(""))?;
    let mut prefix = serde_json::Map::new();
    for (key, file) in [("tape", "anonymous-trades.jsonl"),
            ("price", "market-prices.jsonl")] {
        let bytes = std::fs::read(runtime_dir.join(file)).map_err(|e| e.to_string())?;
        let rows = verify_chained_rows(&bytes)?;
        let tail = rows.last().cloned().unwrap_or(V::Null);
        // JS: tail?.sequence || 0, tail?.hash || ''
        prefix.insert(key.to_string(), json!({"bytes": bytes.len(),
            "hash": sha(&bytes),
            "sequence": if tail.is_null() { json!(0) } else { tail["sequence"].clone() },
            "tailHash": if tail.is_null() { json!("") } else { tail["hash"].clone() } }));
    }
    let cutoff = now_iso();
    let bank = json!({"schemaVersion": 1,
        "mode": "coupled_bulk_history_trade_tape_paper_v2", "name": name,
        "cutoff": cutoff,
        "entryExpiresAt": baal_service_runtime::contract::ms_to_iso(
            parse_iso_ms(&cutoff).unwrap_or(f64::NAN) + 21600000.0),
        "tournamentId": universe["tournamentId"], "universe": universe,
        "universeSourceJson": serde_json::to_string(&raw).map_err(|e| e.to_string())?,
        "legs": universe["exchanges"].as_array().cloned().unwrap_or_default()
            .iter().map(|x| json!({"marketId": x["marketId"],
                "exchangeId": x["exchangeId"]})).collect::<Vec<_>>(),
        "policy": coupled_bulk_policy(),
        "codeIdentity": coupled_bulk_code_identity()?,
        "tapePrefix": prefix["tape"], "pricePrefix": prefix["price"],
        "selectionMode": "entire_validated_universe_bulk_stability_then_fresh_depth",
        "independentFromPriorWallet": true, "counterfactualOnly": true,
        "executionAuthority": false, "competitionOrders": 0, "fees": 0,
        "profitabilityEstablished": false});
    let files = collect_frozen_paper_files(&workspace,
        &["live-paper/coupled-maker-host-v2.mjs".to_string()])?;
    let files_v: V = files.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let snapshot = payload_hash(&files_v)?;
    let frozen_root = runtime_dir.join("frozen-code").join(&snapshot);
    if !frozen_root.exists() {
        mkdir_700(&frozen_root)?;
        for (relative, hash) in &files {
            let target = frozen_root.join(relative);
            let resolved = std::fs::canonicalize(&frozen_root).map_err(|e| e.to_string())?;
            if !target.starts_with(&resolved) {
                return Err("Frozen scope".into());
            }
            if let Some(parent) = target.parent() { mkdir_700(parent)?; }
            let bytes = std::fs::read(relative).map_err(|e| e.to_string())?;
            if sha(&bytes) != *hash { return Err("Source changed".into()); }
            write_wx(&target, &bytes)?;
        }
        write_wx(&frozen_root.join("manifest.json"),
            format!("{}\n", serde_json::to_string_pretty(&json!({
                "schemaVersion": 1, "files": files_v})).map_err(|e| e.to_string())?)
            .as_bytes())?;
    }
    let index_file = runtime_dir.join("frozen-code").join("index.json");
    let mut index = read_json(&index_file)?;
    index["banks"].as_object_mut().ok_or("assertion failed")?
        .insert(payload_hash(&bank)?, json!({"snapshot": snapshot}));
    baal_service_runtime::atomic::atomic_json_canonical(&index_file, &index)?;
    mkdir_700(&dir)?;
    baal_service_runtime::atomic::atomic_json_canonical(&dir.join("bank.json"), &bank)?;
    baal_service_runtime::atomic::atomic_json_canonical(
        &runtime_dir.join("coupled-bulk-active.json"),
        &json!({"schemaVersion": 1, "cohort": name,
            "bankHash": payload_hash(&bank)?}))?;
    let host = CoupledBulkHost::open(&runtime_dir,
        Arc::new(|| now_ms()), Arc::new(|| true))?
        .ok_or("assertion failed")?;
    let status = host.status()?;
    let wallet = &status["wallets"]["COUPLED"];
    if !(wallet["fills"].is_null() || wallet["fills"].as_i64() == Some(0))
        || wallet["cash"].as_str() != Some("100.000000")
        || !(wallet["pairedRedemptions"].is_null()
            || wallet["pairedRedemptions"].as_i64() == Some(0)) {
        return Err("Unexpected new shared wallet state".into());
    }
    write_wx(Path::new("data/coupled-bulk-live-bank-v2c-20261005.json"),
        format!("{}\n", serde_json::to_string_pretty(&json!({
            "schemaVersion": 1, "at": cutoff,
            "bankHash": payload_hash(&bank)?, "snapshot": snapshot, "bank": bank,
            "initialPaperState": {"COUPLED": {"cash": wallet["cash"], "fills": 0,
                "pairedRedemptions": 0}},
            "originalWalletsReset": false, "newServices": 0,
            "officialActivationChanged": false})).map_err(|e| e.to_string())?)
        .as_bytes())?;
    Ok(json!({"cohort": name, "cutoff": cutoff, "bankHash": payload_hash(&bank)?,
        "scope": bank["legs"].as_array().map(|a| a.len()).unwrap_or(0),
        "frozenFiles": files.len(), "sourceFrontiers": status["sourceFrontiers"],
        "networkRequests": 0, "competitionOrders": 0}))
}
