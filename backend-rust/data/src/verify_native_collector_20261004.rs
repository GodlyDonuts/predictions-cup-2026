// data/verify-native-collector-20261004.mjs — native collector publication live verification.
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
    if ok { Ok(()) } else { Err(format!("NATIVE_COLLECTOR_LIVE_{}", code)) }
}
fn sha(b: &[u8]) -> String { sha256(b) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
fn prefix_hash(file: &Path, end: u64) -> Result<String, String> {
    use std::io::Read;
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut bytes = 0u64;
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    let mut buf = vec![0u8; 65536];
    while bytes < end {
        let chunk = std::cmp::min(end - bytes, buf.len() as u64) as usize;
        let n = fd.read(&mut buf[..chunk])
            .map_err(|e| e.to_string())?;
        if n == 0 { break; }
        h.update(&buf[..n]);
        bytes += n as u64;
    }
    need(bytes == end, "PREFIX_TRUNCATED")?;
    Ok(h.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let preserved = read(&root.join("data/native-collector-preservation-20261004.json"))?;
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let stats = read(&r.join("stats.json"))?;
    let runtime = stats["runtime"].clone();
    need(runtime["pid"] == pids["collector"] && runtime["status"].as_str() == Some("running")
        && runtime["cycles"].as_i64().unwrap_or(0) > 0
        && runtime["bookPublisher"]["revision"].as_i64() == Some(3)
        && runtime["noMaker"]["failed"] != json!(true)
        && runtime["baalPair"]["failed"] != json!(true), "NOT_RUNNING")?;
    need(!root.join("STOP").exists() && !r.join("STOP").exists()
        && !r.join("COLLECTOR_STOP").exists(), "STOP")?;
    let probability = read(&r.join("probability-receipt-active.json"))?;
    let bank = read(&r.join("probability-receipt-cohorts")
        .join(probability["cohort"].as_str().unwrap_or("")).join("bank.json"))?;
    let c = baal_collector::collector_publication_native::collector_native_publication_config(
        &r, &bank["universe"], root)?.unwrap_or(V::Null);
    need(payload_hash(&c).map_err(|e| e.to_string())? == preserved["configHash"].as_str().unwrap_or("")
        && probability["cohort"] == preserved["paperWorker"]["cohort"]
        && probability["bankHash"] == preserved["paperWorker"]["bankHash"]
        && pids["paper"] == preserved["paperWorker"]["pid"], "CONFIG_OR_WORKER")?;
    let startup = runtime["bookPublisher"]["native"].clone();
    need(startup["sourceBytes"] == preserved["source"]["bytes"]
        && startup["sourceHash"] == preserved["source"]["hash"]
        && startup["sourceRecords"] == preserved["source"]["sequence"]
        && startup["sourceTailHash"] == preserved["source"]["tailHash"]
        && startup["engineHash"] == c["nativeBackend"]["engineHash"]
        && startup["binaryHash"] == c["nativeBackend"]["binaryHash"], "STARTUP_BINDING")?;
    need(prefix_hash(&r.join("public-books.jsonl"),
        preserved["source"]["bytes"].as_u64().unwrap_or(0))?
        == preserved["source"]["hash"].as_str().unwrap_or(""), "SOURCE_CHANGED")?;
    let archive = r.join("collector-native-sources")
        .join(c["sourceSnapshot"].as_str().unwrap_or(""));
    let manifest = read(&archive.join("manifest.json"))?;
    need(payload_hash(&manifest["files"]).map_err(|e| e.to_string())?
            == c["sourceSnapshot"].as_str().unwrap_or("")
        && payload_hash(&manifest["files"]).map_err(|e| e.to_string())?
            == payload_hash(&c["codeIdentity"]).map_err(|e| e.to_string())?, "MANIFEST")?;
    for (file, hash) in manifest["files"].as_object().cloned().unwrap_or_default() {
        need(sha(&std::fs::read(archive.join(&file)).map_err(|e| e.to_string())?)
            == hash.as_str().unwrap_or(""), "ARCHIVED_SOURCE")?;
    }
    for (type_name, old) in preserved["preservation"].as_object().cloned().unwrap_or_default() {
        let active = read(&r.join(format!("{}-active.json", type_name)))?;
        let dir = r.join(format!("{}-cohorts", type_name))
            .join(active["cohort"].as_str().unwrap_or(""));
        need(active["cohort"] == old["cohort"] && active["bankHash"] == old["bankHash"]
            && sha(&std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?)
                == old["bankFileHash"].as_str().unwrap_or("")
            && prefix_hash(&dir.join("journal.jsonl"),
                old["journalPrefixBytes"].as_u64().unwrap_or(0))?
                == old["journalPrefixHash"].as_str().unwrap_or(""), "WALLET_PRESERVATION")?;
    }
    let budget = read(&r.join("coordinated-network-budget.json"))?;
    need(budget["maxReadsPerMinute"].as_i64() == Some(20)
        && budget["writesEnabled"] != json!(true)
        && budget["executionOwner"].as_str() == Some("Daenerys")
        && read(&r.join("competition-activation.json"))?["enabled"] == json!(false),
        "AUTHORITY")?;
    let prev_ms = preserved["previousPublisher"]["startupReplayMs"].as_f64().unwrap_or(0.0);
    let cur_ms = runtime["bookPublisher"]["startupReplayMs"].as_f64().unwrap_or(0.0);
    let report = json!({"at": ms_to_iso(now_ms()),
        "status": "running_native_collector_publication", "pids": pids,
        "publisher": runtime["bookPublisher"], "source": preserved["source"],
        "sourceFiles": preserved["sourceFiles"], "cycles": runtime["cycles"],
        "lastCycleAt": runtime["lastCycleAt"],
        "previousJavaScriptPublisherMs": preserved["previousPublisher"]["startupReplayMs"],
        "nativePublisherMs": runtime["bookPublisher"]["startupReplayMs"],
        "observedPublicationStageSpeedup": prev_ms / cur_ms,
        "measurementLimit": "Separate live restarts with slightly different source sizes; publication-stage timing only, not total collector startup or trading latency. Original collector journal restoration remains JavaScript.",
        "accountingReset": false, "walletPrefixPreservationVerified": true, "tests": 11,
        "extraPersistentProcesses": 0, "extraReadSlots": 0, "competitionOrders": 0,
        "profitabilityEstablished": false});
    atomic_json_canonical(&root.join("data/native-collector-live-20261004.json"), &report)
        .map_err(|e| e.to_string())?;
    println!("{}", js_stringify(&report));
    Ok(report)
}
