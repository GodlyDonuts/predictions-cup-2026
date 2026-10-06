// live-paper/public-recovery-watch.mjs — offline reader of the single
// collector's normalized publication. No network client, credential reader
// or account-order method exists here.
use serde_json::{json, Value as V};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const MAX: usize = 8 * 1024 * 1024;
const RESERVED: [&str; 22] = ["con", "prn", "aux", "nul",
    "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9"];
fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok { return Err(c.to_string()); }
    Ok(())
}
fn hash(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn name_ok(x: &str) -> bool {
    // /^[a-z][a-z0-9-]{0,63}$/ && !/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/
    !x.is_empty() && x.len() <= 64 && x.as_bytes()[0].is_ascii_lowercase()
        && x.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !RESERVED.contains(&x)
}
/// publicRecoveryCodeIdentity() — sha256 of the watch code file set.
pub fn public_recovery_code_identity(workspace_dir: &Path) -> Result<V, String> {
    let files = ["live-paper/public-recovery-watch.mjs", "live-paper/public-recovery.mjs",
        "live-paper/public-recovery-confirmation.mjs", "live-paper/book-publication.mjs",
        "live-paper/paper-persistence.mjs", "live-paper/model.mjs",
        "paper/fixed.mjs", "feed/contract.mjs"];
    let mut m = serde_json::Map::new();
    for p in files {
        m.insert(p.to_string(),
            json!(hash(&std::fs::read(workspace_dir.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}
fn durable(file: &Path, bytes: &[u8], append: bool, create_new: bool) -> Result<(), String> {
    let mut o = OpenOptions::new();
    if create_new { o.write(true).create_new(true); } else if append { o.append(true); } else { o.write(true).truncate(true); }
    #[cfg(unix)]
    o.mode(0o600);
    let mut fd = o.open(file).map_err(|e| e.to_string())?;
    fd.write_all(bytes).map_err(|_| "PUBLIC_RECOVERY_WRITE".to_string())?;
    fd.sync_all().map_err(|e| e.to_string())
}
/// rows(file, {start, size, partial}) — lazy line reader; offset is the byte
/// position just past each yielded record.
pub struct PublicRecoveryRows {
    file: File,
    pos: u64,
    end: u64,
    offset: u64,
    buf: Vec<u8>,
    partial: bool,
    done: bool,
}
pub struct PublicRecoveryRow {
    pub row: V,
    pub offset: u64,
}
impl PublicRecoveryRows {
    pub fn open(file: &Path, start: u64, size: Option<u64>, partial: bool) -> Result<Self, String> {
        let f = File::open(file).map_err(|e| e.to_string())?;
        let end = size.unwrap_or_else(|| f.metadata().map(|m| m.len()).unwrap_or(0));
        Ok(Self { file: f, pos: start, end, offset: start, buf: Vec::new(), partial, done: false })
    }
    pub fn next_item(&mut self) -> Result<Option<PublicRecoveryRow>, String> {
        if self.done { return Ok(None); }
        loop {
            if let Some(rel) = self.buf.iter().position(|x| *x == b'\n') {
                let raw: Vec<u8> = self.buf.drain(..rel + 1).collect();
                need(raw.len() > 1 && raw.len() <= MAX, "PUBLIC_RECOVERY_RECORD")?;
                self.offset += raw.len() as u64;
                let row: V = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
                return Ok(Some(PublicRecoveryRow { row, offset: self.offset }));
            }
            need(self.buf.len() < MAX, "PUBLIC_RECOVERY_RECORD")?;
            if self.pos >= self.end {
                need(self.partial || self.buf.is_empty(), "PUBLIC_RECOVERY_INCOMPLETE_JOURNAL")?;
                self.done = true;
                return Ok(None);
            }
            let want = ((self.end - self.pos).min(1024 * 1024)) as usize;
            let mut b = vec![0u8; want];
            self.file.seek(SeekFrom::Start(self.pos)).map_err(|e| e.to_string())?;
            let n = self.file.read(&mut b).map_err(|e| e.to_string())?;
            need(n > 0, "PUBLIC_RECOVERY_READ")?;
            self.pos += n as u64;
            self.buf.extend_from_slice(&b[..n]);
        }
    }
}
fn prefix_hash(file: &Path, n: u64) -> Result<String, String> {
    let mut f = File::open(file).map_err(|e| e.to_string())?;
    let mut offset = 0u64;
    let mut out: Vec<u8> = Vec::new();
    while offset < n {
        let want = (n - offset).min(1024 * 1024) as usize;
        let mut b = vec![0u8; want];
        f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let count = f.read(&mut b).map_err(|e| e.to_string())?;
        need(count > 0, "PUBLIC_RECOVERY_TRUNCATED")?;
        out.extend_from_slice(&b[..count]);
        offset += count as u64;
    }
    Ok(hash(&out))
}
fn boundary(file: &Path, offset: u64) -> Result<String, String> {
    let n = offset.min(4096) as usize;
    let mut b = vec![0u8; n];
    let mut f = File::open(file).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(offset - n as u64)).map_err(|e| e.to_string())?;
    let got = f.read(&mut b).map_err(|e| e.to_string())?;
    need(got == n, "PUBLIC_RECOVERY_TRUNCATED")?;
    Ok(hash(&b))
}
fn stat_of(file: &Path) -> Result<(u64, f64, u64), String> {
    let m = std::fs::metadata(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    return Ok((m.ino(), m.created().map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs_f64() * 1000.0, m.len()));
    #[cfg(not(unix))]
    return Ok((0, 0.0, m.len()));
}

/// publicRecoveryEvent(record, eventId, availableAt)
pub fn public_recovery_event(record: &V, event_id: i64, available_at: &str) -> V {
    let b = &record["payload"]["book"];
    json!({"kind":"book","eventId":event_id,"tournamentId":record["tournamentId"],
        "marketId":b["marketId"],"exchangeId":b["exchangeId"],
        "origin":"rest_observation","sourceKind":"verified_public_rest_book_publication",
        "publicationSequence":record["sequence"],"publicationHash":record["hash"],
        "publicationSessionId":record["sessionId"],
        "requestStartedAt":b["requestStartedAt"],"quoteReceivedAt":b["receivedAt"],
        "confirmedAt":record["at"],"availableAt":available_at,
        "nextExpiryAt":null,"expiryKnown":false,"decisionEligible":true,
        "continuity":{"ready":true,"continuous":true,
            "evidence":"verified_authoritative_snapshot_not_continuous_trade_tape"},
        "book":b})
}

/// openPublicRecoveryWatch({runtimeDir, cohort, initialize, readOnly, universe,
/// identities, now, workspaceDir, canOperate})
pub struct PublicRecoveryWatch {
    pub dir: PathBuf,
    pub bank: V,
    state: V,
    publication: V,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    cohort: String,
    read_only: bool,
    source_file: PathBuf,
    lock_file: PathBuf,
    code_identity: V,
    source_identity: (u64, f64, u64),
    source_offset: u64,
    source_boundary: String,
    bank_file_hash: String,
    journal_sequence: i64,
    journal_hash: String,
    journal_bytes: u64,
    ended: bool,
    closed: bool,
    failed: bool,
    lock_fd: Option<File>,
    claim: Option<(u64, f64)>,
    now: Box<dyn Fn() -> f64>,
    can_operate: Box<dyn Fn() -> bool>,
}
impl PublicRecoveryWatch {
    fn journal_file(&self) -> PathBuf { self.dir.join("journal.jsonl") }
    fn bank_file(&self) -> PathBuf { self.dir.join("bank.json") }
    /// stopped()
    pub fn stopped(&self) -> bool {
        !(self.can_operate)()
            || self.runtime_dir.join("STOP").exists()
            || self.runtime_dir.join("PUBLIC_RECOVERY_STOP").exists()
            || self.workspace_dir.join("STOP").exists()
            || self.dir.join("STOP").exists()
    }
    /// owned()
    pub fn owned(&self) -> bool {
        if self.closed { return false; }
        self.read_only || std::fs::read_to_string(&self.lock_file)
            .map(|s| s == std::process::id().to_string()).unwrap_or(false)
    }
    fn live(&self) -> Result<(), String> {
        need(!self.read_only && self.owned() && !self.stopped(), "PUBLIC_RECOVERY_STOPPED_OR_READ_ONLY")
    }
    /// current()
    pub fn current(&self) -> bool {
        !self.ended && !baal_service_runtime::json_text::js_truthy(self.publication.get("closed"))
    }
    fn expected(&self) -> V {
        json!({"sourceBytes":self.source_offset,
            "sourceSequence":self.publication["sequence"],
            "sourceHash":self.publication["hash"],
            "sourceStateHash":crate::public_recovery::public_recovery_hash(&self.publication),
            "evidenceThrough":self.publication["lastAt"],
            "stateHash":crate::public_recovery::public_recovery_hash(&self.state)})
    }
    fn events_for(&self, record: &V, available_at: &str) -> Vec<V> {
        let last = baal_service_runtime::json_text::js_int(&self.state["lastEventId"]).unwrap_or(0);
        if record["kind"].as_str() == Some("book") {
            vec![public_recovery_event(record, last + 1, available_at)]
        } else if record["kind"].as_str() == Some("invalidation")
            || record["kind"].as_str() == Some("session_stop") {
            let id = if record["kind"].as_str() == Some("invalidation") {
                record["payload"]["marketId"].clone()
            } else { V::Null };
            self.bank["markets"].as_array().cloned().unwrap_or_default().iter()
                .filter(|m| id.is_null() || m["marketId"] == id).enumerate()
                .map(|(i, m)| json!({"kind":"invalidate","eventId":last + i as i64 + 1,
                    "tournamentId":self.bank["tournamentId"],"marketId":m["marketId"],
                    "exchangeId":m["exchangeId"],"confirmedAt":record["at"],
                    "availableAt":available_at,"reason":record["payload"]["reason"]}))
                .collect()
        } else { vec![] }
    }
    /// reduceBatch(sourceRows, availableAt, allowDecisions)
    fn reduce_batch(&mut self, source_rows: &[PublicRecoveryRow], available_at: &str,
        allow_decisions: bool) -> Result<(), String> {
        let mut events: Vec<V> = vec![];
        let mut event_id = baal_service_runtime::json_text::js_int(&self.state["lastEventId"]).unwrap_or(0);
        for x in source_rows {
            let r = &x.row;
            need(r["sessionId"] == self.bank["sourceSessionId"]
                && r["kind"].as_str() != Some("session_start"), "PUBLIC_RECOVERY_SESSION_CHANGED")?;
            self.publication = crate::book_publication::apply_book_publication(&self.publication, r)?;
            self.source_offset = x.offset;
            let mut mapped = self.events_for(r, available_at);
            for e in mapped.iter_mut() {
                event_id += 1;
                e["eventId"] = json!(event_id);
            }
            events.extend(mapped);
            if r["kind"].as_str() == Some("session_stop") { self.ended = true; }
        }
        self.state = crate::public_recovery::advance_public_recovery(&self.state,
            &json!(events), &self.bank,
            &json!({"availableAt":available_at,
                "evidenceThrough":self.publication["lastAt"],
                "allowDecisions":allow_decisions && !self.ended}))?;
        Ok(())
    }
    fn append(&mut self, body: V) -> Result<(), String> {
        self.live()?;
        need(std::fs::metadata(self.journal_file()).map_err(|e| e.to_string())?.len()
            == self.journal_bytes, "PUBLIC_RECOVERY_JOURNAL_CHANGED")?;
        let mut row = json!({"sequence":self.journal_sequence + 1,"previous":self.journal_hash,
            "bankHash":crate::public_recovery::public_recovery_hash(&self.bank)});
        for (k, v) in body.as_object().unwrap() { row[k] = v.clone(); }
        let h = crate::public_recovery::public_recovery_hash(&row);
        row["hash"] = json!(h.clone());
        let bytes = format!("{}\n", baal_service_runtime::json_text::js_stringify(&row));
        durable(&self.journal_file(), bytes.as_bytes(), true, false)?;
        self.journal_sequence += 1;
        self.journal_hash = h;
        self.journal_bytes += bytes.len() as u64;
        Ok(())
    }
    /// consume() — records consumed this pass.
    pub fn consume(&mut self) -> Result<usize, String> {
        need(!self.closed && !self.failed, "PUBLIC_RECOVERY_FAILED_CLOSED")?;
        let before = (self.publication.clone(), self.state.clone(), self.source_offset, self.ended);
        let mut committed = false;
        let r = (|h: &mut Self| -> Result<usize, String> {
            h.live()?;
            need(h.current(), "PUBLIC_RECOVERY_SOURCE_ENDED")?;
            let stat = stat_of(&h.source_file)?;
            need(stat.0 == h.source_identity.0 && stat.1 == h.source_identity.1
                && stat.2 >= h.source_offset, "PUBLIC_RECOVERY_SOURCE_REPLACED")?;
            need(boundary(&h.source_file, h.source_offset)? == h.source_boundary
                && hash(&std::fs::read(h.bank_file()).map_err(|e| e.to_string())?) == h.bank_file_hash
                && crate::public_recovery::public_recovery_hash(&public_recovery_code_identity(&h.workspace_dir)?)
                    == crate::public_recovery::public_recovery_hash(&h.code_identity),
                "PUBLIC_RECOVERY_IDENTITY_CHANGED")?;
            let mut rows = PublicRecoveryRows::open(&h.source_file, h.source_offset, Some(stat.2), true)?;
            let mut pending: Vec<PublicRecoveryRow> = vec![];
            while let Some(x) = rows.next_item()? { pending.push(x); }
            if pending.is_empty() { return Ok(0); }
            let available_at = baal_service_runtime::contract::ms_to_iso((h.now)());
            h.reduce_batch(&pending, &available_at, true)?;
            let mut body = json!({"kind":"batch","availableAt":available_at,
                "allowDecisions":!h.ended});
            for (k, v) in h.expected().as_object().unwrap() { body[k] = v.clone(); }
            h.append(body)?;
            committed = true;
            h.source_boundary = boundary(&h.source_file, h.source_offset)?;
            Ok(pending.len())
        })(self);
        match r {
            Ok(n) => Ok(n),
            Err(e) => {
                if !committed {
                    self.publication = before.0;
                    self.state = before.1;
                    self.source_offset = before.2;
                    self.ended = before.3;
                }
                self.failed = true;
                Err(e)
            }
        }
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let mut out = crate::public_recovery::public_recovery_report(&self.state, &self.bank,
            &baal_service_runtime::contract::ms_to_iso((self.now)()))?;
        out["cohort"] = json!(self.cohort);
        out["sourceBytes"] = json!(self.source_offset);
        out["sourceSequence"] = self.publication["sequence"].clone();
        out["sourceHash"] = self.publication["hash"].clone();
        out["sourceStateHash"] = json!(crate::public_recovery::public_recovery_hash(&self.publication));
        out["sourceSessionId"] = self.publication["sessionId"].clone();
        out["sourceClosed"] = self.publication["closed"].clone();
        out["evidenceThrough"] = self.publication["lastAt"].clone();
        out["journalSequence"] = json!(self.journal_sequence);
        out["journalHash"] = json!(self.journal_hash);
        out["stateHash"] = json!(crate::public_recovery::public_recovery_hash(&self.state));
        out["pid"] = if self.read_only { V::Null } else { json!(std::process::id()) };
        out["readOnly"] = json!(self.read_only);
        out["networkRequests"] = json!(0);
        if self.read_only { out["runtimeWrites"] = json!(0); }
        // JS emits runtimeWrites:undefined when !readOnly — JSON.stringify omits the key.
        Ok(out)
    }
    /// close()
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed { return Ok(()); }
        let ours = self.owned();
        self.closed = true;
        if let Some(fd) = self.lock_fd.take() {
            drop(fd);
            if ours {
                let (ino, bt, _) = stat_of(&self.lock_file)?;
                if let Some((c_ino, c_bt)) = self.claim {
                    if ino == c_ino && bt == c_bt {
                        std::fs::remove_file(&self.lock_file).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        Ok(())
    }
}
/// openPublicRecoveryWatch(...)
pub fn open_public_recovery_watch(runtime_dir: &Path, cohort: &str, initialize: bool,
    read_only: bool, universe: &V, identities: &V, now: impl Fn() -> f64 + 'static,
    workspace_dir: &Path, can_operate: impl Fn() -> bool + 'static)
    -> Result<PublicRecoveryWatch, String> {
    need(runtime_dir.is_absolute() && name_ok(cohort) && (!initialize || !read_only),
        "PUBLIC_RECOVERY_DIRECTORY")?;
    let dir = runtime_dir.join("public-recovery-cohorts").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let source_file = runtime_dir.join("public-books.jsonl");
    let lock_file = runtime_dir.join("public-recovery.lock");
    let can_operate: Box<dyn Fn() -> bool> = Box::new(can_operate);
    let stopped = |workspace_dir: &Path| -> bool {
        !can_operate()
            || runtime_dir.join("STOP").exists()
            || runtime_dir.join("PUBLIC_RECOVERY_STOP").exists()
            || workspace_dir.join("STOP").exists()
            || dir.join("STOP").exists()
    };
    need(read_only || !stopped(workspace_dir), "PUBLIC_RECOVERY_STOPPED")?;
    if !read_only {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let r = (|| -> Result<PublicRecoveryWatch, String> {
        let mut lock_fd = None;
        let mut claim = None;
        if !read_only {
            let mut o = OpenOptions::new();
            o.write(true).create_new(true);
            #[cfg(unix)]
            o.mode(0o600);
            let mut fd = o.open(&lock_file).map_err(|e| e.to_string())?;
            claim = Some({
                let c = stat_of(&lock_file)?;
                (c.0, c.1)
            });
            fd.write_all(std::process::id().to_string().as_bytes()).map_err(|e| e.to_string())?;
            fd.sync_all().map_err(|e| e.to_string())?;
            lock_fd = Some(fd);
        }
        let mut watch = PublicRecoveryWatch {
            dir: dir.clone(), bank: V::Null, state: V::Null, publication: V::Null,
            runtime_dir: runtime_dir.to_path_buf(), workspace_dir: workspace_dir.to_path_buf(),
            cohort: cohort.to_string(), read_only,
            source_file: source_file.clone(), lock_file: lock_file.clone(),
            code_identity: V::Null, source_identity: (0, 0.0, 0), source_offset: 0,
            source_boundary: String::new(), bank_file_hash: String::new(),
            journal_sequence: 0, journal_hash: String::new(), journal_bytes: 0,
            ended: false, closed: false, failed: false,
            lock_fd, claim, now: Box::new(now), can_operate,
        };
        match open_inner(&mut watch, initialize, universe, identities) {
            Ok(()) => Ok(watch),
            Err(e) => {
                // JS catch: try{close()}catch{} — release the lock like close().
                let _ = watch.close();
                Err(e)
            }
        }
    })();
    r
}
fn open_inner(w: &mut PublicRecoveryWatch, initialize: bool, universe: &V,
    identities: &V) -> Result<(), String> {
    let bank_file = w.dir.join("bank.json");
    let journal_file = w.dir.join("journal.jsonl");
    let prior_bank: Option<V> = if initialize { None } else { Some(read_json(&bank_file)?) };
    let universe = match &prior_bank {
        Some(b) if baal_service_runtime::json_text::js_truthy(b.get("universe")) => {
            b["universe"].clone()
        }
        _ => universe.clone(),
    };
    need(universe["exchanges"].as_array().is_some_and(|a| !a.is_empty()),
        "PUBLIC_RECOVERY_UNIVERSE")?;
    let code_identity = public_recovery_code_identity(&w.workspace_dir)?;
    let source_identity = stat_of(&w.source_file)?;
    let mut publication = crate::book_publication::create_book_publication_state(&universe)?;
    let mut source_offset = 0u64;
    let mut warm: Vec<V> = vec![];
    let source_prefix_size: u64 = match &prior_bank {
        Some(b) if !b["sourcePrefixBytes"].is_null() => {
            b["sourcePrefixBytes"].as_u64().unwrap_or(0)
        }
        _ => source_identity.2,
    };
    need(source_prefix_size > 0 && source_prefix_size <= 512 * 1024 * 1024,
        "PUBLIC_RECOVERY_PREFIX")?;
    {
        let mut rows = PublicRecoveryRows::open(&w.source_file, 0, Some(source_prefix_size),
            initialize)?;
        while let Some(x) = rows.next_item()? {
            publication = crate::book_publication::apply_book_publication(&publication, &x.row)?;
            source_offset = x.offset;
            if x.row["kind"].as_str() == Some("book") {
                warm.push(public_recovery_event(&x.row, 0, x.row["at"].as_str().unwrap_or("")));
            }
        }
    }
    need(!baal_service_runtime::json_text::js_truthy(publication.get("closed"))
        && source_offset > 0, "PUBLIC_RECOVERY_SOURCE_CLOSED")?;
    let bank: V;
    if initialize {
        let markets: Vec<V> = publication["scope"]["exchanges"].as_array()
            .cloned().unwrap_or_default().iter().map(|x| {
                let mut m = serde_json::Map::new();
                if let Some(obj) = identities[x["marketId"].as_str().unwrap_or("")].as_object() {
                    for (k, v) in obj { m.insert(k.clone(), v.clone()); }
                }
                m.insert("marketId".to_string(), x["marketId"].clone());
                m.insert("exchangeId".to_string(), x["exchangeId"].clone());
                V::Object(m)
            }).collect();
        bank = json!({"schemaVersion":1,"mode":"public_rest_recovery_paper","cohort":w.cohort,
            "tournamentId":universe["tournamentId"],"universe":universe,"markets":markets,
            "cutoff":baal_service_runtime::contract::ms_to_iso((w.now)()),"startEventId":0,
            "sourceScopeHash":publication["scopeHash"],"sourceSessionId":publication["sessionId"],
            "sourcePrefixBytes":source_offset,"sourcePrefixSequence":publication["sequence"],
            "sourcePrefixJournalHash":publication["hash"],
            "sourcePrefixHash":prefix_hash(&w.source_file, source_offset)?,
            "sourceFile":"public-books.jsonl",
            "policy":crate::public_recovery::public_recovery_policy(),
            "variants":crate::public_recovery::public_recovery_variants(),
            "confirmationPolicy":crate::public_recovery::public_confirmation_policy(),
            "codeIdentity":code_identity});
        need(!bank_file.exists() && !journal_file.exists(), "PUBLIC_RECOVERY_COHORT_EXISTS")?;
    } else {
        let pb = prior_bank.clone().unwrap();
        bank = pb.clone();
        need(source_offset == pb["sourcePrefixBytes"].as_u64().unwrap_or(u64::MAX)
            && prefix_hash(&w.source_file, source_offset)?
                == pb["sourcePrefixHash"].as_str().unwrap_or("")
            && publication["sequence"] == pb["sourcePrefixSequence"]
            && publication["hash"] == pb["sourcePrefixJournalHash"]
            && publication["scopeHash"] == pb["sourceScopeHash"]
            && publication["sessionId"] == pb["sourceSessionId"]
            && crate::public_recovery::public_recovery_hash(&code_identity)
                == crate::public_recovery::public_recovery_hash(&pb["codeIdentity"]),
            "PUBLIC_RECOVERY_FROZEN_IDENTITY")?;
    }
    let initial = crate::public_recovery::create_public_recovery(&bank)?;
    let state = crate::public_recovery::warm_public_recovery(&initial, &json!(warm), &bank)?;
    w.bank = bank;
    w.state = state;
    w.publication = publication;
    w.source_offset = source_offset;
    w.code_identity = code_identity;
    w.source_identity = source_identity;
    w.journal_sequence = 0;
    w.journal_hash = crate::public_recovery::public_recovery_hash(&w.bank);
    w.journal_bytes = 0;
    w.ended = false;
    if !initialize {
        let mut input_rows = PublicRecoveryRows::open(&w.source_file, w.source_offset, None, true)?;
        let replay = (|watch: &mut PublicRecoveryWatch,
            input_rows: &mut PublicRecoveryRows| -> Result<(), String> {
            let mut journal_rows = PublicRecoveryRows::open(&watch.journal_file(), 0, None, false)?;
            while let Some(saved) = journal_rows.next_item()? {
                let mut body = saved.row.clone();
                let h = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
                need(h.as_str() == Some(crate::public_recovery::public_recovery_hash(&body).as_str())
                    && body["sequence"].as_i64() == Some(watch.journal_sequence + 1)
                    && body["previous"].as_str() == Some(watch.journal_hash.as_str())
                    && body["bankHash"].as_str()
                        == Some(crate::public_recovery::public_recovery_hash(&watch.bank).as_str()),
                    "PUBLIC_RECOVERY_JOURNAL_CHAIN")?;
                if body["kind"].as_str() == Some("genesis") {
                    need(watch.journal_sequence == 0
                        && body["stateHash"].as_str()
                            == Some(crate::public_recovery::public_recovery_hash(&watch.state).as_str()),
                        "PUBLIC_RECOVERY_GENESIS")?;
                } else {
                    need(body["kind"].as_str() == Some("batch")
                        && body["sourceSequence"].as_i64().unwrap_or(0)
                            > watch.publication["sequence"].as_i64().unwrap_or(0),
                        "PUBLIC_RECOVERY_CHECKPOINT")?;
                    let mut pending: Vec<PublicRecoveryRow> = vec![];
                    while watch.publication["sequence"].as_i64().unwrap_or(0)
                        + (pending.len() as i64)
                        < body["sourceSequence"].as_i64().unwrap_or(0) {
                        let x = input_rows.next_item()?;
                        need(x.is_some(), "PUBLIC_RECOVERY_SOURCE_TRUNCATED")?;
                        pending.push(x.unwrap());
                    }
                    watch.reduce_batch(&pending,
                        body["availableAt"].as_str().unwrap_or(""),
                        baal_service_runtime::json_text::js_truthy(body.get("allowDecisions")))?;
                    for (k, v) in watch.expected().as_object().unwrap() {
                        need(crate::public_recovery::public_recovery_hash(&body[k])
                            == crate::public_recovery::public_recovery_hash(v),
                            "PUBLIC_RECOVERY_REPLAY_MISMATCH")?;
                    }
                }
                watch.journal_sequence += 1;
                watch.journal_hash = h.as_str().unwrap_or("").to_string();
                watch.journal_bytes = saved.offset;
            }
            Ok(())
        })(w, &mut input_rows);
        drop(input_rows);
        replay?;
        need(w.journal_sequence > 0, "PUBLIC_RECOVERY_GENESIS_MISSING")?;
    }
    if initialize {
        w.live()?;
        durable(&bank_file,
            format!("{}\n", serde_json::to_string_pretty(&w.bank).map_err(|e| e.to_string())?)
                .as_bytes(), false, true)?;
        durable(&journal_file, &[], false, true)?;
        w.append(json!({"kind":"genesis",
            "stateHash":crate::public_recovery::public_recovery_hash(&w.state)}))?;
    }
    w.bank_file_hash = hash(&std::fs::read(&bank_file).map_err(|e| e.to_string())?);
    w.source_boundary = boundary(&w.source_file, w.source_offset)?;
    Ok(())
}

/// runPublicRecoveryWatch({runtimeDir, cohort, initialize, universe, identities,
/// once, now, sleep, workspaceDir}) — synchronous; `ending` replaces the JS
/// SIGINT/SIGTERM flag (the caller wires signal handlers).
pub fn run_public_recovery_watch(runtime_dir: &Path, cohort: &str, initialize: bool,
    universe: &V, identities: &V, once: bool, now: impl Fn() -> f64 + 'static,
    sleep: impl Fn(u64), workspace_dir: &Path, ending: impl Fn() -> bool + 'static)
    -> Result<(), String> {
    let mut watch: Option<PublicRecoveryWatch> = None;
    let publish = |watch: &PublicRecoveryWatch, status: &str| -> Result<(), String> {
        let mut report = watch.status()?;
        report["status"] = json!(status);
        if status != "running" {
            if let Some(wallets) = report["wallets"].as_object_mut() {
                for (_, w) in wallets.iter_mut() {
                    let has_positions = w["positions"].as_array().is_some_and(|a| !a.is_empty());
                    if has_positions { w["totalPnl"] = V::Null; }
                    w["marksComplete"] = json!(!has_positions);
                    if let Some(positions) = w["positions"].as_array_mut() {
                        for p in positions.iter_mut() {
                            p["markReady"] = V::Bool(false);
                            p["liquidationValue"] = V::Null;
                        }
                    }
                }
            }
        }
        crate::paper_persistence::atomic_paper_json(&watch.dir.join("stats.json"), &report,
            &|| watch.owned() && (status != "running" || !watch.stopped()))
            .map_err(|e| e.to_string())?;
        Ok(())
    };
    let result = (|| -> Result<(), String> {
        let mut w = open_public_recovery_watch(runtime_dir, cohort, initialize, false,
            universe, identities, now, workspace_dir, move || !ending())?;
        loop {
            if w.stopped() || !w.current() { break; }
            w.consume()?;
            publish(&w, if w.current() { "running" } else { "source_session_ended" })?;
            if once { break; }
            sleep(250);
        }
        if w.stopped() || !w.current() {
            publish(&w, if w.stopped() { "stopped" } else { "source_session_ended" })?;
        }
        watch = Some(w);
        Ok(())
    })();
    let out = match result {
        Err(_) => {
            // JS catch: if(watch) try{await publish(...)}catch{} — publish only
            // when the watch was opened.
            if let Some(w) = &watch {
                let _ = publish(w, if w.stopped() { "stopped" } else { "failed" });
            }
            Err("PUBLIC_RECOVERY_WORKER_FAILED_CLOSED".to_string())
        }
        ok => ok,
    };
    if let Some(mut w) = watch.take() { w.close()?; }
    out
}
