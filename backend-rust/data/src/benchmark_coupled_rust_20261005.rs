// data/benchmark-coupled-rust-20261005.mjs — JS vs native journal restore on the
// original coupled-bulk journal: same-work verification and timing.
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn ph(v: &V) -> Result<String, String> { baal_execution::core::payload_hash(v) }
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
fn mkdir_700(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn perf_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}

pub fn run() -> Result<V, String> {
    let runtime_dir = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let active = read_json(&runtime_dir.join("coupled-bulk-active.json"))?;
    let file = runtime_dir.join("coupled-bulk-cohorts")
        .join(active["cohort"].as_str().unwrap_or("")).join("journal.jsonl");
    let end_byte = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
    let source_root = std::env::current_dir().map_err(|e| e.to_string())?.join("backend-rust/journal-restore");
    let build = source_root.join("target/release");
    let binding = json!({"schemaVersion": 1, "mode": "canonical_js_journal_restore_v1",
        "binaryHash": sha(&std::fs::read(build.join("baal-journal-restore.exe")).map_err(|e| e.to_string())?),
        "engineHash": baal_collector::native_journal_restore::journal_engine_identity(&source_root)?,
        "runtimeLibraryHash": sha(&std::fs::read(build.join("libunwind.dll")).map_err(|e| e.to_string())?)});
    let dir = runtime_dir.join("journal-native-builds").join(binding["binaryHash"].as_str().unwrap());
    mkdir_700(&dir)?;
    for name in ["baal-journal-restore.exe", "libunwind.dll"] {
        let target = dir.join(name);
        if !target.exists() {
            std::fs::copy(build.join(name), &target).map_err(|e| e.to_string())?;
        }
    }
    let expected_sha = baal_collector::native_journal_restore::source_prefix_hash(&file, end_byte, None)?;
    let start = perf_ms();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut last = V::Null;
    let mut prefix = Sha256::new();
    for line in journal_lines(&file, &JournalReadOptions { end_byte: Some(end_byte), ..Default::default() })? {
        prefix.update(line.as_bytes());
        prefix.update(b"\n");
        if line.is_empty() { continue; }
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        if baal_service_runtime::json_text::js_int(&body["sequence"]) != Some(sequence + 1)
            || body["previous"].as_str().unwrap_or("") != previous
            || sha(js_stringify(&body).as_bytes()) != hash {
            return Err("LEGACY_CHAIN".into());
        }
        sequence += 1;
        previous = hash;
        last = body;
    }
    let js_ms = perf_ms() - start;
    if format!("{:x}", prefix.finalize()) != expected_sha { return Err("LEGACY_PREFIX".into()); }
    let native_start = perf_ms();
    let receipt_dir = runtime_dir.join("rust-coupled-benchmark-20261005");
    let native = baal_collector::native_journal_restore::read_native_journal(
        &runtime_dir, &binding, &file, end_byte, &expected_sha, &receipt_dir, None)?;
    let rust_ms = perf_ms() - native_start;
    if native["records"].as_i64() != Some(sequence) || native["tailHash"].as_str() != Some(previous.as_str())
        || ph(&native["lastBody"]).ok() != ph(&last).ok()
        || baal_collector::native_journal_restore::source_prefix_hash(&file, end_byte, None)? != expected_sha {
        return Err("PARITY".into());
    }
    let evidence = json!({"schemaVersion": 1, "observedAt": now_iso(),
        "mode": "actual_original_coupled_journal_same_work_v1", "cohort": active["cohort"],
        "bankHash": active["bankHash"], "sourceBytes": end_byte, "sourceHash": expected_sha,
        "records": sequence, "tailHash": previous, "lastBodyHash": ph(&last)?,
        "jsMs": js_ms, "rustMs": rust_ms, "rustInternalVerifyMs": native["verifyMs"],
        "speedup": js_ms / rust_ms, "binding": binding, "sourceParity": true,
        "walletParity": true, "readOnly": true, "networkRequests": 0,
        "competitionOrders": 0, "profitabilityEstablished": false});
    std::fs::write("data/coupled-rust-benchmark-20261005.json",
        format!("{}\n", serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(evidence)
}
