// data/prepare-funded-settlement-paper-20261005.mjs — initialize the funded
// settlement paper cohort and freeze its code snapshot.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::model::digest;
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::settlement_paper_host::SettlementPaperHost;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let cohort = "funded-kansas-settlement-20261005-v1";
    let active_file = format!("{rt}/settlement-paper-active.json");
    for p in [
        "STOP".to_string(),
        format!("{rt}/STOP"),
        format!("{rt}/EXECUTION_STOP"),
        active_file.clone(),
        format!("{rt}/settlement-paper-cohorts/{cohort}/bank.json"),
    ] {
        if Path::new(&p).exists() {
            return Err("STOP or existing experimental bank; inspect before changing".into());
        }
    }
    let bundle_file = "research/current-election/funded-settlement-bundle-20261005-v1.json";
    let bundle = read_json(Path::new(bundle_file))?;
    let research = bundle["research"].as_array().and_then(|a| a.first()).cloned().unwrap_or(V::Null);
    if research["marketId"] != json!("356") || research["exchangeId"] != json!("1045")
        || research["method"] != json!("subjective_range")
        || now_ms() >= baal_service_runtime::contract::parse_iso_ms(research["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0) as i64
    {
        return Err("Wrong or expired research".into());
    }
    let model_file = "research/current-election/settlement-model-result-20261005-v1.json";
    let result = read_json(Path::new(model_file))?;
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
    let files = collect_frozen_paper_files(Path::new("."), &["live-paper/settlement-paper-host.mjs".to_string()])?;
    let snapshot = payload_hash(&json!(files.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<_, _>>()))?;
    let root = format!("{rt}/frozen-code/{snapshot}");
    let index_file = format!("{rt}/frozen-code/index.json");
    let mut index = read_json(Path::new(&index_file))?;
    if !Path::new(&root).exists() {
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        for (name, hash) in &files {
            let target = format!("{root}/{name}");
            std::fs::create_dir_all(Path::new(&target).parent().unwrap()).map_err(|e| e.to_string())?;
            let bytes = std::fs::read(name).map_err(|e| e.to_string())?;
            if sha(&bytes) != *hash { return Err("Archive bytes".into()); }
            write_exclusive(Path::new(&target), &bytes)?;
        }
        atomic_json(Path::new(&format!("{root}/manifest.json")), &json!({
            "schemaVersion": 1,
            "files": files.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<_, _>>(),
        }))?;
    } else {
        let manifest = read_json(Path::new(&format!("{root}/manifest.json")))?;
        if payload_hash(&manifest["files"])? != snapshot { return Err("Archive identity".into()); }
        for (name, hash) in &files {
            if sha(&std::fs::read(format!("{root}/{name}")).map_err(|e| e.to_string())?) != *hash {
                return Err("Archive bytes".into());
            }
        }
    }
    let native_backend = read_json(Path::new("data/settlement-value-milestone-20261004.json"))?["nativeBackend"].clone();
    let bundle_opt = json!({"research": bundle["research"], "identities": bundle["identities"], "nativeBackend": native_backend});
    let mut host = SettlementPaperHost::open(Path::new(&rt), cohort, true, Some(&bundle_opt), Path::new("."), std::sync::Arc::new(|| now_ms()), Path::new("live-paper"))?;
    let open_result = (|| {
        let bank = host.bank.clone();
        let stats = host.status()?;
        host.publish()?;
        Ok::<(V, V), String>((bank, stats))
    })();
    let _ = host.close();
    let (bank, stats) = open_result?;
    if payload_hash(&json!(collect_frozen_paper_files(Path::new("."), &["live-paper/settlement-paper-host.mjs".to_string()])?
        .iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<_, _>>()))? != snapshot
    {
        return Err("Code changed during initialization".into());
    }
    index.as_object_mut().unwrap()["banks"].as_object_mut().unwrap()
        .insert(payload_hash(&bank)?, json!({"snapshot": snapshot}));
    atomic_json(Path::new(&index_file), &index)?;
    frozen_paper_module(Path::new(&rt), &bank, "live-paper/settlement-paper-host.mjs")?;
    atomic_json(Path::new(&active_file), &json!({"cohort": cohort, "bankHash": digest(&bank)}))?;
    let evidence = json!({
        "at": ms_to_iso(now_ms() as f64), "cohort": cohort, "bankHash": digest(&bank),
        "canonicalIndexKey": payload_hash(&bank)?, "snapshot": snapshot,
        "frozenFiles": files.len(), "cutoff": bank["cutoff"], "researchId": research["id"],
        "researchHash": digest(&research), "researchSourceAt": research["sourceAt"],
        "researchRecordedAt": research["recordedAt"], "researchExpiresAt": research["expiresAt"],
        "lower": research["lower"], "upper": research["upper"],
        "severeOpponentAllocationStress": result["texasValuation"]["severeOpponentAllocationStress"],
        "modelResultHash": sha(&std::fs::read(model_file).map_err(|e| e.to_string())?),
        "nativeBackend": native_backend, "startup": stats["startup"], "preserved": preserved,
        "activationFileHash": sha(&std::fs::read(format!("{rt}/competition-activation.json")).map_err(|e| e.to_string())?),
        "competitionOrders": 0, "additionalReadSlots": 0, "additionalPersistentServices": 0,
        "profitabilityEstablished": false,
    });
    atomic_json(Path::new("data/funded-settlement-paper-initialization-20261005.json"), &evidence)?;
    Ok(json!({
        "cohort": cohort, "bankHash": digest(&bank), "cutoff": bank["cutoff"],
        "coldBootMs": stats["startup"]["coldBootMs"], "sourceClockPreserved": true, "orders": 0,
    }))
}
fn write_exclusive(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())
}
