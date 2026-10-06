// data/prepare-primary-breadth-paper-20261005.mjs — initialize the primary
// texas-governor probability cohort with a frozen host snapshot.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::probability_receipt_host::open_probability_receipt;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let cohort = "primary-texas-governor-20261005-v3";
    let active_file = format!("{rt}/primary-breadth-paper-active.json");
    for p in [
        "STOP".to_string(), format!("{rt}/STOP"), format!("{rt}/EXECUTION_STOP"),
        active_file.clone(),
        format!("{rt}/probability-receipt-cohorts/{cohort}/bank.json"),
    ] {
        if Path::new(&p).exists() {
            return Err("STOP or existing experimental bank; inspect before changing".into());
        }
    }
    let bundle_file = "research/current-polls-v3/texas-governor-paper-bundle-20261005-v3.json";
    let bundle = read_json(Path::new(bundle_file))?;
    if bundle["research"].as_array().map(|a| a.len()).unwrap_or(0) != 1
        || bundle["research"][0]["marketId"] != json!("195")
        || bundle["research"][0]["exchangeId"] != json!("884")
        || bundle["research"][0]["method"] != json!("subjective_range")
        || (now_ms() as f64) >= parse_iso_ms(bundle["research"][0]["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0)
    { return Err("Wrong or expired research".into()); }
    let mut preserved = Vec::new();
    for name in ["probability-receipt-active.json", "calibrated-poll-paper-active.json"] {
        let pointer = read_json(Path::new(&format!("{rt}/{name}")))?;
        let dir = format!("{rt}/probability-receipt-cohorts/{}", pointer["cohort"].as_str().unwrap_or(""));
        preserved.push(json!({
            "pointer": name, "cohort": pointer["cohort"], "bankHash": pointer["bankHash"],
            "bankFileHash": sha(&std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?),
            "journalPrefixBytes": std::fs::metadata(format!("{dir}/journal.jsonl")).map_err(|e| e.to_string())?.len(),
            "journalPrefixHash": sha(&std::fs::read(format!("{dir}/journal.jsonl")).map_err(|e| e.to_string())?),
        }));
    }
    let files = collect_frozen_paper_files(Path::new("."), &[
        "live-paper/probability-receipt-host.mjs".to_string()])?;
    let files_v = json!(files.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<String, V>>());
    let snapshot = payload_hash(&files_v)?;
    let root = format!("{rt}/frozen-code/{snapshot}");
    let index_file = format!("{rt}/frozen-code/index.json");
    let mut index = read_json(Path::new(&index_file))?;
    if !Path::new(&root).exists() {
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        for name in files.keys() {
            let target = PathBuf::from(format!("{root}/{name}"));
            std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                .and_then(|mut f| std::fs::read(name).and_then(|b| f.write_all(&b).map(|_| b)))
                .map_err(|e| e.to_string())?;
        }
        atomic_json(Path::new(&format!("{root}/manifest.json")), &json!({
            "schemaVersion": 1, "files": files_v,
        }))?;
    } else {
        let manifest = read_json(Path::new(&format!("{root}/manifest.json")))?;
        if payload_hash(&manifest["files"])? != snapshot {
            return Err("Archive identity".into());
        }
        for (name, hash) in &files {
            if sha(&std::fs::read(format!("{root}/{name}")).map_err(|e| e.to_string())?) != *hash {
                return Err("Archive bytes".into());
            }
        }
    }
    let now = || crate::publication_pack::now_ms() as f64;
    let mut w = open_probability_receipt(
        Path::new(&rt), cohort, true, Some(&bundle), now, Path::new("."))?;
    let bank = w.bank.clone();
    let status = w.status();
    w.close()?;
    let recheck = collect_frozen_paper_files(Path::new("."), &[
        "live-paper/probability-receipt-host.mjs".to_string()])?;
    let recheck_v = json!(recheck.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<String, V>>());
    if payload_hash(&recheck_v)? != snapshot {
        return Err("Reducer changed during initialization".into());
    }
    index["banks"][payload_hash(&bank)?] = json!({"snapshot": snapshot});
    atomic_json(Path::new(&index_file), &index)?;
    let _frozen = frozen_paper_module(Path::new(&rt), &bank, "live-paper/probability-receipt-host.mjs")?;
    atomic_json(Path::new(&format!("{}/stats.json", w.dir.to_string_lossy())), &status)?;
    atomic_json(Path::new(&active_file), &json!({
        "cohort": cohort, "bankHash": status["bankHash"],
    }))?;
    let wallets = json!(status["wallets"].as_object().map(|o| o.iter().map(|(k, v)| (
        k.clone(), json!({
            "cash": v["cash"], "realizedPnl": v["realizedPnl"], "fills": v["fills"],
            "closedTrades": v["closedTrades"], "positions": v["positions"],
        })
    )).collect::<serde_json::Map<String, V>>()).unwrap_or_default());
    atomic_json(Path::new("data/primary-breadth-paper-initialization-20261005.json"), &json!({
        "at": ms_to_iso(now_ms() as f64), "cohort": cohort,
        "bankHash": status["bankHash"], "canonicalIndexKey": payload_hash(&bank)?,
        "snapshot": snapshot, "frozenFiles": files.len(), "cutoff": bank["cutoff"],
        "researchId": bundle["research"][0]["id"],
        "researchBundleHash": sha(&std::fs::read(bundle_file).map_err(|e| e.to_string())?),
        "researchExpiresAt": bundle["research"][0]["expiresAt"], "markets": ["195"],
        "config": {"fees": 0, "shares": 1, "maximumMarketCost": "1.000000",
            "latenciesMs": bank["policy"]["latenciesMs"],
            "maxHoldMs": bank["policy"]["maxHoldMs"]},
        "wallets": wallets, "preserved": preserved, "competitionOrders": 0,
        "additionalPersistentServices": 0, "additionalReadSlots": 0,
        "profitabilityEstablished": false,
    }))?;
    Ok(json!({
        "cohort": cohort, "bankHash": status["bankHash"], "cutoff": bank["cutoff"],
        "sourceClosed": status["sourceClosed"], "competitionOrders": 0,
    }))
}
