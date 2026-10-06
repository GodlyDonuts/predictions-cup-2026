// live-paper/paper-persistence.mjs — atomic .json commit with EBUSY/EPERM
// (win32) retry, fsync, exclusive temp, and PaperPersistenceError details.
use serde_json::Value as V;
use std::io::Write;
use std::path::{Path, PathBuf};

const DELAYS_MS: [u64; 3] = [100, 200, 400];
const DEADLINE_MS: u64 = 1000;

#[derive(Debug)]
pub struct PaperPersistenceError {
    pub details: V,
}
impl std::fmt::Display for PaperPersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { write!(f, "Paper checkpoint persistence failed") }
}
impl std::error::Error for PaperPersistenceError {}

fn basename_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 201 && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}
fn err(phase: &str, os_code: &str, attempts: u64, target: &str, temporary: Option<String>, payload_hash: Option<String>, bytes: Option<u64>) -> PaperPersistenceError {
    PaperPersistenceError { details: serde_json::json!({"phase": phase, "osCode": os_code, "attempts": attempts,
        "target": target, "temporary": temporary, "payloadHash": payload_hash, "bytes": bytes}) }
}
/// atomicPaperJson(file, value, {canCommit}) — synchronous port; rename retry
/// loop preserves the JS delay schedule and deadline accounting.
pub fn atomic_paper_json(file: &Path, value: &V, can_commit: &dyn Fn() -> bool) -> Result<V, PaperPersistenceError> {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !file.is_absolute() || !basename_ok(name) || !name.ends_with(".json") {
        return Err(err("open", "INVALID_TARGET", 0, name, None, None, None));
    }
    let temporary: PathBuf = file.with_file_name(format!(
        "{}.{}.{}.tmp", name, std::process::id(), uuid_v4()));
    let mut phase = "serialize";
    let target = name.to_string();
    let mut temp_created = false;
    let mut payload_hash: Option<String> = None;
    let mut byte_len: Option<u64> = None;
    let result = (|| -> Result<V, PaperPersistenceError> {
        let body = serde_json::to_string_pretty(value).map_err(|_| {
            err(phase, "UNKNOWN", 0, &target, None, None, None)
        })?;
        let bytes = body.as_bytes();
        payload_hash = Some(baal_service_runtime::json_text::sha256(bytes));
        byte_len = Some(bytes.len() as u64);
        phase = "open";
        let mut fd = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)
            .map_err(|e| err(phase, os_code(&e), 0, &target, None, payload_hash.clone(), byte_len))?;
        temp_created = true;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600));
        }
        phase = "write";
        fd.write_all(bytes).map_err(|e| err(phase, os_code(&e), 0, &target, Some(tmp_name(&temporary)), payload_hash.clone(), byte_len))?;
        phase = "fsync";
        fd.sync_all().map_err(|e| err(phase, os_code(&e), 0, &target, Some(tmp_name(&temporary)), payload_hash.clone(), byte_len))?;
        phase = "close";
        drop(fd);
        let started = std::time::Instant::now();
        let mut attempts = 0u64;
        loop {
            phase = "guard";
            if !can_commit() {
                return Err(err(phase, "COMMIT_BLOCKED", attempts, &target, Some(tmp_name(&temporary)), payload_hash.clone(), byte_len));
            }
            phase = "rename";
            attempts += 1;
            match std::fs::rename(&temporary, file) {
                Ok(()) => return Ok(serde_json::json!({"attempts": attempts, "payloadHash": payload_hash, "bytes": bytes.len()})),
                Err(e) => {
                    let transient = matches!(e.raw_os_error(), Some(16) | Some(5)); // EBUSY / EPERM-ish
                    let delay = DELAYS_MS.get(attempts as usize - 1).copied();
                    let elapsed = started.elapsed().as_millis() as u64;
                    match delay {
                        Some(d) if transient && elapsed + d <= DEADLINE_MS => {
                            std::thread::sleep(std::time::Duration::from_millis(d));
                            if started.elapsed().as_millis() as u64 >= DEADLINE_MS {
                                return Err(err(phase, os_code(&e), attempts, &target, Some(tmp_name(&temporary)), payload_hash.clone(), byte_len));
                            }
                        }
                        _ => return Err(err(phase, os_code(&e), attempts, &target, Some(tmp_name(&temporary)), payload_hash.clone(), byte_len)),
                    }
                }
            }
        }
    })();
    let _ = temp_created;
    result
}
fn tmp_name(p: &Path) -> String { p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string() }
fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ^ (std::process::id() as u128) << 64;
    for (i, x) in b.iter_mut().enumerate() { *x = (seed >> ((i % 16) * 8)) as u8; }
    b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80;
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes(b[0..4].try_into().unwrap()), u16::from_be_bytes(b[4..6].try_into().unwrap()),
        u16::from_be_bytes(b[6..8].try_into().unwrap()), u16::from_be_bytes(b[8..10].try_into().unwrap()),
        u64::from_be_bytes([0, 0, b[10], b[11], b[12], b[13], b[14], b[15]]))
}
fn os_code(e: &std::io::Error) -> &'static str {
    match e.raw_os_error() {
        Some(16) => "EBUSY", Some(1) => "EPERM", Some(13) => "EACCES", Some(28) => "ENOSPC",
        Some(122) => "EDQUOT", Some(5) => "EIO", Some(30) => "EROFS", Some(2) => "ENOENT",
        Some(17) => "EEXIST", Some(24) => "EMFILE", Some(23) => "ENFILE", Some(22) => "EINVAL",
        Some(18) => "EXDEV", Some(21) => "EISDIR", Some(20) => "ENOTDIR", Some(4) => "EINTR",
        Some(9) => "EBADF", _ => "UNKNOWN",
    }
}
/// paperFault(error, {cohort, phase, at, ...}) — sanitized fault row.
pub fn paper_fault(details: Option<&V>, cohort: &str, phase: &str, at: Option<f64>,
    bank_hash: Option<&str>, durable_source_sequence: Option<i64>, attempted_source_sequence: Option<i64>) -> V {
    let phases = ["serialize", "open", "write", "fsync", "close", "guard", "rename", "read_bank",
        "read_state", "read_source", "verify_source", "advance", "state", "report", "stats", "lock", "release", "host"];
    let codes = ["EBUSY", "EPERM", "EACCES", "ENOSPC", "EDQUOT", "EIO", "EROFS", "ENOENT", "EEXIST",
        "EMFILE", "ENFILE", "EINVAL", "EXDEV", "EISDIR", "ENOTDIR", "EINTR", "EBADF"];
    let d = details.cloned().unwrap_or_else(|| serde_json::json!({}));
    let pick = |k: &str| d.get(k).cloned().unwrap_or(V::Null);
    let ph = pick("phase");
    let ph = if ph.as_str().is_some_and(|s| phases.contains(&s)) { ph }
        else if phases.contains(&phase) { serde_json::json!(phase) } else { serde_json::json!("host") };
    let code = pick("osCode");
    let code = if code.as_str().is_some_and(|s| codes.contains(&s)) { code }
        else if code.as_str() == Some("COMMIT_BLOCKED") { code }
        else { serde_json::json!("UNKNOWN") };
    let int = |v: &V| -> V { if v.as_i64().is_some_and(|n| n >= 0) { v.clone() } else { V::Null } };
    let hex64 = |v: &V| -> V {
        if v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) { v.clone() } else { V::Null }
    };
    let base = |v: &V| -> V { if v.as_str().is_some_and(basename_ok) { v.clone() } else { V::Null } };
    serde_json::json!({
        "schemaVersion": 1, "type": "paper_worker_fault",
        "cohort": if ["dip", "dip-scale", "dip-responsive"].contains(&cohort) { serde_json::json!(cohort) } else { V::Null },
        "at": at.map(|ms| serde_json::json!(baal_service_runtime::contract::ms_to_iso(ms))).unwrap_or(V::Null),
        "phase": ph, "osCode": code, "attempts": int(&pick("attempts")),
        "target": base(&pick("target")), "temporary": base(&pick("temporary")), "bytes": int(&pick("bytes")),
        "payloadHash": hex64(&pick("payloadHash")),
        "bankHash": bank_hash.map(|h| hex64(&serde_json::json!(h))).unwrap_or(V::Null),
        "durableSourceSequence": durable_source_sequence.map(|n| serde_json::json!(n)).unwrap_or(V::Null),
        "attemptedSourceSequence": attempted_source_sequence.map(|n| serde_json::json!(n)).unwrap_or(V::Null),
    })
}
/// persistPaperFault(runtimeDir, fault) -> filename
pub fn persist_paper_fault(runtime_dir: &Path, fault: &V) -> Result<String, PaperPersistenceError> {
    let dir = runtime_dir.join("paper-faults");
    std::fs::create_dir_all(&dir).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let name = format!("{}-{}-{}.json",
        fault.get("cohort").and_then(V::as_str).unwrap_or("paper"),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(),
        uuid_v4());
    atomic_paper_json(&dir.join(&name), fault, &|| true)?;
    Ok(name)
}
