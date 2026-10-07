// live-paper/durable-read-timing-v1.mjs — append a validated read-timing
// record durably (open 'a' 0o600, write loop, fsync); on any failure write a
// diagnostic sidecar via atomicJson and throw {code,fatal,durableRecordingFailed}.
use crate::read_timing::validate_timing;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_stringify};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::io::Write;
use std::path::Path;

// JS `causes` Map: validation message -> durable code.
const CAUSES: [(&str, &str); 8] = [
    ("Invalid timing schema", "TIMING_SCHEMA"),
    ("Invalid timing clock", "TIMING_CLOCK"),
    ("Invalid timing admission", "TIMING_ADMISSION"),
    ("Invalid timing duration", "TIMING_DURATION"),
    ("Invalid timing status", "TIMING_STATUS"),
    ("Invalid timing instrumentation", "TIMING_INSTRUMENTATION"),
    ("Invalid event-loop duration", "TIMING_EVENT_LOOP"),
    ("Invalid event-loop quantiles", "TIMING_QUANTILES"),
];
const OUTCOMES: [&str; 6] = ["success", "blocked", "connection_error", "http_error", "body_error", "aborted"];

/// Thrown-object stand-in: Object.assign(Error(code),{code,fatal:true,durableRecordingFailed:true}).
#[derive(Debug)]
pub struct TimingFault {
    pub message: String,
    pub code: String,
    pub fatal: bool,
    pub durable_recording_failed: bool,
}
impl std::fmt::Display for TimingFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for TimingFault {}

// Error channel inside the JS `try`: validator messages, filesystem errno
// errors, and our own TIMING_SHORT_WRITE message.
enum Stage {
    Message(String),
    Io(std::io::Error),
}
fn errno_code(e: &std::io::Error) -> Option<&'static str> {
    // Node fs `error.code` names for the errno values the JS maps.
    #[cfg(unix)]
    match e.raw_os_error() {
        Some(16) => Some("EBUSY"),
        Some(13) => Some("EACCES"),
        Some(1) => Some("EPERM"),
        Some(5) => Some("EIO"),
        Some(28) => Some("ENOSPC"),
        _ => None,
    }
    #[cfg(not(unix))]
    {
        let _ = e;
        None
    }
}
fn retryable_open(e: &std::io::Error) -> bool {
    errno_code(e).is_some_and(|c| ["EBUSY", "EACCES", "EPERM"].contains(&c))
}
fn code_of(e: &Stage) -> String {
    match e {
        Stage::Message(m) => CAUSES
            .iter()
            .find(|(k, _)| *k == m.as_str())
            .map(|(_, c)| c.to_string())
            .unwrap_or_else(|| {
                if m == "TIMING_SHORT_WRITE" { "TIMING_SHORT_WRITE".into() } else { "TIMING_PERSISTENCE_FAILED".into() }
            }),
        Stage::Io(e) => errno_code(e).map(str::to_string).unwrap_or_else(|| "TIMING_PERSISTENCE_FAILED".into()),
    }
}

fn is_integer(v: Option<&V>) -> bool {
    v.and_then(V::as_f64).is_some_and(|x| x.is_finite() && x.fract() == 0.0)
}

fn append_inner(file: &Path, event: &V, written: &mut usize, pause: &dyn Fn(u64)) -> Result<(), Stage> {
    validate_timing(event).map_err(Stage::Message)?;
    let bytes = format!("{}\n", js_stringify(event)).into_bytes();
    // Retry OPEN only: a failed/partial write is never appended twice.
    let mut attempt = 0;
    let mut fd = loop {
        let mut o = fs::OpenOptions::new();
        o.append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        match o.open(file) {
            Ok(f) => break f,
            Err(e) => {
                if !retryable_open(&e) || attempt == 3 {
                    return Err(Stage::Io(e));
                }
                pause(20);
                attempt += 1;
            }
        }
    };
    while *written < bytes.len() {
        let n = fd.write(&bytes[*written..]).map_err(Stage::Io)?;
        if n < 1 || n > bytes.len() - *written {
            return Err(Stage::Message("TIMING_SHORT_WRITE".into()));
        }
        *written += n;
    }
    fd.sync_all().map_err(Stage::Io)?;
    Ok(())
}

/// appendReadTimingV1({file,diagnosticFile,event,filesystem=fs,pause})
/// The JS `filesystem`/`pause` injections collapse to std::fs plus a `pause`
/// closure; pass `&|ms| std::thread::sleep(std::time::Duration::from_millis(ms))`
/// for the Atomics.wait default.
pub fn append_read_timing_v1(file: &Path, diagnostic_file: &Path, event: &V, pause: &dyn Fn(u64)) -> Result<(), TimingFault> {
    let mut written = 0usize;
    if let Err(e) = append_inner(file, event, &mut written, pause) {
        let code = code_of(&e);
        // {schemaVersion:1,at:new Date().toISOString(),...safe} — the spread's
        // schemaVersion overwrites the leading 1 at its original position.
        let mut safe = Map::new();
        safe.insert("schemaVersion".into(), json!(1));
        safe.insert("at".into(), json!(ms_to_iso(chrono::Utc::now().timestamp_millis() as f64)));
        safe.insert("code".into(), json!(code));
        safe.insert("bytesWritten".into(), json!(written as i64));
        safe.insert(
            "schemaVersion".into(),
            event.get("schemaVersion").and_then(js_int).map(|x| json!(x)).unwrap_or(V::Null),
        );
        safe.insert(
            "outcome".into(),
            event.get("outcome").filter(|o| o.as_str().is_some_and(|s| OUTCOMES.contains(&s))).cloned().unwrap_or(V::Null),
        );
        safe.insert(
            "statusCode".into(),
            if is_integer(event.get("statusCode")) { event.get("statusCode").unwrap().clone() } else { V::Null },
        );
        safe.insert(
            "eventLoopSamples".into(),
            event.get("eventLoopSamples").and_then(js_int).map(|x| json!(x)).unwrap_or(V::Null),
        );
        safe.insert(
            "eventLoopP99Ms".into(),
            if event.get("eventLoopP99Ms").and_then(V::as_f64).is_some_and(|x| x.is_finite()) {
                event.get("eventLoopP99Ms").unwrap().clone()
            } else {
                V::Null
            },
        );
        safe.insert(
            "eventLoopMaxMs".into(),
            if event.get("eventLoopMaxMs").and_then(V::as_f64).is_some_and(|x| x.is_finite()) {
                event.get("eventLoopMaxMs").unwrap().clone()
            } else {
                V::Null
            },
        );
        let _ = atomic_json_canonical(diagnostic_file, &V::Object(safe));
        return Err(TimingFault {
            message: code.clone(),
            code,
            fatal: true,
            durable_recording_failed: true,
        });
    }
    // JS finally{closeSync(fd)}: the File drops here; a closeSync failure would
    // have replaced the result in JS — Drop has no such path in Rust.
    Ok(())
}
