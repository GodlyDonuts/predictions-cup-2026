// data/prepare-no-maker-v4-20261004.mjs — carry exact v3 accounting to v4 with
// a fresh capacity review and consumed-frontier check.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::canonical_no_capacity::validate_capacity_review;
use baal_collector::frozen_paper_runtime::frozen_paper_module;
use baal_collector::no_maker_host_v3::no_maker_code_identity_v3;
use baal_collector::no_maker_host_v4::no_maker_code_identity_v4;
use baal_collector::no_maker_paper_v2::no_maker_policy;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::Path;

fn wallets_of(stats: &V) -> V {
    json!(stats["wallets"].as_object().map(|o| o.iter().map(|(k, w)| (
        k.clone(), json!({
            "cash": w["cash"], "reserved": w["reserved"], "fills": w["fills"],
            "realizedPnl": w["realizedPnl"],
            "positions": w["positions"].as_array().map(|a| a.len()).unwrap_or(0),
            "unresolvedQuotes": w["unresolvedQuotes"].as_array().map(|a| a.len()).unwrap_or(0),
        })
    )).collect::<serde_json::Map<String, V>>()).unwrap_or_default())
}
fn tape_chain(source: &[u8]) -> Result<(u64, String), String> {
    let mut sequence = 0u64;
    let mut previous = String::new();
    for line in String::from_utf8_lossy(source).trim_end().split('\n').filter(|l| !l.is_empty()) {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().remove("hash");
        sequence += 1;
        if row["sequence"] != json!(sequence) || row["previous"] != json!(previous)
            || sha(serde_json::to_string(&body).unwrap().as_bytes()) != hash
        { return Err("Tape integrity".into()); }
        previous = hash;
    }
    Ok((sequence, previous))
}
pub fn run(review_file: Option<&str>) -> Result<V, String> {
    let r = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let marker = read(&format!("{r}/COLLECTOR_STOP"))?;
    if marker["source"] != json!("no-maker-v4-deployment-20261004") {
        return Err("Deployment STOP provenance".into());
    }
    if Path::new(&format!("{r}/../bot.lock")).exists() || Path::new("STOP").exists()
        || Path::new(&format!("{r}/STOP")).exists()
    { return Err("Lease or human STOP".into()); }
    if read(&format!("{r}/competition-activation.json"))?["enabled"] != json!(false) {
        return Err("Contest activation".into());
    }
    let active_file = format!("{r}/no-maker-active.json");
    let old_active = read(&active_file)?;
    if old_active["cohort"] != json!("no-maker-tape-20261004-v3") {
        return Err("Unexpected active cohort".into());
    }
    let old_dir = format!("{r}/no-maker-cohorts/{}", old_active["cohort"].as_str().unwrap_or(""));
    let old_bytes = std::fs::read(format!("{old_dir}/bank.json")).map_err(|e| e.to_string())?;
    let old: V = serde_json::from_slice(&old_bytes).map_err(|e| e.to_string())?;
    let old_stats = read(&format!("{old_dir}/stats.json"))?;
    let old_journal = std::fs::read(format!("{old_dir}/journal.jsonl")).map_err(|e| e.to_string())?;
    let _preserved = frozen_paper_module(Path::new(&r), &old, "live-paper/no-maker-host-v3.mjs")?;
    if payload_hash(&old)? != old_active["bankHash"].as_str().unwrap_or("")
        || payload_hash(&old["codeIdentity"])? != payload_hash(&no_maker_code_identity_v3(Path::new("live-paper"))?)?
    { return Err("Original frozen identity".into()); }
    if old_stats["failed"] == json!(true) {
        return Err("Predecessor failed; accounting must be investigated".into());
    }
    let text = String::from_utf8_lossy(&old_journal).trim_end().to_string();
    let mut old_rows = Vec::new();
    if !text.is_empty() {
        let mut old_previous = String::new();
        for (i, line) in text.split('\n').enumerate() {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let hash = row["hash"].as_str().unwrap_or("").to_string();
            let mut body = row.clone();
            body.as_object_mut().unwrap().remove("hash");
            if row["sequence"] != json!(i + 1) || row["previous"] != json!(old_previous)
                || sha(serde_json::to_string(&body).unwrap().as_bytes()) != hash
            { return Err("Predecessor journal integrity".into()); }
            old_previous = hash;
            old_rows.push(row);
        }
    }
    let old_previous = old_rows.last().map(|r| r["hash"].as_str().unwrap_or("").to_string()).unwrap_or_default();
    let seed = old_rows.last().unwrap()["state"]["paper"].clone();
    if seed["bankHash"] != old_active["bankHash"]
        || old_stats["journalSequence"] != json!(old_rows.len())
        || old_stats["journalHash"] != json!(old_previous)
    { return Err("Predecessor publication frontier".into()); }
    let seed_bytes = serde_json::to_string_pretty(&seed).unwrap().into_bytes();
    let source = std::fs::read(format!("{r}/anonymous-trades.jsonl")).map_err(|e| e.to_string())?;
    if !source.is_empty() && *source.last().unwrap() != b'\n' {
        return Err("Partial tape source".into());
    }
    let (sequence, previous) = tape_chain(&source)?;
    if old_rows.last().unwrap()["state"]["tapeSequence"] != json!(sequence)
        || old_rows.last().unwrap()["state"]["tapeHash"] != json!(previous)
    { return Err("Unconsumed source suffix; resume predecessor before migration".into()); }
    let review = match review_file {
        Some(f) => read(f)?,
        None => old["review"].clone(),
    };
    validate_capacity_review(&review)?;
    if review["legs"].as_array().map(|a| a.len()).unwrap_or(0) != old["legs"].as_array().map(|a| a.len()).unwrap_or(0)
        || review["legs"].as_array().unwrap().iter().zip(old["legs"].as_array().unwrap())
            .any(|(leg, old_leg)| ["marketId", "exchangeId", "outcome"].iter().any(|k| leg[k] != old_leg[k]))
    { return Err("Carried accounting scope changed".into()); }
    if parse_iso_ms(review["expiresAt"].as_str().unwrap_or("")).unwrap_or(0.0) <= now_ms() as f64
        || parse_iso_ms(review["recordedAt"].as_str().unwrap_or("")).unwrap_or(0.0) > now_ms() as f64
    { return Err("Fresh source-backed review required".into()); }
    let name = "no-maker-tape-20261004-v4";
    let dir = format!("{r}/no-maker-cohorts/{name}");
    let cutoff = ms_to_iso(now_ms() as f64);
    if Path::new(&dir).exists() { return Err("Frozen successor exists; no overwrite".into()); }
    let bank = json!({
        "schemaVersion": 1, "hostRevision": 4, "mode": old["mode"], "name": name,
        "cutoff": cutoff, "policy": no_maker_policy(),
        "tournamentId": old["tournamentId"], "reviewExpiresAt": review["expiresAt"],
        "legs": review["legs"], "review": review, "capacityPolicy": old["capacityPolicy"],
        "baseCapacityBankHash": old["baseCapacityBankHash"],
        "codeIdentity": no_maker_code_identity_v4(Path::new("live-paper"))?,
        "tapePrefixBytes": source.len(), "tapePrefixHash": sha(&source),
        "tapePrefixSequence": sequence, "tapePrefixTailHash": previous,
        "paperRevision": 2,
        "accountingContinuation": {
            "cohort": old["name"], "bankHash": old_active["bankHash"],
            "bankFileHash": sha(&old_bytes), "journalPrefixBytes": old_journal.len(),
            "journalPrefixHash": sha(&old_journal),
            "openingStateFileHash": sha(&seed_bytes), "accountingCarried": true,
            "reason": "consumed_frontier_checked_continuation",
            "openingWallets": wallets_of(&old_stats),
        },
        "representation": old["representation"], "nativeNoObserved": false,
        "executionAuthority": false,
    });
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    atomic_json(Path::new(&format!("{dir}/bank.json")), &bank)?;
    std::fs::OpenOptions::new().write(true).create(true).open(format!("{dir}/opening-state.json"))
        .and_then(|mut f| f.write_all(&seed_bytes)).map_err(|e| e.to_string())?;
    let preserved = json!({
        "at": cutoff, "cohort": old["name"], "bankHash": old_active["bankHash"],
        "bankFileHash": sha(&old_bytes), "codeIdentity": old["codeIdentity"],
        "journalPrefixBytes": old_journal.len(), "journalPrefixHash": sha(&old_journal),
        "journalSequence": old_stats["journalSequence"], "journalHash": old_stats["journalHash"],
        "reason": "Retired after consumed-frontier transition correction; exact cash, inventory, fills and unresolved quotes carried to V4 with source lineage",
        "wallets": wallets_of(&old_stats),
    });
    atomic_json(Path::new(&format!("{old_dir}/STOP")), &json!({
        "source": "Codex successor deployment", "at": cutoff,
        "reason": preserved["reason"], "successor": name,
    }))?;
    atomic_json(Path::new(&active_file), &json!({
        "cohort": name, "bankHash": payload_hash(&bank)?, "hostRevision": 4,
    }))?;
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/no-maker-v3-preservation-20261004.json")
        .and_then(|mut f| f.write_all(serde_json::to_string_pretty(&preserved).unwrap().as_bytes()))
        .map_err(|e| e.to_string())?;
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/no-maker-bank-20261004-v4.json")
        .and_then(|mut f| f.write_all(serde_json::to_string_pretty(&bank).unwrap().as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "cohort": name, "cutoff": cutoff, "bankHash": payload_hash(&bank)?,
        "hostRevision": 4, "sealedConsumedTapePages": sequence,
        "originalWalletsRetained": true, "competitionOrders": 0,
    }))
}
