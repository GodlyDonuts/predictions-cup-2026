// data/recover-pair-anchor-preparation-20261004.mjs — pair-anchor archive recovery preparation.
use baal_collector::baal_pair_host_v4::{baal_pair_code_identity, BaalPairHost};
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::native_journal_restore::source_prefix_hash;
use baal_service_runtime::atomic::atomic_json_canonical as atomic_json;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::ledger::payload_hash;
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn need(v: bool, c: &str) -> Result<(), String> {
    if !v { return Err(format!("PAIR_ARCHIVE_RECOVERY_{}", c)); }
    Ok(())
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn parse_iso(s: &str) -> f64 { baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN) }
fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}
// path.posix.normalize(path.posix.join('live-paper', f))
fn posix_norm(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => { out.pop(); }
            x => out.push(x),
        }
    }
    out.join("/")
}
fn guard() -> Result<bool, String> {
    let r = rt();
    Ok(read(&r.join("COLLECTOR_STOP"))?["source"].as_str() == Some("pair-anchor-deployment-20261004")
        && !r.join("..").join("bot.lock").exists()
        && !Path::new("STOP").exists()
        && !r.join("STOP").exists())
}
fn mkdir_700(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p).map_err(|e| e.to_string())
}
fn write_excl_600(f: &Path, b: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(f)
        .and_then(|mut fd| fd.write_all(b)).map_err(|e| e.to_string())
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    need(guard()?, "STOP_OR_LEASE")?;
    need(read(&r.join("competition-activation.json"))?["enabled"] == json!(false), "ACTIVATION")?;
    let active = read(&r.join("baal-pair-active.json"))?;
    need(active["hostRevision"].as_i64() == Some(4)
        && active["cohort"].as_str() == Some("baal-no-pair-anchor-20261004-v4"), "ACTIVE")?;
    let dir = r.join("baal-pair-cohorts").join(active["cohort"].as_str().unwrap_or(""));
    let bank = read(&dir.join("bank.json"))?;
    need(payload_hash(&bank).map_err(|e| e.to_string())? == active["bankHash"].as_str().unwrap_or("")
        && payload_hash(&bank["codeIdentity"]).map_err(|e| e.to_string())?
            == payload_hash(&baal_pair_code_identity(&root.join("live-paper"))).map_err(|e| e.to_string())?
        && !dir.join("journal.jsonl").exists() && !dir.join("stats.json").exists(), "UNSTARTED_FROZEN_BANK")?;
    let prior = r.join("baal-pair-cohorts").join(bank["accountingContinuation"]["cohort"].as_str().unwrap_or(""));
    let prior_bank = std::fs::read(prior.join("bank.json")).map_err(|e| e.to_string())?;
    let c = &bank["accountingContinuation"];
    let seed = std::fs::read(dir.join("opening-state.json")).map_err(|e| e.to_string())?;
    let old = Journal::open(&prior.join("journal.jsonl"))?;
    need(sha(&prior_bank) == c["bankFileHash"].as_str().unwrap_or("")
        && payload_hash(&serde_json::from_slice::<V>(&prior_bank).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?
            == c["bankHash"].as_str().unwrap_or("")
        && source_prefix_hash(&prior.join("journal.jsonl"), c["journalPrefixBytes"].as_u64().unwrap_or(0), None)?
            == c["journalPrefixHash"].as_str().unwrap_or("")
        && sha(&seed) == c["openingStateFileHash"].as_str().unwrap_or("")
        && payload_hash(&serde_json::from_slice::<V>(&seed).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?
            == payload_hash(&old.last.clone().unwrap_or(V::Null)["state"]["paper"]).map_err(|e| e.to_string())?,
        "UNCHANGED_ACCOUNTING")?;
    need(source_prefix_hash(&r.join("public-books.jsonl"), bank["sourcePrefixBytes"].as_u64().unwrap_or(0), None)?
            == bank["sourcePrefixHash"].as_str().unwrap_or("")
        && now_ms() < parse_iso(bank["review"]["expiresAt"].as_str().unwrap_or("")), "SOURCE_OR_REVIEW")?;
    let live_paper_dir = root.join("live-paper");
    let declared: Vec<String> = bank["codeIdentity"].as_object().cloned().unwrap_or_default().keys()
        .map(|f| posix_norm(&format!("live-paper/{}", f))).collect();
    let mut entries = vec!["live-paper/baal-pair-host-v4.mjs".to_string()];
    entries.extend(declared);
    let files = collect_frozen_paper_files(&std::env::current_dir().map_err(|e| e.to_string())?, &entries)?;
    let files_v = V::Object(files.iter().map(|(k, v)| (k.clone(), json!(v))).collect());
    let snapshot = payload_hash(&files_v).map_err(|e| e.to_string())?;
    let archive_root = r.join("frozen-code").join(&snapshot);
    mkdir_700(&archive_root)?;
    for (name, hash) in &files {
        let bytes = std::fs::read(name).map_err(|e| e.to_string())?;
        need(sha(&bytes) == *hash, "SOURCE_CHANGED")?;
        let file = archive_root.join(name);
        if let Some(parent) = file.parent() { mkdir_700(parent)?; }
        if file.exists() {
            need(sha(&std::fs::read(&file).map_err(|e| e.to_string())?) == *hash, "ARCHIVE_CHANGED")?;
        } else {
            write_excl_600(&file, &bytes)?;
        }
    }
    atomic_json(&archive_root.join("manifest.json"), &json!({"schemaVersion": 1, "files": files_v}))?;
    let index_file = r.join("frozen-code").join("index.json");
    let mut index = read(&index_file)?;
    let failed_snapshot = index["banks"][active["bankHash"].as_str().unwrap_or("")]["snapshot"].clone();
    need(failed_snapshot.is_string() && failed_snapshot != json!(snapshot), "EXPECTED_INCOMPLETE_SNAPSHOT")?;
    index["banks"][active["bankHash"].as_str().unwrap_or("").to_string()] = json!({"snapshot": snapshot});
    atomic_json(&index_file, &index)?;
    let _loaded = frozen_paper_module(&r, &bank, "live-paper/baal-pair-host-v4.mjs")?
        .unwrap_or_else(|| json!({"modulePath": "live-paper/baal-pair-host-v4.mjs", "fallback": true}));
    let host = BaalPairHost::open(&r,
            std::sync::Arc::new(|| now_ms() as i64), std::sync::Arc::new(|| true))?
        .ok_or("PAIR_ARCHIVE_RECOVERY_HOST_NULL")?;
    let opening = host.status();
    let prior_stats = read(&prior.join("stats.json"))?;
    need(!opening["failed"].as_bool().unwrap_or(false)
        && opening["cash"] == prior_stats["cash"] && opening["fills"] == prior_stats["fills"]
        && opening["realizedPnl"] == prior_stats["realizedPnl"] && guard()?, "EXACT_OPENING")?;
    atomic_json(&prior.join("STOP"), &json!({"source": "Codex corrected successor deployment",
        "at": bank["cutoff"], "successor": bank["name"], "reason": c["reason"]}))?;
    let mut untouched = serde_json::Map::new();
    for ty in ["no-maker", "probability-receipt"] {
        let a = read(&r.join(format!("{}-active.json", ty)))?;
        let d = r.join(format!("{}-cohorts", ty)).join(a["cohort"].as_str().unwrap_or(""));
        let f = d.join("journal.jsonl");
        let bytes = std::fs::metadata(&f).map_err(|e| e.to_string())?.len();
        untouched.insert(ty.to_string(), json!({"cohort": a["cohort"], "bankHash": a["bankHash"],
            "bankFileHash": sha(&std::fs::read(d.join("bank.json")).map_err(|e| e.to_string())?),
            "journalPrefixBytes": bytes as i64,
            "journalPrefixHash": source_prefix_hash(&f, bytes, None)?}));
    }
    let original_file = r.join("journal.jsonl");
    let bytes = std::fs::metadata(&original_file).map_err(|e| e.to_string())?.len();
    let report = json!({"at": bank["cutoff"], "cohort": bank["name"], "bankHash": active["bankHash"],
        "reviewExpiresAt": bank["review"]["expiresAt"],
        "policyUnchanged": payload_hash(&bank["policy"]).map_err(|e| e.to_string())?
            == payload_hash(&serde_json::from_slice::<V>(&prior_bank).map_err(|e| e.to_string())?["policy"]).map_err(|e| e.to_string())?,
        "accountingContinuation": c, "frozenSnapshot": snapshot, "frozenFiles": files.len() as i64,
        "publicSource": {"bytes": bank["sourcePrefixBytes"], "hash": bank["sourcePrefixHash"],
            "sequence": bank["sourcePrefixSequence"]},
        "originalSource": {"bytes": bytes as i64, "hash": source_prefix_hash(&original_file, bytes, None)?},
        "opening": {"cash": opening["cash"], "fills": opening["fills"],
            "realizedPnl": opening["realizedPnl"], "pending": opening["pending"]},
        "untouched": untouched, "pids": read(&root.join("data/current-runtime-pids.json"))?,
        "deploymentFinding": {"reason": "Static graph omitted an unused dependency still declared in the bank identity",
            "failedSnapshot": failed_snapshot, "recoveredSnapshot": snapshot, "bankAndSeedPreserved": true},
        "accountingReset": false, "extraReadSlots": 0, "competitionOrders": 0,
        "profitabilityEstablished": false});
    atomic_json(&root.join("data/baal-pair-anchor-bank-20261004-v4.json"), &bank)?;
    atomic_json(&root.join("data/pair-anchor-preservation-20261004.json"), &report)?;
    println!("{}", baal_service_runtime::json_text::js_stringify(
        &json!({"cohort": bank["name"], "bankHash": active["bankHash"], "frozenFiles": report["frozenFiles"],
            "opening": report["opening"], "recovery": "complete_pinned_dependency_graph", "accountingReset": false})));
    Ok(report)
}
