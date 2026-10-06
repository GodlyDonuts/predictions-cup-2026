// live-paper/native-journal-append.mjs — exclusive native journal append adapter:
// binding verification, fresh reconciliation of the immutable prefix, then a
// serialized {op:open}/{op:commit} protocol over a pinned child process.
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use baal_execution::core::payload_hash;
use baal_paper::fixed::instant;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};

fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(format!("NATIVE_APPEND_{}", c)); }
    Ok(())
}
fn hex64(v: &V) -> bool {
    v.as_str().map(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())).unwrap_or(false)
}
fn object(v: &V) -> bool { v.is_object() }
pub(crate) fn sorted_keys(v: &V) -> String {
    let mut keys: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    keys.sort();
    keys.join("|")
}
/// path.resolve: absolute paths normalize lexically; relative resolve on cwd.
pub(crate) fn path_resolve(file: &str) -> PathBuf {
    let p = PathBuf::from(file);
    let p = if p.is_absolute() { p } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => { out.pop(); }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

const SOURCES: [&str; 6] = ["Cargo.toml", "Cargo.lock", "build.rs",
    "src/lib.rs", "src/main.rs", "src/canonical.rs"];
/// receiptKeys sorted.
const RECEIPT_KEYS: &str = "contestOrders|engineHash|failed|fileId|lastBody|mode|networkRequests|schemaVersion|sequence|sourceBytes|sourceHash|tailHash|windowsShareMode";

fn stat_append(file: &Path) -> Result<std::fs::Metadata, String> {
    let s = std::fs::metadata(file).map_err(|e| e.to_string())?;
    need(s.is_file(), "FILE")?;
    Ok(s)
}

#[cfg(unix)]
pub(crate) fn birthtime_ns(m: &std::fs::Metadata) -> u64 {
    m.created().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64).unwrap_or(0)
}
#[cfg(not(unix))]
pub(crate) fn birthtime_ns(m: &std::fs::Metadata) -> u64 {
    m.created().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64).unwrap_or(0)
}
#[cfg(unix)]
fn stat_ns(m: &std::fs::Metadata) -> (u64, u64, u64, u64, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (m.dev(), m.ino(), birthtime_ns(m), m.size(),
        m.mtime() as u64 * 1_000_000_000 + m.mtime_nsec() as u64,
        m.ctime() as u64 * 1_000_000_000 + m.ctime_nsec() as u64)
}
#[cfg(not(unix))]
fn stat_ns(m: &std::fs::Metadata) -> (u64, u64, u64, u64, u64, u64) {
    (0, 0, birthtime_ns(m), m.len(),
        m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0),
        0)
}
/// identity(s) = dev:ino:birthtimeNs
fn identity(s: &std::fs::Metadata) -> String {
    let (dev, ino, b, _, _, _) = stat_ns(s);
    format!("{}:{}:{}", dev, ino, b)
}
/// stamp(s) = dev:ino:birthtimeNs:size:mtimeNs:ctimeNs
fn stamp(s: &std::fs::Metadata) -> String {
    let (dev, ino, b, size, m, c) = stat_ns(s);
    format!("{}:{}:{}:{}:{}:{}", dev, ino, b, size, m, c)
}

