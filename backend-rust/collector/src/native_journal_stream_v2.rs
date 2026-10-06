// live-paper/native-journal-stream-v2.mjs — one sequential Rust restore worker
// per adapter over a pinned child process; restore receipts are pinned per file
// by node identity (dev/ino/birthtimeNs).
use serde_json::{json, Value as V};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256 as sha};
use num_bigint::BigInt;

use crate::native_dip_host_v1::{aborted, AbortSignal};
use crate::native_journal_append::{path_resolve, PipeChild, PipeEvent};
use crate::native_journal_restore::{journal_engine_identity, NativeJournal};

fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(format!("NATIVE_STREAM_V2_{}", c)); }
    Ok(())
}
fn hash_hex(v: &V) -> bool {
    v.as_str().map(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())).unwrap_or(false)
}
fn object(v: &V) -> bool { v.is_object() }

#[cfg(unix)]
fn stat_bigints(s: &std::fs::Metadata) -> (u64, u64, u64, u64, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (s.dev(), s.ino(), crate::native_journal_append::birthtime_ns(s),
        s.mtime() as u64 * 1_000_000_000 + s.mtime_nsec() as u64,
        s.ctime() as u64 * 1_000_000_000 + s.ctime_nsec() as u64, s.size())
}
#[cfg(not(unix))]
fn stat_bigints(s: &std::fs::Metadata) -> (u64, u64, u64, u64, u64, u64) {
    (0, 0, crate::native_journal_append::birthtime_ns(s),
        s.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0),
        0, s.len())
}
/// nodeStamp(s) = dev:ino:birthtimeNs:mtimeNs:ctimeNs:size
fn node_stamp(s: &std::fs::Metadata) -> String {
    let (dev, ino, b, m, c, size) = stat_bigints(s);
    format!("{}:{}:{}:{}:{}:{}", dev, ino, b, m, c, size)
}
/// identity(s) = dev:ino:birthtimeNs
fn identity(s: &std::fs::Metadata) -> String {
    let (dev, ino, b, _, _, _) = stat_bigints(s);
    format!("{}:{}:{}", dev, ino, b)
}
/// sourceStampMatches(raw, s): 8 decimal fields — dev, ino hi/lo, birthtime
/// FILETIME hi/lo, mtime FILETIME hi/lo, size — where ticks = ns/100 +
/// 116444736000000000.
fn source_stamp_matches(raw: &V, s: &std::fs::Metadata) -> bool {
    let raw = match raw.as_str() { Some(x) => x, None => return false };
    let parts: Vec<&str> = raw.split(':').collect();
    if parts.len() != 8 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let x: Vec<BigInt> = parts.iter().map(|p| p.parse::<BigInt>().unwrap()).collect();
    let (dev, ino, birth, mtime, _ctime, size) = stat_bigints(s);
    let ticks = |n: u64| -> BigInt { BigInt::from(n) / BigInt::from(100u64) + BigInt::from(116444736000000000u64) };
    let shl32 = |a: &BigInt, b: &BigInt| -> BigInt { (a.clone() << 32) | b.clone() };
    x[0] == BigInt::from(dev)
        && shl32(&x[1], &x[2]) == BigInt::from(ino)
        && shl32(&x[3], &x[4]) == ticks(birth)
        && shl32(&x[5], &x[6]) == ticks(mtime)
        && x[7] == BigInt::from(size)
}

/// validateJournalStreamV2Binding({executable, sourceRoot, binding}).
pub fn validate_journal_stream_v2_binding(executable: &Path, source_root: &Path, binding: &V)
    -> Result<PathBuf, String> {
    need(executable.is_absolute() && source_root.is_absolute()
        && binding["schemaVersion"].as_i64() == Some(2)
        && binding["mode"].as_str() == Some("canonical_js_journal_restore_v2")
        && ["binaryHash", "engineHash", "runtimeLibraryHash"].iter().all(|k| hash_hex(&binding[*k])),
        "BINDING")?;
    need(sha(&std::fs::read(executable).map_err(|e| e.to_string())?) == binding["binaryHash"].as_str().unwrap_or("")
        && sha(&std::fs::read(executable.parent().unwrap_or(Path::new("")).join("libunwind.dll"))
            .map_err(|e| e.to_string())?) == binding["runtimeLibraryHash"].as_str().unwrap_or("")
        && journal_engine_identity(source_root)? == binding["engineHash"].as_str().unwrap_or(""),
        "PIN_CHANGED")?;
    Ok(executable.to_path_buf())
}

