// live-paper/native-journal-restore.mjs — verified native journal restoration.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{digest, js_int, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("NATIVE_JOURNAL_{}", code)); } Ok(()) }
pub(crate) fn hex64(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
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

/// journalSourceFiles — hashed inputs of the native restore engine.
pub const JOURNAL_SOURCE_FILES: [&str; 5] = ["Cargo.toml", "Cargo.lock", "build.rs", "src/main.rs", "src/canonical.rs"];
/// journalEngineIdentity(sourceRoot) — length-prefixed name/byte digest.
pub fn journal_engine_identity(source_root: &Path) -> Result<String, String> {
    let mut h = Sha256::new();
    for name in JOURNAL_SOURCE_FILES {
        let bytes = fs::read(source_root.join(name)).map_err(|e| e.to_string())?;
        let n = name.as_bytes();
        h.update((n.len() as u64).to_le_bytes());
        h.update(n);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    Ok(format!("{:x}", h.finalize()))
}
/// nativeJournalBinding(runtimeDir, binding) — validates the binding and the
/// on-disk binary bytes, returns the executable path.
pub fn native_journal_binding(runtime_dir: &Path, binding: &V) -> Result<PathBuf, String> {
    need(runtime_dir.is_absolute() && binding["schemaVersion"].as_i64() == Some(1)
        && binding["mode"].as_str() == Some("canonical_js_journal_restore_v1")
        && ["binaryHash", "engineHash", "runtimeLibraryHash"].iter().all(|k| hex64(&binding[*k])), "BINDING")?;
    let root = runtime_dir.join("journal-native-builds").join(binding["binaryHash"].as_str().unwrap());
    let exe = root.join("baal-journal-restore.exe");
    need(sha256(&fs::read(&exe).map_err(|e| e.to_string())?) == binding["binaryHash"].as_str().unwrap()
        && sha256(&fs::read(root.join("libunwind.dll")).map_err(|e| e.to_string())?) == binding["runtimeLibraryHash"].as_str().unwrap(),
        "BINARY_CHANGED")?;
    Ok(exe)
}
// invoke(): spawn with piped stderr; >2048 stderr bytes or an abort kills the
// child; a JOURNAL_* stderr label passes through as the error code.
pub(crate) fn run_native_binary(exe: &Path, args: &[String], signal: Option<&(dyn Fn() -> bool + Send + Sync)>,
    unavailable: &str, stopped: &str, failed: &str) -> Result<(), String> {
    if aborted(signal) { return Err(stopped.to_string()); }
    let mut child = match std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn() {
        Ok(c) => c,
        Err(_) => return Err(unavailable.to_string()),
    };
    let mut stderr_pipe = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match stderr_pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() { break; }
                }
            }
        }
    });
    let mut error = String::new();
    let code: Option<i32>;
    loop {
        while let Ok(chunk) = rx.try_recv() {
            error.push_str(&String::from_utf8_lossy(&chunk));
            if error.len() > 2048 { let _ = child.kill(); }
        }
        if aborted(signal) { let _ = child.kill(); }
        match child.try_wait() {
            Ok(Some(status)) => { code = status.code(); break; }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(2)),
            Err(_) => return Err(unavailable.to_string()),
        }
    }
    while let Ok(chunk) = rx.try_recv() { error.push_str(&String::from_utf8_lossy(&chunk)); }
    if aborted(signal) { return Err(stopped.to_string()); }
    match code {
        Some(0) => Ok(()),
        _ => {
            let label = error.trim();
            Err(if label.len() > 8 && label.starts_with("JOURNAL_")
                && label[8..].bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
                label.to_string()
            } else {
                failed.to_string()
            })
        }
    }
}
/// readNativeJournal({runtimeDir, binding, file, endByte, expectedSha, receiptDir, signal})
pub fn read_native_journal(runtime_dir: &Path, binding: &V, file: &Path, end_byte: u64,
    expected_sha: &str, receipt_dir: &Path, signal: Option<&(dyn Fn() -> bool + Send + Sync)>) -> Result<V, String> {
    need(file.is_absolute() && receipt_dir.is_absolute() && hex64(&V::from(expected_sha)), "PREFIX_BINDING")?;
    let executable = native_journal_binding(runtime_dir, binding)?;
    let existed = receipt_dir.exists();
    fs::create_dir_all(receipt_dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(receipt_dir, fs::Permissions::from_mode(0o700));
    }
    let out = receipt_dir.join(format!("{}.state.json", uuid_v4()));
    run_native_binary(&executable, &[
        "--journal".into(), file.to_string_lossy().into_owned(),
        "--end-byte".into(), end_byte.to_string(),
        "--expected-sha".into(), expected_sha.to_string(),
        "--out".into(), out.to_string_lossy().into_owned()], signal,
        "NATIVE_JOURNAL_UNAVAILABLE", "NATIVE_JOURNAL_STOPPED", "NATIVE_JOURNAL_FAILED")?;
    native_journal_binding(runtime_dir, binding)?;
    let bytes = fs::read(&out).map_err(|e| e.to_string())?;
    let result: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    need(result["schemaVersion"].as_i64() == Some(1)
        && result["mode"] == binding["mode"]
        && result["engineHash"] == binding["engineHash"]
        && js_int(&result["sourceBytes"]) == Some(end_byte as i64)
        && result["sourceHash"].as_str() == Some(expected_sha)
        && js_int(&result["records"]).is_some_and(|r| r >= 0)
        && result["verifyMs"].as_f64().is_some_and(|v| v >= 0.0), "RESULT_BINDING")?;
    let records = js_int(&result["records"]).unwrap();
    if records == 0 {
        need(result["lastBody"].is_null() && result["tailHash"].as_str() == Some(""), "EMPTY_STATE")?;
    } else {
        need(js_int(&result["lastBody"]["sequence"]) == Some(records)
            && digest(&result["lastBody"]) == result["tailHash"].as_str().unwrap_or(""), "LAST_BODY_PARITY")?;
    }
    atomic_json_canonical(&PathBuf::from(format!("{}.receipt.json", out.display())), &json!({
        "schemaVersion": 1, "at": now_iso(), "mode": result["mode"], "engineHash": result["engineHash"],
        "binaryHash": binding["binaryHash"], "runtimeLibraryHash": binding["runtimeLibraryHash"],
        "sourceHash": expected_sha, "sourceBytes": end_byte, "records": result["records"],
        "tailHash": result["tailHash"], "outputHash": sha256(&bytes), "networkRequests": 0, "competitionOrders": 0}))?;
    let mut result = result;
    result["receiptFile"] = json!(format!("{}.receipt.json", out.display()));
    Ok(result)
}
/// sourcePrefixHash(file, endByte, signal) — sha256 over bytes [0, endByte).
pub fn source_prefix_hash(file: &Path, end_byte: u64, signal: Option<&(dyn Fn() -> bool + Send + Sync)>) -> Result<String, String> {
    let mut h = Sha256::new();
    if end_byte == 0 { return Ok(format!("{:x}", h.finalize())); }
    let mut fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut size = 0u64;
    while size < end_byte {
        if aborted(signal) { return Err("NATIVE_JOURNAL_STOPPED".into()); }
        let want = (end_byte - size).min(1_048_576) as usize;
        let mut buf = vec![0u8; want];
        let n = fd.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        h.update(&buf[..n]);
        size += n as u64;
    }
    need(size == end_byte, "PREFIX_TRUNCATED")?;
    Ok(format!("{:x}", h.finalize()))
}
fn stamp(m: &fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return format!("{}:{}:{}:{}:{}", m.dev(), m.ino(), m.size(),
            m.mtime() * 1_000_000_000 + m.mtime_nsec(), m.ctime() * 1_000_000_000 + m.ctime_nsec());
    }
    #[cfg(not(unix))]
    format!("0:0:{}", m.len())
}
/// Restored journal with the non-enumerable nativeRestore metrics attached.
pub struct NativeJournal {
    pub journal: Journal,
    pub native_restore: V,
}
/// openNativeJournal({runtimeDir, binding, file, receiptDir, signal, canOperate})
pub fn open_native_journal(runtime_dir: &Path, binding: &V, file: &Path, receipt_dir: &Path,
    signal: Option<&(dyn Fn() -> bool + Send + Sync)>, can_operate: &dyn Fn() -> bool) -> Result<NativeJournal, String> {
    need(can_operate() && !aborted(signal), "STOPPED")?;
    native_journal_binding(runtime_dir, binding)?;
    if !file.exists() {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(file).map_err(|e| e.to_string())?;
    }
    let initial = fs::metadata(file).map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    let bytes = initial.len();
    let hash_started = std::time::Instant::now();
    let expected_sha = source_prefix_hash(file, bytes, signal)?;
    let hash_ms = hash_started.elapsed().as_secs_f64() * 1000.0;
    let result = read_native_journal(runtime_dir, binding, file, bytes, &expected_sha, receipt_dir, signal)?;
    need(can_operate() && !aborted(signal)
        && stamp(&fs::metadata(file).map_err(|e| e.to_string())?) == stamp(&initial), "STOP_OR_SOURCE_CHANGED")?;
    let journal = Journal {
        file: file.to_path_buf(),
        sequence: js_int(&result["records"]).unwrap_or(0) as u64,
        previous: result["tailHash"].as_str().unwrap_or("").to_string(),
        last: if result["lastBody"].is_null() { None } else { Some(result["lastBody"].clone()) },
    };
    let native_restore = json!({
        "mode": result["mode"], "sourceBytes": bytes, "sourceHash": expected_sha,
        "sequence": result["records"], "tailHash": result["tailHash"],
        "lastBodyHash": if result["lastBody"].is_null() { V::Null } else { json!(payload_hash(&result["lastBody"]).unwrap_or_default()) },
        "engineHash": result["engineHash"], "binaryHash": binding["binaryHash"],
        "verifyMs": result["verifyMs"], "prefixHashMs": hash_ms,
        "totalMs": started.elapsed().as_secs_f64() * 1000.0});
    Ok(NativeJournal { journal, native_restore })
}
