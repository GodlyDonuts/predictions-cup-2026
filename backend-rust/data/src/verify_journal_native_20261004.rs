// data/verify-journal-native-20261004.mjs — native journal live verification.
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::ledger::payload_hash;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("JOURNAL_LIVE_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let old = read(&root.join("data/journal-native-preservation-20261004.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let runtime = read(&r.join("stats.json"))?["runtime"].clone();
    let config = baal_collector::collector_journal_native::collector_native_journal_config(&r)?
        .unwrap_or(V::Null);
    need(runtime["pid"] == pids["collector"] && runtime["status"].as_str() == Some("running")
        && runtime["cycles"].as_i64().unwrap_or(0) > 0
        && runtime["originalJournal"]["revision"].as_i64() == Some(2)
        && runtime["bookPublisher"]["revision"].as_i64() == Some(3), "RUNNING")?;
    need(pids["paper"] == old["pids"]["paper"] && pids["dashboard"] == old["pids"]["dashboard"]
        && payload_hash(&config).map_err(|e| e.to_string())? == old["configHash"].as_str().unwrap_or("")
        && payload_hash(&read(&r.join("collector-native-publication.json"))?)
            .map_err(|e| e.to_string())? == old["publicProducerConfigHash"].as_str().unwrap_or(""),
        "PRODUCER_OR_WORKER_CHANGED")?;
    let startup = runtime["originalJournal"]["native"].clone();
    need(startup["sourceBytes"] == old["source"]["bytes"]
        && startup["sourceHash"] == old["source"]["sha256"]
        && startup["sequence"] == old["source"]["records"]
        && startup["tailHash"] == old["source"]["tailHash"]
        && startup["lastBodyHash"] == old["source"]["lastBodyHash"]
        && startup["engineHash"] == config["nativeBackend"]["engineHash"]
        && startup["binaryHash"] == config["nativeBackend"]["binaryHash"], "STARTUP_PARITY")?;
    need(baal_collector::native_journal_restore::source_prefix_hash(
        &r.join("journal.jsonl"), old["source"]["bytes"].as_u64().unwrap_or(0), None)?
        == old["source"]["sha256"].as_str().unwrap_or(""), "ORIGINAL_CHANGED")?;
    let archive = r.join("collector-journal-native-sources")
        .join(config["sourceSnapshot"].as_str().unwrap_or(""));
    let manifest = read(&archive.join("manifest.json"))?;
    need(payload_hash(&manifest["files"]).map_err(|e| e.to_string())?
        == config["sourceSnapshot"].as_str().unwrap_or(""), "ARCHIVE")?;
    for (name, hash) in manifest["files"].as_object().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(archive.join(&name)).map_err(|e| e.to_string())?)
            == hash.as_str().unwrap_or(""), "ARCHIVED_SOURCE")?;
    }
    need(baal_collector::native_journal_restore::journal_engine_identity(
        &r.join("journal-native-sources")
            .join(config["nativeBackend"]["engineHash"].as_str().unwrap_or("")))?
        == config["nativeBackend"]["engineHash"].as_str().unwrap_or(""), "COMPILED_SOURCE")?;
    for (type_name, prior) in old["preservation"].as_object().cloned().unwrap_or_default() {
        let active = read(&r.join(format!("{}-active.json", type_name)))?;
        let dir = r.join(format!("{}-cohorts", type_name))
            .join(active["cohort"].as_str().unwrap_or(""));
        need(active["cohort"] == prior["cohort"] && active["bankHash"] == prior["bankHash"]
            && sha(&std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?)
                == prior["bankFileHash"].as_str().unwrap_or("")
            && baal_collector::native_journal_restore::source_prefix_hash(
                &dir.join("journal.jsonl"),
                prior["journalPrefixBytes"].as_u64().unwrap_or(0), None)?
                == prior["journalPrefixHash"].as_str().unwrap_or(""), "WALLET_RESET")?;
    }
    let budget = read(&r.join("coordinated-network-budget.json"))?;
    need(budget["schemaVersion"].as_i64() == Some(3)
        && budget["maxReadsPerMinute"].as_i64() == Some(20)
        && budget["writesEnabled"] == json!(false)
        && budget["executionOwner"].as_str() == Some("Daenerys")
        && read(&r.join("competition-activation.json"))?["enabled"] == json!(false),
        "AUTHORITY")?;
    need(!root.join("STOP").exists() && !r.join("STOP").exists()
        && !r.join("COLLECTOR_STOP").exists(), "STOP")?;
    let report = json!({"at": ms_to_iso(now_ms()),
        "status": "running_native_original_journal", "pids": pids, "source": old["source"],
        "startup": runtime["originalJournal"],
        "publicationStartupMs": runtime["bookPublisher"]["startupReplayMs"],
        "sourceFiles": manifest["files"].as_object().map(|m| m.len() as i64).unwrap_or(0),
        "cycles": runtime["cycles"], "lastCycleAt": runtime["lastCycleAt"],
        "originalPrefixPreserved": true, "walletsAndBanksPreserved": true,
        "accountingReset": false, "extraPersistentProcesses": 0, "extraReadSlots": 0,
        "competitionOrders": 0, "profitabilityEstablished": false});
    atomic_json_canonical(&root.join("data/journal-native-live-20261004.json"), &report)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&report));
    Ok(report)
}
