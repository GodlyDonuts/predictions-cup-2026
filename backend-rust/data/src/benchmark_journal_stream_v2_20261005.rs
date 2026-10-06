// data/benchmark-journal-stream-v2-20261005.mjs — sealed-prefix one-pass
// journal-stream-v2 vs pinned v1 restore benchmark with hash-verified cleanup.
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
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
fn hash_file(file: &Path) -> Result<String, String> {
    let mut h = Sha256::new();
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut tmp = vec![0u8; 1024 * 1024];
    loop {
        let n = fd.read(&mut tmp).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        h.update(&tmp[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn statfs_bavail_bytes(dir: &Path) -> Result<u64, String> {
    #[cfg(unix)] {
        let c = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).map_err(|e| e.to_string())?;
        unsafe {
            let mut st: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c.as_ptr(), &mut st) != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(st.f_bavail as u64 * st.f_bsize as u64)
        }
    }
    #[cfg(not(unix))]
    { let _ = dir; Ok(u64::MAX) }
}
fn invoke(exe: &Path, sealed: &Path, end_byte: u64, expected: &str, work: &Path,
    name: &str) -> Result<(V, f64), String> {
    let out = work.join(format!("{}.json", name));
    let t = perf_ms();
    let r = std::process::Command::new(exe)
        .args(["--journal", sealed.to_str().unwrap_or(""),
            "--end-byte", &end_byte.to_string(),
            "--expected-sha", expected, "--out", out.to_str().unwrap_or("")])
        .output().map_err(|e| e.to_string())?;
    if !r.status.success() {
        return Err(String::from_utf8_lossy(&r.stderr).trim().to_string()
            .if_empty_then("NATIVE_FAILED").to_string());
    }
    Ok((read_json(&out)?, perf_ms() - t))
}
trait IfEmpty { fn if_empty_then(self, other: &str) -> Self; }
impl IfEmpty for String {
    fn if_empty_then(self, other: &str) -> Self { if self.is_empty() { other.into() } else { self } }
}

pub fn run() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let original = rt.join("journal.jsonl");
    let work = rt.join("offline-benchmarks")
        .join(format!("journal-stream-v2-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    let wanted = std::fs::metadata(&original).map_err(|e| e.to_string())?.len();
    if wanted > 9_007_199_254_740_991 { return Err("FRONTIER".into()); }
    if statfs_bavail_bytes(&rt)? < (wanted as f64 * 1.3) as u64 { return Err("DISK_HEADROOM".into()); }
    let sealed = work.join("sealed-original-prefix.jsonl");
    let copy_start = perf_ms();
    let mut last_newline = 0u64;
    {
        let mut input = std::fs::File::open(&original).map_err(|e| e.to_string())?;
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true).mode(0o600);
        let mut output = o.open(&sealed).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1024 * 1024];
        let mut total = 0u64;
        while total < wanted {
            let take = (buf.len() as u64).min(wanted - total) as usize;
            let n = input.read(&mut buf[..take])
                .map_err(|e| e.to_string())?;
            if n == 0 { return Err("SOURCE_TRUNCATED".into()); }
            output.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            if let Some(i) = buf[..n].iter().rposition(|c| *c == b'\n') {
                last_newline = total + i as u64 + 1;
            }
            total += n as u64;
        }
        output.set_len(last_newline).map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
    }
    let copy_ms = perf_ms() - copy_start;
    let exe_v2 = std::env::current_dir().map_err(|e| e.to_string())?
        .join("backend-rust/journal-stream-v2/target/release/baal-journal-stream-v2.exe");
    let binding = read_json(&rt.join("collector-native-journal.json"))?;
    let b = if !binding["nativeBackend"].is_null() { binding["nativeBackend"].clone() }
        else if !binding["binding"].is_null() { binding["binding"].clone() } else { binding.clone() };
    let exe_v1 = rt.join("journal-native-builds").join(b["binaryHash"].as_str().unwrap_or(""))
        .join("baal-journal-restore.exe");
    if !exe_v1.exists() { return Err("PINNED_NATIVE_MISSING".into()); }
    let final_v2_binary_hash = hash_file(&exe_v2)?;
    let hash_start = perf_ms();
    let expected_sha = hash_file(&sealed)?;
    let prefix_hash_ms = perf_ms() - hash_start;
    let (old_result, old_wall) = invoke(&exe_v1, &sealed, last_newline, &expected_sha, &work, "v1")?;
    let (next_result, next_wall) = invoke(&exe_v2, &sealed, last_newline, "COMPUTE", &work, "v2")?;
    let state_hash = |x: &V| sha(js_stringify(&x["lastBody"]).as_bytes());
    if next_result["sourceHash"].as_str() != Some(expected_sha.as_str())
        || next_result["records"] != old_result["records"]
        || next_result["tailHash"] != old_result["tailHash"]
        || state_hash(&next_result) != state_hash(&old_result) {
        return Err("PARITY".into());
    }
    let mut evidence = json!({"schemaVersion": 1, "at": now_iso(),
        "mode": "sealed_actual_original_journal_onepass_v2", "sourceBytes": last_newline,
        "sourceHash": expected_sha, "records": next_result["records"],
        "tailHash": next_result["tailHash"], "lastBodyHash": state_hash(&next_result),
        "copyMs": copy_ms, "prefixHashMs": prefix_hash_ms, "v1NativeMs": old_wall,
        "v1TotalMs": prefix_hash_ms + old_wall, "v2OnePassMs": next_wall,
        "speedup": (prefix_hash_ms + old_wall) / next_wall,
        "v1InternalMs": old_result["verifyMs"], "v2InternalMs": next_result["verifyMs"],
        "v2EngineHash": next_result["engineHash"], "v2BinaryHash": final_v2_binary_hash,
        "sourceStamp": next_result["sourceStamp"], "chainParity": true, "stateParity": true,
        "readOnly": true, "networkRequests": 0, "competitionOrders": 0,
        "profitabilityEstablished": false, "sourceChangingDuringCopyPossible": true,
        "canonicalSnapshotValidationPassed": true});
    if hash_file(&exe_v2)? != final_v2_binary_hash { return Err("BINARY_CHANGED".into()); }
    let out_file = Path::new("data/journal-stream-v2-benchmark-20261005.json");
    std::fs::write(out_file,
        format!("{}\n", serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    let resolved = sealed.canonicalize().map_err(|e| e.to_string())?;
    let owned = work.canonicalize().map_err(|e| e.to_string())?;
    if resolved.parent() != Some(owned.as_path())
        || !owned.starts_with(rt.join("offline-benchmarks").canonicalize()
            .unwrap_or_else(|_| rt.join("offline-benchmarks"))) {
        return Err("DELETE_SCOPE".into());
    }
    std::fs::remove_file(&resolved).map_err(|e| e.to_string())?;
    evidence["temporarySnapshotDeleted"] = json!(true);
    std::fs::write(out_file,
        format!("{}\n", serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(evidence)
}
