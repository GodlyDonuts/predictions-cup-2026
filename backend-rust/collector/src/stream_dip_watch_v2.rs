// live-paper/stream-dip-watch-v2.mjs — stream dip worker: verifies the input
// stream-book journal, replays it into the engine, and appends batch
// checkpoints to its own chained journal.
// Async sleeps/signals are synchronous here: can_operate is the stop callback.
use crate::paper_persistence::atomic_paper_json;
use crate::stream_book_adapter_v2::{advance_stream_book_adapter, stream_digest};
use crate::stream_book_watch_v2::stream_book_code_identity;
use crate::stream_dip::{
    advance_stream_dip, create_stream_dip, stream_dip_hash, stream_dip_policy,
    stream_dip_report, warm_stream_dip,
};
use crate::stream_dip_replay_v2::replay_stream_dip;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
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
fn name(x: &str) -> bool {
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
/// streamDipCodeIdentity()
pub fn stream_dip_code_identity(workspace_dir: &Path) -> Result<V, String> {
    let mut m = stream_book_code_identity(workspace_dir)?
        .as_object().cloned().unwrap_or_default();
    for p in [
        "live-paper/stream-dip.mjs",
        "live-paper/stream-dip-replay-v2.mjs",
        "live-paper/stream-dip-watch-v2.mjs",
    ] {
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
    fd.write_all(bytes).map_err(|_| "STREAM_DIP_WRITE".to_string())?;
    fd.sync_all().map_err(|e| e.to_string())
}
/// rows(file,{start,size,partial})
fn rows(file: &Path, start: u64, size: Option<u64>, partial: bool) -> Result<Vec<(Vec<u8>, V, u64)>, String> {
    use std::os::unix::fs::FileExt;
    let size = match size {
        Some(s) => s,
        None => fs::metadata(file).map_err(|e| e.to_string())?.len(),
    };
    let mut out = Vec::new();
    let fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut pos = start;
    let mut offset = start;
    let mut tail: Vec<u8> = Vec::new();
    while pos < size {
        let want = std::cmp::min(1024 * 1024, size - pos) as usize;
        let mut b = vec![0u8; want];
        let n = fd.read_at(&mut b, pos).map_err(|e| e.to_string())?;
        need(n > 0, "STREAM_DIP_READ")?;
        pos += n as u64;
        let mut data = tail;
        data.extend_from_slice(&b[..n]);
        let mut begin = 0usize;
        while let Some(rel) = data[begin..].iter().position(|&x| x == 10) {
            let end = begin + rel;
            let raw = &data[begin..=end];
            need(raw.len() <= MAX && raw.len() > 1, "STREAM_DIP_RECORD_SIZE")?;
            offset += raw.len() as u64;
            let row: V = serde_json::from_slice(raw).map_err(|e| e.to_string())?;
            out.push((raw.to_vec(), row, offset));
            begin = end + 1;
        }
        tail = data[begin..].to_vec();
        need(tail.len() < MAX, "STREAM_DIP_RECORD_SIZE")?;
    }
    need(partial || tail.is_empty(), "STREAM_DIP_INCOMPLETE_JOURNAL")?;
    Ok(out)
}
/// prefix(file,size) — exactly `size` bytes, looping short reads.
fn prefix(file: &Path, size: u64) -> Result<Vec<u8>, String> {
    use std::os::unix::fs::FileExt;
    let fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut b = vec![0u8; size as usize];
    let mut i = 0u64;
    while i < size {
        let n = fd.read_at(&mut b[i as usize..size as usize], i).map_err(|e| e.to_string())?;
        need(n > 0, "STREAM_DIP_TRUNCATED")?;
        i += n as u64;
    }
    Ok(b)
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
fn same_file(file: &Path, ino: u64, birth: f64, min_size: u64) -> Result<fs::Metadata, String> {
    let s = fs::metadata(file).map_err(|e| e.to_string())?;
    need(
        meta_ino(&s) == ino && birth_ms(&s) == birth && s.len() >= min_size,
        "STREAM_DIP_SOURCE_REPLACED",
    )?;
    Ok(s)
}
fn slice_hash(file: &Path, start: u64, n: usize) -> Result<String, String> {
    use std::os::unix::fs::FileExt;
    let f = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut b = vec![0u8; n];
    let got = f.read_at(&mut b, start).map_err(|e| e.to_string())?;
    need(got == n, "STREAM_DIP_TRUNCATED")?;
    Ok(hash(&b))
}
/// JS streamDipHash(body[k]) on an absent key throws TypeError.
fn dip_digest_field(body: &Map<String, V>, key: &str) -> Result<String, String> {
    match body.get(key) {
        Some(v) => Ok(stream_dip_hash(v)),
        None => Err("The \"data\" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received undefined".into()),
    }
}
/// new Date(Number(BigInt(adapter.capture.lastObservedNs)/1000000n)).toISOString()
fn last_observed_iso(adapter: &V) -> Result<String, String> {
    match &adapter["capture"]["lastObservedNs"] {
        V::Null => Err("Cannot convert null to a BigInt".into()),
        V::String(s) => {
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err(format!("Cannot convert {} to a BigInt", s));
            }
            let n: i128 = s.parse().map_err(|_| "Cannot convert to BigInt".to_string())?;
            Ok(ms_to_iso((n / 1_000_000) as f64))
        }
        _ => Err("Cannot convert to a BigInt".into()),
    }
}
fn kill0(pid: i64) -> bool {
    if pid < 0 {
        return false;
    }
    std::process::Command::new("/bin/kill").arg("-0").arg(pid.to_string()).output()
        .map(|o| o.status.success())
        .unwrap_or_else(|_| Path::new(&format!("/proc/{pid}")).exists())
}
/// openStreamDipWatch({runtimeDir,cohort,inputCohort,initialize,readOnly,markets,now,workspaceDir,canOperate})
pub struct StreamDipWatch {
    pub dir: PathBuf,
    pub bank: V,
    adapter: V,
    state: V,
    source_offset: u64,
    input_bytes: u64,
    input_sequence: i64,
    input_hash: String,
    evidence_through: String,
    journal_sequence: i64,
    journal_hash: String,
    journal_bytes: u64,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    input_dir: PathBuf,
    input_bank: V,
    input_bank_file: PathBuf,
    input_journal: PathBuf,
    input_cohort: String,
    cohort: String,
    source_file: PathBuf,
    source_ino: u64,
    source_birth_ms: f64,
    input_ino: u64,
    input_birth_ms: f64,
    bank_file: PathBuf,
    journal_file: PathBuf,
    lock_file: PathBuf,
    gateway_lock: PathBuf,
    bank_file_hash: String,
    input_bank_file_hash: String,
    code_identity: V,
    source_boundary_length: usize,
    source_boundary_hash: String,
    input_boundary_length: usize,
    input_boundary_hash: String,
    source_rows: Option<(Vec<(Vec<u8>, V, u64)>, usize)>,
    lock: Option<fs::File>,
    closed: bool,
    failed: bool,
    read_only: bool,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl StreamDipWatch {
    /// stopped()
    pub fn stopped(&self) -> bool {
        !(self.can_operate)()
            || self.runtime_dir.join("STOP").exists()
            || self.runtime_dir.join("STREAM_DIP_V2_STOP").exists()
            || self.workspace_dir.join("STOP").exists()
            || self.dir.join("STOP").exists()
    }
    /// owned()
    pub fn owned(&self) -> bool {
        if self.closed {
            return false;
        }
        if self.read_only {
            return true;
        }
        fs::read_to_string(&self.lock_file)
            .map(|s| s == std::process::id().to_string())
            .unwrap_or(false)
    }
    fn live(&self) -> Result<(), String> {
        need(!self.read_only, "STREAM_DIP_READ_ONLY")?;
        need(self.owned() && !self.stopped(), "STREAM_DIP_V2_STOPPED")
    }
    /// current()
    pub fn current(&self) -> bool {
        match read_json(&self.runtime_dir.join("realtime-session.json")) {
            Ok(s) => {
                s["file"].as_str() == self.input_bank["sourceFile"].as_str()
                    && stream_digest(&s["scope"]) == stream_digest(&self.input_bank["scope"])
                    && !truthy(&self.adapter["capture"]["sourceEnded"])
            }
            Err(_) => false,
        }
    }
    /// healthy() — the upstream stream-book gateway is live and publishing.
    pub fn healthy(&self) -> bool {
        (|| -> Option<bool> {
            let s = read_json(&self.input_dir.join("stats.json")).ok()?;
            let age = (self.now)() - baal_service_runtime::contract::parse_iso_ms(
                s["updatedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
            let pid = js_int(&s["pid"])?;
            if !kill0(pid) {
                return Some(false);
            }
            Some(
                s["status"].as_str() == Some("running")
                    && s["bankHash"].as_str() == Some(stream_digest(&self.input_bank).as_str())
                    && s["cohort"].as_str() == Some(self.input_cohort.as_str())
                    && age >= 0.0 && age < 5000.0
                    && fs::read_to_string(&self.gateway_lock)
                        .map(|x| x == pid.to_string()).unwrap_or(false),
            )
        })().unwrap_or(false)
    }
    /// next source row from the lazily-created cursor; resets once exhausted
    /// (JS: sourceRows.return() then a fresh generator from current offset).
    fn next_source_row(&mut self) -> Result<Option<(Vec<u8>, V, u64)>, String> {
        if self.source_rows.is_none() {
            let v = rows(&self.source_file, self.source_offset, None, true)?;
            self.source_rows = Some((v, 0));
        }
        {
            let (rows_v, pos) = self.source_rows.as_mut().unwrap();
            if *pos < rows_v.len() {
                let r = rows_v[*pos].clone();
                *pos += 1;
                return Ok(Some(r));
            }
        }
        let v = rows(&self.source_file, self.source_offset, None, true)?;
        self.source_rows = Some((v, 0));
        let (rows_v, pos) = self.source_rows.as_mut().unwrap();
        if *pos < rows_v.len() {
            let r = rows_v[*pos].clone();
            *pos += 1;
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }
    /// advanceInput(saved) — verify and apply one input journal row.
    fn advance_input(&mut self, saved_row: &V, saved_offset: u64) -> Result<Vec<V>, String> {
        let mut body = saved_row.as_object().cloned().unwrap_or_default();
        let h = body.remove("hash").unwrap_or(V::Null);
        need(
            body["kind"].as_str() == Some("capture")
                && h.as_str() == Some(stream_digest(&V::Object(body.clone())).as_str())
                && body["bankHash"].as_str() == Some(stream_digest(&self.input_bank).as_str())
                && js_int(&body["sequence"]) == Some(self.input_sequence + 1)
                && body["previous"].as_str() == Some(self.input_hash.as_str()),
            "STREAM_DIP_INPUT_CHAIN",
        )?;
        let x = self.next_source_row()?;
        need(
            x.as_ref().is_some_and(|x| {
                hash(&x.0) == body["sourceLineHash"].as_str().unwrap_or("")
                    && js_int(&x.1["captureIndex"]) == js_int(&body["sourceCaptureIndex"])
            }),
            "STREAM_DIP_CAPTURE_CHANGED",
        )?;
        let x = x.unwrap();
        let out = advance_stream_book_adapter(&self.adapter, &x.1, &self.input_bank,
            body.get("availableAt").and_then(|v| v.as_str()).unwrap_or(""))?;
        need(
            stream_digest(&out["events"]) == stream_digest(&body["events"])
                && stream_digest(&out["state"]) == body["stateHash"].as_str().unwrap_or(""),
            "STREAM_DIP_INPUT_REPLAY",
        )?;
        self.adapter = out["state"].clone();
        self.source_offset = x.2;
        self.input_bytes = saved_offset;
        self.input_sequence = js_int(&body["sequence"]).unwrap_or(self.input_sequence);
        self.input_hash = h.as_str().unwrap_or("").to_string();
        self.evidence_through = last_observed_iso(&self.adapter)?;
        Ok(out["events"].as_array().cloned().unwrap_or_default())
    }
    fn append(&mut self, body: V) -> Result<(), String> {
        self.live()?;
        need(
            fs::metadata(&self.journal_file).map_err(|e| e.to_string())?.len()
                == self.journal_bytes,
            "STREAM_DIP_JOURNAL_CHANGED",
        )?;
        let mut record = Map::new();
        record.insert("sequence".into(), json!(self.journal_sequence + 1));
        record.insert("previous".into(), json!(self.journal_hash));
        record.insert("bankHash".into(), json!(stream_dip_hash(&self.bank)));
        if let V::Object(m) = body {
            for (k, v) in m { record.insert(k, v); }
        }
        let h = stream_dip_hash(&V::Object(record.clone()));
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
        need(!self.closed && !self.failed, "STREAM_DIP_FAILED_CLOSED")?;
        let before = (
            self.adapter.clone(), self.source_offset, self.input_bytes,
            self.input_sequence, self.input_hash.clone(), self.evidence_through.clone(),
        );
        let mut committed = false;
        let result = (|| -> Result<i64, String> {
            self.live()?;
            need(self.current(), "STREAM_DIP_SESSION_ENDED")?;
            need(
                hash(&fs::read(&self.bank_file).map_err(|e| e.to_string())?)
                    == self.bank_file_hash
                    && hash(&fs::read(&self.input_bank_file).map_err(|e| e.to_string())?)
                        == self.input_bank_file_hash
                    && stream_dip_hash(&stream_dip_code_identity(&self.workspace_dir)?)
                        == stream_dip_hash(&self.code_identity),
                "STREAM_DIP_CODE_CHANGED",
            )?;
            let input_stat = same_file(&self.input_journal, self.input_ino,
                self.input_birth_ms, self.input_bytes)?;
            same_file(&self.source_file, self.source_ino, self.source_birth_ms,
                self.source_offset)?;
            need(
                slice_hash(&self.source_file,
                    self.source_offset - self.source_boundary_length as u64,
                    self.source_boundary_length)? == self.source_boundary_hash
                    && slice_hash(&self.input_journal,
                        self.input_bytes - self.input_boundary_length as u64,
                        self.input_boundary_length)? == self.input_boundary_hash,
                "STREAM_DIP_SOURCE_CHANGED",
            )?;
            let mut events: Vec<V> = Vec::new();
            let mut count = 0i64;
            for x in rows(&self.input_journal, self.input_bytes, Some(input_stat.len()), true)? {
                events.extend(self.advance_input(&x.1, x.2)?);
                count += 1;
            }
            if count == 0 {
                return Ok(0);
            }
            let available_at = ms_to_iso((self.now)());
            let allow_decisions = self.healthy();
            let next = advance_stream_dip(&self.state, &json!(events), &self.bank,
                &json!({"availableAt":available_at,"evidenceThrough":self.evidence_through,
                    "allowDecisions":allow_decisions}))?;
            self.append(json!({"kind":"batch","inputSequence":self.input_sequence,
                "inputBytes":self.input_bytes,"inputHash":self.input_hash,
                "evidenceThrough":self.evidence_through,"availableAt":available_at,
                "allowDecisions":allow_decisions,
                "stateHash":stream_dip_hash(&next)}))?;
            self.state = next;
            committed = true;
            self.source_boundary_length = self.source_offset.min(4096) as usize;
            self.input_boundary_length = self.input_bytes.min(4096) as usize;
            self.source_boundary_hash = slice_hash(&self.source_file,
                self.source_offset - self.source_boundary_length as u64,
                self.source_boundary_length)?;
            self.input_boundary_hash = slice_hash(&self.input_journal,
                self.input_bytes - self.input_boundary_length as u64,
                self.input_boundary_length)?;
            Ok(count)
        })();
        match result {
            Ok(n) => Ok(n),
            Err(e) => {
                if !committed {
                    self.adapter = before.0;
                    self.source_offset = before.1;
                    self.input_bytes = before.2;
                    self.input_sequence = before.3;
                    self.input_hash = before.4;
                    self.evidence_through = before.5;
                    self.source_rows = None;
                }
                self.failed = true;
                Err(e)
            }
        }
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let mut r = stream_dip_report(&self.state, &self.bank, &ms_to_iso((self.now)()))?
            .as_object().cloned().unwrap_or_default();
        r.insert("cohort".into(), json!(self.cohort));
        r.insert("inputCohort".into(), json!(self.input_cohort));
        r.insert("inputSequence".into(), json!(self.input_sequence));
        r.insert("inputBytes".into(), json!(self.input_bytes));
        r.insert("inputHash".into(), json!(self.input_hash));
        r.insert("sourceBytes".into(), json!(self.source_offset));
        r.insert("evidenceThrough".into(), json!(self.evidence_through));
        r.insert("journalSequence".into(), json!(self.journal_sequence));
        r.insert("journalHash".into(), json!(self.journal_hash));
        r.insert("stateHash".into(), json!(stream_dip_hash(&self.state)));
        r.insert("inputHealthy".into(), json!(self.healthy()));
        r.insert("pid".into(), if self.read_only { V::Null } else { json!(std::process::id()) });
        r.insert("readOnly".into(), json!(self.read_only));
        r.insert("networkRequests".into(), json!(0));
        if self.read_only {
            // JS: runtimeWrites: undefined is present but JSON.stringify drops it;
            // readOnly emits the real 0.
            r.insert("runtimeWrites".into(), json!(0));
        }
        Ok(V::Object(r))
    }
    /// close()
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        let ours = self.owned();
        self.closed = true;
        self.source_rows = None;
        if !self.read_only {
            self.lock = None;
            if ours {
                let _ = fs::remove_file(&self.lock_file);
            }
        }
    }
}
/// openStreamDipWatch(...)
pub fn open_stream_dip_watch(runtime_dir: &Path, cohort: &str, input_cohort: Option<&str>,
    initialize: bool, read_only: bool, markets: Option<&V>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>, workspace_dir: &Path,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<StreamDipWatch, String> {
    need(runtime_dir.is_absolute() && name(cohort), "STREAM_DIP_DIRECTORY")?;
    let dir = runtime_dir.join("stream-dip-v2-cohorts").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let lock_file = runtime_dir.join("stream-dip.lock");
    let stopped_now = |can_op: &dyn Fn() -> bool, d: &Path| {
        !can_op()
            || runtime_dir.join("STOP").exists()
            || runtime_dir.join("STREAM_DIP_V2_STOP").exists()
            || workspace_dir.join("STOP").exists()
            || d.join("STOP").exists()
    };
    need(!read_only || !initialize, "STREAM_DIP_READ_ONLY_INIT")?;
    let mut lock: Option<fs::File> = None;
    if !read_only {
        need(!(stopped_now)(&|| (can_operate)(), &dir), "STREAM_DIP_V2_STOPPED")?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        }
        let mut o = fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let fd = o.open(&lock_file).map_err(|e| e.to_string())?;
        let claim = fd.metadata().map_err(|e| e.to_string())?;
        let claim_ino = meta_ino(&claim);
        let claim_birth = birth_ms(&claim);
        let mut fd_mut = fd;
        if let Err(e) = (|| -> Result<(), String> {
            fd_mut.write_all(std::process::id().to_string().as_bytes()).map_err(|e| e.to_string())?;
            fd_mut.sync_all().map_err(|e| e.to_string())
        })() {
            drop(fd_mut);
            if let Ok(claim) = fs::metadata(&lock_file) {
                if meta_ino(&claim) == claim_ino && birth_ms(&claim) == claim_birth {
                    let _ = fs::remove_file(&lock_file);
                }
            }
            return Err(e);
        }
        lock = Some(fd_mut);
    }
    let lock_file_for_close = lock_file.clone();
    let build = (|| -> Result<StreamDipWatch, String> {
        let bank_opt: Option<V> = if initialize { None } else { Some(read_json(&bank_file)?) };
        // JS: inputCohort = bank?.inputCohort || inputCohort — falsy falls back.
        let input_cohort_v: V = bank_opt.as_ref()
            .filter(|b| truthy(&b["inputCohort"]))
            .map(|b| b["inputCohort"].clone())
            .or_else(|| input_cohort.map(|s| json!(s)))
            .unwrap_or(V::Null);
        need(input_cohort_v.is_string() && name(input_cohort_v.as_str().unwrap_or("")),
            "STREAM_DIP_INPUT_NAME")?;
        let input_cohort = input_cohort_v.as_str().unwrap_or("").to_string();
        let input_dir = runtime_dir.join("stream-book-v2-cohorts").join(&input_cohort);
        let input_bank_file = input_dir.join("bank.json");
        let input_bank = read_json(&input_bank_file)?;
        let input_journal = input_dir.join("journal.jsonl");
        need(
            input_bank["cohort"].as_str() == Some(input_cohort.as_str())
                && stream_digest(&input_bank["codeIdentity"])
                    == stream_digest(&stream_book_code_identity(workspace_dir)?),
            "STREAM_DIP_INPUT_IDENTITY",
        )?;
        {
            let f = input_bank["sourceFile"].as_str().unwrap_or("");
            need(
                f.starts_with("realtime-") && f.ends_with(".jsonl")
                    && f.len() > "realtime-".len() + ".jsonl".len()
                    && f.bytes().skip(9).take(f.len() - 15)
                        .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-'),
                "STREAM_DIP_CAPTURE_NAME",
            )?;
        }
        let source_file = runtime_dir.join(input_bank["sourceFile"].as_str().unwrap_or(""));
        let source_meta = fs::metadata(&source_file).map_err(|e| e.to_string())?;
        let input_meta = fs::metadata(&input_journal).map_err(|e| e.to_string())?;
        let source_ino = meta_ino(&source_meta);
        let source_birth = birth_ms(&source_meta);
        let input_ino = meta_ino(&input_meta);
        let input_birth = birth_ms(&input_meta);
        let code_identity = stream_dip_code_identity(workspace_dir)?;
        let mut records: Vec<V> = Vec::new();
        let mut input_bytes = 0u64;
        // JS: bank?.inputPrefixBytes ?? inputIdentity.size
        let prefix_size: u64 = bank_opt.as_ref()
            .and_then(|b| b.as_object().and_then(|m| m.get("inputPrefixBytes")))
            .filter(|v| !v.is_null())
            .map(|v| v.as_f64().unwrap_or(0.0) as u64)
            .unwrap_or(input_meta.len());
        need(prefix_size > 0 && prefix_size <= 512 * 1024 * 1024, "STREAM_DIP_PREFIX_SIZE")?;
        for (_raw, row, off) in rows(&input_journal, 0, Some(prefix_size), initialize)? {
            records.push(row);
            input_bytes = off;
        }
        need(!records.is_empty(), "STREAM_DIP_INPUT_EMPTY")?;
        let input_prefix_hash = hash(&prefix(&input_journal, input_bytes)?);
        if let Some(bank) = &bank_opt {
            need(
                input_bytes == js_int(&bank["inputPrefixBytes"]).unwrap_or(-1) as u64
                    && input_prefix_hash == bank["inputPrefixHash"].as_str().unwrap_or("")
                    && stream_dip_hash(&code_identity)
                        == stream_dip_hash(&bank["codeIdentity"]),
                "STREAM_DIP_FROZEN_IDENTITY",
            )?;
        }
        // Verify actual raw capture, input checkpoints, every derived event and
        // state.
        let markets_arg: Option<&V> = bank_opt
            .as_ref()
            .filter(|b| truthy(&b["markets"]))
            .map(|b| &b["markets"])
            .or_else(|| markets.filter(|m| truthy(*m)))
            .or_else(|| Some(&input_bank["scope"]["markets"]));
        let verified = replay_stream_dip(&input_bank,
            &prefix(&source_file, source_meta.len())?, &records, markets_arg)?;
        let adapter = verified["verifiedInputState"].clone();
        let source_offset = js_int(&verified["report"]["sourceBytes"]).unwrap_or(0) as u64;
        let input_sequence = js_int(&records[records.len() - 1]["sequence"]).unwrap_or(0);
        let input_hash = records[records.len() - 1]["hash"].as_str().unwrap_or("").to_string();
        let evidence_through = last_observed_iso(&adapter)?;
        let bank: V = if initialize {
            let b = json!({"schemaVersion":1,"mode":"stream_native_dip_paper","inputVersion":2,
                "cohort":cohort,"inputCohort":input_cohort,
                "tournamentId":input_bank["scope"]["tournamentId"],
                "inputBankHash":stream_digest(&input_bank),
                "cutoff":ms_to_iso((now)()),
                "startEventId":adapter["lastEventId"],
                "markets":markets_arg.cloned(),
                "policy":stream_dip_policy(),
                "inputPrefixBytes":input_bytes,"inputPrefixHash":input_prefix_hash,
                "inputPrefixSequence":input_sequence,
                "inputPrefixJournalHash":input_hash,
                "sourceFile":input_bank["sourceFile"],
                "scope":input_bank["scope"],
                "sourcePrefixBytes":source_offset,
                "sourcePrefixHash":hash(&prefix(&source_file, source_offset)?),
                "sourceHash":hash(&prefix(&source_file, source_offset)?),
                "codeIdentity":code_identity});
            need(!bank_file.exists() && !journal_file.exists(), "STREAM_DIP_COHORT_EXISTS")?;
            b
        } else {
            let b = bank_opt.clone().unwrap();
            need(
                b["inputBankHash"].as_str() == Some(stream_digest(&input_bank).as_str())
                    && js_int(&b["sourcePrefixBytes"]).unwrap_or(-1) as u64 == source_offset
                    && b["sourcePrefixHash"].as_str()
                        == Some(hash(&prefix(&source_file, source_offset)?).as_str()),
                "STREAM_DIP_SOURCE_PREFIX",
            )?;
            b
        };
        // warm = verified.warm + all book events from input checkpoint rows.
        let mut warm: Vec<V> = verified["warm"].as_array().cloned().unwrap_or_default();
        for x in &records {
            for e in x["events"].as_array().cloned().unwrap_or_default() {
                if e["kind"].as_str() == Some("book") {
                    warm.push(e);
                }
            }
        }
        let state = warm_stream_dip(&create_stream_dip(&bank)?, &json!(warm), &bank)?;
        let journal_sequence = 0i64;
        let journal_hash = stream_dip_hash(&bank);
        let journal_bytes = 0u64;
        let verify_checkpoint = |saved: &(Vec<u8>, V, u64), expected: &V,
                                 js: &mut i64, jh: &mut String, jb: &mut u64|
            -> Result<(), String> {
            let mut body = saved.1.as_object().cloned().unwrap_or_default();
            let h = body.remove("hash").unwrap_or(V::Null);
            need(
                h.as_str() == Some(stream_dip_hash(&V::Object(body.clone())).as_str())
                    && js_int(&body["sequence"]) == Some(*js + 1)
                    && body["previous"].as_str() == Some(jh.as_str())
                    && body["bankHash"].as_str() == Some(stream_dip_hash(&bank).as_str()),
                "STREAM_DIP_JOURNAL_CHAIN",
            )?;
            for (k, v) in expected.as_object().cloned().unwrap_or_default() {
                need(dip_digest_field(&body, &k)? == stream_dip_hash(&v),
                    "STREAM_DIP_REPLAY_MISMATCH")?;
            }
            *js += 1;
            *jh = h.as_str().unwrap_or("").to_string();
            *jb = saved.2;
            Ok(())
        };
        let mut w = StreamDipWatch {
            dir: dir.clone(), bank: bank.clone(), adapter, state: state.clone(),
            source_offset, input_bytes, input_sequence, input_hash,
            evidence_through: evidence_through.clone(),
            journal_sequence, journal_hash, journal_bytes,
            runtime_dir: runtime_dir.to_path_buf(), workspace_dir: workspace_dir.to_path_buf(),
            input_dir: input_dir.clone(), input_bank: input_bank.clone(),
            input_bank_file: input_bank_file.clone(), input_journal: input_journal.clone(),
            input_cohort: input_cohort.clone(), cohort: cohort.to_string(),
            source_file: source_file.clone(), source_ino, source_birth_ms: source_birth,
            input_ino, input_birth_ms: input_birth,
            bank_file: bank_file.clone(), journal_file: journal_file.clone(),
            lock_file: lock_file_for_close.clone(),
            gateway_lock: runtime_dir.join("stream-book-gateway.lock"),
            bank_file_hash: String::new(), input_bank_file_hash: String::new(),
            code_identity, source_boundary_length: 0, source_boundary_hash: String::new(),
            input_boundary_length: 0, input_boundary_hash: String::new(),
            source_rows: None, lock: None, closed: false, failed: false,
            read_only, now: now.clone(), can_operate: can_operate.clone(),
        };
        if !initialize {
            let input_rows = rows(&input_journal, w.input_bytes, None, true)?;
            let mut ipos = 0usize;
            let journal_rows = rows(&journal_file, 0, None, false)?;
            for saved in &journal_rows {
                if saved.1["kind"].as_str() == Some("genesis") {
                    need(w.journal_sequence == 0, "STREAM_DIP_GENESIS")?;
                    verify_checkpoint(saved,
                        &json!({"kind":"genesis","stateHash":stream_dip_hash(&w.state)}),
                        &mut w.journal_sequence, &mut w.journal_hash, &mut w.journal_bytes)?;
                    continue;
                }
                need(
                    saved.1["kind"].as_str() == Some("batch")
                        && js_int(&saved.1["inputSequence"]).is_some()
                        && js_int(&saved.1["inputSequence"]).unwrap_or(0) > w.input_sequence,
                    "STREAM_DIP_CHECKPOINT_KIND",
                )?;
                let mut events: Vec<V> = Vec::new();
                while w.input_sequence < js_int(&saved.1["inputSequence"]).unwrap_or(0) {
                    need(ipos < input_rows.len(), "STREAM_DIP_INPUT_TRUNCATED")?;
                    events.extend(w.advance_input(&input_rows[ipos].1, input_rows[ipos].2)?);
                    ipos += 1;
                }
                w.state = advance_stream_dip(&w.state, &json!(events), &w.bank,
                    &json!({"availableAt":saved.1["availableAt"],
                        "evidenceThrough":w.evidence_through,
                        "allowDecisions":saved.1["allowDecisions"]}))?;
                verify_checkpoint(saved,
                    &json!({"kind":"batch","inputSequence":w.input_sequence,
                        "inputBytes":w.input_bytes,"inputHash":w.input_hash,
                        "evidenceThrough":w.evidence_through,
                        "stateHash":stream_dip_hash(&w.state)}),
                    &mut w.journal_sequence, &mut w.journal_hash, &mut w.journal_bytes)?;
            }
            need(w.journal_sequence > 0, "STREAM_DIP_GENESIS_MISSING")?;
        }
        if !read_only {
            w.live()?;
        }
        if initialize {
            let body = format!("{}\n", serde_json::to_string_pretty(&w.bank)
                .map_err(|e| e.to_string())?);
            durable(&bank_file, body.as_bytes(), true)?;
            durable(&journal_file, &[], true)?;
            w.append(json!({"kind":"genesis","stateHash":stream_dip_hash(&w.state)}))?;
        }
        w.bank_file_hash = hash(&fs::read(&bank_file).map_err(|e| e.to_string())?);
        w.input_bank_file_hash = hash(&fs::read(&input_bank_file).map_err(|e| e.to_string())?);
        w.source_boundary_length = w.source_offset.min(4096) as usize;
        w.input_boundary_length = w.input_bytes.min(4096) as usize;
        w.source_boundary_hash = slice_hash(&w.source_file,
            w.source_offset - w.source_boundary_length as u64, w.source_boundary_length)?;
        w.input_boundary_hash = slice_hash(&w.input_journal,
            w.input_bytes - w.input_boundary_length as u64, w.input_boundary_length)?;
        Ok(w)
    })();
    match build {
        Ok(mut w) => {
            w.lock = lock;
            Ok(w)
        }
        Err(e) => {
            drop(lock);
            if !read_only {
                let ours = fs::read_to_string(&lock_file_for_close)
                    .map(|s| s == std::process::id().to_string()).unwrap_or(false);
                if ours {
                    let _ = fs::remove_file(&lock_file_for_close);
                }
            }
            Err(e)
        }
    }
}
/// runStreamDipWatch({runtimeDir,cohort,inputCohort,initialize,markets,once,now,workspaceDir})
pub fn run_stream_dip_watch(runtime_dir: &Path, cohort: &str, input_cohort: Option<&str>,
    initialize: bool, markets: Option<&V>, once: bool,
    now: Arc<dyn Fn() -> f64 + Send + Sync>, workspace_dir: &Path) -> Result<(), String> {
    let can_operate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| true);
    let mut watch = match open_stream_dip_watch(runtime_dir, cohort, input_cohort,
        initialize, false, markets, now, workspace_dir, can_operate) {
        Ok(w) => w,
        Err(_) => return Err("STREAM_DIP_WORKER_FAILED_CLOSED".into()),
    };
    let publish = |watch: &StreamDipWatch, status: &str| -> Result<(), String> {
        let mut report = watch.status()?;
        report["status"] = json!(status);
        if status != "running" {
            if let Some(w) = report["wallets"].as_object_mut() {
                for (_k, wallet) in w.iter_mut() {
                    let has_pos = wallet["positions"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
                    if has_pos {
                        wallet["totalPnl"] = V::Null;
                    }
                    wallet["marksComplete"] = json!(wallet["positions"].as_array()
                        .map(|a| a.is_empty()).unwrap_or(true));
                    if let Some(pos) = wallet["positions"].as_array_mut() {
                        for p in pos.iter_mut() {
                            p["markReady"] = json!(false);
                            p["liquidationValue"] = V::Null;
                        }
                    }
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
            publish(&watch, if watch.healthy() { "running" } else { "awaiting_verified_input" })?;
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
        return Err("STREAM_DIP_WORKER_FAILED_CLOSED".into());
    }
    watch.close();
    Ok(())
}
