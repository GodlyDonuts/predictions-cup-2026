// data/prepare-mn-source-value-paper-20261006.mjs — declare the Minnesota
// source-value probability paper cohort behind a frozen code snapshot.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::probability_receipt_host::open_probability_receipt;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::Path;

pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let cohort = "mn-republican-source-value-20261006-v1";
    let pointer_file = format!("{rt}/source-value-paper-active.json");
    for f in [
        "STOP".to_string(), format!("{rt}/STOP"), pointer_file.clone(),
        format!("{rt}/probability-receipt-cohorts/{cohort}/bank.json"),
    ] {
        if Path::new(&f).exists() { return Err("STOP or existing source experiment".into()); }
    }
    let bundle_file = "data/mn-source-value-paper-bundle-20261006-v1.json";
    let bundle = read_json(Path::new(bundle_file))?;
    let r = &bundle["research"][0];
    if bundle["research"].as_array().map(|a| a.len()).unwrap_or(0) != 1
        || r["marketId"] != json!("361") || r["exchangeId"] != json!("1050")
        || now_ms() as f64 >= parse_iso_ms(r["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0)
    { return Err("Wrong or expired source scope".into()); }
    let mut preserved = Vec::new();
    for p in ["probability-receipt-active.json", "calibrated-poll-paper-active.json",
        "primary-breadth-paper-active.json"] {
        let a = read_json(Path::new(&format!("{rt}/{p}")))?;
        let dir = format!("{rt}/probability-receipt-cohorts/{}", a["cohort"].as_str().unwrap_or(""));
        preserved.push(json!({
            "pointer": p, "cohort": a["cohort"], "bankHash": a["bankHash"],
            "bankFileHash": sha(&std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?),
        }));
    }
    let workspace = Path::new(".").canonicalize().map_err(|e| e.to_string())?;
    let files = collect_frozen_paper_files(&workspace, &[
        "live-paper/probability-receipt-host.mjs".to_string()])?;
    let files_v = json!(files.iter().map(|(k, v)| (k.clone(), V::String(v.clone())))
        .collect::<serde_json::Map<String, V>>());
    let snapshot = payload_hash(&files_v)?;
    let archive = format!("{rt}/frozen-code/{snapshot}");
    let index_file = format!("{rt}/frozen-code/index.json");
    let mut index = read_json(Path::new(&index_file))?;
    if !Path::new(&archive).exists() {
        std::fs::create_dir_all(&archive).map_err(|e| e.to_string())?;
        for file in files.keys() {
            let target = format!("{archive}/{file}");
            std::fs::create_dir_all(Path::new(&target).parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                .and_then(|mut f| std::fs::read(file).and_then(|b| f.write_all(&b).map(|_| b)))
                .map_err(|e| e.to_string())?;
        }
        atomic_json(Path::new(&format!("{archive}/manifest.json")), &json!({
            "schemaVersion": 1, "files": files_v,
        }))?;
    }
    for (file, hash) in &files {
        if sha(&std::fs::read(format!("{archive}/{file}")).map_err(|e| e.to_string())?) != *hash {
            return Err("Frozen source changed".into());
        }
    }
    let mut w = open_probability_receipt(Path::new(&rt), cohort, true, Some(&bundle),
        || crate::publication_pack::now_ms() as f64, &workspace)?;
    let bank = w.bank.clone();
    let status = w.status();
    w.close()?;
    let recheck = collect_frozen_paper_files(&workspace, &[
        "live-paper/probability-receipt-host.mjs".to_string()])?;
    let recheck_v = json!(recheck.iter().map(|(k, v)| (k.clone(), V::String(v.clone())))
        .collect::<serde_json::Map<String, V>>());
    if payload_hash(&recheck_v)? != snapshot { return Err("Reducer changed during freeze".into()); }
    index["banks"][payload_hash(&bank)?] = json!({"snapshot": snapshot});
    atomic_json(Path::new(&index_file), &index)?;
    frozen_paper_module(Path::new(&rt), &bank, "live-paper/probability-receipt-host.mjs")?;
    atomic_json(&w.dir.join("stats.json"), &status)?;
    atomic_json(Path::new(&pointer_file), &json!({
        "cohort": cohort, "bankHash": status["bankHash"],
    }))?;
    let proof = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "cohort": cohort,
        "bankHash": status["bankHash"], "canonicalIndexKey": payload_hash(&bank)?,
        "snapshot": snapshot, "frozenFiles": files.len(), "cutoff": bank["cutoff"],
        "researchId": r["id"], "researchBundleHash": sha(&std::fs::read(bundle_file).map_err(|e| e.to_string())?),
        "originalSourceAt": r["sourceAt"], "researchExpiresAt": r["expiresAt"],
        "entryValuation": r["lower"],
        "estimateMeaning": "Point estimate minus10pp judgment reserve, not a confidence lower bound. Known sensitivitymin0.115 and severe0 retained.",
        "policy": {"fees": 0, "shares": 1, "latenciesMs": bank["policy"]["latenciesMs"],
            "maxHoldMs": bank["policy"]["maxHoldMs"], "oneAdverseTick": true},
        "preserved": preserved, "workerStarted": false, "accountRequests": 0,
        "competitionOrders": 0, "profitabilityEstablished": false,
    });
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/mn-source-value-paper-prepared-20261006.json")
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&proof).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(proof)
}