// One sequential Rust worker per adapter. No account clients and no per-record
// spawn.
pub struct NativeJournalStreamV2 {
    options_timeout: Duration,
    binding: V,
    executable: PathBuf,
    source_root: PathBuf,
    failure: Option<String>,
    receipts: HashMap<String, V>,
    child: PipeChild,
    queue: Arc<Mutex<()>>,
    signal: Option<AbortSignal>,
}

impl NativeJournalStreamV2 {
    /// new NativeJournalStreamV2({executable, sourceRoot, binding, signal, timeoutMs}).
    pub fn new(executable: &Path, source_root: &Path, binding: &V,
        signal: Option<AbortSignal>, timeout_ms: Option<i64>) -> Result<Self, String> {
        let binding = binding.clone();
        validate_journal_stream_v2_binding(executable, source_root, &binding)?;
        let child = PipeChild::spawn(executable, &["--worker".to_string()], None, 18 * 1024 * 1024)
            .map_err(|_| "NATIVE_STREAM_V2_WORKER_UNAVAILABLE".to_string());
        let mut this = NativeJournalStreamV2 {
            options_timeout: Duration::from_millis(timeout_ms.unwrap_or(120000) as u64),
            binding, executable: executable.to_path_buf(), source_root: source_root.to_path_buf(),
            failure: None, receipts: HashMap::new(), queue: Arc::new(Mutex::new(())), signal,
            child: match child { Ok(c) => c, Err(e) => return Err(e) },
        };
        if aborted(this.signal.as_ref()) { this.fail("STOPPED"); }
        Ok(this)
    }
    fn fail(&mut self, label: &str) -> String {
        if self.failure.is_none() {
            self.failure = Some(format!("NATIVE_STREAM_V2_{}", label));
            self.child.kill();
        }
        self.failure.clone().unwrap()
    }
    pub fn failure_message(&self) -> Option<String> { self.failure.clone() }
    fn request(&mut self, req: &V) -> Result<V, String> {
        need(self.failure.is_none(), "WORKER_FAILED")?;
        if let Some(ev) = self.child.drain_event() {
            return Err(match ev {
                PipeEvent::Line(_) => self.fail("UNSOLICITED_RESPONSE"),
                PipeEvent::Stderr(_) => self.fail("WORKER_STDERR"),
                PipeEvent::Overflow => self.fail("RESPONSE_LIMIT"),
                PipeEvent::Eof => self.fail("WORKER_DEAD"),
            });
        }
        if self.child.write_line(&js_stringify(req)).is_err() {
            return Err(self.fail("WORKER_DEAD"));
        }
        let line = loop {
            match self.child.recv(self.options_timeout) {
                Ok(PipeEvent::Line(l)) => break l,
                Ok(PipeEvent::Stderr(_)) => return Err(self.fail("WORKER_STDERR")),
                Ok(PipeEvent::Overflow) => return Err(self.fail("RESPONSE_LIMIT")),
                Ok(PipeEvent::Eof) => return Err(self.fail("WORKER_DEAD")),
                Err(()) => return Err(self.fail("WORKER_TIMEOUT")),
            }
        };
        let response: Result<V, _> = serde_json::from_str(&line);
        let response = match response {
            Ok(v) => v,
            Err(_) => return Err(self.fail("MALFORMED_RESPONSE")),
        };
        if response["ok"].as_bool() != Some(true) {
            let label = response["error"].as_str()
                .filter(|e| e.starts_with("JOURNAL_") && e.len() > 8
                    && e.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
                .unwrap_or("MALFORMED_RESPONSE").to_string();
            let msg = self.fail(&label);
            return Err(msg);
        }
        Ok(response["result"].clone())
    }
    /// restore(options) — serialized; failures poison the adapter.
    pub fn restore(&mut self, file: &Path, end_byte: Option<i64>, expected: Option<&V>,
        prior_binding: Option<&V>,
        can_operate: &(dyn Fn() -> bool + Send + Sync)) -> Result<V, String> {
        let _q = self.queue.clone();
        let _g = _q.lock().unwrap_or_else(|e| e.into_inner());
        match self.restore_sequential(file, end_byte, expected, prior_binding, can_operate) {
            Ok(r) => Ok(r),
            Err(e) => { self.fail("RESTORE_REJECTED"); Err(e) }
        }
    }
    fn revalidate_binding(&self) -> Result<(), String> {
        validate_journal_stream_v2_binding(&self.executable, &self.source_root, &self.binding)?;
        Ok(())
    }
    fn restore_sequential(&mut self, file: &Path, end_byte: Option<i64>, expected: Option<&V>,
        prior_binding: Option<&V>,
        can_operate: &(dyn Fn() -> bool + Send + Sync)) -> Result<V, String> {
        need(self.failure.is_none(), "WORKER_FAILED")?;
        need(can_operate() && !aborted(self.signal.as_ref()), "STOPPED")?;
        self.revalidate_binding()?;
        need(file.is_absolute(), "SOURCE_PATH")?;
        let before = std::fs::metadata(file).map_err(|e| e.to_string())?;
        let end_byte = end_byte.unwrap_or(before.len() as i64);
        need(js_int(&json!(end_byte)).is_some() && end_byte >= 0 && end_byte as u64 <= before.len(),
            "FRONTIER")?;
        let resolved = path_resolve(&file.to_string_lossy());
        let mut prior_binding = prior_binding.cloned();
        if let Some(known) = self.receipts.get(&resolved.to_string_lossy().to_string()).cloned() {
            need(identity(&before) == known["nodeIdentity"].as_str().unwrap_or(""), "SOURCE_REPLACED")?;
            need(end_byte >= js_int(&known["sourceBytes"]).unwrap_or(i64::MIN), "SOURCE_ROLLBACK")?;
            prior_binding = Some(known);
        }
        let binding_mode = self.binding["mode"].clone();
        let binding_engine_hash = self.binding["engineHash"].clone();
        let validate = |r: &V, bytes: i64, bound: Option<&V>| -> Result<V, String> {
            need(object(r) && r["schemaVersion"].as_i64() == Some(2)
                && r["mode"] == binding_mode
                && r["engineHash"] == binding_engine_hash
                && r["sourceBytes"].as_i64() == Some(bytes)
                && hash_hex(&r["sourceHash"])
                && js_int(&r["records"]).map_or(false, |n| n >= 0)
                && r["verifyMs"].as_f64().map_or(false, |x| x.is_finite() && x >= 0.0),
                "RESULT_BINDING")?;
            need(source_stamp_matches(&r["sourceStamp"], &before), "SOURCE_STAMP")?;
            if r["records"].as_i64() == Some(0) {
                need(r["lastBody"].is_null() && r["tailHash"].as_str() == Some(""), "EMPTY_STATE")?;
            } else {
                need(r["lastBody"]["sequence"] == r["records"]
                    && hash_hex(&r["tailHash"])
                    && sha(js_stringify(&r["lastBody"]).as_bytes()) == r["tailHash"].as_str().unwrap_or(""),
                    "TAIL_STATE")?;
            }
            if bound.map(|b| baal_service_runtime::json_text::js_truthy(b.get("sourceStamp"))).unwrap_or(false) {
                let bound_stamp = bound.unwrap()["sourceStamp"].as_str().unwrap_or("");
                let r_stamp = r["sourceStamp"].as_str().unwrap_or("");
                let bound5: Vec<&str> = bound_stamp.split(':').collect();
                let r5: Vec<&str> = r_stamp.split(':').collect();
                need(bound5.len() >= 5 && r5.len() >= 5
                    && bound5[..5].join(":") == r5[..5].join(":"), "CALLER_IDENTITY")?;
                if bound.unwrap()["sourceBytes"].as_i64() == Some(before.len() as i64) {
                    need(bound_stamp == r_stamp, "CALLER_STAMP")?;
                }
            }
            if let Some(bound) = bound {
                need(bound["sourceBytes"].as_i64() == Some(bytes)
                    && bound["sourceHash"] == r["sourceHash"]
                    && bound["records"] == r["records"]
                    && bound["tailHash"] == r["tailHash"], "CALLER_BINDING")?;
            }
            Ok(r.clone())
        };
        if let Some(pb) = &prior_binding {
            if baal_service_runtime::json_text::js_truthy(pb.get("nodeIdentity")) {
                need(identity(&before) == pb["nodeIdentity"].as_str().unwrap_or(""), "SOURCE_REPLACED")?;
            }
            need(js_int(&pb["sourceBytes"]).is_some()
                && pb["sourceBytes"].as_i64().unwrap_or(i64::MAX) <= end_byte
                && hash_hex(&pb["sourceHash"])
                && js_int(&pb["records"]).is_some(), "PRIOR_BINDING")?;
            let pb_bytes = pb["sourceBytes"].as_i64().unwrap_or(0);
            if pb_bytes < end_byte {
                let r = self.request(&json!({"journal": file.to_string_lossy(),
                    "endByte": pb_bytes,
                    "expectedSha": pb["sourceHash"].as_str().unwrap_or("")}))?;
                validate(&r, pb_bytes, Some(pb))?;
            }
        }
        let mut expected = expected.cloned();
        if expected.is_none() && prior_binding.is_some()
            && prior_binding.as_ref().map(|pb| pb["sourceBytes"].as_i64().unwrap_or(-1) == end_byte).unwrap_or(false) {
            expected = prior_binding.clone();
        }
        if let Some(e) = &expected {
            if baal_service_runtime::json_text::js_truthy(e.get("nodeIdentity")) {
                need(identity(&before) == e["nodeIdentity"].as_str().unwrap_or(""), "SOURCE_REPLACED")?;
            }
            need(e["sourceBytes"].as_i64() == Some(end_byte)
                && hash_hex(&e["sourceHash"])
                && js_int(&e["records"]).is_some(), "CALLER_BINDING")?;
        }
        let result = {
            let r = self.request(&json!({"journal": file.to_string_lossy(), "endByte": end_byte,
                "expectedSha": expected.as_ref().and_then(|e| e["sourceHash"].as_str()).unwrap_or("COMPUTE")}))?;
            validate(&r, end_byte, expected.as_ref())?
        };
        need(can_operate() && !aborted(self.signal.as_ref())
            && node_stamp(&std::fs::metadata(file).map_err(|e| e.to_string())?) == node_stamp(&before),
            "STOP_OR_SOURCE_CHANGED")?;
        self.revalidate_binding()?;
        let mut receipt = result;
        receipt["nodeStamp"] = json!(node_stamp(&before));
        receipt["nodeIdentity"] = json!(identity(&before));
        receipt["file"] = json!(resolved.to_string_lossy());
        receipt["binaryHash"] = self.binding["binaryHash"].clone();
        self.receipts.insert(resolved.to_string_lossy().to_string(), receipt.clone());
        Ok(receipt)
    }
    /// openJournal(options) — returns a Journal plus the nativeRestore receipt.
    pub fn open_journal(&mut self, file: &Path, end_byte: Option<i64>, expected: Option<&V>,
        prior_binding: Option<&V>,
        can_operate: &(dyn Fn() -> bool + Send + Sync),
        validate_state: &(dyn Fn(&V, &V) -> Result<bool, String> + Send + Sync))
        -> Result<NativeJournal, String> {
        let receipt = self.restore(file, end_byte, expected, prior_binding, can_operate)?;
        let check = (|| -> Result<(), String> {
            need(validate_state(&receipt["lastBody"], &receipt)? == true, "REDUCER_REJECTED")?;
            need(receipt["records"].as_i64() == Some(0)
                || sha(js_stringify(&receipt["lastBody"]).as_bytes()) == receipt["tailHash"].as_str().unwrap_or(""),
                "REDUCER_MUTATED_BODY")?;
            Ok(())
        })();
        if let Err(e) = check {
            self.fail("REDUCER_REJECTED");
            return Err(e);
        }
        need(self.failure.is_none() && !aborted(self.signal.as_ref()) && can_operate()
            && node_stamp(&std::fs::metadata(file).map_err(|e| e.to_string())?) == receipt["nodeStamp"].as_str().unwrap_or(""),
            "STOP_OR_SOURCE_CHANGED")?;
        let journal = Journal {
            file: file.to_path_buf(),
            sequence: receipt["records"].as_u64().unwrap_or(0),
            previous: receipt["tailHash"].as_str().unwrap_or("").to_string(),
            last: if receipt["lastBody"].is_null() { None } else { Some(receipt["lastBody"].clone()) },
        };
        Ok(NativeJournal { journal, native_restore: receipt })
    }
    /// close()
    pub fn close(&mut self) { self.fail("CLOSED"); }
}
impl Drop for NativeJournalStreamV2 {
    fn drop(&mut self) { self.child.shutdown(); }
}
