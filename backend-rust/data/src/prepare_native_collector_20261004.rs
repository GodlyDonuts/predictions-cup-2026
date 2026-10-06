// data/prepare-native-collector-20261004.mjs — validate the native publication
// state end-to-end, preserve the two owned cohorts, and install the native
// publication collector config.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::collector_publication_native::collector_publication_code_identity;
use baal_collector::native_publication::native_publication_state;
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

fn need(v: bool, c: &str) -> Result<(), String> {
    if v { Ok(()) } else { Err(format!("NATIVE_COLLECTOR_PREP_{c}")) }
}
pub fn run() -> Result<V, String> {
    let r = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    need(
        read(&format!("{r}/COLLECTOR_STOP"))?["source"] == json!("collector-native-v3-deployment-20261004")
            && !Path::new(&format!("{r}/../bot.lock")).exists()
            && !Path::new("STOP").exists()
            && !Path::new(&format!("{r}/STOP")).exists(),
        "STOP_OR_LEASE",
    )?;
    need(read(&format!("{r}/competition-activation.json"))?["enabled"] == json!(false), "ACTIVATION")?;
    need(!Path::new(&format!("{r}/collector-native-publication.json")).exists(), "NO_OVERWRITE")?;
    let probability = read(&format!("{r}/probability-receipt-active.json"))?;
    let bank = read(&format!("{r}/probability-receipt-cohorts/{}/bank.json", probability["cohort"].as_str().unwrap_or("")))?;
    let file = PathBuf::from(format!("{r}/public-books.jsonl"));
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    let hash = sha(&bytes);
    let native = native_publication_state(
        Path::new(&r), &bank["nativeBackend"], &bank["universe"], &file,
        bytes.len() as u64, &hash,
        Path::new(&format!("{r}/collector-native-deployment-receipts")),
    )?;
    need(native["state"]["closed"] == json!(true), "SOURCE_NOT_CLOSED")?;
    let identities = collector_publication_code_identity(Path::new("."))?;
    let identities_v = json!(identities.iter().map(|(k, v)| (k.clone(), V::String(v.clone()))).collect::<serde_json::Map<String, V>>());
    let snapshot = payload_hash(&identities_v)?;
    let root = format!("{r}/collector-native-sources/{snapshot}");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    for (name, hash) in &identities {
        let source = std::fs::read(name).map_err(|e| e.to_string())?;
        let target = format!("{root}/{name}");
        need(sha(&source) == *hash, "SOURCE_CHANGED")?;
        std::fs::create_dir_all(Path::new(&target).parent().unwrap()).map_err(|e| e.to_string())?;
        if Path::new(&target).exists() {
            need(sha(&std::fs::read(&target).map_err(|e| e.to_string())?) == *hash, "SNAPSHOT_CHANGED")?;
        } else {
            std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                .and_then(|mut f| f.write_all(&source)).map_err(|e| e.to_string())?;
        }
    }
    atomic_json(Path::new(&format!("{root}/manifest.json")), &json!({
        "schemaVersion": 1, "files": identities_v,
    }))?;
    let mut preservation = serde_json::Map::<String, V>::new();
    for typ in ["no-maker", "baal-pair"] {
        let active = read(&format!("{r}/{typ}-active.json"))?;
        let dir = format!("{r}/{typ}-cohorts/{}", active["cohort"].as_str().unwrap_or(""));
        let prior_bank = std::fs::read(format!("{dir}/bank.json")).map_err(|e| e.to_string())?;
        let mut prefix = Sha256::new();
        let mut sequence = 0u64;
        let mut previous = String::new();
        let mut last = V::Null;
        let mut byte_count = 0u64;
        for line in journal_lines(Path::new(&format!("{dir}/journal.jsonl")), &JournalReadOptions::default())? {
            let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let hash = row["hash"].as_str().unwrap_or("").to_string();
            let mut body = row.clone();
            body.as_object_mut().unwrap().remove("hash");
            sequence += 1;
            need(
                row["sequence"] == json!(sequence) && row["previous"] == json!(previous)
                    && sha(serde_json::to_string(&body).unwrap().as_bytes()) == hash,
                "WALLET_CHAIN",
            )?;
            prefix.update(format!("{line}\n").as_bytes());
            byte_count += (line.len() + 1) as u64;
            previous = hash;
            last = row;
        }
        let stats = read(&format!("{dir}/stats.json"))?;
        need(
            last["event"]["kind"] == json!("boundary")
                && last["event"]["reason"] == json!("close")
                && stats["failed"] != json!(true)
                && stats["journalSequence"] == json!(sequence)
                && stats["journalHash"] == json!(previous),
            "WALLET_NOT_CLOSED",
        )?;
        let wallets = if typ == "no-maker" {
            json!(stats["wallets"].as_object().unwrap().iter().map(|(k, w)| (
                k.clone(), json!({
                    "cash": w["cash"], "realizedPnl": w["realizedPnl"],
                    "fills": w["fills"],
                    "positions": w["positions"].as_array().map(|a| a.len()).unwrap_or(0),
                })
            )).collect::<serde_json::Map<String, V>>())
        } else {
            json!({
                "cash": stats["cash"], "realizedPnl": stats["realizedPnl"],
                "fills": stats["fills"],
                "positions": last["state"]["paper"]["positions"].as_object().map(|o| o.len()).unwrap_or(0),
            })
        };
        preservation.insert(typ.to_string(), json!({
            "cohort": active["cohort"], "bankHash": active["bankHash"],
            "bankFileHash": sha(&prior_bank), "journalPrefixBytes": byte_count,
            "journalPrefixHash": format!("{:x}", prefix.finalize()),
            "journalSequence": sequence, "journalHash": previous, "wallets": wallets,
        }));
    }
    let config = json!({
        "schemaVersion": 1, "mode": "native_publication_collector_v1",
        "scopeHash": native["state"]["scopeHash"], "nativeBackend": bank["nativeBackend"],
        "codeIdentity": identities_v, "sourceSnapshot": snapshot,
    });
    let receipt = json!({
        "at": ms_to_iso(now_ms() as f64),
        "source": {"bytes": bytes.len(), "hash": hash,
            "sequence": native["state"]["sequence"], "tailHash": native["state"]["hash"]},
        "previousPublisher": read(&format!("{r}/stats.json"))?["runtime"]["bookPublisher"],
        "configHash": payload_hash(&config)?, "config": config,
        "sourceFiles": identities.len(), "preservation": V::Object(preservation),
        "paperWorker": {"cohort": probability["cohort"], "bankHash": probability["bankHash"],
            "pid": read("data/current-runtime-pids.json")?["paper"], "stopped": false},
        "accountingReset": false, "additionalPersistentProcesses": 0,
        "additionalReadSlots": 0, "competitionOrders": 0,
    });
    atomic_json(Path::new(&format!("{r}/collector-native-publication.json")), &config)?;
    atomic_json(Path::new("data/native-collector-preservation-20261004.json"), &receipt)?;
    Ok(json!({
        "at": receipt["at"], "source": receipt["source"],
        "previousPublisher": receipt["previousPublisher"],
        "sourceFiles": receipt["sourceFiles"], "accountingReset": false,
        "competitionOrders": 0,
    }))
}
