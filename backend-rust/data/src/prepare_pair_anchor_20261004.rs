// data/prepare-pair-anchor-20261004.mjs — replace the baal-pair host with the
// anchor-pinned v4 across the serialized refresh, carrying accounting.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::canonical_no_capacity_v2::validate_capacity_review;
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::native_journal_restore::source_prefix_hash;
use baal_collector::native_publication::native_publication_state;
use baal_collector::paper_book_continuation_frontier::paper_book_continuation_frontier;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

fn need(v: bool, c: &str) -> Result<(), String> {
    if v { Ok(()) } else { Err(format!("PAIR_ANCHOR_PREP_{c}")) }
}
pub fn run() -> Result<V, String> {
    let r = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let guard = || -> bool {
        read(&format!("{r}/COLLECTOR_STOP")).map(|v| v["source"] == json!("pair-anchor-deployment-20261004")).unwrap_or(false)
            && !Path::new(&format!("{r}/../bot.lock")).exists()
            && !Path::new("STOP").exists()
            && !Path::new(&format!("{r}/STOP")).exists()
    };
    need(guard(), "STOP_OR_LEASE")?;
    need(read(&format!("{r}/competition-activation.json"))?["enabled"] == json!(false), "ACTIVATION")?;
    let active = read(&format!("{r}/baal-pair-active.json"))?;
    need(active["hostRevision"] == json!(3) && active["cohort"] == json!("baal-no-pair-20261004-v3"), "ACTIVE")?;
    let dir = format!("{r}/baal-pair-cohorts/{}", active["cohort"].as_str().unwrap_or(""));
    let bank_bytes = std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?;
    let old: V = serde_json::from_slice(&bank_bytes).map_err(|e| e.to_string())?;
    let stats = read(&format!("{dir}/stats.json"))?;
    need(payload_hash(&old)? == active["bankHash"].as_str().unwrap_or("") && stats["failed"] != json!(true), "PRIOR_BANK")?;
    validate_capacity_review(&old["review"])?;
    need((now_ms() as f64) < parse_iso_ms(old["review"]["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0), "REVIEW")?;
    let _original = frozen_paper_module(Path::new(&r), &old, "live-paper/baal-pair-host-v3.mjs")?;
    need(
        payload_hash(&baal_collector::baal_pair_host_v3::baal_pair_code_identity(Path::new("live-paper")))?
            == payload_hash(&old["codeIdentity"])?,
        "PRIOR_CODE",
    )?;
    let mut sequence = 0u64;
    let mut previous = String::new();
    let mut last = V::Null;
    let mut prefix = Sha256::new();
    let journal = format!("{dir}/journal.jsonl");
    let bytes = std::fs::metadata(&journal).map_err(|e| e.to_string())?.len();
    let options = JournalReadOptions { end_byte: Some(bytes), ..Default::default() };
    for line in journal_lines(Path::new(&journal), &options)? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().remove("hash");
        sequence += 1;
        need(
            body["sequence"] == json!(sequence) && body["previous"] == json!(previous)
                && sha(serde_json::to_string(&body).unwrap().as_bytes()) == hash,
            "PRIOR_CHAIN",
        )?;
        previous = hash;
        last = row;
        prefix.update(format!("{line}\n").as_bytes());
    }
    need(
        last["event"]["kind"] == json!("boundary") && last["event"]["reason"] == json!("close")
            && stats["journalSequence"] == json!(sequence) && stats["journalHash"] == json!(previous),
        "PRIOR_CLOSED",
    )?;
    let p_active = read(&format!("{r}/probability-receipt-active.json"))?;
    let p_bank = read(&format!("{r}/probability-receipt-cohorts/{}/bank.json", p_active["cohort"].as_str().unwrap_or("")))?;
    let source_file = format!("{r}/public-books.jsonl");
    let source_bytes = std::fs::read(&source_file).map_err(|e| e.to_string())?;
    let source_hash = sha(&source_bytes);
    let native = native_publication_state(
        Path::new(&r), &p_bank["nativeBackend"], &p_bank["universe"],
        Path::new(&source_file), source_bytes.len() as u64, &source_hash,
        Path::new(&format!("{r}/pair-anchor-native-preparation")),
    )?;
    need(native["state"]["closed"] == json!(true), "SOURCE_NOT_CLOSED")?;
    let legs = old["review"]["legs"].as_array().cloned().unwrap_or_default();
    let frontier = paper_book_continuation_frontier(
        Path::new(&source_file), source_bytes.len() as u64, &last["state"], &legs)?;
    let cutoff = ms_to_iso(now_ms() as f64);
    let seed_bytes = serde_json::to_string(&last["state"]["paper"]).unwrap().into_bytes();
    let next_identity = baal_collector::baal_pair_host_v4::baal_pair_code_identity(Path::new("live-paper"));
    let mut declared: Vec<String> = next_identity.as_object().map(|o| o.keys()
        .map(|f| format!("live-paper/{}", f.trim_start_matches("./"))).collect()).unwrap_or_default();
    declared.sort();
    let mut roots = vec!["live-paper/baal-pair-host-v4.mjs".to_string()];
    roots.extend(declared);
    let files = collect_frozen_paper_files(Path::new("."), &roots)?;
    let files_v = json!(files.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<String, V>>());
    let snapshot = payload_hash(&files_v)?;
    let snapshot_root = format!("{r}/frozen-code/{snapshot}");
    std::fs::create_dir_all(&snapshot_root).map_err(|e| e.to_string())?;
    for (name, hash) in &files {
        let data = std::fs::read(name).map_err(|e| e.to_string())?;
        let target = PathBuf::from(format!("{snapshot_root}/{name}"));
        need(sha(&data) == *hash, "SOURCE_CHANGED")?;
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        if target.exists() {
            need(sha(&std::fs::read(&target).map_err(|e| e.to_string())?) == *hash, "ARCHIVE_CHANGED")?;
        } else {
            std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                .and_then(|mut f| f.write_all(&data)).map_err(|e| e.to_string())?;
        }
    }
    atomic_json(Path::new(&format!("{snapshot_root}/manifest.json")), &json!({
        "schemaVersion": 1, "files": files_v,
    }))?;
    let mut bank = old.clone();
    for (k, v) in [
        ("name", json!("baal-no-pair-anchor-20261004-v4")), ("hostRevision", json!(4)),
        ("paperRevision", json!(3)), ("cutoff", json!(cutoff)),
        ("codeIdentity", next_identity),
        ("sourcePrefixBytes", json!(source_bytes.len())),
        ("sourcePrefixHash", json!(source_hash)),
        ("sourcePrefixSequence", native["state"]["sequence"].clone()),
        ("sourcePrefixTailHash", native["state"]["hash"].clone()),
        ("accountingContinuation", json!({
            "cohort": active["cohort"], "bankHash": active["bankHash"],
            "bankFileHash": sha(&bank_bytes), "journalPrefixBytes": bytes,
            "journalPrefixHash": format!("{:x}", prefix.finalize()),
            "openingStateFileHash": sha(&seed_bytes), "accountingCarried": true,
            "reason": "bounded_confirmation_marker_across_serialized_refresh",
            "sourceFrontier": frontier,
        })),
        ("executionAuthority", json!(false)),
    ] { bank.as_object_mut().unwrap().insert(k.into(), v); }
    let target = format!("{}/baal-pair-cohorts/{}", r, bank["name"].as_str().unwrap_or(""));
    need(!Path::new(&target).exists(), "NO_OVERWRITE")?;
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{target}/opening-state.json"))
        .and_then(|mut f| f.write_all(&seed_bytes)).map_err(|e| e.to_string())?;
    atomic_json(Path::new(&format!("{target}/bank.json")), &bank)?;
    let index_file = format!("{r}/frozen-code/index.json");
    let mut index = read(&index_file)?;
    index["banks"][payload_hash(&bank)?] = json!({"snapshot": snapshot});
    atomic_json(Path::new(&index_file), &index)?;
    need(
        guard() && (now_ms() as f64) < parse_iso_ms(old["review"]["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0),
        "STOP_OR_REVIEW_CHANGED",
    )?;
    atomic_json(Path::new(&format!("{r}/baal-pair-active.json")), &json!({
        "schemaVersion": 1, "cohort": bank["name"], "bankHash": payload_hash(&bank)?,
        "hostRevision": 4, "executionAuthority": false,
    }))?;
    let _loaded = frozen_paper_module(Path::new(&r), &bank, "live-paper/baal-pair-host-v4.mjs")?;
    let host = baal_collector::baal_pair_host_v4::BaalPairHost::open(
        Path::new(&r),
        std::sync::Arc::new(|| crate::publication_pack::now_ms()),
        std::sync::Arc::new(|| true),
    )?.ok_or("PAIR_ANCHOR_PREP_ACCOUNTING")?;
    let opening = host.status();
    need(
        opening["failed"] != json!(true)
            && opening["cash"] == stats["cash"]
            && opening["fills"] == stats["fills"]
            && opening["realizedPnl"] == stats["realizedPnl"]
            && sha(&std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?) == sha(&bank_bytes)
            && source_prefix_hash(Path::new(&journal), bytes, None)?
                == bank["accountingContinuation"]["journalPrefixHash"].as_str().unwrap_or(""),
        "ACCOUNTING",
    )?;
    atomic_json(Path::new(&format!("{dir}/STOP")), &json!({
        "source": "Codex corrected successor deployment", "at": cutoff,
        "successor": bank["name"], "reason": bank["accountingContinuation"]["reason"],
    }))?;
    let original_journal = format!("{r}/journal.jsonl");
    let original_bytes = std::fs::metadata(&original_journal).map_err(|e| e.to_string())?.len();
    let original_hash = source_prefix_hash(Path::new(&original_journal), original_bytes, None)?;
    let mut untouched = serde_json::Map::<String, V>::new();
    for typ in ["no-maker", "probability-receipt"] {
        let a = read(&format!("{r}/{typ}-active.json"))?;
        let d = format!("{r}/{typ}-cohorts/{}", a["cohort"].as_str().unwrap_or(""));
        let f = format!("{d}/journal.jsonl");
        let size = std::fs::metadata(&f).map_err(|e| e.to_string())?.len();
        untouched.insert(typ.to_string(), json!({
            "cohort": a["cohort"], "bankHash": a["bankHash"],
            "bankFileHash": sha(&std::fs::read(format!("{d}/bank.json")).map_err(|e| e.to_string())?),
            "journalPrefixBytes": size,
            "journalPrefixHash": source_prefix_hash(Path::new(&f), size, None)?,
        }));
    }
    let report = json!({
        "at": cutoff, "cohort": bank["name"], "bankHash": payload_hash(&bank)?,
        "reviewExpiresAt": bank["review"]["expiresAt"],
        "policyUnchanged": payload_hash(&bank["policy"])? == payload_hash(&old["policy"])?,
        "accountingContinuation": bank["accountingContinuation"],
        "frozenSnapshot": snapshot, "frozenFiles": files.len(),
        "publicSource": {"bytes": source_bytes.len(), "hash": source_hash,
            "sequence": native["state"]["sequence"]},
        "originalSource": {"bytes": original_bytes, "hash": original_hash},
        "opening": {"cash": opening["cash"], "fills": opening["fills"],
            "realizedPnl": opening["realizedPnl"], "pending": opening["pending"]},
        "untouched": V::Object(untouched), "pids": read("data/current-runtime-pids.json")?,
        "accountingReset": false, "extraReadSlots": 0, "competitionOrders": 0,
        "profitabilityEstablished": false,
    });
    atomic_json(Path::new("data/baal-pair-anchor-bank-20261004-v4.json"), &bank)?;
    atomic_json(Path::new("data/pair-anchor-preservation-20261004.json"), &report)?;
    Ok(json!({
        "at": cutoff, "cohort": bank["name"], "bankHash": report["bankHash"],
        "opening": report["opening"], "frozenFiles": report["frozenFiles"],
        "policyUnchanged": true, "accountingReset": false,
    }))
}
