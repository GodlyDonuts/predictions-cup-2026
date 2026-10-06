// data/prepare-probability-owned-risk-20261004.mjs — carry exact accounting
// into the owned-risk probability v2 cohort.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::model::digest;
use baal_collector::probability_continuation::{
    export_probability_opening, probability_continuation_bundle,
};
use baal_collector::probability_receipt_host_v2::open_probability_receipt;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::Path;

pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let marker = read(&format!("{rt}/COLLECTOR_STOP"))?;
    if marker["source"] != json!("probability-owned-risk-20261004") {
        return Err("Engineering marker provenance".into());
    }
    for f in [
        "STOP".to_string(),
        format!("{rt}/STOP"),
        format!("{rt}/../bot.lock"),
    ] {
        if Path::new(&f).exists() { return Err("Human STOP or collector ownership".into()); }
    }
    if read(&format!("{rt}/competition-activation.json"))?["enabled"] != json!(false) {
        return Err("Contest activation".into());
    }
    let active = read(&format!("{rt}/probability-receipt-active.json"))?;
    let old_dir = format!("{rt}/probability-receipt-cohorts/{}", active["cohort"].as_str().unwrap_or(""));
    let old_stats = read(&format!("{old_dir}/stats.json"))?;
    if active["cohort"] != json!("source-probability-20261003-v1")
        || old_stats["failed"] == json!(true) || old_stats["sourceClosed"] != json!(true)
    { return Err("Predecessor has not consumed closed source".into()); }
    let cohort = "source-probability-owned-risk-20261004-v2";
    let next_dir = format!("{rt}/probability-receipt-cohorts/{cohort}");
    let existing = Path::new(&format!("{next_dir}/bank.json")).exists();
    let opening = if existing {
        read(&format!("{next_dir}/opening-state.json"))?
    } else {
        export_probability_opening(Path::new(&rt), active["cohort"].as_str().unwrap_or(""),
            Path::new("live-paper"))?
    };
    if digest(&opening["priorBank"]) != active["bankHash"].as_str().unwrap_or("")
        || opening["journalHash"] != old_stats["journalHash"]
        || digest(&opening["state"]) != old_stats["stateHash"].as_str().unwrap_or("")
    { return Err("Published predecessor frontier".into()); }
    if sha(&std::fs::read(format!("{old_dir}/bank.json")).map_err(|e| e.to_string())?) != opening["priorBankFileHash"].as_str().unwrap_or("")
        || sha(&std::fs::read(format!("{old_dir}/journal.jsonl")).map_err(|e| e.to_string())?) != opening["journalFileHash"].as_str().unwrap_or("")
        || sha(&std::fs::read(format!("{rt}/public-books.jsonl")).map_err(|e| e.to_string())?) != opening["sourcePrefixHash"].as_str().unwrap_or("")
    { return Err("Sealed predecessor bytes changed".into()); }
    let index_file = format!("{rt}/frozen-code/index.json");
    let mut index = read(&index_file)?;
    if index["banks"][payload_hash(&opening["priorBank"])?].is_null() {
        return Err("Original frozen source missing".into());
    }
    let bundle = probability_continuation_bundle(&opening)?;
    let now = std::sync::Arc::new(|| now_ms() as f64);
    let mut w = open_probability_receipt(
        Path::new(&rt), cohort, !existing, Some(&bundle), now,
        Path::new("."), Path::new("live-paper"))?;
    let bank = w.bank.clone();
    let status = w.status()?;
    w.close()?;
    let files = collect_frozen_paper_files(Path::new("."), &[
        "live-paper/probability-receipt-host-v2.mjs".to_string(),
        "live-paper/probability-receipt-observe-v2.mjs".to_string(),
        "live-paper/probability-continuation.mjs".to_string()])?;
    let files_v = json!(files.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<String, V>>());
    let snapshot = payload_hash(&files_v)?;
    let root = format!("{rt}/frozen-code/{snapshot}");
    if Path::new(&root).exists() { return Err("Snapshot collision; inspect".into()); }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    for name in files.keys() {
        let file = format!("{root}/{name}");
        std::fs::create_dir_all(Path::new(&file).parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::OpenOptions::new().write(true).create_new(true).open(&file)
            .and_then(|mut f| std::fs::read(name).and_then(|b| f.write_all(&b).map(|_| b)))
            .map_err(|e| e.to_string())?;
    }
    atomic_json(Path::new(&format!("{root}/manifest.json")), &json!({
        "schemaVersion": 1, "files": files_v,
    }))?;
    index["banks"][payload_hash(&bank)?] = json!({"snapshot": snapshot});
    atomic_json(Path::new(&index_file), &index)?;
    atomic_json(Path::new(&format!("{}/stats.json", w.dir.to_string_lossy())), &status)?;
    let preserved = json!({
        "at": ms_to_iso(now_ms() as f64), "cohort": active["cohort"],
        "bankHash": active["bankHash"], "bankFileHash": opening["priorBankFileHash"],
        "journalBytes": opening["journalBytes"], "journalFileHash": opening["journalFileHash"],
        "journalSequence": opening["journalSequence"], "journalHash": opening["journalHash"],
        "stateHash": digest(&opening["state"]),
    });
    let wallets = json!(status["wallets"].as_object().map(|o| o.iter().map(|(latency, s)| (
        latency.clone(), json!({
            "cash": s["cash"], "realizedPnl": s["realizedPnl"], "fees": s["fees"],
            "fills": s["fills"], "closedTrades": s["closedTrades"],
            "positions": s["positions"].as_array().map(|a| a.iter().map(|p| json!({
                "instrument": p["instrument"], "quantity": p["quantity"], "cost": p["cost"],
            })).collect::<Vec<V>>()).unwrap_or_default(),
            "liquidationPnl": s["liquidationPnl"],
        }))
    ).collect::<serde_json::Map<String, V>>()).unwrap_or_default());
    atomic_json(Path::new(&format!("{old_dir}/preserved-owned-risk-20261004.json")), &preserved)?;
    atomic_json(Path::new("data/probability-owned-risk-preservation-20261004.json"), &preserved)?;
    atomic_json(Path::new("data/probability-owned-risk-deployment-20261004.json"), &json!({
        "at": bank["cutoff"], "cohort": cohort, "bankHash": digest(&bank),
        "accountingContinuation": bank["accountingContinuation"],
        "frozenSourceSnapshot": snapshot, "frozenFiles": files.len(),
        "wallets": wallets, "competitionOrders": 0,
        "additionalPersistentProcesses": 0, "additionalReadSlots": 0,
        "profitabilityEstablished": false,
    }))?;
    Ok(json!({
        "cohort": cohort, "bankHash": digest(&bank),
        "sourceClosed": status["sourceClosed"], "wallets": wallets, "competitionOrders": 0,
    }))
}
