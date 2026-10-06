// live-paper/native-journal-append.mjs — exclusive native journal append adapter.
// Spawns baal-journal-append.exe and exchanges JSON-lines requests/receipts over
// stdio. The child owns the file; every commit is acknowledged with a receipt.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;

enum Msg { Line(String), Stderr, Eof }
use std::time::Duration;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("NATIVE_APPEND_{}", code)); }
    Ok(())
}
fn sha(v: &V) -> Result<String, String> { Ok(sha256(js_stringify(v).as_bytes())) }
fn hex64(v: &V) -> bool { v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())) }
fn object(v: &V) -> bool { v.is_object() }
fn instant(v: &V) -> Result<i64, String> { baal_paper::fixed::instant(v.as_str().unwrap_or("")) }

const SOURCES: [&str; 6] = ["Cargo.toml", "Cargo.lock", "build.rs", "src/lib.rs", "src/main.rs", "src/canonical.rs"];
const RECEIPT_KEYS: &str = "contestOrders|engineHash|failed|fileId|lastBody|mode|networkRequests|schemaVersion|sequence|sourceBytes|sourceHash|tailHash|windowsShareMode";

fn stat(file: &Path) -> Result<std::fs::Metadata, String> {
    let s = std::fs::metadata(file).map_err(|e| e.to_string())?;
    need(s.is_file(), "FILE")?;
    Ok(s)
}
fn identity(s: &std::fs::Metadata) -> String {
    let btime = s.created().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().to_string()).unwrap_or_else(|| "0".to_string());
    format!("{}:{}:{}", s.dev(), s.ino(), btime)
}
fn stamp(s: &std::fs::Metadata) -> Result<String, String> {
    let mtime = s.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().to_string()).unwrap_or_else(|| "0".to_string());
    let ctime = s.ctime().to_string() + &format!("{:09}", s.ctime_nsec());
    Ok(format!("{}:{}:{}:{}", identity(s), s.len(), mtime, ctime))
}
/// journalAppendEngineHash(root)
pub fn journal_append_engine_hash(root: &Path) -> Result<String, String> {
    let mut h = Sha256::new();
    for name in SOURCES {
        h.update(name.as_bytes());
        h.update([0u8]);
        h.update(std::fs::read(root.join(name)).map_err(|e| e.to_string())?);
        h.update([0u8]);
    }
    Ok(format!("{:x}", h.finalize()))
}
/// journalAppendBinding({binaryPath, sourceRoot, dependencies})
pub fn journal_append_binding(binary_path: &Path, source_root: &Path, dependencies: &[&Path]) -> Result<V, String> {
    need(binary_path.is_absolute() && source_root.is_absolute(), "PATH")?;
    let mut files = Map::new();
    for f in SOURCES {
        files.insert(f.to_string(), json!(sha256(&std::fs::read(source_root.join(f)).map_err(|e| e.to_string())?)));
    }
    let deps: Vec<V> = dependencies.iter().map(|file| -> Result<V, String> {
        let p = file.canonicalize().map_err(|e| e.to_string())?;
        Ok(json!({"path": p.to_string_lossy(), "hash": sha256(&std::fs::read(&p).map_err(|e| e.to_string())?)}))
    }).collect::<Result<_, _>>()?;
    let sources_v = V::Object(files);
    Ok(json!({"schemaVersion": 1, "mode": "exclusive_js_journal_append_v1",
        "binaryHash": sha256(&std::fs::read(binary_path).map_err(|e| e.to_string())?),
        "engineHash": journal_append_engine_hash(source_root)?,
        "sourceHash": payload_hash(&sources_v)?, "sources": sources_v, "dependencies": deps}))
}
fn verify_binding(binary_path: &Path, source_root: &Path, b: &V) -> Result<Vec<(PathBuf, String)>, String> {
    need(b["schemaVersion"].as_i64() == Some(1)
        && b["mode"].as_str() == Some("exclusive_js_journal_append_v1")
        && hex64(&b["binaryHash"]) && hex64(&b["engineHash"]) && hex64(&b["sourceHash"])
        && object(&b["sources"])
        && {
            let mut keys: Vec<String> = b["sources"].as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys.join("|") == {
                let mut s: Vec<&str> = SOURCES.to_vec();
                s.sort();
                s.join("|")
            }
        }
        && b["dependencies"].is_array(), "BINDING")?;
    need(sha256(&std::fs::read(binary_path).map_err(|e| e.to_string())?) == b["binaryHash"].as_str().unwrap_or("")
        && payload_hash(&b["sources"]).ok().as_deref() == b["sourceHash"].as_str()
        && journal_append_engine_hash(source_root)? == b["engineHash"].as_str().unwrap_or(""), "BINARY_ENGINE")?;
    let mut files = vec![binary_path.to_path_buf()];
    for f in SOURCES {
        let p = source_root.join(f);
        need(hex64(&b["sources"][f])
            && sha256(&std::fs::read(&p).map_err(|e| e.to_string())?) == b["sources"][f].as_str().unwrap_or(""), "SOURCE")?;
        files.push(p);
    }
    for d in b["dependencies"].as_array().cloned().unwrap_or_default() {
        let dp = PathBuf::from(d["path"].as_str().unwrap_or(""));
        need(dp.is_absolute() && hex64(&d["hash"])
            && sha256(&std::fs::read(&dp).map_err(|e| e.to_string())?) == d["hash"].as_str().unwrap_or(""), "DEPENDENCY")?;
        files.push(dp);
    }
    files.into_iter().map(|f| stat(&f).and_then(|s| stamp(&s)).map(|st| (f, st))).collect()
}
/// Result head of reconcileJournalAppendSource / a committed journal head.
#[derive(Clone)]
pub struct ReconciledJournal {
    pub head: V,
    pub prefix: Sha256,
    pub file_identity: String,
}
/// reconcileJournalAppendSource({file, validateRow, canOperate})
pub fn reconcile_journal_append_source(file: &Path,
    validate_row: &mut dyn FnMut(&V, &V) -> Result<bool, String>,
    can_operate: &dyn Fn() -> bool) -> Result<ReconciledJournal, String> {
    need(file.is_absolute(), "RECONCILE_OPTIONS")?;
    need(can_operate(), "STOPPED")?;
    let before = stat(file)?;
    let end = before.len();
    let mut bytes = 0u64;
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut last_body = V::Null;
    let mut prefix = Sha256::new();
    for line in baal_service_runtime::journal::journal_lines(file,
        &baal_service_runtime::journal::JournalReadOptions { end_byte: Some(end), ..Default::default() })?
    {
        prefix.update(format!("{}\n", line).as_bytes());
        bytes += (line.len() + 1) as u64;
        if line.is_empty() { continue; }
        let row: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().and_then(|m| m.shift_remove("hash"));
        sequence += 1;
        need(row["sequence"].as_i64() == Some(sequence)
            && row["previous"].as_str().unwrap_or("") == previous
            && hex64(&row["hash"])
            && sha(&body).ok().as_deref() == Some(hash.as_str())
            && js_stringify(&row) == line, "SOURCE_CHAIN")?;
        instant(&body["at"])?;
        need(validate_row(&row, &json!({"sequence": sequence, "previous": previous,
            "lastBody": last_body.clone()}))?, "ORIGINAL_STATE")?;
        previous = hash;
        last_body = body;
        need(can_operate(), "STOPPED")?;
    }
    need(bytes == end && stamp(&stat(file)?)? == stamp(&before)?, "SOURCE_CHANGED")?;
    let head = json!({"sourceBytes": bytes, "sourceHash": format!("{:x}", prefix.clone().finalize()),
        "sequence": sequence, "tailHash": previous,
        "lastBody": if last_body.is_null() { V::Null } else { last_body }});
    Ok(ReconciledJournal { head, prefix, file_identity: identity(&before) })
}
/// Exclusive native journal adapter. `commit` is synchronous (the JS serial
/// promise chain maps to &mut self call order).
pub struct ExclusiveNativeJournal {
    pub binding: V,
    pub pid: u32,
    file: PathBuf,
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<Msg>,
    current: V,
    prefix: Sha256,
    file_id: Option<String>,
    pins: Vec<(PathBuf, String)>,
    restored_identity: String,
    stop_paths: Vec<PathBuf>,
    can_operate: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
    validate_transition: std::sync::Arc<dyn Fn(&V, &V, &V) -> Result<bool, String> + Send + Sync>,
    timeout: Duration,
    closed: bool,
    failed: Option<String>,
    uncertain: V,
}
impl ExclusiveNativeJournal {
    /// openExclusiveNativeJournal({file, binaryPath, sourceRoot, binding, head,
    /// ownerToken, generation, stopPaths, canOperate, validateRow,
    /// validateTransition, timeoutMs, maxResponseBytes})
    #[allow(clippy::too_many_arguments)]
    pub fn open(file: &Path, binary_path: &Path, source_root: &Path, binding: &V, head: &V,
        owner_token: &str, generation: i64, stop_paths: &[PathBuf],
        can_operate: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        validate_row: &mut dyn FnMut(&V, &V) -> Result<bool, String>,
        validate_transition: std::sync::Arc<dyn Fn(&V, &V, &V) -> Result<bool, String> + Send + Sync>,
        timeout_ms: i64, max_response_bytes: i64) -> Result<Self, String> {
        need([file, binary_path, source_root].iter().all(|p| p.is_absolute())
            && stop_paths.iter().all(|p| p.is_absolute())
            && !owner_token.is_empty()
            && generation >= 0 && timeout_ms > 0 && max_response_bytes > 0, "OPTIONS")?;
        let b = binding.clone();
        let pins = verify_binding(binary_path, source_root, &b)?;
        let restored = reconcile_journal_append_source(file, validate_row,
            &|| can_operate())?;
        need(payload_hash(&restored.head).ok() == payload_hash(head).ok(), "FRESH_RECONCILIATION_HEAD")?;
        let current = restored.head.clone();
        let prefix = restored.prefix.clone();
        let system_root = std::env::var("SystemRoot").or_else(|_| std::env::var("WINDIR")).unwrap_or_default();
        let mut path_var = binary_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let dep_paths: Vec<PathBuf> = b["dependencies"].as_array().cloned().unwrap_or_default().iter()
            .filter_map(|d| PathBuf::from(d["path"].as_str().unwrap_or("")).parent().map(|p| p.to_path_buf()))
            .collect();
        let mut env_path = vec![path_var.clone()];
        env_path.extend(dep_paths);
        env_path.push(PathBuf::from(if system_root.is_empty() { "C:/Windows".to_string() } else { system_root.clone() }).join("System32"));
        path_var = env_path.iter().fold(PathBuf::new(), |mut acc, p| { acc.push(p); acc });
        let joined = env_path.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>().join(":");
        let mut child = Command::new(binary_path)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .env_clear()
            .env("SystemRoot", &system_root)
            .env("WINDIR", if system_root.is_empty() { &system_root } else { &system_root })
            .env("PATH", &joined)
            .spawn().map_err(|_| "NATIVE_APPEND_PROCESS_UNAVAILABLE".to_string())?;
        let mut stderr = child.stderr.take();
        let stdout = child.stdout.take().unwrap();
        let stdin = child.stdin.take().unwrap();
        let (tx, lines) = mpsc::channel::<Msg>();
        let tx_out = tx.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) | Err(_) => { let _ = tx_out.send(Msg::Eof); break; }
                    Ok(_) => { let _ = tx_out.send(Msg::Line(buf.trim_end_matches('\n').to_string())); }
                }
            }
        });
        std::thread::spawn(move || {
            if let Some(mut e) = stderr.take() {
                let mut buf = [0u8; 1024];
                if e.read(&mut buf).map(|n| n > 0).unwrap_or(false) {
                    let _ = tx.send(Msg::Stderr);
                }
            }
        });
        let mut journal = Self {
            binding: b.clone(), pid: child.id(), file: file.to_path_buf(), child, stdin: Some(stdin), lines,
            current, prefix, file_id: None, pins, restored_identity: restored.file_identity,
            stop_paths: stop_paths.to_vec(), can_operate, validate_transition,
            timeout: Duration::from_millis(timeout_ms as u64), closed: false, failed: None,
            uncertain: V::Null,
        };
        let opened = journal.request(&json!({"op": "open", "path": file.to_string_lossy(),
            "createNew": false, "stopPaths": stop_paths.iter().map(|p| json!(p.to_string_lossy())).collect::<Vec<_>>(),
            "ownerToken": owner_token, "generation": generation,
            "canOperate": true, "stopped": false,
            "expectedBytes": journal.current["sourceBytes"], "expectedSourceHash": journal.current["sourceHash"],
            "expectedSequence": journal.current["sequence"], "expectedTailHash": journal.current["tailHash"]}));
        match opened.and_then(|o| {
            let mut keys: Vec<String> = o.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
            keys.sort();
            need(keys.join("|") == "opened|receipt" && o["opened"] == V::Bool(true), "OPEN_RESPONSE")?;
            let last = journal.current["lastBody"].clone();
            journal.validate_receipt(&o["receipt"], &journal.current.clone(),
                if last.is_null() { None } else { Some(js_stringify(&last)) })?;
            journal.file_id = o["receipt"]["fileId"].as_str().map(String::from);
            Ok(())
        }) {
            Ok(()) => Ok(journal),
            Err(e) => {
                journal.failed = Some(format!("NATIVE_APPEND_{}", "OPEN_FAILED"));
                let _ = journal.close_inner();
                Err(e)
            }
        }
    }
    fn bound(&mut self) -> Result<(), String> {
        need(!self.closed && self.failed.is_none()
            && self.child.try_wait().ok().flatten().is_none(), "UNAVAILABLE")?;
        for (f, s) in &self.pins {
            if stamp(&stat(f)?)? != *s {
                self.failed = Some("NATIVE_APPEND_BINDING_OR_FILE_CHANGED".into());
                return Err(self.failed.clone().unwrap());
            }
        }
        if identity(&stat(&self.file)?) != self.restored_identity {
            self.failed = Some("NATIVE_APPEND_BINDING_OR_FILE_CHANGED".into());
            return Err(self.failed.clone().unwrap());
        }
        Ok(())
    }
    fn operating(&self) -> Result<(), String> {
        need((self.can_operate)() && !self.stop_paths.iter().any(|f| f.exists()), "STOPPED")
    }
    fn guards(&self) -> V {
        json!({"canOperate": true, "stopped": false,
            "expectedBytes": self.current["sourceBytes"], "expectedSourceHash": self.current["sourceHash"],
            "expectedSequence": self.current["sequence"], "expectedTailHash": self.current["tailHash"]})
    }
    fn request(&mut self, q: &V) -> Result<V, String> {
        self.bound()?;
        let line = js_stringify(q) + "\n";
        let write_ok = self.stdin.as_mut()
            .map(|w| w.write_all(line.as_bytes()).and_then(|_| w.flush()).is_ok())
            .unwrap_or(false);
        if !write_ok {
            self.failed = Some("NATIVE_APPEND_STDIN_UNKNOWN_ACK".into());
            return Err(self.failed.clone().unwrap());
        }
        let line = match self.lines.recv_timeout(self.timeout) {
            Ok(Msg::Line(l)) => l,
            Ok(Msg::Stderr) => {
                self.failed = Some("NATIVE_APPEND_UNEXPECTED_STDERR".into());
                return Err(self.failed.clone().unwrap());
            }
            Ok(Msg::Eof) => {
                self.failed = Some("NATIVE_APPEND_PROCESS_DIED_UNKNOWN_ACK".into());
                return Err(self.failed.clone().unwrap());
            }
            Err(_) => {
                self.failed = Some("NATIVE_APPEND_TIMEOUT_UNKNOWN_ACK".into());
                return Err(self.failed.clone().unwrap());
            }
        };
        self.bound()?;
        let r: V = match serde_json::from_str::<V>(&line) {
            Ok(v) if v.is_object() => v,
            _ => {
                self.failed = Some("NATIVE_APPEND_MALFORMED_OUTPUT_UNKNOWN_ACK".into());
                return Err("NATIVE_APPEND_MALFORMED_OUTPUT_UNKNOWN_ACK".into());
            }
        };
        if baal_service_runtime::json_text::js_truthy(r.get("error")) {
            let e = r["error"].as_str().unwrap_or("");
            need(r.as_object().map(|m| m.len() == 1).unwrap_or(false)
                && !e.is_empty() && e.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'), "ERROR_SCHEMA")?;
            if ["UNKNOWN", "DURABILITY", "FILE_CHANGED", "PATH_CHANGED", "FAILED_RECONCILE"]
                .iter().any(|w| e.contains(w))
            {
                self.failed = Some(format!("NATIVE_APPEND_WORKER_{}", e));
                return Err(self.failed.clone().unwrap());
            }
            return Err(format!("NATIVE_APPEND_REJECTED_{}", e));
        }
        Ok(r)
    }
    fn validate_receipt(&self, r: &V, expected: &V, body_json: Option<String>) -> Result<(), String> {
        let mut keys: Vec<String> = r.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        keys.sort();
        need(object(r) && keys.join("|") == RECEIPT_KEYS
            && r["schemaVersion"].as_i64() == Some(1)
            && r["mode"] == self.binding["mode"] && r["engineHash"] == self.binding["engineHash"]
            && r["fileId"].as_str().is_some_and(|f| {
                let b = f.as_bytes();
                b.len() == 25 && b[8] == b':'
                    && b[..8].iter().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                    && b[9..].iter().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            })
            && (self.file_id.is_none() || r["fileId"].as_str() == self.file_id.as_deref())
            && r["sequence"] == expected["sequence"] && r["tailHash"] == expected["tailHash"]
            && r["sourceBytes"] == expected["sourceBytes"] && r["sourceHash"] == expected["sourceHash"]
            && match &body_json {
                Some(j) => r["lastBody"].as_str() == Some(j.as_str()),
                None => r["lastBody"].is_null(),
            }
            && r["failed"] == V::Bool(false)
            && r["windowsShareMode"].as_str() == Some("FILE_SHARE_READ_ONLY")
            && r["networkRequests"].as_i64() == Some(0)
            && r["contestOrders"].as_i64() == Some(0), "RECEIPT_BINDING")?;
        need(stat(&self.file)?.len() == expected["sourceBytes"].as_u64().unwrap_or(0), "DURABLE_SIZE")
    }
    /// head()
    pub fn head(&self) -> V { self.current.clone() }
    /// status()
    pub fn status(&self) -> V {
        json!({"pid": self.pid, "closed": self.closed, "failed": self.failed,
            "lastAcknowledgedHead": self.current, "unknownAcceptance": self.uncertain,
            "fileId": self.file_id, "binaryHash": self.binding["binaryHash"],
            "engineHash": self.binding["engineHash"], "networkRequests": 0, "contestOrders": 0})
    }
    /// commit(state, event, {at})
    pub fn commit(&mut self, state: &V, event: &V, at: &str) -> Result<V, String> {
        instant(&V::String(at.to_string()))?;
        let r = (|| -> Result<V, String> {
            self.bound()?;
            self.operating()?;
            let vt_args = json!({"lastBody": self.current["lastBody"], "head": self.current, "at": at});
            need((self.validate_transition)(state, event, &vt_args)?, "ECONOMIC_TRANSITION")?;
            self.operating()?;
            let body = json!({"sequence": self.current["sequence"].as_i64().unwrap_or(0) + 1,
                "previous": self.current["tailHash"], "at": at, "event": event, "state": state});
            let body_json = js_stringify(&body);
            let hash = sha256(body_json.as_bytes());
            let mut record = body.clone();
            record["hash"] = json!(hash);
            let record_json = js_stringify(&record);
            let line = format!("{}\n", record_json);
            let mut next_prefix = self.prefix.clone();
            next_prefix.update(line.as_bytes());
            let expected = json!({"sourceBytes": self.current["sourceBytes"].as_u64().unwrap_or(0) + line.len() as u64,
                "sourceHash": format!("{:x}", next_prefix.clone().finalize()),
                "sequence": body["sequence"], "tailHash": hash, "lastBody": body});
            self.uncertain = json!({"sequence": body["sequence"], "hash": hash,
                "bodyHash": payload_hash(&body)?, "stateHash": payload_hash(state)?});
            let request_result = (|| -> Result<V, String> {
                let mut q = json!({"op": "commit", "bodyJson": body_json});
                for (k, v) in self.guards().as_object().cloned().unwrap_or_default() { q[k] = v; }
                let r = self.request(&q)?;
                let mut keys: Vec<String> = r.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
                keys.sort();
                need(keys.join("|") == "durable|receipt|recordJson"
                    && r["durable"] == V::Bool(true)
                    && r["recordJson"].as_str() == Some(record_json.as_str()), "ACK_RECORD")?;
                self.validate_receipt(&r["receipt"], &expected, Some(body_json.clone()))?;
                Ok(r)
            })();
            match request_result {
                Ok(r) => {
                    self.current = expected;
                    self.prefix = next_prefix;
                    self.uncertain = V::Null;
                    Ok(json!({"hash": hash, "sequence": body["sequence"], "receipt": r["receipt"]}))
                }
                Err(e) => {
                    if self.failed.is_none() && e.starts_with("NATIVE_APPEND_REJECTED_") {
                        self.uncertain = V::Null;
                        return Err(e);
                    }
                    if self.failed.is_none() {
                        self.failed = Some("NATIVE_APPEND_INVALID_OR_UNKNOWN_ACK".into());
                    }
                    Err(self.failed.clone().unwrap_or(e))
                }
            }
        })();
        r
    }
    fn close_inner(&mut self) -> Result<(), String> {
        if self.closed { return Ok(()); }
        self.closed = true;
        self.stdin.take(); // end stdin
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
    /// close()
    pub fn close(&mut self) -> Result<(), String> { self.close_inner() }
}
