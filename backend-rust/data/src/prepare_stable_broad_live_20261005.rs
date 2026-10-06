// data/prepare-stable-broad-live-20261005.mjs — declare the broad stable-maker
// paper cohort over the verified anonymous-trades tape.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::stable_maker_host_v1::{stable_maker_code_identity, StableMakerHost};
use baal_collector::stable_maker_paper_v2::stable_maker_policy;
use baal_collector::stable_maker_replay_v2::verify_chained_rows;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};

fn freeze(workspace: &Path, rt: &str, modules: &[String]) -> Result<(V, String, usize), String> {
    let files = collect_frozen_paper_files(workspace, modules)?;
    let files_v = json!(files.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<String, V>>());
    let snapshot = payload_hash(&files_v)?;
    let frozen_root = format!("{rt}/frozen-code/{snapshot}");
    if !Path::new(&frozen_root).exists() {
        std::fs::create_dir_all(&frozen_root).map_err(|e| e.to_string())?;
        let root = Path::new(&frozen_root).canonicalize().map_err(|e| e.to_string())?;
        for (relative, hash) in &files {
            let target = root.join(relative);
            if !target.starts_with(&root) { return Err("Frozen scope".into()); }
            std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            let b = std::fs::read(relative).map_err(|e| e.to_string())?;
            if sha(&b) != *hash { return Err("Source changed".into()); }
            std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                .and_then(|mut f| f.write_all(&b)).map_err(|e| e.to_string())?;
        }
        std::fs::OpenOptions::new().write(true).create_new(true)
            .open(format!("{frozen_root}/manifest.json"))
            .and_then(|mut f| f.write_all(format!("{}\n",
                serde_json::to_string_pretty(&json!({"schemaVersion": 1, "files": files_v})).unwrap()).as_bytes()))
            .map_err(|e| e.to_string())?;
    }
    Ok((files_v, snapshot, files.len()))
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let marker = read_json(Path::new(&format!("{rt}/COLLECTOR_STOP")))?;
    if marker["source"] != json!("stable_broad_paper_deployment_20261005") {
        return Err("Deployment provenance missing".into());
    }
    let name = "baal-stable-broad-20261005-v2-live";
    let dir = format!("{rt}/stable-maker-cohorts/{name}");
    if Path::new(&dir).exists() || Path::new(&format!("{rt}/stable-maker-broad-active.json")).exists() {
        return Err("Existing stable cohort must be preserved".into());
    }
    let source = std::fs::read(format!("{rt}/anonymous-trades.jsonl")).map_err(|e| e.to_string())?;
    let rows = verify_chained_rows(&source)?;
    let tape = rows.last().cloned().unwrap_or(V::Null);
    let cutoff = ms_to_iso(now_ms() as f64);
    let declaration = read_json(Path::new("data/stable-broad-selection-20261005.json"))?;
    let n_candidates = declaration["candidates"].as_array().map(|a| a.len()).unwrap_or(0);
    if !(2..=16).contains(&n_candidates) {
        return Err("Insufficient screened broad scope".into());
    }
    let code_identity = stable_maker_code_identity(Path::new("."))?;
    let bank = json!({
        "schemaVersion": 1, "mode": "stable_two_side_trade_tape_paper_v2", "name": name,
        "cutoff": cutoff,
        "entryExpiresAt": ms_to_iso(parse_iso_ms(&cutoff).unwrap_or(0.0) + 21600000.0),
        "tournamentId": declaration["tournamentId"],
        "legs": declaration["candidates"].as_array().map(|a| a.iter().map(|x| json!({
            "marketId": x["marketId"], "exchangeId": x["exchangeId"],
        })).collect::<Vec<V>>()).unwrap_or_default(),
        "policy": stable_maker_policy(), "codeIdentity": code_identity,
        "tapePrefixBytes": source.len(), "tapePrefixHash": sha(&source),
        "tapePrefixSequence": tape["sequence"].as_u64().unwrap_or(0),
        "tapePrefixTailHash": tape["hash"].as_str().unwrap_or(""),
        "protocolFileHash": sha(&std::fs::read("data/stable-broad-selection-20261005.json").map_err(|e| e.to_string())?),
        "selectionPricePrefix": declaration["source"], "selectionMode": declaration["selection"],
        "independentFromPriorWallet": true, "counterfactualOnly": true,
        "executionAuthority": false, "competitionOrders": 0, "fees": 0,
        "profitabilityEstablished": false,
    });
    let (files_v, snapshot, file_count) = freeze(Path::new("."), &rt, &[
        "live-paper/stable-maker-host-v1.mjs".to_string()])?;
    let _ = files_v;
    let index_file = format!("{rt}/frozen-code/index.json");
    let mut index = read_json(Path::new(&index_file))?;
    index["banks"][payload_hash(&bank)?] = json!({"snapshot": snapshot});
    atomic_json(Path::new(&index_file), &index)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    atomic_json(Path::new(&format!("{dir}/bank.json")), &bank)?;
    atomic_json(Path::new(&format!("{rt}/stable-maker-broad-active.json")), &json!({
        "schemaVersion": 1, "cohort": name, "bankHash": payload_hash(&bank)?,
    }))?;
    let h = StableMakerHost::open(
        Path::new(&rt),
        std::sync::Arc::new(|| crate::publication_pack::now_ms()),
        std::sync::Arc::new(|| true),
        "stable-maker-broad-active.json", Path::new("."),
    )?.ok_or("HOST")?;
    let s = h.status()?;
    if s["wallets"]["YES"]["fills"] != json!(0) || s["wallets"]["NO"]["fills"] != json!(0)
        || s["wallets"]["YES"]["cash"] != json!("100.000000")
        || s["wallets"]["NO"]["cash"] != json!("100.000000")
    { return Err("Unexpected new wallet state".into()); }
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/stable-broad-live-bank-20261005.json")
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&json!({
            "schemaVersion": 1, "at": cutoff, "bankHash": payload_hash(&bank).unwrap_or_default(),
            "snapshot": snapshot, "bank": bank,
            "initialPaperState": {"YES": {"cash": s["wallets"]["YES"]["cash"], "fills": 0},
                "NO": {"cash": s["wallets"]["NO"]["cash"], "fills": 0}},
            "originalWalletsReset": false, "newServices": 0, "officialActivationChanged": false,
        })).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "cohort": name, "cutoff": cutoff, "bankHash": payload_hash(&bank)?,
        "frozenFiles": file_count, "sourcePages": rows.len(),
        "networkRequests": 0, "competitionOrders": 0,
    }))
}
