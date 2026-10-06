// live-paper/public-recovery-watch.mjs — cohort watch + journal replay loop.
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::probability_receipt_host_v2::{sha, to_iso, NowFn};
use crate::public_recovery::*;

const MAX: usize = 8 * 1024 * 1024;
fn need(x: bool, m: &str) -> Result<(), String> {
    if !x {
        return Err(m.to_string());
    }
    Ok(())
}
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
/// name(x) — cohort shape plus the Windows reserved-name exclusion.
fn name_ok(x: &str) -> bool {
    let b = x.as_bytes();
    let shape = b.len() >= 1
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-');
    let reserved = matches!(
        x,
        "con" | "prn" | "aux" | "nul" | "com1" | "com2" | "com3" | "com4" | "com5" | "com6"
            | "com7" | "com8" | "com9" | "lpt1" | "lpt2" | "lpt3" | "lpt4" | "lpt5" | "lpt6"
            | "lpt7" | "lpt8" | "lpt9"
    );
    shape && !reserved
}
/// publicRecoveryCodeIdentity() — reads under `workspace` (the repo root).
pub fn public_recovery_code_identity(workspace: &Path) -> Result<V, String> {
    let mut out = serde_json::Map::new();
    for p in [
        "live-paper/public-recovery-watch.mjs",
        "live-paper/public-recovery.mjs",
        "live-paper/public-recovery-confirmation.mjs",
        "live-paper/book-publication.mjs",
        "live-paper/paper-persistence.mjs",
        "live-paper/model.mjs",
        "paper/fixed.mjs",
        "feed/contract.mjs",
    ] {
        out.insert(
            p.to_string(),
            json!(sha(&fs::read(workspace.join(p)).map_err(|e| e.to_string())?)),
        );
    }
    Ok(V::Object(out))
}
fn durable(file: &Path, bytes: &[u8], flags: &str) -> Result<(), String> {
    crate::probability_receipt_host_v2::write(file, bytes, flags)
}
/// Streaming rows reader — JS generator semantics (lazy pull, early drop = return()).
struct WatchRows {
    f: File,
    pos: u64,
    offset: u64,
    size: u64,
    partial: bool,
    tail: Vec<u8>,
    done: bool,
}
struct WatchRow {
    row: V,
    offset: u64,
}
impl WatchRows {
    fn open(file: &Path, start: u64, size: u64, partial: bool) -> Result<WatchRows, String> {
        let f = File::open(file).map_err(|e| e.to_string())?;
        Ok(WatchRows { f, pos: start, offset: start, size, partial, tail: Vec::new(), done: false })
    }
    fn next(&mut self) -> Result<Option<WatchRow>, String> {
        loop {
            // Emit the first buffered complete line (JS extracts all newlines
            // per chunk; per-line emission reaches the same states).
            if let Some(end) = self.tail.iter().position(|&b| b == 10) {
                let raw: Vec<u8> = self.tail.drain(..=end).collect();
                need(raw.len() > 1 && raw.len() <= MAX, "PUBLIC_RECOVERY_RECORD")?;
                self.offset += raw.len() as u64;
                let row: V = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
                return Ok(Some(WatchRow { row, offset: self.offset }));
            }
            // tail.length<MAX — checked after every extraction pass in JS.
            need(self.tail.len() < MAX, "PUBLIC_RECOVERY_RECORD")?;
            if self.done || self.pos >= self.size {
                need(
                    self.partial || self.tail.is_empty(),
                    "PUBLIC_RECOVERY_INCOMPLETE_JOURNAL",
                )?;
                return Ok(None);
            }
            let want = (1024 * 1024usize).min((self.size - self.pos) as usize);
            let mut buf = vec![0u8; want];
            self.f.seek(SeekFrom::Start(self.pos)).map_err(|e| e.to_string())?;
            let mut got = 0usize;
            while got < want {
                let n = self.f.read(&mut buf[got..]).map_err(|e| e.to_string())?;
                need(n > 0, "PUBLIC_RECOVERY_READ")?;
                got += n;
            }
            buf.truncate(got);
            self.pos += got as u64;
            self.tail.extend_from_slice(&buf);
        }
    }
}
fn prefix_hash(file: &Path, n: u64) -> Result<String, String> {
    let mut f = File::open(file).map_err(|e| e.to_string())?;
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    let mut offset = 0u64;
    while offset < n {
        let want = (1024 * 1024usize).min((n - offset) as usize);
        let mut buf = vec![0u8; want];
        let count = f.read(&mut buf).map_err(|e| e.to_string())?;
        need(count > 0, "PUBLIC_RECOVERY_TRUNCATED")?;
        h.update(&buf[..count]);
        offset += count as u64;
    }
    Ok(format!("{:x}", h.finalize()))
}
fn boundary(file: &Path, offset: u64) -> Result<String, String> {
    let n = 4096u64.min(offset);
    let mut buf = vec![0u8; n as usize];
    let mut f = File::open(file).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(offset - n)).map_err(|e| e.to_string())?;
    // JS requires a single readSync to return exactly n bytes.
    let got = f.read(&mut buf).map_err(|e| e.to_string())?;
    need(got == buf.len(), "PUBLIC_RECOVERY_TRUNCATED")?;
    Ok(sha(&buf))
}
/// publicRecoveryEvent(record, eventId, availableAt)
pub fn public_recovery_event(record: &V, event_id: f64, available_at: &V) -> V {
    let b = &record["payload"]["book"];
    json!({"kind": "book", "eventId": event_id, "tournamentId": record["tournamentId"],
        "marketId": b["marketId"], "exchangeId": b["exchangeId"],
        "origin": "rest_observation", "sourceKind": "verified_public_rest_book_publication",
        "publicationSequence": record["sequence"], "publicationHash": record["hash"],
        "publicationSessionId": record["sessionId"],
        "requestStartedAt": b["requestStartedAt"], "quoteReceivedAt": b["receivedAt"],
        "confirmedAt": record["at"], "availableAt": available_at, "nextExpiryAt": V::Null,
        "expiryKnown": false, "decisionEligible": true,
        "continuity": {"ready": true, "continuous": true,
            "evidence": "verified_authoritative_snapshot_not_continuous_trade_tape"},
        "book": b.clone()})
}

