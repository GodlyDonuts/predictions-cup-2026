// data/prepare-native-mechanisms-20261004.mjs — declare the frozen trend-
// mechanism probe cohort with pinned native sources and binaries.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::rust_backend::native_executable;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
fn graph_files() -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string("backend-rust/build.rs").map_err(|e| e.to_string())?;
    Ok(text.lines().filter_map(|l| {
        let t = l.trim();
        if t.starts_with('"') && t.ends_with("\",") {
            Some(t[1..t.len() - 2].to_string())
        } else { None }
    }).collect())
}
pub fn run() -> Result<V, String> {
    let rt = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let active = read(&format!("{rt}/native-quote-active.json"))?;
    let original_dir = format!("{rt}/research-cohorts/{}", active["cohort"].as_str().unwrap_or(""));
    let graph = graph_files()?;
    let mut digest = Sha256::new();
    let mut source_hashes = serde_json::Map::<String, V>::new();
    for file in &graph {
        let bytes = std::fs::read(format!("backend-rust/{file}")).map_err(|e| e.to_string())?;
        digest.update((file.len() as u64).to_le_bytes());
        digest.update(file.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(&bytes);
        source_hashes.insert(format!("backend-rust/{file}"), json!(sha(&bytes)));
    }
    let engine_hash = format!("{:x}", digest.finalize());
    let exe_dir = native_executable().parent().unwrap().to_path_buf();
    let binary_hash = sha(&std::fs::read(native_executable()).map_err(|e| e.to_string())?);
    let cohort = "baal-trend-mechanisms-20261004-v1";
    let dir = format!("{rt}/research-cohorts/{cohort}");
    if Path::new(&dir).exists() {
        return Err("Refusing to replace a frozen mechanism protocol".into());
    }
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let declared_at = ms_to_iso(now_ms() as f64);
    let protocol = json!({
        "schemaVersion": 1, "mode": "frozen_quote_mechanism_probe_v1", "cohort": cohort,
        "declaredAt": declared_at, "signalAfter": parse_iso_ms(&declared_at).unwrap_or(0.0),
        "claimComment": 5983314037i64, "engineHash": engine_hash, "binaryHash": binary_hash,
        "runtimeLibraryHash": sha(&std::fs::read(exe_dir.join("libunwind.dll")).map_err(|e| e.to_string())?),
        "sourceHashes": V::Object(source_hashes.clone()),
        "baseCohort": active["cohort"],
        "baseDeclarationHash": sha(&std::fs::read(format!("{original_dir}/declaration.json")).map_err(|e| e.to_string())?),
        "developmentInputHash": sha(&std::fs::read(format!("{original_dir}/seed.sources.json")).map_err(|e| e.to_string())?),
        "families": ["trend_continuation", "trend_reversal"],
        "contractSides": ["YES", "NO"],
        "lookbackMs": 900000, "minimumSpanMs": 840000, "minimumDistinctMinutes": 10,
        "minimumMidpointMove": "0.020000", "minimumBidAndAskMove": "0.020000",
        "maximumObservationGapMs": 120000,
        "horizonMs": [300000, 1800000, 7200000],
        "scenarios": read(&format!("{original_dir}/declaration.json"))?["scenarios"],
        "entry": "first_independent_request_after_latency_within_original_ask_limit_and120s",
        "outcomes": "separate_strict_path_and_fixed_endpoint_views_no_intraperiod_stops_inferred",
        "fees": 0, "networkRequests": 0, "competitionOrders": 0,
        "additionalPersistentProcesses": 0, "notAnActiveWalletOrExecutionBank": true,
        "profitabilityEstablished": false,
        "futureValidation": "Signals require both original request and publication strictly after this cutoff. Historical comparison is exploratory; prospective quote results still require independent executable-depth evidence.",
    });
    let bytes = format!("{}\n", serde_json::to_string_pretty(&protocol).unwrap());
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{dir}/protocol.json"))
        .and_then(|mut f| f.write_all(bytes.as_bytes())).map_err(|e| e.to_string())?;
    copy_exclusive(&format!("{original_dir}/declaration.json"), &format!("{dir}/base-declaration.json"))?;
    copy_exclusive(&format!("{original_dir}/seed.sources.json"), &format!("{dir}/development.sources.json"))?;
    for (file, hash) in &source_hashes {
        let target = format!("{rt}/native-sources/{engine_hash}/{}", file.replace("backend-rust/", ""));
        std::fs::create_dir_all(Path::new(&target).parent().unwrap()).map_err(|e| e.to_string())?;
        if Path::new(&target).exists() {
            assert_eq(&json!(sha(&std::fs::read(&target).map_err(|e| e.to_string())?)), hash)?;
        } else {
            copy_exclusive(file, &target)?;
        }
    }
    let binary_dir = format!("{rt}/native-builds/{binary_hash}");
    std::fs::create_dir_all(&binary_dir).map_err(|e| e.to_string())?;
    for file in ["baal-backend.exe", "libunwind.dll"] {
        let source = exe_dir.join(file);
        let target = format!("{binary_dir}/{file}");
        if Path::new(&target).exists() {
            assert_eq(
                &json!(sha(&std::fs::read(&target).map_err(|e| e.to_string())?)),
                &json!(sha(&std::fs::read(&source).map_err(|e| e.to_string())?)),
            )?;
        } else {
            copy_exclusive(source.to_str().unwrap(), &target)?;
        }
    }
    std::fs::OpenOptions::new().write(true).create_new(true).open("data/native-mechanisms-protocol-20261004.json")
        .and_then(|mut f| f.write_all(bytes.as_bytes())).map_err(|e| e.to_string())?;
    Ok(json!({
        "cohort": cohort, "declaredAt": declared_at, "signalAfter": protocol["signalAfter"],
        "engineHash": engine_hash, "binaryHash": binary_hash,
        "protocolHash": sha(bytes.as_bytes()), "cases": 36, "fees": 0, "newLiveWallets": 0,
    }))
}
fn copy_exclusive(source: &str, target: &str) -> Result<(), String> {
    std::fs::copy(source, target).map(|_| ()).map_err(|e| e.to_string())
}
