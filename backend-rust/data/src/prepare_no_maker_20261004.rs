// data/prepare-no-maker-20261004.mjs — declare the first no-maker tape cohort
// from the canonical-capacity review scope.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::no_maker_host::no_maker_code_identity;
use baal_collector::no_maker_paper::no_maker_policy;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::Path;

pub fn run() -> Result<V, String> {
    let r = runtime_dir();
    let a = read_json(Path::new(&format!("{r}/canonical-capacity-active.json")))?;
    let base = read_json(Path::new(&format!("{r}/canonical-capacity-cohorts/{}/bank.json", a["cohort"].as_str().unwrap_or(""))))?;
    let source = std::fs::read(format!("{r}/anonymous-trades.jsonl")).map_err(|e| e.to_string())?;
    if !source.is_empty() && *source.last().unwrap() != b'\n' {
        return Err("Partial tape source; prepare again after complete append".into());
    }
    let rows_text = String::from_utf8_lossy(&source).trim_end().to_string();
    let mut sequence = 0u64;
    let mut previous = String::new();
    if !rows_text.is_empty() {
        for line in rows_text.split('\n') {
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
    }
    let name = "no-maker-tape-20261004-v1";
    let dir = format!("{r}/no-maker-cohorts/{name}");
    let cutoff = ms_to_iso(now_ms() as f64);
    if Path::new(&dir).exists() {
        return Err("Frozen cohort already exists; no overwrite".into());
    }
    let bank = json!({
        "schemaVersion": 1, "mode": "no_maker_trade_tape_paper", "name": name,
        "cutoff": cutoff, "policy": no_maker_policy(),
        "tournamentId": base["tournamentId"], "reviewExpiresAt": base["review"]["expiresAt"],
        "legs": base["review"]["legs"], "review": base["review"],
        "capacityPolicy": base["policy"], "baseCapacityBankHash": a["bankHash"],
        "codeIdentity": no_maker_code_identity(Path::new("live-paper"))?,
        "tapePrefixBytes": source.len(), "tapePrefixHash": sha(&source),
        "tapePrefixSequence": sequence, "tapePrefixTailHash": previous,
        "representation": "documented_same_exchange_yes_book_complement",
        "nativeNoObserved": false, "executionAuthority": false,
    });
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    atomic_json(Path::new(&format!("{dir}/bank.json")), &bank)?;
    atomic_json(Path::new(&format!("{r}/no-maker-active.json")), &json!({
        "cohort": name, "bankHash": payload_hash(&bank)?,
    }))?;
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/no-maker-bank-20261004.json")
        .and_then(|mut f| f.write_all(serde_json::to_string_pretty(&bank).unwrap().as_bytes()))
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "cohort": name, "cutoff": cutoff, "bankHash": payload_hash(&bank)?,
        "priorTapePagesSkipped": sequence, "priorTapeBytes": source.len(),
        "ordersSubmitted": 0, "newProcesses": 0,
    }))
}
