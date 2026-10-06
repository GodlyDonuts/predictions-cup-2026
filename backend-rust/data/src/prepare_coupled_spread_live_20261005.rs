// data/prepare-coupled-spread-live-20261005.mjs — declare the coupled spread
// live cohort bank over the screened broad selection.
use baal_collector::coupled_maker_host_v1::{coupled_maker_code_identity, CoupledMakerHost};
use baal_collector::coupled_maker_paper_v1::coupled_maker_policy;
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
    if marker["source"].as_str() != Some("coupled_spread_paper_deployment_20261005") {
        return Err("Deployment provenance missing".into());
    }
    let name = "baal-coupled-spread-20261005-v1-live";
    let dir = runtime_dir.join("coupled-maker-cohorts").join(name);
    if dir.exists() || runtime_dir.join("coupled-maker-active.json").exists() {
        return Err("Existing stable cohort must be preserved".into());
    }
    let source = std::fs::read(runtime_dir.join("anonymous-trades.jsonl"))
        .map_err(|e| e.to_string())?;
    let rows = verify_chained_rows(&source)?;
    let tape = rows.last().cloned().unwrap_or(V::Null);
    let cutoff = now_iso();
    let declaration = read_json(Path::new("data/stable-broad-selection-20261005.json"))?;
    let candidate_count = declaration["candidates"].as_array()
        .map(|a| a.len()).unwrap_or(0);
    if candidate_count < 2 || candidate_count > 16 {
        return Err("Insufficient screened broad scope".into());
    }
    let code_identity = coupled_maker_code_identity()?;
    let bank = json!({"schemaVersion": 1,
        "mode": "coupled_two_side_trade_tape_paper_v1", "name": name,
        "cutoff": cutoff,
        "entryExpiresAt": baal_service_runtime::contract::ms_to_iso(
            parse_iso_ms(&cutoff).unwrap_or(f64::NAN) + 21600000.0),
        "tournamentId": declaration["tournamentId"],
        "legs": declaration["candidates"].as_array().cloned().unwrap_or_default()
            .iter().map(|x| json!({"marketId": x["marketId"],
                "exchangeId": x["exchangeId"]})).collect::<Vec<_>>(),
        "policy": coupled_maker_policy(), "codeIdentity": code_identity,
        "tapePrefixBytes": source.len(), "tapePrefixHash": sha(&source),
        // JS: tape?.sequence || 0, tape?.hash || ''
        "tapePrefixSequence": if tape.is_null() { json!(0) }
            else { tape["sequence"].clone() },
        "tapePrefixTailHash": if tape.is_null() { json!("") }
            else { tape["hash"].clone() },
        "protocolFileHash": sha(&std::fs::read(
            "data/stable-broad-selection-20261005.json").map_err(|e| e.to_string())?),
        "selectionPricePrefix": declaration["source"],
        "selectionMode": declaration["selection"],
        "independentFromPriorWallet": true, "counterfactualOnly": true,
        "executionAuthority": false, "competitionOrders": 0, "fees": 0,
        "profitabilityEstablished": false});
    let files = collect_frozen_paper_files(&workspace,
        &["live-paper/coupled-maker-host-v1.mjs".to_string()])?;
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
        &runtime_dir.join("coupled-maker-active.json"),
        &json!({"schemaVersion": 1, "cohort": name,
            "bankHash": payload_hash(&bank)?}))?;
    let host = CoupledMakerHost::open(&runtime_dir,
        Arc::new(|| now_ms()), Arc::new(|| true), "coupled-maker-active.json")?
        .ok_or("assertion failed")?;
    let st = host.status()?;
    let w = &st["wallets"]["COUPLED"];
    if !(w["fills"].is_null() || w["fills"].as_i64() == Some(0))
        || w["cash"].as_str() != Some("100.000000")
        || !(w["pairedRedemptions"].is_null()
            || w["pairedRedemptions"].as_i64() == Some(0)) {
        return Err("Unexpected new shared wallet state".into());
    }
    write_wx(Path::new("data/coupled-spread-live-bank-20261005.json"),
        format!("{}\n", serde_json::to_string_pretty(&json!({
            "schemaVersion": 1, "at": cutoff,
            "bankHash": payload_hash(&bank)?, "snapshot": snapshot, "bank": bank,
            "initialPaperState": {"COUPLED": {"cash": w["cash"], "fills": 0,
                "pairedRedemptions": 0}},
            "originalWalletsReset": false, "newServices": 0,
            "officialActivationChanged": false,
            "primaryMechanism": {
                "url": "https://sig.thesuper.market/docs/markets-and-trading",
                "verifiedAt": cutoff,
                "rule": "Opposite purchased shares cancel existing shares and pay one SUSQie per pair immediately."}}))
            .map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"cohort": name, "cutoff": cutoff, "bankHash": payload_hash(&bank)?,
        "frozenFiles": files.len(), "sourcePages": rows.len(),
        "networkRequests": 0, "competitionOrders": 0}))
}