/// journalAppendEngineHash(root) — sha256 over name\0bytes\0 per source file.
pub fn journal_append_engine_hash(root: &Path) -> Result<String, String> {
    let mut h = Sha256::new();
    for name in SOURCES {
        let bytes = std::fs::read(root.join(name)).map_err(|e| e.to_string())?;
        h.update(name.as_bytes());
        h.update([0u8]);
        h.update(&bytes);
        h.update([0u8]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// journalAppendBinding({binaryPath, sourceRoot, dependencies}).
pub fn journal_append_binding(binary_path: &Path, source_root: &Path, dependencies: &[&str])
    -> Result<V, String> {
    need(binary_path.is_absolute() && source_root.is_absolute(), "PATH")?;
    let mut files = serde_json::Map::new();
    for f in SOURCES {
        files.insert(f.to_string(), json!(sha256(&std::fs::read(source_root.join(f)).map_err(|e| e.to_string())?)));
    }
    Ok(json!({
        "schemaVersion": 1, "mode": "exclusive_js_journal_append_v1",
        "binaryHash": sha256(&std::fs::read(binary_path).map_err(|e| e.to_string())?),
        "engineHash": journal_append_engine_hash(source_root)?,
        "sourceHash": payload_hash(&V::Object(files.clone()))?,
        "sources": V::Object(files),
        "dependencies": dependencies.iter().map(|file| {
            let p = path_resolve(file);
            std::fs::read(&p).map(|b| json!({"path": p.to_string_lossy(), "hash": sha256(&b)}))
        }).collect::<Result<Vec<_>, _>>().map_err(|e: std::io::Error| e.to_string())?,
    }))
}

fn verify_binding(binary_path: &Path, source_root: &Path, b: &V)
    -> Result<Vec<(PathBuf, String)>, String> {
    let mut sorted_sources: Vec<&str> = SOURCES.to_vec();
    sorted_sources.sort();
    need(b["schemaVersion"].as_i64() == Some(1)
        && b["mode"].as_str() == Some("exclusive_js_journal_append_v1")
        && hex64(&b["binaryHash"]) && hex64(&b["engineHash"]) && hex64(&b["sourceHash"])
        && object(&b["sources"])
        && sorted_keys(&b["sources"]) == sorted_sources.join("|")
        && b["dependencies"].is_array(), "BINDING")?;
    need(sha256(&std::fs::read(binary_path).map_err(|e| e.to_string())?) == b["binaryHash"].as_str().unwrap_or("")
        && payload_hash(&b["sources"])? == b["sourceHash"].as_str().unwrap_or("")
        && journal_append_engine_hash(source_root)? == b["engineHash"].as_str().unwrap_or(""),
        "BINARY_ENGINE")?;
    let mut files = vec![binary_path.to_path_buf()];
    for f in SOURCES {
        let p = source_root.join(f);
        need(hex64(&b["sources"][f])
            && sha256(&std::fs::read(&p).map_err(|e| e.to_string())?) == b["sources"][f].as_str().unwrap_or(""),
            "SOURCE")?;
        files.push(p);
    }
    for d in b["dependencies"].as_array().cloned().unwrap_or_default() {
        let dp = PathBuf::from(d["path"].as_str().unwrap_or(""));
        need(dp.is_absolute() && hex64(&d["hash"])
            && sha256(&std::fs::read(&dp).map_err(|e| e.to_string())?) == d["hash"].as_str().unwrap_or(""),
            "DEPENDENCY")?;
        files.push(dp);
    }
    files.iter().map(|f| stat_append(f).map(|s| (f.clone(), stamp(&s)))).collect()
}

/// Result of reconcileJournalAppendSource — the verified head plus the running
/// prefix bytes and file identity.
pub struct ReconciledAppendSource {
    pub head: V,
    pub prefix_bytes: Vec<u8>,
    pub file_identity: String,
}

// Required caller callback verifies the ORIGINAL economic/source transitions;
// a valid hash chain alone is not a funded-accounting proof.
/// reconcileJournalAppendSource({file, validateRow, canOperate}).
/// validateRow(cloned_row, {sequence,previous,lastBody}) must return true.
pub fn reconcile_journal_append_source(
    file: &Path,
    validate_row: &(dyn Fn(&V, &V) -> Result<bool, String> + Send + Sync),
    can_operate: &(dyn Fn() -> bool + Send + Sync),
) -> Result<ReconciledAppendSource, String> {
    need(file.is_absolute(), "RECONCILE_OPTIONS")?;
    need(can_operate(), "STOPPED")?;
    let before = stat_append(file)?;
    let end = before.len();
    need(end <= i64::MAX as u64, "SIZE")?;
    let mut bytes: u64 = 0;
    let mut sequence: u64 = 0;
    let mut previous = String::new();
    let mut last_body: Option<V> = None;
    let mut prefix_bytes: Vec<u8> = Vec::new();
    for line in journal_lines(file, &JournalReadOptions {
        end_byte: Some(end), ..JournalReadOptions::default()
    })? {
        prefix_bytes.extend_from_slice(line.as_bytes());
        prefix_bytes.push(b'\n');
        bytes += (line.len() + 1) as u64;
        if line.is_empty() { continue; }
        let row: V = serde_json::from_str(&line).map_err(|_| "NATIVE_APPEND_SOURCE_CHAIN".to_string())?;
        let hash = row["hash"].clone();
        let mut body = row.clone();
        body.as_object_mut().map(|o| o.remove("hash"));
        sequence += 1;
        need(row["sequence"].as_i64() == Some(sequence as i64)
            && row["previous"].as_str() == Some(previous.as_str())
            && hex64(&hash)
            && sha256(js_stringify(&body).as_bytes()) == hash.as_str().unwrap_or("")
            && js_stringify(&row) == line, "SOURCE_CHAIN")?;
        instant(&crate::native_dip_host_v1::jstr(&body["at"]))?;
        let ctx = json!({"sequence": sequence, "previous": previous,
            "lastBody": last_body.clone().unwrap_or(V::Null)});
        need(validate_row(&row, &ctx)? == true, "ORIGINAL_STATE")?;
        previous = hash.as_str().unwrap_or("").to_string();
        last_body = Some(body);
        need(can_operate(), "STOPPED")?;
    }
    need(bytes == end && stamp(&stat_append(file)?) == stamp(&before), "SOURCE_CHANGED")?;
    let head = json!({"sourceBytes": bytes,
        "sourceHash": sha256(&prefix_bytes),
        "sequence": sequence, "tailHash": previous,
        "lastBody": last_body.unwrap_or(V::Null)});
    Ok(ReconciledAppendSource { head, prefix_bytes, file_identity: identity(&before) })
}

// --- serialized JSONL pipe child (shared by the native adapter modules) ---

pub(crate) enum PipeEvent {
    Line(String),
    /// cumulative stderr bytes observed (account-queue) — modules that fail on
    /// any stderr byte treat nonzero as fatal.
    Stderr(usize),
    /// stdout buffer exceeded the module's limit.
    Overflow,
    /// stdout reached EOF (child died or closed the pipe).
    Eof,
}

pub(crate) struct PipeChild {
    pub child: Child,
    pub(crate) stdin: Option<ChildStdin>,
    rx: mpsc::Receiver<PipeEvent>,
}

impl PipeChild {
    /// spawn(path, args, {stdio:'pipe', env}) — env None inherits the process env.
    pub(crate) fn spawn(path: &Path, args: &[String], env: Option<Vec<(String, String)>>,
        max_buffer_bytes: usize) -> std::io::Result<PipeChild> {
        let mut cmd = Command::new(path);
        cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(env) = env {
            cmd.env_clear();
            for (k, v) in env { cmd.env(k, v); }
        }
        let mut child = cmd.spawn()?;
        let stdin = Some(child.stdin.take().unwrap());
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel::<PipeEvent>();
        {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut pending: Vec<u8> = Vec::new();
                let mut buf = [0u8; 65536];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => { let _ = tx.send(PipeEvent::Eof); return; }
                        Ok(n) => {
                            pending.extend_from_slice(&buf[..n]);
                            if pending.len() > max_buffer_bytes {
                                let _ = tx.send(PipeEvent::Overflow);
                                return;
                            }
                            while let Some(pos) = pending.iter().position(|b| *b == b'\n') {
                                let line: Vec<u8> = pending.drain(..pos).collect();
                                pending.remove(0);
                                let _ = tx.send(PipeEvent::Line(String::from_utf8_lossy(&line).to_string()));
                            }
                        }
                    }
                }
            });
        }
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut total = 0usize;
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        total += n;
                        if tx.send(PipeEvent::Stderr(total)).is_err() { return; }
                    }
                }
            }
        });
        Ok(PipeChild { child, stdin, rx })
    }
    /// True while the child has not exited (JS child.exitCode===null && !killed).
    pub(crate) fn alive(&mut self) -> bool {
        self.child.try_wait().map(|s| s.is_none()).unwrap_or(false)
    }
    /// Write one JSON line to stdin.
    pub(crate) fn write_line(&mut self, text: &str) -> Result<(), ()> {
        match self.stdin.as_mut() {
            Some(s) => s.write_all(text.as_bytes()).and_then(|_| s.write_all(b"\n")).map_err(|_| ()),
            None => Err(()),
        }
    }
    /// Any event already queued before a request is written is unsolicited.
    pub(crate) fn drain_event(&mut self) -> Option<PipeEvent> { self.rx.try_recv().ok() }
    pub(crate) fn recv(&mut self, timeout: Duration) -> Result<PipeEvent, ()> {
        self.rx.recv_timeout(timeout).map_err(|_| ())
    }
    pub(crate) fn kill(&mut self) { let _ = self.child.kill(); }
    /// close(): end stdin, kill, and reap the child (JS await closedPromise).
    pub(crate) fn shutdown(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The serialized adapter state for openExclusiveNativeJournal.
pub struct ExclusiveNativeJournal {
    pub binding: V,
    pub pid: u32,
    pins: Vec<(PathBuf, String)>,
    file: PathBuf,
    file_identity: String,
    current: V,
    prefix_bytes: Vec<u8>,
    file_id: Option<String>,
    failed: Option<String>,
    closed: bool,
    uncertain: Option<V>,
    child: PipeChild,
    timeout: Duration,
    can_operate: Box<dyn Fn() -> bool + Send + Sync>,
    validate_transition: Box<dyn Fn(&V, &V, &V) -> Result<bool, String> + Send + Sync>,
    stop_paths: Vec<PathBuf>,
    owner_token: String,
    generation: i64,
    serial: Arc<Mutex<()>>,
}

impl ExclusiveNativeJournal {
    fn poison(&mut self, code: &str) -> String {
        if self.failed.is_none() {
            self.failed = Some(format!("NATIVE_APPEND_{}", code));
            self.child.kill();
        }
        self.failed.clone().unwrap()
    }
    fn bound(&mut self) -> Result<(), String> {
        need(!self.closed && self.failed.is_none() && self.child.alive(), "UNAVAILABLE")?;
        let checks: Result<(), String> = (|| {
            for (f, s) in &self.pins {
                need(stamp(&stat_append(f)?) == *s, "BINDING_CHANGED")?;
            }
            need(identity(&stat_append(&self.file)?) == self.file_identity, "SOURCE_REPLACED")?;
            Ok(())
        })();
        if let Err(_) = checks {
            let msg = self.poison("BINDING_OR_FILE_CHANGED");
            return Err(msg);
        }
        Ok(())
    }
    fn operating(&self) -> Result<(), String> {
        need((self.can_operate)() && !self.stop_paths.iter().any(|f| f.exists()), "STOPPED")
    }
    fn request(&mut self, q: &V) -> Result<V, String> {
        self.bound()?;
        if let Some(ev) = self.child.drain_event() {
            return Err(match ev {
                PipeEvent::Line(_) => self.poison("UNSOLICITED_RESPONSE"),
                PipeEvent::Stderr(_) => self.poison("UNEXPECTED_STDERR"),
                PipeEvent::Overflow => self.poison("RESPONSE_LIMIT_UNKNOWN_ACK"),
                PipeEvent::Eof => self.poison("PROCESS_DIED_UNKNOWN_ACK"),
            });
        }
        if self.child.write_line(&js_stringify(q)).is_err() {
            return Err(self.poison("STDIN_UNKNOWN_ACK"));
        }
        let line = loop {
            match self.child.recv(self.timeout) {
                Ok(PipeEvent::Line(l)) => break l,
                Ok(PipeEvent::Stderr(_)) => return Err(self.poison("UNEXPECTED_STDERR")),
                Ok(PipeEvent::Overflow) => return Err(self.poison("RESPONSE_LIMIT_UNKNOWN_ACK")),
                Ok(PipeEvent::Eof) => return Err(self.poison("PROCESS_DIED_UNKNOWN_ACK")),
                Err(()) => return Err(self.poison("TIMEOUT_UNKNOWN_ACK")),
            }
        };
        let r: Result<V, _> = serde_json::from_str(&line);
        let r = match r {
            Ok(v) if object(&v) => v,
            _ => return Err(self.poison("MALFORMED_OUTPUT_UNKNOWN_ACK")),
        };
        self.bound()?;
        if !r["error"].is_null() {
            let keys_ok = sorted_keys(&r) == "error";
            let code_ok = r["error"].as_str()
                .map(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
                .unwrap_or(false);
            need(keys_ok && code_ok, "ERROR_SCHEMA")?;
            let e = r["error"].as_str().unwrap_or("");
            if e.contains("UNKNOWN") || e.contains("DURABILITY")
                || e.contains("FILE_CHANGED") || e.contains("PATH_CHANGED")
                || e.contains("FAILED_RECONCILE") {
                let msg = self.poison(&format!("WORKER_{}", e));
                return Err(msg);
            }
            return Err(format!("NATIVE_APPEND_REJECTED_{}", e));
        }
        Ok(r)
    }
    fn guards(&self) -> V {
        json!({"ownerToken": self.owner_token, "generation": self.generation,
            "canOperate": true, "stopped": false,
            "expectedBytes": self.current["sourceBytes"],
            "expectedSourceHash": self.current["sourceHash"],
            "expectedSequence": self.current["sequence"],
            "expectedTailHash": self.current["tailHash"]})
    }
    fn validate_receipt(&mut self, r: &V, expected: &V, body_json: Option<&str>) -> Result<(), String> {
        need(object(r) && sorted_keys(r) == RECEIPT_KEYS
            && r["schemaVersion"].as_i64() == Some(1)
            && r["mode"] == self.binding["mode"]
            && r["engineHash"] == self.binding["engineHash"]
            && r["fileId"].as_str().map(|s| {
                let parts: Vec<&str> = s.split(':').collect();
                parts.len() == 2 && parts[0].len() == 8 && parts[1].len() == 16
                    && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
            }).unwrap_or(false)
            && (self.file_id.is_none() || r["fileId"].as_str() == self.file_id.as_deref())
            && r["sequence"] == expected["sequence"]
            && r["tailHash"] == expected["tailHash"]
            && r["sourceBytes"] == expected["sourceBytes"]
            && r["sourceHash"] == expected["sourceHash"]
            && (match body_json { Some(j) => r["lastBody"].as_str() == Some(j), None => r["lastBody"].is_null() })
            && r["failed"].as_bool() == Some(false)
            && r["windowsShareMode"].as_str() == Some("FILE_SHARE_READ_ONLY")
            && r["networkRequests"].as_i64() == Some(0)
            && r["contestOrders"].as_i64() == Some(0), "RECEIPT_BINDING")?;
        need(stat_append(&self.file)?.len() == expected["sourceBytes"].as_u64().unwrap_or(u64::MAX),
            "DURABLE_SIZE")?;
        Ok(())
    }
    /// adapter.head()
    pub fn head(&self) -> V { self.current.clone() }
    /// adapter.status()
    pub fn status(&self) -> V {
        json!({"pid": self.pid, "closed": self.closed,
            "failed": self.failed.clone().map_or(V::Null, V::String),
            "lastAcknowledgedHead": self.current,
            "unknownAcceptance": self.uncertain.clone().unwrap_or(V::Null),
            "fileId": self.file_id.clone().map_or(V::Null, V::String),
            "binaryHash": self.binding["binaryHash"], "engineHash": self.binding["engineHash"],
            "networkRequests": 0, "contestOrders": 0})
    }
    /// adapter.commit(state, event, {at}).
    pub fn commit(&mut self, state: &V, event: &V, at: Option<&str>) -> Result<V, String> {
        let at = match at {
            Some(a) => a.to_string(),
            None => {
                let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64).unwrap_or(0.0);
                baal_service_runtime::contract::ms_to_iso(ms)
            }
        };
        instant(&at)?;
        let _serial = self.serial.clone();
        let _guard = _serial.lock().unwrap_or_else(|e| e.into_inner());
        let proposed = state.clone();
        let input = event.clone();
        self.bound()?;
        self.operating()?;
        let ctx = json!({"lastBody": self.current["lastBody"], "head": self.current, "at": at});
        need((self.validate_transition)(&proposed, &input, &ctx)? == true, "ECONOMIC_TRANSITION")?;
        self.operating()?;
        let body = json!({"sequence": self.current["sequence"].as_i64().unwrap_or(0) + 1,
            "previous": self.current["tailHash"], "at": at, "event": input, "state": proposed});
        let body_json = js_stringify(&body);
        let hash = sha256(body_json.as_bytes());
        let mut record = body.clone();
        record["hash"] = json!(hash);
        let record_json = js_stringify(&record);
        let line = format!("{}\n", record_json);
        let mut next_prefix = self.prefix_bytes.clone();
        next_prefix.extend_from_slice(line.as_bytes());
        let expected = json!({
            "sourceBytes": self.current["sourceBytes"].as_u64().unwrap_or(0) + line.len() as u64,
            "sourceHash": sha256(&next_prefix),
            "sequence": body["sequence"], "tailHash": hash, "lastBody": body,
        });
        self.uncertain = Some(json!({"sequence": body["sequence"], "hash": hash,
            "bodyHash": payload_hash(&body).unwrap_or_default(),
            "stateHash": payload_hash(&proposed).unwrap_or_default()}));
        let result = (|s: &mut Self| -> Result<V, String> {
            let mut q = json!({"op": "commit"});
            for (k, v) in s.guards().as_object().unwrap().iter() { q[k.clone()] = v.clone(); }
            q["bodyJson"] = json!(body_json);
            let r = s.request(&q)?;
            need(sorted_keys(&r) == "durable|receipt|recordJson"
                && r["durable"].as_bool() == Some(true)
                && r["recordJson"].as_str() == Some(record_json.as_str()), "ACK_RECORD")?;
            s.validate_receipt(&r["receipt"], &expected, Some(&body_json))?;
            Ok(r["receipt"].clone())
        })(self);
        match result {
            Ok(receipt) => {
                self.current = expected;
                self.prefix_bytes = next_prefix;
                self.uncertain = None;
                Ok(json!({"hash": hash, "sequence": body["sequence"], "receipt": receipt}))
            }
            Err(e) => {
                if e.starts_with("NATIVE_APPEND_REJECTED_") && self.failed.is_none() {
                    self.uncertain = None;
                    return Err(e);
                }
                if self.failed.is_none() { self.poison("INVALID_OR_UNKNOWN_ACK"); }
                Err(self.failed.clone().unwrap_or(e))
            }
        }
    }
    /// adapter.close()
    pub fn close(&mut self) {
        if self.closed { return; }
        self.closed = true;
        self.child.shutdown();
    }
}
impl Drop for ExclusiveNativeJournal {
    fn drop(&mut self) { self.close(); }
}

/// openExclusiveNativeJournal({file, binaryPath, sourceRoot, binding, head,
/// ownerToken, generation, stopPaths, canOperate, validateRow,
/// validateTransition, timeoutMs, maxResponseBytes}).
pub fn open_exclusive_native_journal(
    file: &Path,
    binary_path: &Path,
    source_root: &Path,
    binding: &V,
    head: &V,
    owner_token: &str,
    generation: i64,
    stop_paths: &[&str],
    can_operate: Box<dyn Fn() -> bool + Send + Sync>,
    validate_row: &(dyn Fn(&V, &V) -> Result<bool, String> + Send + Sync),
    validate_transition: Box<dyn Fn(&V, &V, &V) -> Result<bool, String> + Send + Sync>,
    timeout_ms: i64,
    max_response_bytes: usize,
) -> Result<ExclusiveNativeJournal, String> {
    need([file.to_path_buf(), binary_path.to_path_buf(), source_root.to_path_buf()]
        .iter().all(|p| p.is_absolute())
        && stop_paths.iter().all(|s| Path::new(s).is_absolute())
        && !owner_token.is_empty()
        && generation >= 0 && timeout_ms > 0 && max_response_bytes > 0, "OPTIONS")?;
    let b = binding.clone();
    let pins = verify_binding(binary_path, source_root, &b)?;
    let restored = reconcile_journal_append_source(file, validate_row, &*can_operate)?;
    need(payload_hash(&restored.head)? == payload_hash(head)?, "FRESH_RECONCILIATION_HEAD")?;
    let current = restored.head.clone();
    // spawn env: SystemRoot/WINDIR + PATH of binary and dependency dirs.
    let sysroot = std::env::var("SystemRoot").or_else(|_| std::env::var("WINDIR")).unwrap_or_default();
    let dep_dirs: Vec<String> = b["dependencies"].as_array().cloned().unwrap_or_default()
        .iter().filter_map(|d| Path::new(d["path"].as_str().unwrap_or("")).parent()
            .map(|p| p.to_string_lossy().to_string()))
        .collect();
    let mut path_parts = vec![binary_path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()];
    path_parts.extend(dep_dirs);
    path_parts.push(Path::new(&sysroot.clone()).join(if sysroot.is_empty() { "C:/Windows".into() } else { sysroot.clone() })
        .join("System32").to_string_lossy().to_string());
    let env = vec![
        ("SystemRoot".to_string(), sysroot.clone()),
        ("WINDIR".to_string(), std::env::var("WINDIR").or_else(|_| std::env::var("SystemRoot")).unwrap_or_default()),
        ("PATH".to_string(), path_parts.join(":")),
    ];
    let child = PipeChild::spawn(binary_path, &[], Some(env), max_response_bytes)
        .map_err(|_| "NATIVE_APPEND_PROCESS_UNAVAILABLE".to_string())?;
    let pid = child.child.id();
    let mut adapter = ExclusiveNativeJournal {
        binding: b, pid, pins, file: file.to_path_buf(),
        file_identity: restored.file_identity,
        current, prefix_bytes: restored.prefix_bytes,
        file_id: None, failed: None, closed: false, uncertain: None,
        child, timeout: Duration::from_millis(timeout_ms as u64),
        can_operate, validate_transition,
        stop_paths: stop_paths.iter().map(PathBuf::from).collect(),
        owner_token: owner_token.to_string(), generation,
        serial: Arc::new(Mutex::new(())),
    };
    let opened = (|a: &mut ExclusiveNativeJournal| -> Result<V, String> {
        a.operating()?;
        let mut q = json!({"op": "open", "path": file.to_string_lossy(),
            "createNew": false, "stopPaths": stop_paths});
        for (k, v) in a.guards().as_object().unwrap().iter() { q[k.clone()] = v.clone(); }
        let opened = a.request(&q)?;
        need(sorted_keys(&opened) == "opened|receipt" && opened["opened"].as_bool() == Some(true),
            "OPEN_RESPONSE")?;
        let last = a.current["lastBody"].clone();
        let body_json = if last.is_null() { None } else { Some(js_stringify(&last)) };
        a.validate_receipt(&opened["receipt"], &a.current.clone(), body_json.as_deref())?;
        a.file_id = opened["receipt"]["fileId"].as_str().map(str::to_string);
        Ok(opened)
    })(&mut adapter);
    match opened {
        Ok(_) => Ok(adapter),
        Err(e) => {
            adapter.poison("OPEN_FAILED");
            adapter.close();
            Err(e)
        }
    }
}

// js_int import retained for parity with callers that pass untyped options.
#[allow(dead_code)]
fn _t(v: &V) -> Option<i64> { js_int(v) }
