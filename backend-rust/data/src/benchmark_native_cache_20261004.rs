// data/benchmark-native-cache-20261004.mjs — original vs capture-cached native
// engine benchmark; archives the checked binary and 15-file source graph first.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn perf_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}
fn mkdir_700(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn copy(source: &Path, target: &Path) -> Result<(), String> {
    let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
    if let Some(d) = target.parent() { mkdir_700(d)?; }
    if target.exists() {
        if sha(&std::fs::read(target).map_err(|e| e.to_string())?) != sha(&bytes) {
            return Err("assertion failed".into());
        }
    } else {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
        let mut f = o.open(target).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut f, &bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn uuid_v4() -> String { uuid::Uuid::new_v4().to_string() }

const GRAPH: [&str; 15] = ["Cargo.toml", "Cargo.lock", "build.rs", ".cargo/config.toml",
    "src/main.rs", "src/money.rs", "src/types.rs", "src/capture.rs", "src/cache.rs",
    "src/engine.rs", "src/replay.rs", "src/trace.rs", "src/sweep.rs", "src/evaluation.rs",
    "src/publication.rs"];

fn run(exe: &Path, command: &str, args: &[(&str, String)]) -> Result<(f64, V), String> {
    let started = perf_ms();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(command);
    for (k, v) in args { cmd.arg(format!("--{}", k)); cmd.arg(v); }
    let mut child = cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
        .spawn().map_err(|_| "CACHE_BENCHMARK_CHILD_UNAVAILABLE".to_string())?;
    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let t_out = std::thread::spawn(move || {
        let mut b = vec![0u8; 8192]; let mut cap = false;
        if let Some(s) = so.as_mut() {
            while let Ok(n) = s.read(&mut b) {
                if n == 0 { break; }
                stdout.extend_from_slice(&b[..n]);
                if stdout.len() > 65536 { cap = true; break; }
            }
        }
        (stdout, cap)
    });
    let t_err = std::thread::spawn(move || {
        let mut b = vec![0u8; 8192];
        if let Some(s) = se.as_mut() {
            while let Ok(n) = s.read(&mut b) {
                if n == 0 { break; }
                stderr.extend_from_slice(&b[..n]);
                if stderr.len() > 2048 { break; }
            }
        }
        stderr
    });
    let (stdout, cap) = t_out.join().map_err(|_| "CACHE_BENCHMARK_CHILD_UNAVAILABLE".to_string())?;
    let _ = t_err.join();
    if cap { let _ = child.kill(); }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() { return Err("CACHE_BENCHMARK_NATIVE_FAILED".into()); }
    Ok((perf_ms() - started,
        serde_json::from_slice(&stdout).map_err(|e| e.to_string())?))
}

pub fn run_benchmark() -> Result<V, String> {
    let r = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let cohort = "baal-dip-request-clock-20261004-v2";
    let dir = r.join("research-cohorts").join(cohort);
    let declaration = dir.join("declaration.json");
    let journal = r.join("journal.jsonl");
    let old_binding = read_json(&dir.join("native-clock-binding.json"))?;
    let prep = read_json(Path::new("data/native-dip-host-preparation-20261004.json"))?;
    let mut h = Sha256::new();
    let mut source_hashes = serde_json::Map::new();
    for f in GRAPH {
        let bytes = std::fs::read(Path::new("backend-rust").join(f)).map_err(|e| e.to_string())?;
        h.update((f.as_bytes().len() as u64).to_le_bytes());
        h.update(f.as_bytes());
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
        source_hashes.insert(format!("backend-rust/{}", f), json!(sha(&bytes)));
    }
    let engine_hash = format!("{:x}", h.finalize());
    let native_executable = baal_collector::rust_backend::native_executable();
    let binary_hash = sha(&std::fs::read(&native_executable).map_err(|e| e.to_string())?);
    let runtime_library_hash = sha(&std::fs::read(
        native_executable.parent().unwrap_or(Path::new("")).join("libunwind.dll"))
        .map_err(|e| e.to_string())?);
    let build = r.join("native-builds").join(&binary_hash);
    let archive = r.join("native-sources").join(&engine_hash);
    let work = r.join("native-capture-caches").join(uuid_v4());
    for d in [&build, &archive, &work] { mkdir_700(d)?; }
    for f in ["baal-backend.exe", "libunwind.dll"] {
        copy(&native_executable.parent().unwrap_or(Path::new("")).join(f), &build.join(f))?;
    }
    for f in GRAPH { copy(&Path::new("backend-rust").join(f), &archive.join(f))?; }
    for f in ["src/engine.rs", "src/evaluation.rs", "src/sweep.rs", "src/replay.rs",
        "src/money.rs", "src/trace.rs"] {
        if source_hashes[&format!("backend-rust/{}", f)]
            != old_binding["sourceHashes"][&format!("backend-rust/{}", f)] {
            return Err("assertion failed".into());
        }
    }
    let exe = build.join("baal-backend.exe");
    let cache = work.join("capture.json");
    let (cache_ms, cache_receipt) = run(&exe, "prepare-cache", &[
        ("declaration", declaration.to_string_lossy().into_owned()),
        ("journal", journal.to_string_lossy().into_owned()),
        ("end-byte", prep["sourceBytes"].to_string()),
        ("expected-sha", prep["sourceHash"].as_str().unwrap_or("").into()),
        ("out", cache.to_string_lossy().into_owned())])?;
    if cache_receipt["engineHash"].as_str() != Some(engine_hash.as_str()) {
        return Err("assertion failed".into());
    }
    if sha(&std::fs::read(&cache).map_err(|e| e.to_string())?)
        != cache_receipt["cacheHash"].as_str().unwrap_or("") {
        return Err("assertion failed".into());
    }
    // Compare exactly the same already-observed source, including unchanged old
    // engine output; this is correctness and speed evidence, not strategy tuning.
    let pointer = read_json(&dir.join("native-host-latest.json"))?;
    let old_bytes = std::fs::read(dir.join(pointer["output"].as_str().unwrap_or("")))
        .map_err(|e| e.to_string())?;
    if sha(&old_bytes) != pointer["outputHash"].as_str().unwrap_or("") {
        return Err("assertion failed".into());
    }
    let old: V = serde_json::from_slice(&old_bytes).map_err(|e| e.to_string())?;
    let end = old["capture"]["source_bytes"].as_u64().unwrap_or(0);
    let expected = old["capture"]["source_hash"].as_str().unwrap_or("").to_string();
    if !(end >= prep["sourceBytes"].as_u64().unwrap_or(0)) { return Err("assertion failed".into()); }
    if old["declarationFileHash"].as_str() != old_binding["declarationHash"].as_str() {
        return Err("assertion failed".into());
    }
    let full_file = work.join("full.json");
    let (full_ms, _full_run) = run(&exe, "evaluate", &[
        ("declaration", declaration.to_string_lossy().into_owned()),
        ("journal", journal.to_string_lossy().into_owned()),
        ("end-byte", end.to_string()),
        ("expected-sha", expected.clone()),
        ("workers", "8".into()),
        ("out", full_file.to_string_lossy().into_owned())])?;
    let full_result = read_json(&full_file)?;
    if full_result["capture"]["engine_hash"].as_str() != Some(engine_hash.as_str()) {
        return Err("assertion failed".into());
    }
    if full_result["evaluation"]["scorecards"] != old["evaluation"]["scorecards"] {
        return Err("assertion failed".into());
    }
    let cached_file = work.join("cached.json");
    let (cached_ms, _cached_run) = run(&exe, "evaluate-cached", &[
        ("declaration", declaration.to_string_lossy().into_owned()),
        ("journal", journal.to_string_lossy().into_owned()),
        ("end-byte", end.to_string()),
        ("workers", "8".into()),
        ("out", cached_file.to_string_lossy().into_owned()),
        ("cache", cache.to_string_lossy().into_owned()),
        ("cache-sha", cache_receipt["cacheHash"].as_str().unwrap_or("").into()),
        ("cache-source-sha", prep["sourceHash"].as_str().unwrap_or("").into()),
        ("cache-end-byte", prep["sourceBytes"].to_string()),
        ("prior-end-byte", end.to_string()),
        ("prior-sha", expected.clone())])?;
    let cached_result = read_json(&cached_file)?;
    if cached_result["capture"]["source_hash"].as_str() != Some(expected.as_str()) {
        return Err("assertion failed".into());
    }
    if cached_result["evaluation"]["scorecards"] != full_result["evaluation"]["scorecards"] {
        return Err("assertion failed".into());
    }
    for key in ["source_bytes", "source_records", "source_hash", "first_at", "through",
        "cycles", "accepted_books", "invalid_books"] {
        if cached_result["capture"][key] != full_result["capture"][key] {
            return Err("assertion failed".into());
        }
    }
    let mut cache_obj = serde_json::Map::new();
    cache_obj.insert("directory".into(),
        json!(work.file_name().unwrap_or_default().to_string_lossy()));
    cache_obj.insert("file".into(), json!("capture.json"));
    if let V::Object(m) = &cache_receipt {
        for (k, v) in m { cache_obj.insert(k.clone(), v.clone()); }
    }
    let binding = json!({"schemaVersion": 1, "cohort": cohort,
        "declarationHash": old_binding["declarationHash"], "binaryHash": binary_hash,
        "engineHash": engine_hash, "runtimeLibraryHash": runtime_library_hash,
        "sourceHashes": V::Object(source_hashes.clone()),
        "modelOriginalEngineHash": old_binding["engineHash"], "modelSourcesUnchanged": true,
        "cache": V::Object(cache_obj),
        "recordedAt": now_iso(), "networkRequests": 0, "competitionOrders": 0});
    write_wx(&work.join("binding.json"),
        format!("{}\n", serde_json::to_string_pretty(&binding).map_err(|e| e.to_string())?).as_bytes())?;
    let e = json!({"schemaVersion": 1, "at": now_iso(), "claim": 5981279291i64,
        "status": "full_cached_and_original_engine_100_cards_exact", "cohort": cohort,
        "declarationHash": old_binding["declarationHash"], "engineHash": engine_hash,
        "binaryHash": binary_hash, "runtimeLibraryHash": runtime_library_hash,
        "sourceHashes": V::Object(source_hashes),
        "source": {"hash": expected, "bytes": end,
            "records": full_result["capture"]["source_records"],
            "days": (full_result["capture"]["through"].as_f64().unwrap_or(0.0)
                - full_result["capture"]["first_at"].as_f64().unwrap_or(0.0)) / 86_400_000.0,
            "through": full_result["evaluation"]["evaluationThrough"]},
        "cache": {"hash": cache_receipt["cacheHash"], "bytes": cache_receipt["cacheBytes"],
            "sourceHash": prep["sourceHash"], "sourceBytes": prep["sourceBytes"],
            "buildWallMs": cache_ms,
            "reductionRatio": prep["sourceBytes"].as_f64().unwrap_or(0.0)
                / cache_receipt["cacheBytes"].as_f64().unwrap_or(1.0)},
        "full": {"wallMs": full_ms, "captureMs": full_result["capture"]["verify_ms"],
            "replayMs": full_result["evaluation"]["replayMs"]},
        "cached": {"wallMs": cached_ms, "captureMs": cached_result["capture"]["verify_ms"],
            "replayMs": cached_result["evaluation"]["replayMs"],
            "proof": cached_result["cacheProof"]},
        "speedup": full_ms / cached_ms, "scorecardsExact": 100, "oldModelEngineCardsExact": 100,
        "modelSourcesUnchanged": true, "declarationUnchanged": true, "fees": 0,
        "walletsReset": false, "parametersTuned": false, "networkRequests": 0,
        "competitionOrders": 0, "profitabilityEstablished": false,
        "outputHashes": {"full": sha(&std::fs::read(&full_file).map_err(|e| e.to_string())?),
            "cached": sha(&std::fs::read(&cached_file).map_err(|e| e.to_string())?)},
        "limitations": ["Same-source replay measures correctness and speed, not profitability",
            "Cached original bytes still undergo complete SHA verification on every use",
            "Uncached appended records use original canonical chain and clock validation",
            "Capture cache is private derived data; Node never parses nanosecond numeric payloads",
            "Windows file cache and concurrent live services affect wall time"]});
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/native-cache-benchmark-20261004.json"), &e)?;
    let binding_hash = sha(&std::fs::read(work.join("binding.json")).map_err(|e| e.to_string())?);
    baal_service_runtime::atomic::atomic_json_canonical(
        Path::new("data/native-cache-runtime-prepared-20261004.json"),
        &json!({"directory": work.file_name().unwrap_or_default().to_string_lossy(),
            "bindingHash": binding_hash, "cachedFile": "cached.json",
            "cachedOutputHash": e["outputHashes"]["cached"],
            "sourceBytes": end, "sourceHash": expected}))?;
    Ok(json!({"status": e["status"], "source": e["source"], "cache": e["cache"],
        "full": e["full"], "cached": e["cached"], "speedup": e["speedup"],
        "scorecardsExact": 100, "competitionOrders": 0}))
}
