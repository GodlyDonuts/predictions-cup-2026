// live-paper/rust-backend.mjs — native binary binding + subprocess invocation.
// In the Rust port these shell out to the baal-backend binary exactly as JS did,
// preserving the binary-hash pin and receipt files byte-for-byte.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

pub fn native_executable() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/release").join(
        if cfg!(windows) { "baal-backend.exe" } else { "baal-backend" })
}
fn sha_file(p: &Path) -> Result<String, String> {
    std::fs::read(p).map(|b| sha256(&b)).map_err(|_| "Native binary unreadable".to_string())
}
fn validate_binding(end_byte: u64, expected_sha: &str, out: &Path, workers: u64) -> Result<(), String> {
    if end_byte < 1 || expected_sha.len() != 64 || !expected_sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || !(1..=32).contains(&workers) {
        return Err("Invalid native replay binding".into());
    }
    if out.exists() || Path::new(&format!("{}.receipt.json", out.display())).exists() {
        return Err("Refusing to overwrite a native replay".into());
    }
    Ok(())
}
fn invoke(executable: &Path, args: &[String]) -> Result<(), String> {
    let status = std::process::Command::new(executable)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| "Native invocation failed".to_string())?;
    if !status.success() { return Err(format!("Native invocation exited {}", status)); }
    Ok(())
}
fn receipt(out: &Path, expected_sha: &str, end_byte: u64, executable: &Path, result: &V,
    bank_file_hash: Option<&str>, config_file_hash: Option<&str>, declaration_file_hash: Option<&str>) -> Result<(), String> {
    let output_hash = sha_file(out)?;
    let body = json!({"at": baal_service_runtime::contract::ms_to_iso(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64),
        "sourceHash": expected_sha, "sourceBytes": end_byte, "engineHash": result["capture"]["engine_hash"],
        "bankFileHash": bank_file_hash, "configFileHash": config_file_hash,
        "declarationFileHash": declaration_file_hash,
        "binaryHash": sha_file(executable)?, "outputHash": output_hash,
        "competitionOrders": 0});
    std::fs::write(format!("{}.receipt.json", out.display()), format!("{}\n", serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?))
        .map_err(|_| "Native receipt write failed".to_string())
}
fn check_result(result: &V, sha: &str, bytes: u64, body: &V) -> Result<(), String> {
    if result["capture"]["source_hash"].as_str() != Some(sha)
        || result["capture"]["source_bytes"].as_u64() != Some(bytes)
        || !result["capture"]["engine_hash"].as_str().is_some_and(|s| s.len() == 64)
        || body["competitionOrders"].as_i64() != Some(0) || body["networkRequests"].as_i64() != Some(0) {
        return Err("Native result binding mismatch".into());
    }
    Ok(())
}
/// readNativePublicationPrefix({journal, scope, endByte, expectedSha, out, executable, pinnedBinaryHash})
pub fn read_native_publication_prefix(journal: &Path, scope: &Path, end_byte: u64, expected_sha: &str,
    out: &Path, executable: &Path, pinned_binary_hash: &str) -> Result<V, String> {
    validate_binding(end_byte, expected_sha, out, 1)?;
    let scope_file_hash = sha_file(scope)?;
    let binary_hash = sha_file(executable)?;
    if binary_hash != pinned_binary_hash { return Err("Native publication binary identity changed".into()); }
    invoke(executable, &["publication".into(), "--journal".into(), journal.display().to_string(),
        "--scope".into(), scope.display().to_string(), "--end-byte".into(), end_byte.to_string(),
        "--expected-sha".into(), expected_sha.into(), "--out".into(), out.display().to_string()])?;
    let result: V = serde_json::from_str(&std::fs::read_to_string(out).map_err(|_| "Native result unreadable".to_string())?)
        .map_err(|_| "Native publication binding mismatch".to_string())?;
    if result["schemaVersion"].as_i64() != Some(1)
        || result["mode"].as_str() != Some("native_public_book_prefix_v1")
        || result["sourceHash"].as_str() != Some(expected_sha)
        || result["sourceBytes"].as_u64() != Some(end_byte)
        || result["scopeFileHash"].as_str() != Some(&scope_file_hash)
        || !result["engineHash"].as_str().is_some_and(|s| s.len() == 64)
        || result["state"]["sequence"].as_i64() != result["sourceRecords"].as_i64()
        || result["state"]["scopeHash"] != result["scopeHash"]
        || result["state"]["schemaVersion"].as_i64() != Some(1)
        || result["networkRequests"].as_i64() != Some(0)
        || result["competitionOrders"].as_i64() != Some(0)
        || sha_file(executable)? != binary_hash {
        return Err("Native publication binding mismatch".into());
    }
    let receipt = json!({"at": baal_service_runtime::contract::ms_to_iso(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64),
        "mode": result["mode"], "sourceHash": expected_sha, "sourceBytes": end_byte,
        "scopeFileHash": scope_file_hash, "scopeHash": result["scopeHash"],
        "engineHash": result["engineHash"], "binaryHash": binary_hash,
        "outputHash": sha_file(out)?, "networkRequests": 0, "competitionOrders": 0});
    std::fs::write(format!("{}.receipt.json", out.display()), format!("{}\n", serde_json::to_string_pretty(&receipt).unwrap()))
        .map_err(|_| "Native receipt write failed".to_string())?;
    Ok(result)
}
/// replayNativeBank / sweepNativeCapture / evaluateNativeCapture
fn native_mode(mode: &str, journal: &Path, input_file: &Path, input_flag: &str, end_byte: u64,
    expected_sha: &str, out: &Path, workers: u64, executable: &Path) -> Result<V, String> {
    validate_binding(end_byte, expected_sha, out, workers)?;
    let input_file_hash = sha_file(input_file)?;
    invoke(executable, &[mode.into(), "--journal".into(), journal.display().to_string(),
        format!("--{}", input_flag), input_file.display().to_string(),
        "--end-byte".into(), end_byte.to_string(), "--expected-sha".into(), expected_sha.into(),
        "--workers".into(), workers.to_string(), "--out".into(), out.display().to_string()])?;
    let result: V = serde_json::from_str(&std::fs::read_to_string(out).map_err(|_| "Native result unreadable".to_string())?)
        .map_err(|_| "Native result binding mismatch".to_string())?;
    let body_key = match mode { "replay" => "replay", "sweep" => "sweep", _ => "evaluation" };
    check_result(&result, expected_sha, end_byte, &result[body_key])?;
    Ok(json!({"result": result, "input_file_hash": input_file_hash}))
}
pub fn replay_native_bank(journal: &Path, bank: &Path, end_byte: u64, expected_sha: &str,
    out: &Path, workers: u64, executable: &Path) -> Result<V, String> {
    let result = native_mode("replay", journal, bank, "bank", end_byte, expected_sha, out, workers, executable)?;
    let input_file_hash = result["input_file_hash"].clone();
    let result = &result["result"];
    if result["replay"]["bankSourceBinding"].is_null() || result["bankFileHash"] != input_file_hash {
        return Err("Native result binding mismatch".into());
    }
    receipt(out, expected_sha, end_byte, executable, result, input_file_hash.as_str(), None, None)?;
    Ok(result.clone())
}
pub fn sweep_native_capture(journal: &Path, config: &Path, end_byte: u64, expected_sha: &str,
    out: &Path, workers: u64, executable: &Path) -> Result<V, String> {
    let c: V = serde_json::from_str(&std::fs::read_to_string(config).map_err(|_| "x".to_string())?).map_err(|_| "x".to_string())?;
    crate::model::validate_config(&c)?;
    if c["feeBps"].as_i64() != Some(0) { return Err("Dip sweep requires zero explicit fees".into()); }
    let result = native_mode("sweep", journal, config, "config", end_byte, expected_sha, out, workers, executable)?;
    let input_file_hash = result["input_file_hash"].clone();
    let result = &result["result"];
    if result["configFileHash"] != input_file_hash { return Err("Native config binding mismatch".into()); }
    receipt(out, expected_sha, end_byte, executable, result, None, input_file_hash.as_str(), None)?;
    Ok(result.clone())
}
pub fn evaluate_native_capture(journal: &Path, declaration: &Path, end_byte: u64, expected_sha: &str,
    out: &Path, workers: u64, executable: &Path) -> Result<V, String> {
    let result = native_mode("evaluate", journal, declaration, "declaration", end_byte, expected_sha, out, workers, executable)?;
    let input_file_hash = result["input_file_hash"].clone();
    let result = &result["result"];
    if result["declarationFileHash"] != input_file_hash { return Err("Native declaration binding mismatch".into()); }
    receipt(out, expected_sha, end_byte, executable, result, None, None, input_file_hash.as_str())?;
    Ok(result.clone())
}