pub struct PublicRecoveryWatch<'a> {
    pub dir: PathBuf,
    pub bank: V,
    cohort: String,
    closed: bool,
    failed: bool,
    fd: Option<File>,
    claim: (u64, Option<std::time::SystemTime>),
    lock_file: PathBuf,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    bank_file: PathBuf,
    journal_file: PathBuf,
    source_file: PathBuf,
    source_identity: (u64, Option<std::time::SystemTime>),
    code_identity: V,
    publication: V,
    state: V,
    source_offset: u64,
    journal_sequence: u64,
    journal_hash: String,
    journal_bytes: u64,
    ended: bool,
    source_boundary: String,
    bank_file_hash: String,
    read_only: bool,
    can_operate: Box<dyn Fn() -> bool + 'a>,
    now: NowFn,
}
impl<'a> PublicRecoveryWatch<'a> {
    fn stopped_inner(&self) -> bool {
        !(self.can_operate)()
            || self.runtime_dir.join("STOP").exists()
            || self.runtime_dir.join("PUBLIC_RECOVERY_STOP").exists()
            || self.workspace_dir.join("STOP").exists()
            || self.dir.join("STOP").exists()
    }
    pub fn owned(&self) -> bool {
        if self.closed {
            return false;
        }
        if self.read_only {
            return true;
        }
        match fs::read_to_string(&self.lock_file) {
            Ok(s) => s == std::process::id().to_string(),
            Err(_) => false,
        }
    }
    pub fn current(&self) -> bool {
        !self.ended && !self.publication["closed"].as_bool().unwrap_or(false)
    }
    pub fn stopped(&self) -> bool {
        self.stopped_inner()
    }
    fn live(&self) -> Result<(), String> {
        need(
            !self.read_only && self.owned() && !self.stopped(),
            "PUBLIC_RECOVERY_STOPPED_OR_READ_ONLY",
        )
    }
    fn expected(&self) -> V {
        json!({"sourceBytes": self.source_offset, "sourceSequence": self.publication["sequence"],
            "sourceHash": self.publication["hash"],
            "sourceStateHash": public_recovery_hash(&self.publication),
            "evidenceThrough": self.publication["lastAt"],
            "stateHash": public_recovery_hash(&self.state)})
    }
    fn events_for(&self, record: &V, available_at: &V) -> Vec<V> {
        let last_id = self.state["lastEventId"].as_f64().unwrap_or(f64::NAN);
        match record["kind"].as_str() {
            Some("book") => vec![public_recovery_event(record, last_id + 1.0, available_at)],
            Some("invalidation") | Some("session_stop") => {
                let id = if record["kind"].as_str() == Some("invalidation") {
                    record["payload"]["marketId"].clone()
                } else {
                    V::Null
                };
                self.bank["markets"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .enumerate()
                    .filter(|(_, m)| {
                        id.is_null() || crate::probability_receipt_host_v2::js_eq(&m["marketId"], &id)
                    })
                    .map(|(i, m)| {
                        json!({"kind": "invalidate", "eventId": last_id + i as f64 + 1.0,
                            "tournamentId": self.bank["tournamentId"], "marketId": m["marketId"],
                            "exchangeId": m["exchangeId"], "confirmedAt": record["at"],
                            "availableAt": available_at, "reason": record["payload"]["reason"]})
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }
    // One consumer timestamp per batch; historical replay reuses exactly that time.
    fn reduce_batch(&mut self, source_rows: &[WatchRow], available_at: &str, allow_decisions: &V) -> Result<(), String> {
        let mut events: Vec<V> = Vec::new();
        let mut event_id = self.state["lastEventId"].as_f64().unwrap_or(f64::NAN);
        for x in source_rows {
            let r = &x.row;
            need(
                r["sessionId"] == self.bank["sourceSessionId"]
                    && r["kind"].as_str() != Some("session_start"),
                "PUBLIC_RECOVERY_SESSION_CHANGED",
            )?;
            self.publication = crate::book_publication::apply_book_publication(&self.publication, r)?;
            self.source_offset = x.offset;
            let mut mapped = self.events_for(r, &json!(available_at));
            for e in mapped.iter_mut() {
                event_id += 1.0;
                e["eventId"] = json!(event_id);
            }
            events.extend(mapped);
            if r["kind"].as_str() == Some("session_stop") {
                self.ended = true;
            }
        }
        // JS: advance(...,{allowDecisions: allowDecisions && !ended}) — a
        // non-boolean allowDecisions becomes undefined-ish (Null) -> EVIDENCE_CLOCK.
        let ad = match allow_decisions.as_bool() {
            Some(b) => json!(b && !self.ended),
            None => V::Null,
        };
        self.state = advance_public_recovery(
            &self.state,
            &events,
            &self.bank,
            available_at,
            self.publication["lastAt"].as_str(),
            &ad,
        )?;
        Ok(())
    }
    fn append(&mut self, body: &V) -> Result<(), String> {
        self.live()?;
        need(
            fs::metadata(&self.journal_file).map_err(|e| e.to_string())?.len() == self.journal_bytes,
            "PUBLIC_RECOVERY_JOURNAL_CHANGED",
        )?;
        let mut row = json!({"sequence": self.journal_sequence as f64 + 1.0,
            "previous": self.journal_hash, "bankHash": public_recovery_hash(&self.bank)});
        for (k, v) in body.as_object().unwrap() {
            row[k] = v.clone();
        }
        let h = public_recovery_hash(&row);
        let mut line = row.as_object().unwrap().clone();
        line.insert("hash".to_string(), json!(h));
        let bytes = format!("{}\n", js_stringify(&V::Object(line))).into_bytes();
        durable(&self.journal_file, &bytes, "a")?;
        self.journal_sequence += 1;
        self.journal_hash = h;
        self.journal_bytes += bytes.len() as u64;
        Ok(())
    }
    pub fn consume(&mut self) -> Result<u64, String> {
        need(!self.closed && !self.failed, "PUBLIC_RECOVERY_FAILED_CLOSED")?;
        let before = (
            self.publication.clone(),
            self.state.clone(),
            self.source_offset,
            self.ended,
        );
        let mut committed = false;
        let mut run = |w: &mut Self| -> Result<u64, String> {
            w.live()?;
            need(w.current(), "PUBLIC_RECOVERY_SOURCE_ENDED")?;
            let stat = fs::metadata(&w.source_file).map_err(|e| e.to_string())?;
            let (ino, btime) = (stat.ino(), stat.created().ok());
            need(
                ino == w.source_identity.0
                    && btime == w.source_identity.1
                    && stat.len() >= w.source_offset,
                "PUBLIC_RECOVERY_SOURCE_REPLACED",
            )?;
            need(
                boundary(&w.source_file, w.source_offset)? == w.source_boundary
                    && sha(&fs::read(&w.bank_file).map_err(|e| e.to_string())?) == w.bank_file_hash
                    && public_recovery_hash(&public_recovery_code_identity(&w.workspace_dir)?)
                        == public_recovery_hash(&w.code_identity),
                "PUBLIC_RECOVERY_IDENTITY_CHANGED",
            )?;
            let mut rows = WatchRows::open(&w.source_file, w.source_offset, stat.len(), true)?;
            let mut pending = Vec::new();
            while let Some(x) = rows.next()? {
                pending.push(x);
            }
            if pending.is_empty() {
                return Ok(0);
            }
            let available_at = to_iso((w.now)())?;
            w.reduce_batch(&pending, &available_at, &json!(true))?;
            let mut body = json!({"kind": "batch", "availableAt": available_at,
                "allowDecisions": !w.ended});
            for (k, v) in w.expected().as_object().unwrap() {
                body[k] = v.clone();
            }
            w.append(&body)?;
            committed = true;
            w.source_boundary = boundary(&w.source_file, w.source_offset)?;
            Ok(pending.len() as u64)
        };
        match run(self) {
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
    pub fn status(&self) -> Result<V, String> {
        let at = to_iso((self.now)())?;
        let report = public_recovery_report(&self.state, &self.bank, &at)?;
        let mut out = report.as_object().unwrap().clone();
        out.insert("cohort".to_string(), json!(self.cohort));
        out.insert("sourceBytes".to_string(), json!(self.source_offset));
        out.insert("sourceSequence".to_string(), self.publication["sequence"].clone());
        out.insert("sourceHash".to_string(), self.publication["hash"].clone());
        out.insert("sourceStateHash".to_string(), json!(public_recovery_hash(&self.publication)));
        out.insert("sourceSessionId".to_string(), self.publication["sessionId"].clone());
        out.insert("sourceClosed".to_string(), self.publication["closed"].clone());
        out.insert("evidenceThrough".to_string(), self.publication["lastAt"].clone());
        out.insert("journalSequence".to_string(), json!(self.journal_sequence));
        out.insert("journalHash".to_string(), json!(self.journal_hash));
        out.insert("stateHash".to_string(), json!(public_recovery_hash(&self.state)));
        out.insert(
            "pid".to_string(),
            if self.read_only { V::Null } else { json!(std::process::id()) },
        );
        out.insert("readOnly".to_string(), json!(self.read_only));
        out.insert("networkRequests".to_string(), json!(0));
        if self.read_only {
            // runtimeWrites: readOnly?0:undefined — undefined is omitted by JSON.stringify.
            out.insert("runtimeWrites".to_string(), json!(0));
        }
        Ok(V::Object(out))
    }
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        let ours = self.owned();
        self.closed = true;
        let had_fd = self.fd.take().is_some(); // fs.closeSync(fd)
        if had_fd && ours {
            if let Ok(s) = fs::metadata(&self.lock_file) {
                if s.ino() == self.claim.0 && s.created().ok() == self.claim.1 {
                    fs::remove_file(&self.lock_file).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
}

/// openPublicRecoveryWatch({runtimeDir,cohort,initialize,readOnly,universe,identities,
/// now,workspaceDir,canOperate}) — `workspace` is the repo root for code identity.
pub fn open_public_recovery_watch<'a>(
    runtime_dir: &Path,
    cohort: &str,
    initialize: bool,
    read_only: bool,
    universe: Option<&V>,
    identities: &V,
    now: NowFn,
    workspace_dir: &Path,
    can_operate: Box<dyn Fn() -> bool + 'a>,
) -> Result<PublicRecoveryWatch<'a>, String> {
    need(
        runtime_dir.is_absolute() && name_ok(cohort) && !(initialize && read_only),
        "PUBLIC_RECOVERY_DIRECTORY",
    )?;
    let dir = runtime_dir.join("public-recovery-cohorts").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let source_file = runtime_dir.join("public-books.jsonl");
    let lock_file = runtime_dir.join("public-recovery.lock");
    let stopped = |can_operate: &dyn Fn() -> bool| -> bool {
        !can_operate()
            || runtime_dir.join("STOP").exists()
            || runtime_dir.join("PUBLIC_RECOVERY_STOP").exists()
            || workspace_dir.join("STOP").exists()
            || dir.join("STOP").exists()
    };
    need(read_only || !stopped(&*can_operate), "PUBLIC_RECOVERY_STOPPED")?;
    if !read_only {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|e| e.to_string())?;
    }
    let mut fd: Option<File> = None;
    let mut claim = (0u64, None::<std::time::SystemTime>);
    let close_cleanup = |fd: Option<File>, ours: bool, claim: (u64, Option<std::time::SystemTime>)| {
        drop(fd);
        if ours {
            if let Ok(s) = fs::metadata(&lock_file) {
                if s.ino() == claim.0 && s.created().ok() == claim.1 {
                    let _ = fs::remove_file(&lock_file);
                }
            }
        }
    };
    let build = |fd: &mut Option<File>, claim: &mut (u64, Option<std::time::SystemTime>)| -> Result<PublicRecoveryWatch<'a>, String> {
        if !read_only {
            let f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&lock_file)
                .map_err(|e| e.to_string())?;
            let meta = f.metadata().map_err(|e| e.to_string())?;
            *claim = (meta.ino(), meta.created().ok());
            *fd = Some(f);
            // write pid + fsync
            if let Some(f) = fd.as_mut() {
                f.write_all(std::process::id().to_string().as_bytes())
                    .map_err(|e| e.to_string())?;
                f.sync_all().map_err(|e| e.to_string())?;
            }
        }
        let mut bank: V = if initialize { V::Null } else { read_json(&bank_file)? };
        let universe_v = if js_truthy_opt(&bank["universe"]) { bank["universe"].clone() } else { universe.cloned().unwrap_or(V::Null) };
        need(
            universe_v["exchanges"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "PUBLIC_RECOVERY_UNIVERSE",
        )?;
        let code_identity = public_recovery_code_identity(workspace_dir)?;
        let source_meta = fs::metadata(&source_file).map_err(|e| e.to_string())?;
        let source_identity = (source_meta.ino(), source_meta.created().ok());
        let mut publication = crate::book_publication::create_book_publication_state(&universe_v)?;
        let mut source_offset = 0u64;
        let mut warm: Vec<V> = Vec::new();
        let source_prefix_size = if bank.is_null() {
            source_meta.len()
        } else {
            bank["sourcePrefixBytes"].as_u64().unwrap_or(source_meta.len())
        };
        need(
            source_prefix_size > 0 && source_prefix_size <= 512 * 1024 * 1024,
            "PUBLIC_RECOVERY_PREFIX",
        )?;
        {
            let mut rows = WatchRows::open(&source_file, 0, source_prefix_size, initialize)?;
            while let Some(x) = rows.next()? {
                publication = crate::book_publication::apply_book_publication(&publication, &x.row)?;
                source_offset = x.offset;
                if x.row["kind"].as_str() == Some("book") {
                    warm.push(public_recovery_event(&x.row, 0.0, &x.row["at"]));
                }
            }
        }
        need(
            !publication["closed"].as_bool().unwrap_or(false) && source_offset > 0,
            "PUBLIC_RECOVERY_SOURCE_CLOSED",
        )?;
        if initialize {
            let markets: Vec<V> = publication["scope"]["exchanges"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|x| {
                    let mut m = serde_json::Map::new();
                    if let Some(idv) = identities.get(x["marketId"].as_str().unwrap_or("")).and_then(|v| v.as_object()) {
                        for (k, v) in idv {
                            m.insert(k.clone(), v.clone());
                        }
                    }
                    m.insert("marketId".to_string(), x["marketId"].clone());
                    m.insert("exchangeId".to_string(), x["exchangeId"].clone());
                    V::Object(m)
                })
                .collect();
            bank = json!({"schemaVersion": 1, "mode": "public_rest_recovery_paper", "cohort": cohort,
                "tournamentId": universe_v["tournamentId"], "universe": universe_v,
                "markets": markets,
                "cutoff": to_iso(now())?, "startEventId": 0,
                "sourceScopeHash": publication["scopeHash"], "sourceSessionId": publication["sessionId"],
                "sourcePrefixBytes": source_offset, "sourcePrefixSequence": publication["sequence"],
                "sourcePrefixJournalHash": publication["hash"],
                "sourcePrefixHash": prefix_hash(&source_file, source_offset)?,
                "sourceFile": "public-books.jsonl",
                "policy": public_recovery_policy(), "variants": public_recovery_variants(),
                "confirmationPolicy": public_confirmation_policy(), "codeIdentity": code_identity});
            need(
                !bank_file.exists() && !journal_file.exists(),
                "PUBLIC_RECOVERY_COHORT_EXISTS",
            )?;
        } else {
            need(
                source_offset as f64 == bank["sourcePrefixBytes"].as_f64().unwrap_or(f64::NAN)
                    && prefix_hash(&source_file, source_offset)?
                        == bank["sourcePrefixHash"].as_str().unwrap_or("")
                    && publication["sequence"] == bank["sourcePrefixSequence"]
                    && publication["hash"] == bank["sourcePrefixJournalHash"]
                    && publication["scopeHash"] == bank["sourceScopeHash"]
                    && publication["sessionId"] == bank["sourceSessionId"]
                    && public_recovery_hash(&code_identity) == public_recovery_hash(&bank["codeIdentity"]),
                "PUBLIC_RECOVERY_FROZEN_IDENTITY",
            )?;
        }
        let state = warm_public_recovery(&create_public_recovery(&bank)?, &warm, &bank)?;
        let mut watch = PublicRecoveryWatch {
            dir: dir.clone(),
            bank: bank.clone(),
            cohort: cohort.to_string(),
            closed: false,
            failed: false,
            fd: None,
            claim: *claim,
            lock_file: lock_file.clone(),
            runtime_dir: runtime_dir.to_path_buf(),
            workspace_dir: workspace_dir.to_path_buf(),
            bank_file: bank_file.clone(),
            journal_file: journal_file.clone(),
            source_file: source_file.clone(),
            source_identity,
            code_identity: code_identity.clone(),
            publication,
            state,
            source_offset,
            journal_sequence: 0,
            journal_hash: public_recovery_hash(&bank),
            journal_bytes: 0,
            ended: false,
            source_boundary: String::new(),
            bank_file_hash: String::new(),
            read_only,
            can_operate,
            now,
        };
        watch.fd = fd.take();
        if !initialize {
            // Journal replay: verify chain then re-reduce each batch.
            let mut input_rows = WatchRows::open(&source_file, watch.source_offset, source_meta.len(), true)?;
            let mut journal_rows = WatchRows::open(&journal_file, 0, fs::metadata(&journal_file).map_err(|e| e.to_string())?.len(), false)?;
            while let Some(saved) = journal_rows.next()? {
                let mut body = saved.row.clone();
                let h = body
                    .as_object_mut()
                    .and_then(|m| m.shift_remove("hash"))
                    .unwrap_or(V::Null);
                need(
                    h.as_str() == Some(public_recovery_hash(&body).as_str())
                        && body["sequence"].as_f64() == Some(watch.journal_sequence as f64 + 1.0)
                        && body["previous"].as_str() == Some(watch.journal_hash.as_str())
                        && body["bankHash"].as_str() == Some(public_recovery_hash(&watch.bank).as_str()),
                    "PUBLIC_RECOVERY_JOURNAL_CHAIN",
                )?;
                if body["kind"].as_str() == Some("genesis") {
                    need(
                        watch.journal_sequence == 0
                            && body["stateHash"].as_str() == Some(public_recovery_hash(&watch.state).as_str()),
                        "PUBLIC_RECOVERY_GENESIS",
                    )?;
                } else {
                    need(
                        body["kind"].as_str() == Some("batch")
                            && body["sourceSequence"].as_i64().unwrap_or(0)
                                > watch.publication["sequence"].as_i64().unwrap_or(0),
                        "PUBLIC_RECOVERY_CHECKPOINT",
                    )?;
                    let mut pending = Vec::new();
                    while (watch.publication["sequence"].as_i64().unwrap_or(0)
                        + pending.len() as i64)
                        < body["sourceSequence"].as_i64().unwrap_or(0)
                    {
                        match input_rows.next()? {
                            Some(x) => pending.push(x),
                            None => return Err("PUBLIC_RECOVERY_SOURCE_TRUNCATED".into()),
                        }
                    }
                    watch.reduce_batch(
                        &pending,
                        body["availableAt"].as_str().unwrap_or(""),
                        &body["allowDecisions"],
                    )?;
                    for (k, v) in watch.expected().as_object().unwrap() {
                        need(
                            public_recovery_hash(&body[k]) == public_recovery_hash(v),
                            "PUBLIC_RECOVERY_REPLAY_MISMATCH",
                        )?;
                    }
                }
                watch.journal_sequence += 1;
                watch.journal_hash = h.as_str().unwrap_or("").to_string();
                watch.journal_bytes = saved.offset;
            }
            need(watch.journal_sequence > 0, "PUBLIC_RECOVERY_GENESIS_MISSING")?;
        }
        if initialize {
            watch.live()?;
            durable(
                &watch.bank_file,
                format!("{}\n", serde_json::to_string_pretty(&watch.bank).map_err(|e| e.to_string())?)
                    .as_bytes(),
                "wx",
            )?;
            durable(&watch.journal_file, &[], "wx")?;
            watch.append(&json!({"kind": "genesis", "stateHash": public_recovery_hash(&watch.state)}))?;
        }
        watch.bank_file_hash = sha(&fs::read(&watch.bank_file).map_err(|e| e.to_string())?);
        watch.source_boundary = boundary(&watch.source_file, watch.source_offset)?;
        Ok(watch)
    };
    match build(&mut fd, &mut claim) {
        Ok(w) => Ok(w),
        Err(e) => {
            // JS catch: try{close()}catch{} — errors swallowed. owned() is false
            // before the lock write, true after (lock content === pid).
            let ours = fs::read_to_string(&lock_file)
                .map(|s| s == std::process::id().to_string())
                .unwrap_or(false);
            close_cleanup(fd, ours, claim);
            Err(e)
        }
    }
}
fn js_truthy_opt(v: &V) -> bool {
    baal_service_runtime::json_text::js_truthy(Some(v))
}

pub struct PublicRecoveryWatchOptions<'a> {
    pub runtime_dir: &'a Path,
    pub cohort: &'a str,
    pub initialize: bool,
    pub universe: Option<&'a V>,
    pub identities: &'a V,
    pub once: bool,
    pub now: NowFn,
    pub workspace_dir: &'a Path,
    /// JS signal flag (SIGINT/SIGTERM set ending=true).
    pub stopping: &'a dyn Fn() -> bool,
    pub sleep: &'a dyn Fn(u64),
}
/// runPublicRecoveryWatch({...})
pub fn run_public_recovery_watch<'a>(options: &'a PublicRecoveryWatchOptions<'a>) -> Result<(), String> {
    let can_operate = move || !(options.stopping)();
    // JS wraps every failure (open included) in PUBLIC_RECOVERY_WORKER_FAILED_CLOSED.
    let mut watch = match open_public_recovery_watch(
        options.runtime_dir,
        options.cohort,
        options.initialize,
        false,
        options.universe,
        options.identities,
        options.now.clone(),
        options.workspace_dir,
        Box::new(can_operate),
    ) {
        Ok(w) => w,
        Err(_) => return Err("PUBLIC_RECOVERY_WORKER_FAILED_CLOSED".into()),
    };
    let publish = |watch: &mut PublicRecoveryWatch, status: &str| -> Result<(), String> {
        let mut report = watch.status()?;
        report.as_object_mut().unwrap().insert("status".to_string(), json!(status));
        if status != "running" {
            if let Some(wallets) = report["wallets"].as_object_mut() {
                for (_, w) in wallets.iter_mut() {
                    let n = w["positions"].as_array().map(|a| a.len()).unwrap_or(0);
                    if n > 0 {
                        w["totalPnl"] = V::Null;
                    }
                    w["marksComplete"] = json!(n == 0);
                    if let Some(ps) = w["positions"].as_array_mut() {
                        for p in ps.iter_mut() {
                            p["markReady"] = json!(false);
                            p["liquidationValue"] = V::Null;
                        }
                    }
                }
            }
        }
        let owned = watch.owned();
        let stoppable = watch.stopped();
        crate::paper_persistence::atomic_paper_json(
            &watch.dir.join("stats.json"),
            &report,
            &move || owned && (status != "running" || !stoppable),
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    };
    let result = (|| -> Result<(), String> {
        loop {
            if watch.stopped() || !watch.current() {
                break;
            }
            watch.consume()?;
            let st = if watch.current() { "running" } else { "source_session_ended" };
            publish(&mut watch, st)?;
            if options.once {
                break;
            }
            (options.sleep)(250);
        }
        if watch.stopped() || !watch.current() {
            let st = if watch.stopped() { "stopped" } else { "source_session_ended" };
            publish(&mut watch, st)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            // finally: watch?.close() — a close error propagates.
            watch.close()
        }
        Err(_) => {
            // catch: publish('stopped'|'failed') swallowed, then throw
            // WORKER_FAILED_CLOSED; finally's close error still wins.
            let st = if watch.stopped() { "stopped" } else { "failed" };
            let _ = publish(&mut watch, st);
            match watch.close() {
                Err(e) => Err(e),
                Ok(()) => Err("PUBLIC_RECOVERY_WORKER_FAILED_CLOSED".into()),
            }
        }
    }
}
