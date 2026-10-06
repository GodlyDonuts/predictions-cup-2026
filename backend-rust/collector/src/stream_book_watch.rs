// live-paper/stream-book-watch.mjs — verified stream-book cohort gateway:
// lock + append-only checkpointed journal + byte-boundary source integrity.
// Async sleeps/signals are synchronous here: can_operate is the stop callback.
use crate::paper_persistence::atomic_paper_json;
use crate::stream_book_adapter::{
    advance_stream_book_adapter, create_stream_book_adapter, stream_book_report, stream_digest,
};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const MAX: usize = 8 * 1024 * 1024;
fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.into()); }
    Ok(())
}
fn hash(b: &[u8]) -> String { sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(p).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn truthy(v: &V) -> bool { js_truthy(Some(v)) }
/// validName
fn valid_name(x: &str) -> bool {
    let b = x.as_bytes();
    if x.is_empty() || x.len() > 64 || !b[0].is_ascii_lowercase() {
        return false;
    }
    if !x[1..].bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-') {
        return false;
    }
    !matches!(x, "con" | "prn" | "aux" | "nul" | "com1" | "com2" | "com3" | "com4"
        | "com5" | "com6" | "com7" | "com8" | "com9" | "lpt1" | "lpt2" | "lpt3"
        | "lpt4" | "lpt5" | "lpt6" | "lpt7" | "lpt8" | "lpt9")
}
/// streamBookCodeIdentity() — sha256 of each JS source file in the workspace.
pub fn stream_book_code_identity(workspace_dir: &Path) -> Result<V, String> {
    let files = [
        "live-paper/stream-book-watch.mjs",
        "live-paper/stream-book-adapter.mjs",
        "live-paper/passive-capture.mjs",
        "live-paper/passive-observer.mjs",
        "feed/contract.mjs",
        "paper/fixed.mjs",
        "live-paper/paper-persistence.mjs",
    ];
    let mut m = Map::new();
    for p in files {
        m.insert(p.to_string(), json!(hash(&fs::read(workspace_dir.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}
fn durable(file: &Path, bytes: &[u8], exclusive: bool) -> Result<(), String> {
    let mut o = fs::OpenOptions::new();
    o.write(true);
    if exclusive { o.create_new(true); } else { o.append(true); }
    let mut fd = o.open(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(file, fs::Permissions::from_mode(0o600));
    }
    fd.write_all(bytes).map_err(|_| "STREAM_WRITE_FAILED".to_string())?;
    fd.sync_all().map_err(|e| e.to_string())
}
/// rows(file,{size,partial}) — yields {raw bytes (with \n), parsed row, end offset}.
fn rows(file: &Path, size: Option<u64>, partial: bool) -> Result<Vec<(Vec<u8>, V, u64)>, String> {
    use std::os::unix::fs::FileExt;
    let size = match size {
        Some(s) => s,
        None => fs::metadata(file).map_err(|e| e.to_string())?.len(),
    };
    let mut out = Vec::new();
    let fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut pos = 0u64;
    let mut offset = 0u64;
    let mut tail: Vec<u8> = Vec::new();
    while pos < size {
        let want = std::cmp::min(1024 * 1024, size - pos) as usize;
        let mut b = vec![0u8; want];
        let n = fd.read_at(&mut b, pos).map_err(|e| e.to_string())?;
        need(n > 0, "STREAM_READ_FAILED")?;
        pos += n as u64;
        let mut bytes = tail;
        bytes.extend_from_slice(&b[..n]);
        let mut start = 0usize;
        while let Some(rel) = bytes[start..].iter().position(|&x| x == 10) {
            let end = start + rel;
            let raw = &bytes[start..=end];
            need(raw.len() > 1 && raw.len() <= MAX, "STREAM_RECORD_SIZE")?;
            offset += raw.len() as u64;
            let row: V = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
            out.push((raw.to_vec(), row, offset));
            start = end + 1;
        }
        tail = bytes[start..].to_vec();
        need(tail.len() <= MAX, "STREAM_RECORD_SIZE")?;
    }
    need(partial || tail.is_empty(), "STREAM_JOURNAL_INCOMPLETE")?;
    Ok(out)
}
fn slice_hash(file: &Path, start: u64, n: usize) -> Result<String, String> {
    use std::os::unix::fs::FileExt;
    let fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut b = vec![0u8; n];
    let got = fd.read_at(&mut b, start).map_err(|e| e.to_string())?;
    need(got == n, "STREAM_SOURCE_CHANGED")?;
    Ok(hash(&b))
}
fn birth_ms(meta: &fs::Metadata) -> f64 {
    meta.created().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}
fn meta_ino(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}
/// JS digest(undefined) throws — mirror for absent expected keys.
fn digest_field(body: &Map<String, V>, key: &str) -> Result<String, String> {
    match body.get(key) {
        Some(v) => Ok(stream_digest(v)),
        None => Err("The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received undefined".into()),
    }
}
/// openStreamBookWatch({runtimeDir,cohort,initialize,now,workspaceDir,canOperate})
pub struct StreamBookWatch {
    pub dir: PathBuf,
    pub bank: V,
    state: V,
    offset: u64,
    journal_sequence: i64,
    journal_hash: String,
    journal_bytes: u64,
    source_file: PathBuf,
    source_ino: u64,
    source_birth_ms: f64,
    bank_file: PathBuf,
    journal_file: PathBuf,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    bank_hash: String,
    head_length: usize,
    head_hash: String,
    boundary_length: usize,
    boundary_hash: String,
    code_identity: V,
    lock_file: PathBuf,
    lock: Option<fs::File>,
    closed: bool,
    failed: bool,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl StreamBookWatch {
    /// stopped()
    pub fn stopped(&self) -> bool {
        !(self.can_operate)()
            || self.workspace_dir.join("STOP").exists()
            || self.runtime_dir.join("STOP").exists()
            || self.runtime_dir.join("STREAM_BOOK_STOP").exists()
            || self.dir.join("STOP").exists()
    }
    /// owned()
    pub fn owned(&self) -> bool {
        !self.closed
            && fs::read_to_string(&self.lock_file)
                .map(|s| s == std::process::id().to_string())
                .unwrap_or(false)
    }
    fn live(&self) -> Result<(), String> {
        need(self.owned() && !self.stopped(), "STREAM_STOPPED")
    }
    /// current()
    pub fn current(&self) -> bool {
        match read_json(&self.runtime_dir.join("realtime-session.json")) {
            Ok(s) => {
                s["file"].as_str() == self.bank["sourceFile"].as_str()
                    && stream_digest(&s["scope"]) == stream_digest(&self.bank["scope"])
                    && !truthy(&self.state["capture"]["sourceEnded"])
            }
            Err(_) => false,
        }
    }
    fn append(&mut self, body: V) -> Result<(), String> {
        self.live()?;
        need(
            fs::metadata(&self.journal_file).map_err(|e| e.to_string())?.len()
                == self.journal_bytes,
            "STREAM_JOURNAL_CHANGED",
        )?;
        let mut record = Map::new();
        record.insert("sequence".into(), json!(self.journal_sequence + 1));
        record.insert("previous".into(), json!(self.journal_hash));
        record.insert("bankHash".into(), json!(stream_digest(&self.bank)));
        if let V::Object(m) = body {
            for (k, v) in m { record.insert(k, v); }
        }
        let h = stream_digest(&V::Object(record.clone()));
        record.insert("hash".into(), json!(h));
        let bytes = format!("{}\n", js_stringify(&V::Object(record))).into_bytes();
        durable(&self.journal_file, &bytes, false)?;
        self.journal_sequence += 1;
        self.journal_hash = h;
        self.journal_bytes += bytes.len() as u64;
        Ok(())
    }
    /// consume()
    pub fn consume(&mut self) -> Result<i64, String> {
        need(!self.failed && !self.closed, "STREAM_FAILED_CLOSED")?;
        let result = (|| -> Result<i64, String> {
            self.live()?;
            need(self.current(), "STREAM_SESSION_ENDED")?;
            need(
                hash(&fs::read(&self.bank_file).map_err(|e| e.to_string())?)
                    == self.bank_hash
                    && stream_digest(&self.code_identity)
                        == stream_digest(&stream_book_code_identity(&self.workspace_dir)?),
                "STREAM_CODE_CHANGED",
            )?;
            let stat = fs::metadata(&self.source_file).map_err(|e| e.to_string())?;
            need(
                meta_ino(&stat) == self.source_ino
                    && birth_ms(&stat) == self.source_birth_ms
                    && stat.len() >= self.offset,
                "STREAM_SOURCE_REPLACED",
            )?;
            need(
                slice_hash(&self.source_file, 0, self.head_length)? == self.head_hash
                    && slice_hash(&self.source_file,
                        self.offset - self.boundary_length as u64,
                        self.boundary_length)? == self.boundary_hash,
                "STREAM_SOURCE_CHANGED",
            )?;
            if stat.len() == self.offset {
                return Ok(0);
            }
            use std::os::unix::fs::FileExt;
            let fd = fs::File::open(&self.source_file).map_err(|e| e.to_string())?;
            let want = std::cmp::min(MAX as u64, stat.len() - self.offset) as usize;
            let mut b = vec![0u8; want];
            let n = fd.read_at(&mut b, self.offset).map_err(|e| e.to_string())?;
            drop(fd);
            need(n > 0, "STREAM_READ_FAILED")?;
            let mut start = 0usize;
            let mut count = 0i64;
            let mut offset = self.offset;
            while let Some(rel) = b[start..n].iter().position(|&x| x == 10) {
                let end = start + rel;
                let raw = &b[start..=end];
                let row: V = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
                let available_at = ms_to_iso((self.now)());
                let out = advance_stream_book_adapter(
                    &self.state, &row, &self.bank, &available_at,
                )?;
                self.append(json!({"kind":"capture","sourceCaptureIndex":row["captureIndex"],
                    "sourceLineHash":hash(raw),"availableAt":available_at,
                    "events":out["events"],"stateHash":stream_digest(&out["state"])}))?;
                self.state = out["state"].clone();
                offset += raw.len() as u64;
                start = end + 1;
                count += 1;
            }
            self.offset = offset;
            need(n - start < MAX, "STREAM_RECORD_SIZE")?;
            self.boundary_length = self.offset.min(4096) as usize;
            self.boundary_hash = slice_hash(&self.source_file,
                self.offset - self.boundary_length as u64, self.boundary_length)?;
            Ok(count)
        })();
        match result {
            Ok(n) => Ok(n),
            Err(e) => {
                self.failed = true;
                Err(e)
            }
        }
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let mut r = stream_book_report(&self.state, &self.bank, &ms_to_iso((self.now)()))?
            .as_object().cloned().unwrap_or_default();
        r.insert("cohort".into(), self.bank["cohort"].clone());
        r.insert("sourceFile".into(), self.bank["sourceFile"].clone());
        r.insert("sourceBytes".into(), json!(self.offset));
        r.insert("journalSequence".into(), json!(self.journal_sequence));
        r.insert("journalHash".into(), json!(self.journal_hash));
        r.insert("pid".into(), json!(std::process::id()));
        Ok(V::Object(r))
    }
    /// close()
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        let ours = self.owned();
        self.closed = true;
        self.lock = None;
        if ours {
            let _ = fs::remove_file(&self.lock_file);
        }
    }
}
/// openStreamBookWatch(...)
pub fn open_stream_book_watch(runtime_dir: &Path, cohort: &str, initialize: bool,
    now: Arc<dyn Fn() -> f64 + Send + Sync>, workspace_dir: &Path,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<StreamBookWatch, String> {
    need(runtime_dir.is_absolute() && valid_name(cohort), "INVALID_STREAM_DIRECTORY")?;
    let dir = runtime_dir.join("stream-book-cohorts").join(cohort);
    let lock_file = runtime_dir.join("stream-book-gateway.lock");
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let stopped_now = |closed: &dyn Fn() -> bool| {
        !closed()
            || workspace_dir.join("STOP").exists()
            || runtime_dir.join("STOP").exists()
            || runtime_dir.join("STREAM_BOOK_STOP").exists()
            || dir.join("STOP").exists()
    };
    need(!(stopped_now)(&|| (can_operate)()), "STREAM_STOPPED")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    let mut lock = {
        let mut o = fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        o.open(&lock_file).map_err(|e| e.to_string())?
    };
    let new_claim = lock.metadata().map_err(|e| e.to_string())?;
    let new_claim_ino = meta_ino(&new_claim);
    let new_claim_birth = birth_ms(&new_claim);
    if let Err(e) = (|| -> Result<(), String> {
        lock.write_all(std::process::id().to_string().as_bytes()).map_err(|e| e.to_string())?;
        lock.sync_all().map_err(|e| e.to_string())
    })() {
        drop(lock);
        if let Ok(claim) = fs::metadata(&lock_file) {
            if meta_ino(&claim) == new_claim_ino && birth_ms(&claim) == new_claim_birth {
                let _ = fs::remove_file(&lock_file);
            }
        }
        return Err(e);
    }
    let lock_file_for_close = lock_file.clone();
    let build = (|| -> Result<StreamBookWatch, String> {
        let code_identity = stream_book_code_identity(workspace_dir)?;
        let session = read_json(&runtime_dir.join("realtime-session.json"))?;
        let session_file = session["file"].as_str().unwrap_or("").to_string();
        need(
            session["file"].is_string()
                && session_file.starts_with("realtime-")
                && session_file.ends_with(".jsonl")
                && session_file.len() > "realtime-".len() + ".jsonl".len()
                && session_file.bytes().skip("realtime-".len())
                    .take(session_file.len() - "realtime-".len() - ".jsonl".len())
                    .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-'),
            "STREAM_SOURCE_NAME",
        )?;
        let source_file = runtime_dir.join(&session_file);
        let source_meta = fs::metadata(&source_file).map_err(|e| e.to_string())?;
        let source_ino = meta_ino(&source_meta);
        let source_birth = birth_ms(&source_meta);
        let source_size = source_meta.len();
        let bank: V = if initialize {
            need(!bank_file.exists() && !journal_file.exists(), "STREAM_COHORT_EXISTS")?;
            let mut records = 0i64;
            let mut prefix_bytes = 0u64;
            let mut ph = Sha256::new();
            for (raw, row, off) in rows(&source_file, Some(source_size), true)? {
                need(js_int(&row["captureIndex"]) == Some(records), "STREAM_CAPTURE_INDEX")?;
                records += 1;
                ph.update(&raw);
                prefix_bytes = off;
            }
            need(records > 0, "STREAM_SOURCE_EMPTY")?;
            let source_hash = format!("{:x}", ph.finalize());
            json!({"schemaVersion":1,"mode":"verified_stream_book_input","cohort":cohort,
                "cutoff":ms_to_iso((now)()),"sourcePrefixRecords":records,
                "sourcePrefixBytes":prefix_bytes,"sourceHash":source_hash,
                "sourceFile":session_file,"sessionId":hash(session_file.as_bytes())[..32],
                "scope":session["scope"],"codeIdentity":code_identity})
        } else {
            read_json(&bank_file)?
        };
        need(
            bank["cohort"].as_str() == Some(cohort)
                && bank["sourceFile"].as_str() == session["file"].as_str()
                && stream_digest(&bank["scope"]) == stream_digest(&session["scope"])
                && stream_digest(&bank["codeIdentity"]) == stream_digest(&code_identity),
            "STREAM_FROZEN_IDENTITY",
        )?;
        let mut state = create_stream_book_adapter(&bank)?;
        let mut offset = 0u64;
        let mut journal_sequence = 0i64;
        let mut journal_hash = stream_digest(&bank);
        let mut journal_bytes = 0u64;
        let journal_rows = if initialize {
            Vec::new()
        } else {
            rows(&journal_file, None, false)?
        };
        let mut jpos = 0usize;
        if !initialize {
            need(
                jpos < journal_rows.len()
                    && journal_rows[jpos].1["kind"].as_str() == Some("prefix"),
                "STREAM_PREFIX_CHECKPOINT_MISSING",
            )?;
        }
        let verify_checkpoint = |saved: &(Vec<u8>, V, u64), expected: &V,
                                 js: &mut i64, jh: &mut String, jb: &mut u64|
            -> Result<(), String> {
            let mut body = saved.1.as_object().cloned().unwrap_or_default();
            let actual = body.remove("hash").unwrap_or(V::Null);
            need(
                actual.as_str() == Some(stream_digest(&V::Object(body.clone())).as_str())
                    && js_int(&body["sequence"]) == Some(*js + 1)
                    && body["previous"].as_str() == Some(jh.as_str())
                    && body["bankHash"].as_str() == Some(stream_digest(&bank).as_str()),
                "STREAM_JOURNAL_CHAIN",
            )?;
            for (k, v) in expected.as_object().cloned().unwrap_or_default() {
                need(digest_field(&body, &k)? == stream_digest(&v), "STREAM_REPLAY_MISMATCH")?;
            }
            *js += 1;
            *jh = actual.as_str().unwrap_or("").to_string();
            *jb = saved.2;
            Ok(())
        };
        let mut prefix_complete = false;
        let mut ph = Sha256::new();
        for (raw, row, off) in rows(&source_file, Some(source_size), true)? {
            if js_int(&row["captureIndex"]).unwrap_or(i64::MAX)
                < js_int(&bank["sourcePrefixRecords"]).unwrap_or(i64::MIN)
            {
                ph.update(&raw);
                state = advance_stream_book_adapter(&state, &row, &bank,
                    bank["cutoff"].as_str().unwrap_or(""))?["state"].clone();
                offset = off;
                if js_int(&row["captureIndex"])
                    == Some(js_int(&bank["sourcePrefixRecords"]).unwrap_or(i64::MIN) - 1)
                {
                    need(
                        offset == js_int(&bank["sourcePrefixBytes"]).unwrap_or(0) as u64
                            && format!("{:x}", ph.clone().finalize())
                                == bank["sourceHash"].as_str().unwrap_or(""),
                        "STREAM_PREFIX_CHANGED",
                    )?;
                    prefix_complete = true;
                    if !initialize {
                        verify_checkpoint(&journal_rows[jpos],
                            &json!({"kind":"prefix","stateHash":stream_digest(&state)}),
                            &mut journal_sequence, &mut journal_hash, &mut journal_bytes)?;
                        jpos += 1;
                    }
                }
                continue;
            }
            if initialize || jpos >= journal_rows.len() {
                break;
            }
            // JS evaluates nextSaved.done on a null cursor — TypeError.
            if jpos == 0 {
                return Err("Cannot read properties of null (reading 'done')".into());
            }
            let saved = journal_rows[jpos].1.clone();
            need(
                saved["kind"].as_str() == Some("capture")
                    && saved["sourceLineHash"].as_str() == Some(hash(&raw).as_str()),
                "STREAM_SOURCE_CHANGED",
            )?;
            let out = advance_stream_book_adapter(&state, &row, &bank,
                saved["availableAt"].as_str().unwrap_or(""))?;
            verify_checkpoint(&journal_rows[jpos],
                &json!({"kind":"capture","sourceCaptureIndex":row["captureIndex"],
                    "events":out["events"],"stateHash":stream_digest(&out["state"])}),
                &mut journal_sequence, &mut journal_hash, &mut journal_bytes)?;
            state = out["state"].clone();
            offset = off;
            jpos += 1;
        }
        need(
            prefix_complete && (initialize || jpos >= journal_rows.len()),
            "STREAM_REPLAY_INCOMPLETE",
        )?;
        need(
            stream_digest(&code_identity)
                == stream_digest(&stream_book_code_identity(workspace_dir)?),
            "STREAM_CODE_CHANGED",
        )?;
        // live()
        need(
            fs::read_to_string(&lock_file_for_close)
                .map(|s| s == std::process::id().to_string()).unwrap_or(false)
                && !(stopped_now)(&|| (can_operate)()),
            "STREAM_STOPPED",
        )?;
        let mut w = StreamBookWatch {
            dir: dir.clone(), bank, state, offset, journal_sequence, journal_hash,
            journal_bytes, source_file: source_file.clone(), source_ino,
            source_birth_ms: source_birth, bank_file: bank_file.clone(),
            journal_file: journal_file.clone(), runtime_dir: runtime_dir.to_path_buf(),
            workspace_dir: workspace_dir.to_path_buf(), bank_hash: String::new(),
            head_length: 0, head_hash: String::new(), boundary_length: 0,
            boundary_hash: String::new(), code_identity,
            lock_file: lock_file_for_close.clone(), lock: None,
            closed: false, failed: false, now, can_operate,
        };
        if initialize {
            let body = format!("{}\n", serde_json::to_string_pretty(&w.bank)
                .map_err(|e| e.to_string())?);
            durable(&bank_file, body.as_bytes(), true)?;
            durable(&journal_file, &[], true)?;
            w.append(json!({"kind":"prefix","stateHash":stream_digest(&w.state)}))?;
        }
        w.bank_hash = hash(&fs::read(&bank_file).map_err(|e| e.to_string())?);
        w.head_length = w.offset.min(4096) as usize;
        w.head_hash = slice_hash(&source_file, 0, w.head_length)?;
        w.boundary_length = w.offset.min(4096) as usize;
        w.boundary_hash = slice_hash(&source_file,
            w.offset - w.boundary_length as u64, w.boundary_length)?;
        w.lock = Some(lock);
        Ok(w)
    })();
    match build {
        Ok(w) => Ok(w),
        Err(e) => {
            let ours = fs::read_to_string(&lock_file_for_close)
                .map(|s| s == std::process::id().to_string()).unwrap_or(false);
            if ours {
                let _ = fs::remove_file(&lock_file_for_close);
            }
            Err(e)
        }
    }
}
/// runStreamBookWatch({runtimeDir,cohort,initialize,once,now,workspaceDir})
/// Signals map onto can_operate; sleep is a synchronous 250ms.
pub fn run_stream_book_watch(runtime_dir: &Path, cohort: &str, initialize: bool, once: bool,
    now: Arc<dyn Fn() -> f64 + Send + Sync>, workspace_dir: &Path) -> Result<(), String> {
    let can_operate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| true);
    let mut watch = match open_stream_book_watch(runtime_dir, cohort, initialize,
        now, workspace_dir, can_operate) {
        Ok(w) => w,
        Err(_) => return Err("STREAM_GATEWAY_FAILED_CLOSED".into()),
    };
    let publish = |watch: &StreamBookWatch, status: &str| -> Result<(), String> {
        let mut report = watch.status()?;
        report["status"] = json!(status);
        if status != "running" {
            if let Some(mk) = report["markets"].as_array_mut() {
                for m in mk.iter_mut() {
                    m["ready"] = json!(false);
                }
            }
        }
        atomic_paper_json(&watch.dir.join("stats.json"), &report,
            &|| watch.owned() && (status != "running" || !watch.stopped()))
            .map_err(|e| e.to_string())?;
        Ok(())
    };
    let outcome = (|| -> Result<(), String> {
        loop {
            if watch.stopped() || !watch.current() {
                break;
            }
            watch.consume()?;
            publish(&watch, "running")?;
            if once {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if watch.stopped() || !watch.current() {
            publish(&watch, if watch.stopped() { "stopped" } else { "source_session_ended" })?;
        }
        Ok(())
    })();
    if let Err(_e) = outcome {
        let _ = publish(&watch, if watch.stopped() { "stopped" } else { "failed" });
        watch.close();
        return Err("STREAM_GATEWAY_FAILED_CLOSED".into());
    }
    watch.close();
    Ok(())
}
