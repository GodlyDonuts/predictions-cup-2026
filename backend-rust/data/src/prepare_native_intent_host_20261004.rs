// data/prepare-native-intent-host-20261004.mjs — prepare the observed native
// intent continuation: cache preparation, full/cached observed evaluation
// parity, then the v3 intent host pointer.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::breadth::validate_universe;
use baal_collector::native_dip_host_v2::verify_cache_proof;
use baal_collector::native_dip_host_v3::native_dip_intent_host_identity;
use baal_collector::native_dip_proof_v1::verify_native_dip_result;
use baal_collector::native_intent_proof_v1::verify_native_intent_result;
use baal_collector::rust_backend::native_executable;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn assert_eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err("assert".into()) }
}
fn copy_exclusive(source: &Path, target: &Path) -> Result<(), String> {
    let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
    if target.exists() {
        assert_eq(&json!(sha(&std::fs::read(target).map_err(|e| e.to_string())?)), &json!(sha(&bytes)))
    } else {
        std::fs::OpenOptions::new().write(true).create_new(true).open(target)
            .and_then(|mut f| f.write_all(&bytes)).map_err(|e| e.to_string())
    }
}
fn run_native(exe: &Path, command: &str, args: &V) -> Result<(f64, V), String> {
    let mut argv: Vec<String> = vec![command.to_string()];
    for (k, v) in args.as_object().unwrap() {
        argv.push(format!("--{k}"));
        argv.push(match v { V::String(s) => s.clone(), other => other.to_string() });
    }
    let started = std::time::Instant::now();
    let out = Command::new(exe).args(&argv).stdin(Stdio::null())
        .stdout(Stdio::piped()).stderr(Stdio::piped()).output()
        .map_err(|_| "INTENT_PREPARATION_CHILD_UNAVAILABLE".to_string())?;
    if !out.status.success() || out.stdout.len() > 65536 || out.stderr.len() > 2048 {
        return Err("INTENT_PREPARATION_NATIVE_FAILED".into());
    }
    Ok((started.elapsed().as_secs_f64() * 1000.0,
        serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?))
}
pub fn run() -> Result<V, String> {
    let r = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let old_active_bytes = std::fs::read(format!("{r}/native-dip-cache-active.json")).map_err(|e| e.to_string())?;
    let old_active: V = serde_json::from_slice(&old_active_bytes).map_err(|e| e.to_string())?;
    let old_dir = format!("{r}/native-capture-caches/{}", old_active["cacheDirectory"].as_str().unwrap_or(""));
    let old_binding = read(&format!("{old_dir}/binding.json"))?;
    let old_pointer = read(&format!("{old_dir}/host-latest.json"))?;
    let old_bytes = std::fs::read(format!("{old_dir}/{}", old_pointer["output"].as_str().unwrap_or(""))).map_err(|e| e.to_string())?;
    let old: V = serde_json::from_slice(&old_bytes).map_err(|e| e.to_string())?;
    let cohort = old_active["cohort"].as_str().unwrap_or("");
    let d = format!("{r}/research-cohorts/{cohort}/declaration.json");
    let declaration = read(&d)?;
    let universe = validate_universe(
        &read("live-paper/price-universe.json")?,
        declaration["config"]["tournamentId"].as_str().unwrap_or(""),
    )?;
    let journal = format!("{r}/journal.jsonl");
    if Path::new(&format!("{r}/native-dip-intent-active.json")).exists() {
        return Err("Existing intent continuation retained".into());
    }
    assert_eq(&json!(sha(&old_bytes)), &old_pointer["outputHash"])?;
    assert_eq(
        &json!(sha(&std::fs::read(format!("{old_dir}/binding.json")).map_err(|e| e.to_string())?)),
        &old_active["bindingHash"],
    )?;
    verify_cache_proof(&old, &old_binding, &old_pointer["prior"])?;
    verify_native_dip_result(&old, &declaration, &old_binding, &universe, None)?;
    let graph = ["Cargo.toml", "Cargo.lock", "build.rs", ".cargo/config.toml", "src/main.rs",
        "src/money.rs", "src/types.rs", "src/capture.rs", "src/cache.rs", "src/engine.rs",
        "src/intent_observation.rs", "src/replay.rs", "src/trace.rs", "src/sweep.rs",
        "src/evaluation.rs", "src/publication.rs"];
    let mut h = Sha256::new();
    let mut source_hashes = serde_json::Map::<String, V>::new();
    for f in &graph {
        let b = std::fs::read(format!("backend-rust/{f}")).map_err(|e| e.to_string())?;
        h.update((f.len() as u64).to_le_bytes());
        h.update(f.as_bytes());
        h.update((b.len() as u64).to_le_bytes());
        h.update(&b);
        source_hashes.insert(format!("backend-rust/{f}"), json!(sha(&b)));
    }
    let engine_hash = format!("{:x}", h.finalize());
    let exe_dir = native_executable().parent().unwrap().to_path_buf();
    let binary_hash = sha(&std::fs::read(native_executable()).map_err(|e| e.to_string())?);
    let runtime_library_hash = sha(&std::fs::read(exe_dir.join("libunwind.dll")).map_err(|e| e.to_string())?);
    let build = format!("{r}/native-builds/{binary_hash}");
    let archive = format!("{r}/native-sources/{engine_hash}");
    let cache_directory = uuid::Uuid::new_v4().to_string();
    let dir = format!("{r}/native-capture-caches/{cache_directory}");
    for x in [&build, &archive, &dir] {
        std::fs::create_dir_all(x).map_err(|e| e.to_string())?;
    }
    for f in ["baal-backend.exe", "libunwind.dll"] {
        copy_exclusive(&exe_dir.join(f), &PathBuf::from(format!("{build}/{f}")))?;
    }
    for f in &graph {
        copy_exclusive(&PathBuf::from(format!("backend-rust/{f}")), &PathBuf::from(format!("{archive}/{f}")))?;
    }
    for f in ["src/engine.rs", "src/money.rs", "src/replay.rs", "src/sweep.rs", "src/trace.rs",
        "src/types.rs", "src/capture.rs", "src/cache.rs"]
    {
        assert_eq(&source_hashes[&format!("backend-rust/{f}")], &old_binding["sourceHashes"][&format!("backend-rust/{f}")])?;
    }
    let exe = PathBuf::from(format!("{build}/baal-backend.exe"));
    let source = json!({
        "sourceBytes": old["capture"]["source_bytes"], "sourceHash": old["capture"]["source_hash"],
    });
    let common = json!({
        "journal": journal, "declaration": d, "end-byte": source["sourceBytes"],
        "expected-sha": source["sourceHash"],
    });
    let cache_file = format!("{dir}/capture.json");
    let mut args = common.clone();
    args.as_object_mut().unwrap().insert("out".into(), json!(cache_file));
    let (cache_ms, cache) = run_native(&exe, "prepare-cache", &args)?;
    assert_eq(&cache["engineHash"], &json!(engine_hash))?;
    assert_eq(&json!(sha(&std::fs::read(&cache_file).map_err(|e| e.to_string())?)), &cache["cacheHash"])?;
    let full_file = format!("{dir}/full-observed.json");
    let mut args = common.clone();
    args.as_object_mut().unwrap().insert("workers".into(), json!(8));
    args.as_object_mut().unwrap().insert("out".into(), json!(full_file));
    let (full_ms, _) = run_native(&exe, "evaluate-observed", &args)?;
    let full = read(&full_file)?;
    assert_eq(&full["capture"]["engine_hash"], &json!(engine_hash))?;
    let stripped: Vec<V> = full["evaluation"]["scorecards"].as_array().unwrap().iter().map(|c| {
        let mut c = c.clone();
        c.as_object_mut().unwrap().remove("pendingReadIntents");
        c
    }).collect();
    assert_eq(&json!(stripped), &old["evaluation"]["scorecards"])?;
    for k in ["source_bytes", "source_hash", "source_records", "first_at", "through",
        "cycles", "accepted_books", "invalid_books"]
    {
        assert_eq(&full["capture"][k], &old["capture"][k])?;
    }
    let seed_file = format!("{dir}/seed.json");
    let mut args = common.clone();
    args.as_object_mut().unwrap().insert("workers".into(), json!(8));
    args.as_object_mut().unwrap().insert("out".into(), json!(seed_file));
    args.as_object_mut().unwrap().insert("cache".into(), json!(cache_file));
    args.as_object_mut().unwrap().insert("cache-sha".into(), cache["cacheHash"].clone());
    args.as_object_mut().unwrap().insert("cache-source-sha".into(), cache["sourceHash"].clone());
    args.as_object_mut().unwrap().insert("cache-end-byte".into(), cache["sourceBytes"].clone());
    args.as_object_mut().unwrap().insert("prior-end-byte".into(), source["sourceBytes"].clone());
    args.as_object_mut().unwrap().insert("prior-sha".into(), source["sourceHash"].clone());
    let (seed_ms, _) = run_native(&exe, "evaluate-observed-cached", &args)?;
    let seed = read(&seed_file)?;
    assert_eq(&seed["evaluation"]["scorecards"], &full["evaluation"]["scorecards"])?;
    let binding = json!({
        "schemaVersion": 1, "cohort": cohort, "engineHash": engine_hash,
        "binaryHash": binary_hash, "runtimeLibraryHash": runtime_library_hash,
        "declarationHash": sha(&std::fs::read(&d).map_err(|e| e.to_string())?),
        "sourceHashes": V::Object(source_hashes.clone()),
        "tradingEngineSourceUnchanged": true, "evaluationSerializationOnly": true,
        "cache": {
            "directory": cache_directory, "file": "capture.json",
            "cacheHash": cache["cacheHash"], "cacheBytes": cache["cacheBytes"],
            "sourceHash": cache["sourceHash"], "sourceBytes": cache["sourceBytes"],
        },
        "predecessor": {
            "activeHash": sha(&old_active_bytes), "bindingHash": old_active["bindingHash"],
            "output": old_pointer["output"], "outputHash": old_pointer["outputHash"],
            "engineHash": old_binding["engineHash"], "binaryHash": old_binding["binaryHash"],
            "sourceHash": source["sourceHash"], "sourceBytes": source["sourceBytes"],
        },
        "parity": {"originalScorecardsExact": 100, "observedCachedScorecardsExact": 100},
    });
    assert_eq(&json!(seed["evaluation"]["scorecards"].as_array().map(|a| a.len()).unwrap_or(0)), &json!(100))?;
    verify_cache_proof(&seed, &binding, &source)?;
    let projection = verify_native_intent_result(&seed, &declaration, &binding, &universe, Some(&old))?;
    let seed_bytes = std::fs::read(&seed_file).map_err(|e| e.to_string())?;
    let output_hash = sha(&seed_bytes);
    let binding_bytes = format!("{}\n", serde_json::to_string_pretty(&binding).unwrap());
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{dir}/binding.json"))
        .and_then(|mut f| f.write_all(binding_bytes.as_bytes())).map_err(|e| e.to_string())?;
    let receipt = json!({
        "at": ms_to_iso(now_ms() as f64), "binaryHash": binary_hash, "engineHash": engine_hash,
        "runtimeLibraryHash": runtime_library_hash,
        "declarationFileHash": binding["declarationHash"],
        "sourceHash": source["sourceHash"], "sourceBytes": source["sourceBytes"],
        "outputHash": output_hash, "cacheHash": cache["cacheHash"],
        "networkRequests": 0, "competitionOrders": 0,
    });
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{seed_file}.receipt.json"))
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&receipt).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let active = json!({
        "schemaVersion": 1, "mode": "frozen_native_dip_intent_host_v3", "cohort": cohort,
        "cacheDirectory": cache_directory, "bindingHash": sha(binding_bytes.as_bytes()),
        "intervalMs": 30000, "workers": 8,
        "hostSourceHashes": native_dip_intent_host_identity(Path::new("."))?,
        "seed": {"output": "seed.json", "outputHash": output_hash, "binaryHash": binary_hash,
            "prior": source},
    });
    let active_bytes = format!("{}\n", serde_json::to_string_pretty(&active).unwrap());
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{r}/native-dip-intent-active.json"))
        .and_then(|mut f| f.write_all(active_bytes.as_bytes())).map_err(|e| e.to_string())?;
    let ranges = |k: &str| -> V {
        let vals: Vec<f64> = seed["evaluation"]["scorecards"].as_array().unwrap().iter()
            .map(|c| c["signalDiagnostics"][k].as_f64().unwrap_or(f64::NAN)).collect();
        json!({"min": vals.iter().cloned().fold(f64::INFINITY, f64::min),
               "max": vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max)})
    };
    let e = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "claim": 5981840605i64,
        "status": "observed_native_continuation_prepared", "cohort": cohort,
        "declarationHash": binding["declarationHash"], "engineHash": engine_hash,
        "binaryHash": binary_hash, "runtimeLibraryHash": runtime_library_hash,
        "sourceHashes": V::Object(source_hashes), "activeHash": sha(active_bytes.as_bytes()),
        "hostSourceHashes": active["hostSourceHashes"],
        "originalScorecardsExact": 100, "observedCachedScorecardsExact": 100,
        "source": source,
        "cache": {"bytes": cache["cacheBytes"], "hash": cache["cacheHash"]},
        "timing": {"cachePreparationMs": cache_ms, "fullObservedMs": full_ms,
            "cachedObservedMs": seed_ms},
        "pendingMarketIds": projection["pendingMarketIds"],
        "pendingIntents": projection["pendingReadIntents"].as_array().map(|a| a.len()).unwrap_or(0),
        "heldMarketIds": projection["heldMarketIds"],
        "signalDiagnosticsRanges": {
            "warmup": ranges("warmup"), "flatHistory": ranges("flatHistory"),
            "belowZThreshold": ranges("belowZThreshold"),
            "costAdjustedMarginFail": ranges("costAdjustedMarginFail"),
            "edgeQualified": ranges("edgeQualified"),
        },
        "frozenTradingEngineUnchanged": true, "oldBinaryCacheAndOutputsPreserved": true,
        "parametersTuned": false, "walletsReset": false, "fees": 0, "competitionOrders": 0,
        "profitabilityEstablished": false,
    });
    atomic_json(Path::new("data/native-intent-preparation-20261004.json"), &e)?;
    Ok(e)
}
