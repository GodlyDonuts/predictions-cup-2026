// data/prepare-journal-native-20261004.mjs — restore the original journal
// natively, preserve the three owned cohorts, and install the native
// collector-journal config.
use crate::publication_pack::{read_json, runtime_dir, sha};
use baal_collector::collector_journal_native::{
    collector_journal_code_identity, collector_native_journal_config,
};
use baal_collector::native_journal_restore::{open_native_journal, source_prefix_hash};
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::journal::{journal_lines, Journal, JournalReadOptions};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn need(v: bool, c: &str) -> Result<(), String> {
    if v { Ok(()) } else { Err(format!("JOURNAL_PREP_{c}")) }
}
pub fn run() -> Result<V, String> {
    let r = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let check = || -> bool {
        read(&format!("{r}/COLLECTOR_STOP")).map(|v| v["source"] == json!("collector-native-journal-deployment-20261004")).unwrap_or(false)
            && !Path::new(&format!("{r}/../bot.lock")).exists()
            && !Path::new("STOP").exists()
            && !Path::new(&format!("{r}/STOP")).exists()
    };
    need(check(), "STOP_OR_LEASE")?;
    need(read(&format!("{r}/competition-activation.json"))?["enabled"] == json!(false), "ACTIVATION")?;
    need(!Path::new(&format!("{r}/collector-native-journal.json")).exists(), "NO_OVERWRITE")?;
    let budget = read(&format!("{r}/coordinated-network-budget.json"))?;
    need(
        budget["schemaVersion"] == json!(3)
            && budget["maxReadsPerMinute"] == json!(20)
            && budget["writesEnabled"] == json!(false)
            && budget["executionOwner"] == json!("Daenerys"),
        "BUDGET",
    )?;
    let file = PathBuf::from(format!("{r}/journal.jsonl"));
    let source_bytes = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
    let js_started = std::time::Instant::now();
    let legacy = Journal::open(&file)?;
    let legacy_ms = js_started.elapsed().as_secs_f64() * 1000.0;
    let binding = read("data/journal-native-binding-20261004.json")?;
    let native = open_native_journal(
        Path::new(&r), &binding, &file,
        Path::new(&format!("{r}/collector-native-journal-preparation-receipts")),
        None, &check,
    )?;
    let native_j = &native.journal;
    need(
        native.native_restore["sourceBytes"] == json!(source_bytes)
            && legacy.sequence == native_j.sequence
            && legacy.previous == native_j.previous
            && payload_hash(&legacy.last.clone().unwrap_or(V::Null))? == payload_hash(&native_j.last.clone().unwrap_or(V::Null))?,
        "FULL_SOURCE_PARITY",
    )?;
    let mut preservation = serde_json::Map::<String, V>::new();
    for typ in ["no-maker", "baal-pair", "probability-receipt"] {
        let active = read(&format!("{r}/{typ}-active.json"))?;
        let dir = format!("{r}/{typ}-cohorts/{}", active["cohort"].as_str().unwrap_or(""));
        let journal = format!("{dir}/journal.jsonl");
        let end_byte = std::fs::metadata(&journal).map_err(|e| e.to_string())?.len();
        need(sha(serde_json::to_string(&read(&format!("{dir}/bank.json"))?).unwrap().as_bytes()) == active["bankHash"].as_str().unwrap_or(""), "BANK")?;
        let mut sequence = 0u64;
        let mut previous = if typ == "probability-receipt" {
            active["bankHash"].as_str().unwrap_or("").to_string()
        } else {
            String::new()
        };
        let options = JournalReadOptions { end_byte: Some(end_byte), ..Default::default() };
        for line in journal_lines(Path::new(&journal), &options)? {
            if line.is_empty() { continue; }
            let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let hash = row["hash"].as_str().unwrap_or("").to_string();
            let mut body = row.clone();
            body.as_object_mut().unwrap().remove("hash");
            sequence += 1;
            need(
                body["sequence"] == json!(sequence) && body["previous"] == json!(previous)
                    && sha(serde_json::to_string(&body).unwrap().as_bytes()) == hash,
                &format!("WALLET_CHAIN_{typ}"),
            )?;
            previous = hash;
        }
        preservation.insert(typ.to_string(), json!({
            "cohort": active["cohort"], "bankHash": active["bankHash"],
            "bankFileHash": sha(&std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?),
            "journalPrefixBytes": end_byte,
            "journalPrefixHash": source_prefix_hash(Path::new(&journal), end_byte, None)?,
            "journalSequence": sequence, "journalHash": previous,
        }));
    }
    let code_identity = collector_journal_code_identity()?;
    let source_snapshot = payload_hash(&code_identity)?;
    let snapshot_root = format!("{r}/collector-journal-native-sources/{source_snapshot}");
    std::fs::create_dir_all(&snapshot_root).map_err(|e| e.to_string())?;
    for (name, hash) in code_identity.as_object().unwrap() {
        let bytes = std::fs::read(name).map_err(|e| e.to_string())?;
        let target = format!("{snapshot_root}/{name}");
        need(sha(&bytes) == hash.as_str().unwrap_or(""), "SOURCE_CHANGED")?;
        std::fs::create_dir_all(Path::new(&target).parent().unwrap()).map_err(|e| e.to_string())?;
        if Path::new(&target).exists() {
            need(sha(&std::fs::read(&target).map_err(|e| e.to_string())?) == hash.as_str().unwrap_or(""), "ARCHIVE_CHANGED")?;
        } else {
            std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                .and_then(|mut f| std::io::Write::write_all(&mut f, &bytes))
                .map_err(|e| e.to_string())?;
        }
    }
    atomic_json(Path::new(&format!("{snapshot_root}/manifest.json")), &json!({
        "schemaVersion": 1, "files": code_identity,
    }))?;
    let config = json!({
        "schemaVersion": 1, "mode": "native_original_journal_collector_v1",
        "nativeBackend": binding, "codeIdentity": code_identity,
        "sourceSnapshot": source_snapshot,
    });
    let receipt = json!({
        "at": baal_service_runtime::contract::ms_to_iso(crate::publication_pack::now_ms() as f64),
        "source": {
            "bytes": source_bytes, "records": legacy.sequence,
            "sha256": native.native_restore["sourceHash"], "tailHash": legacy.previous,
            "lastBodyHash": payload_hash(&legacy.last.clone().unwrap_or(V::Null))?,
        },
        "legacyMs": legacy_ms, "native": native.native_restore,
        "exactWholeBodyParity": true, "configHash": payload_hash(&config)?, "config": config,
        "preservation": V::Object(preservation),
        "publicProducerConfigHash": payload_hash(&read(&format!("{r}/collector-native-publication.json"))?)?,
        "pids": read("data/current-runtime-pids.json")?,
        "accountingReset": false, "competitionOrders": 0,
    });
    need(check(), "STOP_CHANGED")?;
    atomic_json(Path::new(&format!("{r}/collector-native-journal.json")), &config)?;
    need(
        payload_hash(&collector_native_journal_config(Path::new(&r))?.ok_or("JOURNAL_PREP_CONFIG")?)?
            == receipt["configHash"].as_str().unwrap_or(""),
        "CONFIG",
    )?;
    atomic_json(Path::new("data/journal-native-preservation-20261004.json"), &receipt)?;
    Ok(json!({
        "at": receipt["at"], "source": receipt["source"], "legacyMs": legacy_ms,
        "nativeMs": receipt["native"]["totalMs"], "exactWholeBodyParity": true,
        "accountingReset": false, "competitionOrders": 0,
    }))
}
