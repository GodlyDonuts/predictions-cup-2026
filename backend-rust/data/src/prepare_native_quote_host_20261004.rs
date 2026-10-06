// data/prepare-native-quote-host-20261004.mjs — declare the causal quote
// history cohort, seed it natively, and install the v1 quote host pointer.
use crate::publication_pack::{now_ms, read_json, runtime_dir, sha};
use baal_collector::breadth::validate_universe;
use baal_collector::native_journal_restore::source_prefix_hash;
use baal_collector::native_quote_host_v1::{native_quote_host_identity, quote_source_prefix};
use baal_collector::native_quote_proof_v1::verify_native_quote_result;
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
fn graph_files() -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string("backend-rust/build.rs").map_err(|e| e.to_string())?;
    Ok(text.lines().filter_map(|l| {
        let t = l.trim();
        if t.starts_with('"') && t.ends_with("\",") { Some(t[1..t.len() - 2].to_string()) } else { None }
    }).collect())
}
fn copy(source: &Path, target: &Path) -> Result<(), String> {
    let b = std::fs::read(source).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
    if target.exists() {
        assert_eq(&json!(sha(&std::fs::read(target).map_err(|e| e.to_string())?)), &json!(sha(&b)))
    } else {
        std::fs::OpenOptions::new().write(true).create_new(true).open(target)
            .and_then(|mut f| f.write_all(&b)).map_err(|e| e.to_string())
    }
}
fn run_native(exe: &Path, command: &str, args: &V) -> Result<(f64, V, V), String> {
    let mut argv: Vec<String> = vec![command.to_string()];
    for (k, v) in args.as_object().unwrap() {
        argv.push(format!("--{k}"));
        argv.push(match v { V::String(s) => s.clone(), other => other.to_string() });
    }
    let started = std::time::Instant::now();
    let out = Command::new(exe).args(&argv).stdin(Stdio::null())
        .stdout(Stdio::piped()).stderr(Stdio::piped()).output()
        .map_err(|_| "QUOTE_PREPARATION_CHILD_UNAVAILABLE".to_string())?;
    if !out.status.success() || out.stdout.len() > 65536 || out.stderr.len() > 4096 {
        let reason = serde_json::from_slice::<V>(&out.stderr).ok()
            .and_then(|v| v["error"].as_str().map(String::from))
            .unwrap_or_else(|| "NATIVE_FAILED".to_string());
        return Err(format!("QUOTE_PREPARATION_{reason}"));
    }
    Ok((started.elapsed().as_secs_f64() * 1000.0,
        serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?, V::Null))
}
pub fn run() -> Result<V, String> {
    let r = runtime_dir();
    let read = |f: &str| read_json(Path::new(f));
    let cohort = "baal-quote-history-20261004-v1";
    let active_file = format!("{r}/native-quote-active.json");
    if Path::new(&active_file).exists() {
        return Err("Existing frozen quote cohort retained".into());
    }
    let old_active = read(&format!("{r}/native-dip-intent-active.json"))?;
    let old_dir = format!("{r}/native-capture-caches/{}", old_active["cacheDirectory"].as_str().unwrap_or(""));
    let old_binding = read(&format!("{old_dir}/binding.json"))?;
    let old_pointer = read(&format!("{old_dir}/host-latest.json"))?;
    let old = read(&format!("{old_dir}/{}", old_pointer["output"].as_str().unwrap_or("")))?;
    assert_eq(
        &json!(sha(&std::fs::read(format!("{old_dir}/{}", old_pointer["output"].as_str().unwrap_or(""))).map_err(|e| e.to_string())?)),
        &old_pointer["outputHash"],
    )?;
    let graph = graph_files()?;
    let mut h = Sha256::new();
    let mut source_hashes = serde_json::Map::<String, V>::new();
    if !graph.iter().any(|f| f == "src/quote_replay.rs") { return Err("assert".into()); }
    for f in &graph {
        let b = std::fs::read(format!("backend-rust/{f}")).map_err(|e| e.to_string())?;
        h.update((f.len() as u64).to_le_bytes());
        h.update(f.as_bytes());
        h.update((b.len() as u64).to_le_bytes());
        h.update(&b);
        source_hashes.insert(format!("backend-rust/{f}"), json!(sha(&b)));
    }
    assert_eq(&source_hashes["backend-rust/src/engine.rs"], &old_binding["sourceHashes"]["backend-rust/src/engine.rs"])?;
    assert_eq(&source_hashes["backend-rust/src/evaluation.rs"], &old_binding["sourceHashes"]["backend-rust/src/evaluation.rs"])?;
    let engine_hash = format!("{:x}", h.finalize());
    let exe_dir = native_executable().parent().unwrap().to_path_buf();
    let binary_hash = sha(&std::fs::read(native_executable()).map_err(|e| e.to_string())?);
    let runtime_library_hash = sha(&std::fs::read(exe_dir.join("libunwind.dll")).map_err(|e| e.to_string())?);
    let build = format!("{r}/native-builds/{binary_hash}");
    let archive = format!("{r}/native-sources/{engine_hash}");
    let dir = format!("{r}/research-cohorts/{cohort}");
    if Path::new(&dir).exists() { return Err("Existing cohort retained".into()); }
    for x in [&build, &archive, &dir] {
        std::fs::create_dir_all(x).map_err(|e| e.to_string())?;
    }
    for f in &graph {
        copy(&PathBuf::from(format!("backend-rust/{f}")), &PathBuf::from(format!("{archive}/{f}")))?;
    }
    for f in ["baal-backend.exe", "libunwind.dll"] {
        copy(&exe_dir.join(f), &PathBuf::from(format!("{build}/{f}")))?;
    }
    let universe = if old["evaluation"].is_object() {
        validate_universe(&read("live-paper/price-universe.json")?,
            read(&format!("{r}/research-cohorts/{}/declaration.json", old_active["cohort"].as_str().unwrap_or("")))?["config"]["tournamentId"].as_str().unwrap_or(""))?
    } else { V::Null };
    let journals = json!({
        "books": format!("{r}/public-books.jsonl"),
        "quotes": format!("{r}/market-prices.jsonl"),
        "eligibility": format!("{r}/account-eligibility.jsonl"),
    });
    let eligibility_path = journals["eligibility"].as_str().unwrap();
    if !Path::new(eligibility_path).exists() {
        std::fs::OpenOptions::new().write(true).create_new(true).open(eligibility_path)
            .and_then(|mut f| f.write_all(b"")).map_err(|e| e.to_string())?;
    }
    let mut sources = serde_json::Map::<String, V>::new();
    let mut development = serde_json::Map::<String, V>::new();
    for (name, file) in journals.as_object().unwrap() {
        let file = file.as_str().unwrap();
        let prefix = quote_source_prefix(Path::new(file))?;
        let hash = if prefix.bytes > 0 {
            source_prefix_hash(Path::new(file), prefix.bytes, None)?
        } else { sha(b"") };
        development.insert(name.clone(), json!({"bytes": prefix.bytes, "hash": hash}));
        sources.insert(name.clone(), json!({
            "path": file, "bytes": prefix.bytes, "hash": hash,
            "checkpoints": [development[name].clone()],
        }));
    }
    let mut config = read(&format!("{r}/research-cohorts/{}/declaration.json", old_active["cohort"].as_str().unwrap_or("")))?["config"].clone();
    for (k, v) in [
        ("marketIds", json!(universe["exchanges"].as_array().map(|a| a.iter().map(|x| x["marketId"].clone()).collect::<Vec<V>>()).unwrap_or_default())),
        ("feeBps", json!(0)), ("initialCash", json!("100.000000")),
        ("cashReserve", json!("50.000000")), ("maxExposure", json!("5.000000")),
        ("orderShares", json!(1)), ("maxPositionShares", json!(1)),
        ("latencyMs", json!(3000)), ("maxDataAgeMs", json!(20000)),
        ("cycleIntervalMs", json!(30000)), ("stopLoss", json!("0.050000")),
        ("fillClockPolicy", json!("independent_request_latency_v2")),
    ] { config.as_object_mut().unwrap().insert(k.into(), v); }
    let mut strategies = Vec::new();
    for lookback_ms in [3600000i64, 10800000, 21600000] {
        for recovery_ticks in [0, 1] {
            for minimum_edge in ["0.020000", "0.040000"] {
                for horizon_ms in [1800000i64, 7200000] {
                    strategies.push(json!({
                        "id": format!("q{}h_r{}_e{}_h{}m", lookback_ms / 3600000, recovery_ticks,
                            if minimum_edge == "0.020000" { 2 } else { 4 }, horizon_ms / 60000),
                        "parameters": {
                            "lookbackMs": lookback_ms, "minimumMinutes": 30,
                            "minimumSpanMs": 1800000, "minimumEdge": minimum_edge,
                            "recoveryTicks": recovery_ticks, "horizonMs": horizon_ms,
                            "takeProfit": if horizon_ms == 1800000 { "0.010000" } else { "0.020000" },
                        },
                    }));
                }
            }
        }
    }
    let declaration = json!({
        "schemaVersion": 1, "mode": "causal_quote_dip_subsequent_v1", "cohort": cohort,
        "declaredAt": ms_to_iso((now_ms() + 30000) as f64), "config": config,
        "universe": universe, "development": V::Object(development),
        "maxManagedMarkets": 1, "strategies": strategies,
        "scenarios": [
            {"id": "baseline_3s", "latencyMs": 3000, "slippageTicks": 1, "depthFractionBps": 100},
            {"id": "delay_10s", "latencyMs": 10000, "slippageTicks": 1, "depthFractionBps": 100},
            {"id": "stress_30s", "latencyMs": 30000, "slippageTicks": 2, "depthFractionBps": 50},
        ],
    });
    let declaration_file = format!("{dir}/declaration.json");
    let sources_file = format!("{dir}/seed.sources.json");
    std::fs::OpenOptions::new().write(true).create_new(true).open(&declaration_file)
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&declaration).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(&sources_file)
        .and_then(|mut f| f.write_all(serde_json::to_string(&V::Object(sources.clone())).unwrap().as_bytes()))
        .map_err(|e| e.to_string())?;
    let exe = PathBuf::from(format!("{build}/baal-backend.exe"));
    let seed_file = format!("{dir}/seed.json");
    let (seed_ms, seed_receipt, _) = run_native(&exe, "quote-replay", &json!({
        "declaration": declaration_file, "sources": sources_file,
        "workers": 8, "out": seed_file,
    }))?;
    let seed = read(&seed_file)?;
    assert_eq(&seed["capture"]["engineHash"], &json!(engine_hash))?;
    assert_eq(&json!(seed["evaluation"]["scorecards"].as_array().map(|a| a.len()).unwrap_or(0)), &json!(72))?;
    if !seed["evaluation"]["scorecards"].as_array().unwrap().iter()
        .all(|c| c["fills"].as_array().map(|f| f.is_empty()).unwrap_or(false))
    { return Err("assert".into()); }
    let original_file = format!("{dir}/original-equivalence.json");
    let (original_ms, _, _) = run_native(&exe, "evaluate-observed", &json!({
        "declaration": format!("{r}/research-cohorts/{}/declaration.json", old_active["cohort"].as_str().unwrap_or("")),
        "journal": format!("{r}/journal.jsonl"),
        "end-byte": old["capture"]["source_bytes"], "expected-sha": old["capture"]["source_hash"],
        "workers": 8, "out": original_file,
    }))?;
    let original = read(&original_file)?;
    assert_eq(&original["evaluation"]["scorecards"], &old["evaluation"]["scorecards"])?;
    let binding = json!({
        "schemaVersion": 1, "cohort": cohort, "engineHash": engine_hash,
        "binaryHash": binary_hash, "runtimeLibraryHash": runtime_library_hash,
        "declarationHash": sha(&std::fs::read(&declaration_file).map_err(|e| e.to_string())?),
        "sourceHashes": V::Object(source_hashes.clone()),
        "originalEngineUnchanged": true, "originalScorecardsExact": 100,
    });
    verify_native_quote_result(&seed, &declaration, &binding, &universe, None)?;
    let output_hash = sha(&std::fs::read(&seed_file).map_err(|e| e.to_string())?);
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{dir}/binding.json"))
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&binding).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let receipt = json!({
        "at": ms_to_iso(now_ms() as f64), "binaryHash": binary_hash, "engineHash": engine_hash,
        "declarationHash": binding["declarationHash"], "sources": seed["capture"]["sources"],
        "outputHash": output_hash, "networkRequests": 0, "competitionOrders": 0,
    });
    std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{seed_file}.receipt.json"))
        .and_then(|mut f| f.write_all(format!("{}\n", serde_json::to_string_pretty(&receipt).unwrap()).as_bytes()))
        .map_err(|e| e.to_string())?;
    let active = json!({
        "schemaVersion": 1, "mode": "frozen_native_quote_history_host_v1", "cohort": cohort,
        "bindingHash": sha(&std::fs::read(format!("{dir}/binding.json")).map_err(|e| e.to_string())?),
        "intervalMs": 30000, "workers": 8,
        "hostSourceHashes": native_quote_host_identity(Path::new("."))?,
        "seed": {"output": "seed.json", "outputHash": output_hash},
    });
    let active_bytes = format!("{}\n", serde_json::to_string_pretty(&active).unwrap());
    std::fs::OpenOptions::new().write(true).create_new(true).open(&active_file)
        .and_then(|mut f| f.write_all(active_bytes.as_bytes())).map_err(|e| e.to_string())?;
    let evidence = json!({
        "schemaVersion": 1, "at": ms_to_iso(now_ms() as f64), "claimComment": 5982065901i64,
        "status": "native_quote_history_cohort_prepared", "cohort": cohort,
        "declaredAt": declaration["declaredAt"], "declarationHash": binding["declarationHash"],
        "engineHash": engine_hash, "binaryHash": binary_hash,
        "runtimeLibraryHash": runtime_library_hash,
        "sourceHashes": V::Object(source_hashes), "activeHash": sha(active_bytes.as_bytes()),
        "hostSourceHashes": active["hostSourceHashes"],
        "markets": universe["exchanges"].as_array().map(|a| a.len()).unwrap_or(0),
        "candidates": 72, "strategies": 24, "scenarios": 3,
        "sourceBytes": sources.values().map(|s| s["bytes"].as_u64().unwrap_or(0)).sum::<u64>(),
        "sourceDays": seed_receipt["sourceDays"], "sourceEvents": seed["capture"]["events"],
        "sources": seed["capture"]["sources"],
        "timing": {"compactWallMs": seed_ms, "compactVerifyMs": seed_receipt["captureMs"],
            "compactReplayMs": seed_receipt["replayMs"], "originalFullWallMs": original_ms},
        "originalScorecardsExact": 100, "originalEngineUnchanged": true, "fees": 0,
        "originalWalletsReset": false, "parametersFitted": false, "prospectiveFills": 0,
        "competitionOrders": 0, "profitabilityEstablished": false,
    });
    atomic_json(Path::new("data/native-quote-preparation-20261004.json"), &evidence)?;
    Ok(json!({
        "cohort": cohort, "candidates": 72, "markets": evidence["markets"],
        "sourceDays": evidence["sourceDays"], "sourceBytes": evidence["sourceBytes"],
        "compactWallMs": seed_ms, "originalFullWallMs": original_ms,
        "originalScorecardsExact": 100, "fees": 0, "profitabilityEstablished": false,
    }))
}
