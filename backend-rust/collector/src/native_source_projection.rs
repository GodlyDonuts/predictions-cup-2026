// live-paper/native-source-projection.mjs — verified suffix projection replay.
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{digest, js_int, sha256};
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

use crate::native_journal_restore::{native_journal_binding, run_native_binary, source_prefix_hash};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("NATIVE_PROJECTION_{}", code)); } Ok(()) }
fn hex64(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
fn aborted(signal: Option<&(dyn Fn() -> bool + Send + Sync)>) -> bool { signal.map_or(false, |s| s()) }
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64 ^ std::process::id() as u64;
    for byte in &mut b {
        seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        *byte = seed as u8;
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15])
}
fn ino(m: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    { use std::os::unix::fs::MetadataExt; m.ino() }
    #[cfg(not(unix))]
    { let _ = m; 0 }
}
fn birthtime_ms(m: &fs::Metadata) -> Option<u128> {
    m.created().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis())
}
/// projectNativeSource({runtimeDir, binding, file, frontier, receiptDir, signal, canOperate, endByte})
/// The checkpoint is an exact sequence/hash; only not-yet-consumed original
/// records come back through the suffix file. Sources are never modified.
pub fn project_native_source(runtime_dir: &Path, binding: &V, file: &Path, frontier: &V,
    receipt_dir: &Path, signal: Option<&(dyn Fn() -> bool + Send + Sync)>, can_operate: &dyn Fn() -> bool,
    requested_end_byte: Option<u64>) -> Result<V, String> {
    let fseq = js_int(&frontier["sequence"]);
    let fhash_ok = (fseq == Some(0) && frontier["hash"].as_str() == Some(""))
        || (fseq.is_some_and(|s| s > 0) && hex64(&frontier["hash"]));
    need(file.is_absolute() && receipt_dir.is_absolute()
        && fseq.is_some_and(|s| s >= 0) && fhash_ok, "BINDING")?;
    need(can_operate() && !aborted(signal), "STOPPED")?;
    let executable = native_journal_binding(runtime_dir, binding)?;
    let initial = fs::metadata(file).map_err(|e| e.to_string())?;
    let end_byte = requested_end_byte.unwrap_or(initial.len());
    need(end_byte <= initial.len(), "SOURCE_PREFIX")?;
    let expected_sha = source_prefix_hash(file, end_byte, signal)?;
    let existed = receipt_dir.exists();
    fs::create_dir_all(receipt_dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(receipt_dir, fs::Permissions::from_mode(0o700));
    }
    let root = receipt_dir.join(uuid_v4());
    let out = PathBuf::from(format!("{}.json", root.display()));
    let suffix = PathBuf::from(format!("{}.suffix.jsonl", root.display()));
    need(can_operate() && !aborted(signal), "STOPPED")?;
    run_native_binary(&executable, &[
        "--journal".into(), file.to_string_lossy().into_owned(),
        "--end-byte".into(), end_byte.to_string(),
        "--expected-sha".into(), expected_sha.clone(),
        "--out".into(), out.to_string_lossy().into_owned(),
        "--after-sequence".into(), fseq.unwrap().to_string(),
        "--after-hash".into(), if fseq.unwrap() != 0 { frontier["hash"].as_str().unwrap_or("").to_string() } else { "EMPTY".into() },
        "--suffix-out".into(), suffix.to_string_lossy().into_owned()], signal,
        "NATIVE_PROJECTION_BINARY_UNAVAILABLE", "NATIVE_PROJECTION_STOPPED", "NATIVE_PROJECTION_FAILED")?;
    native_journal_binding(runtime_dir, binding)?;
    let after = fs::metadata(file).map_err(|e| e.to_string())?;
    need(can_operate() && !aborted(signal)
        && ino(&after) == ino(&initial) && birthtime_ms(&after) == birthtime_ms(&initial) && after.len() >= end_byte,
        "STOP_OR_SOURCE_REPLACED")?;
    let output = fs::read(&out).map_err(|e| e.to_string())?;
    let result: V = serde_json::from_slice(&output).map_err(|e| e.to_string())?;
    let p = &result["projection"];
    let records = js_int(&result["records"]);
    need(result["schemaVersion"].as_i64() == Some(1)
        && result["mode"].as_str() == Some("canonical_js_journal_projection_v1")
        && result["engineHash"] == binding["engineHash"]
        && js_int(&result["sourceBytes"]) == Some(end_byte as i64)
        && result["sourceHash"].as_str() == Some(expected_sha.as_str())
        && records.is_some_and(|r| r >= fseq.unwrap())
        && js_int(&p["afterSequence"]) == fseq
        && p["afterHash"] == frontier["hash"]
        && js_int(&p["records"]) == Some(records.unwrap() - fseq.unwrap())
        && js_int(&p["bytes"]) == Some(fs::metadata(&suffix).map_err(|e| e.to_string())?.len() as i64)
        && p["hash"].as_str() == Some(source_prefix_hash(&suffix, js_int(&p["bytes"]).unwrap() as u64, signal)?.as_str()),
        "RESULT_BINDING")?;
    let mut sequence = fseq.unwrap();
    let mut previous = frontier["hash"].as_str().unwrap_or("").to_string();
    for line in journal_lines(&suffix, &JournalReadOptions::default())? {
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().remove("hash");
        sequence += 1;
        need(js_int(&row["sequence"]) == Some(sequence)
            && row["previous"].as_str() == Some(previous.as_str())
            && digest(&body) == hash, "SUFFIX_CHAIN")?;
        previous = hash;
    }
    need(sequence == records.unwrap() && previous == result["tailHash"].as_str().unwrap_or(""), "SUFFIX_FRONTIER")?;
    atomic_json_canonical(&PathBuf::from(format!("{}.receipt.json", out.display())), &json!({
        "schemaVersion": 1, "at": now_iso(), "mode": result["mode"],
        "sourceBytes": end_byte, "sourceHash": expected_sha,
        "afterSequence": frontier["sequence"], "afterHash": frontier["hash"],
        "records": result["records"], "tailHash": result["tailHash"],
        "suffixBytes": p["bytes"], "suffixHash": p["hash"],
        "binaryHash": binding["binaryHash"], "engineHash": binding["engineHash"],
        "outputHash": sha256(&output), "networkRequests": 0, "competitionOrders": 0}))?;
    let mut result = result;
    result["suffixFile"] = json!(suffix.to_string_lossy());
    result["receiptFile"] = json!(format!("{}.receipt.json", out.display()));
    Ok(result)
}
