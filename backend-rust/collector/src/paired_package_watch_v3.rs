// live-paper/paired-package-watch-v3.mjs — durable sequential dual-NO paired watch (v3).
// Async sleeps/signals are synchronous here: can_operate is the stop callback.
use crate::book_publication_v2::{apply_book_publication, create_book_publication_state};
use crate::paired_package_paper_v3::{
    advance_paired_paper_v3, create_paired_paper_v3, paired_paper_confirmation_v3, paired_paper_hash_v3,
    paired_paper_policy_v3, paired_paper_report_v3, paired_paper_variants_v3, warm_paired_paper_v3,
};
use crate::paper_persistence::atomic_paper_json;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{digest, js_int, js_stringify, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const MAX: usize = 8 * 1024 * 1024;
fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(code.into()); } Ok(()) }
fn name_ok(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && s.len() <= 64
        && !["con","prn","aux","nul","com1","com2","com3","com4","com5","com6","com7","com8","com9",
            "lpt1","lpt2","lpt3","lpt4","lpt5","lpt6","lpt7","lpt8","lpt9"].contains(&s)
}
fn read_json_file(p: &Path) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
/// pairedPackageCodeIdentity() — sha256 of each JS source under workspace_dir
/// (workspace_dir is the repo root, the JS `workspace` constant).
pub fn paired_package_code_identity_v3(workspace_dir: &Path) -> Result<V, String> {
    let mut m = Map::new();
    for p in [
        "live-paper/paired-package-watch-v3.mjs", "live-paper/paired-package-paper-v3.mjs",
        "live-paper/book-publication.mjs", "live-paper/paper-persistence.mjs",
        "live-paper/model.mjs", "paper/fixed.mjs", "feed/contract.mjs",
    ] {
        m.insert(p.to_string(), json!(sha256(&fs::read(workspace_dir.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}
/// durable(file,bytes,flags) — 'wx' or 'a', 0o600, write loop + fsync.
fn durable(file: &Path, bytes: &[u8], exclusive: bool) -> Result<(), String> {
    let mut o = fs::OpenOptions::new();
    if exclusive {
        o.write(true).create_new(true);
    } else {
        o.write(true).append(true).create(true);
    }
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(file).map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|_| "PAIRED_PACKAGE_WRITE".to_string())?;
    f.sync_all().map_err(|e| e.to_string())
}
/// rows(file,{start,size,partial}) — the JS generator, materialized: (row, end offset).
fn rows(file: &Path, start: u64, size: Option<u64>, partial: bool) -> Result<Vec<(V, u64)>, String> {
    let mut fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let size = size.unwrap_or_else(|| fd.metadata().map(|m| m.len()).unwrap_or(0));
    fd.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut pos = start;
    let mut offset = start;
    let mut tail: Vec<u8> = Vec::new();
    let mut out = Vec::new();
    while pos < size {
        let want = std::cmp::min(1024 * 1024u64, size - pos) as usize;
        let mut b = vec![0u8; want];
        let n = fd.read(&mut b).map_err(|e| e.to_string())?;
        need(n > 0, "PAIRED_PACKAGE_READ")?;
        pos += n as u64;
        tail.extend_from_slice(&b[..n]);
        while let Some(end) = tail.iter().position(|c| *c == 10) {
            let raw_len = end + 1;
            need(raw_len > 1 && raw_len <= MAX, "PAIRED_PACKAGE_RECORD")?;
            offset += raw_len as u64;
            let row: V = serde_json::from_slice(&tail[..raw_len]).map_err(|e| e.to_string())?;
            out.push((row, offset));
            tail.drain(..raw_len);
        }
        need(tail.len() < MAX, "PAIRED_PACKAGE_RECORD")?;
    }
    need(partial || tail.is_empty(), "PAIRED_PACKAGE_INCOMPLETE_JOURNAL")?;
    Ok(out)
}
fn prefix_hash(file: &Path, n: u64) -> Result<String, String> {
    let mut h = Sha256::new();
    let mut fd = fs::File::open(file).map_err(|e| e.to_string())?;
    let mut offset = 0u64;
    while offset < n {
        let want = std::cmp::min(n - offset, 1024 * 1024u64) as usize;
        let mut b = vec![0u8; want];
        fd.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let count = fd.read(&mut b).map_err(|e| e.to_string())?;
        need(count > 0, "PAIRED_PACKAGE_TRUNCATED")?;
        h.update(&b[..count]);
        offset += count as u64;
    }
    Ok(format!("{:x}", h.finalize()))
}
fn boundary(file: &Path, offset: u64) -> Result<String, String> {
    let n = std::cmp::min(4096u64, offset);
    let mut b = vec![0u8; n as usize];
    let mut fd = fs::File::open(file).map_err(|e| e.to_string())?;
    fd.seek(SeekFrom::Start(offset - n)).map_err(|e| e.to_string())?;
    let got = fd.read(&mut b).map_err(|e| e.to_string())?;
    need(got as u64 == n, "PAIRED_PACKAGE_TRUNCATED")?;
    Ok(sha256(&b))
}
/// pairedPackageEvent(record,eventId,availableAt)
pub fn paired_package_event_v3(record: &V, event_id: i64, available_at: &str) -> Result<V, String> {
    let b = &record["payload"]["book"];
    if !b.is_object() {
        // JS: record.payload.book.marketId on undefined book — TypeError.
        return Err("Cannot read properties of undefined (reading 'marketId')".into());
    }
    Ok(json!({"kind":"book","eventId":event_id,"tournamentId":record["tournamentId"],
        "marketId":b["marketId"],"exchangeId":b["exchangeId"],
        "origin":"rest_observation","sourceKind":"verified_public_rest_book_publication",
        "publicationSequence":record["sequence"],"publicationHash":record["hash"],
        "publicationSessionId":record["sessionId"],
        "requestStartedAt":b["requestStartedAt"],"quoteReceivedAt":b["receivedAt"],
        "confirmedAt":record["at"],"availableAt":available_at,
        "nextExpiryAt":V::Null,"expiryKnown":false,"decisionEligible":true,
        "continuity":{"ready":true,"continuous":true,
            "evidence":"verified_authoritative_snapshot_not_continuous_trade_tape"},
        "book":b.clone()}))
}
fn events_for(state: &V, bank: &V, record: &V, available_at: &str) -> Result<Vec<V>, String> {
    let last = state["lastEventId"].as_i64().unwrap_or(0);
    let kind = record["kind"].as_str().unwrap_or("");
    if kind == "book" {
        return Ok(vec![paired_package_event_v3(record, last + 1, available_at)?]);
    }
    if kind == "invalidation" || kind == "session_stop" {
        let id = if kind == "invalidation" { record["payload"]["marketId"].clone() } else { V::Null };
        if record["payload"].is_null() {
            // JS: record.payload.reason on undefined payload — TypeError.
            return Err("Cannot read properties of undefined (reading 'reason')".into());
        }
        return Ok(bank["markets"].as_array().cloned().unwrap_or_default().iter()
            .filter(|m| id.is_null() || m["marketId"] == id)
            .enumerate()
            .map(|(i, m)| {
                let mut e = json!({"kind":"invalidate","eventId":last + i as i64 + 1,
                    "tournamentId":bank["tournamentId"],"marketId":m["marketId"],
                    "exchangeId":m["exchangeId"],"confirmedAt":record["at"],"availableAt":available_at});
                // JS reason:record.payload.reason — undefined-valued keys are dropped.
                if let Some(r) = record["payload"].get("reason") { e["reason"] = r.clone(); }
                e
            }).collect());
    }
    Ok(vec![])
}
/// Object spread of identities[marketId]: object -> entries, string -> index keys, else {}.
fn identity_entry(identities: &V, market_id: &str) -> Map<String, V> {
    match &identities[market_id] {
        V::Object(m) => m.clone(),
        V::String(s) => s.chars().enumerate().map(|(i, c)| (i.to_string(), json!(c.to_string()))).collect(),
        _ => Map::new(),
    }
}

/// The object returned by openPairedPackageWatchV3.
pub struct PairedPackageWatchV3 {
    pub dir: PathBuf,
    pub bank: V,
    workspace_dir: PathBuf,
    runtime_dir: PathBuf,
    journal_file: PathBuf,
    source_file: PathBuf,
    lock_file: PathBuf,
    bank_file: PathBuf,
    cohort: String,
    read_only: bool,
    publication: V,
    state: V,
    source_offset: u64,
    ended: bool,
    journal_sequence: i64,
    journal_hash: String,
    journal_bytes: u64,
    source_boundary: String,
    bank_file_hash: String,
    code_identity: V,
    source_ino: u64,
    source_birth: Option<SystemTime>,
    lock: Option<fs::File>,
    claim_ino: u64,
    claim_birth: Option<SystemTime>,
    closed: bool,
    failed: bool,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl PairedPackageWatchV3 {
    fn stopped(&self) -> bool {
        !(self.can_operate)()
            || self.runtime_dir.join("STOP").exists()
            || self.runtime_dir.join("PAIRED_PACKAGE_STOP").exists()
            || self.workspace_dir.join("STOP").exists()
            || self.dir.join("STOP").exists()
    }
    /// owned()
    pub fn owned(&self) -> bool {
        if self.closed { return false; }
        if self.read_only { return true; }
        fs::read_to_string(&self.lock_file).map(|s| s == std::process::id().to_string()).unwrap_or(false)
    }
    fn live(&self) -> Result<(), String> {
        need(!self.read_only && self.owned() && !self.stopped(), "PAIRED_PACKAGE_STOPPED_OR_READ_ONLY")
    }
    /// current()
    pub fn current(&self) -> bool {
        !self.ended && !js_truthy(self.publication.get("closed"))
    }
    fn expected(&self) -> V {
        json!({"sourceBytes":self.source_offset,"sourceSequence":self.publication["sequence"],
            "sourceHash":self.publication["hash"],"sourceStateHash":digest(&self.publication),
            "evidenceThrough":self.publication["lastAt"],"stateHash":digest(&self.state)})
    }
    fn reduce_batch(&mut self, source_rows: &[(V, u64)], available_at: &str, allow_decisions: bool) -> Result<(), String> {
        let mut events = Vec::new();
        let mut event_id = self.state["lastEventId"].as_i64().unwrap_or(0);
        for (r, offset) in source_rows {
            need(r["sessionId"] == self.bank["sourceSessionId"]
                && r["kind"].as_str() != Some("session_start"), "PAIRED_PACKAGE_SESSION_CHANGED")?;
            self.publication = apply_book_publication(&self.publication, r)?;
            self.source_offset = *offset;
            for mut e in events_for(&self.state, &self.bank, r, available_at)? {
                event_id += 1;
                e["eventId"] = json!(event_id);
                events.push(e);
            }
            if r["kind"].as_str() == Some("session_stop") { self.ended = true; }
        }
        self.state = advance_paired_paper_v3(&self.state, &events, &self.bank, available_at,
            allow_decisions && !self.ended)?;
        Ok(())
    }
    fn append(&mut self, body: V) -> Result<(), String> {
        self.live()?;
        need(fs::metadata(&self.journal_file).map_err(|e| e.to_string())?.len() == self.journal_bytes,
            "PAIRED_PACKAGE_JOURNAL_CHANGED")?;
        let mut row = Map::new();
        row.insert("sequence".into(), json!(self.journal_sequence + 1));
        row.insert("previous".into(), json!(self.journal_hash));
        row.insert("bankHash".into(), json!(paired_paper_hash_v3(&self.bank)));
        if let V::Object(m) = body { for (k, v) in m { row.insert(k, v); } }
        let h = paired_paper_hash_v3(&V::Object(row.clone()));
        let mut line = V::Object(row);
        line.as_object_mut().unwrap().insert("hash".into(), json!(h));
        let bytes = format!("{}\n", js_stringify(&line)).into_bytes();
        durable(&self.journal_file, &bytes, false)?;
        self.journal_sequence += 1;
        self.journal_hash = h;
        self.journal_bytes += bytes.len() as u64;
        Ok(())
    }
    /// consume()
    pub fn consume(&mut self) -> Result<i64, String> {
        need(!self.closed && !self.failed, "PAIRED_PACKAGE_FAILED_CLOSED")?;
        let before = (self.publication.clone(), self.state.clone(), self.source_offset, self.ended);
        let mut committed = false;
        let result = (|| -> Result<i64, String> {
            self.live()?;
            need(self.current(), "PAIRED_PACKAGE_SOURCE_ENDED")?;
            let stat = fs::metadata(&self.source_file).map_err(|e| e.to_string())?;
            let ino = { #[cfg(unix)] { use std::os::unix::fs::MetadataExt; stat.ino() }
                #[cfg(not(unix))] { 0 } };
            let birth = stat.created().ok();
            need(ino == self.source_ino && birth == self.source_birth && stat.len() >= self.source_offset,
                "PAIRED_PACKAGE_SOURCE_REPLACED")?;
            need(boundary(&self.source_file, self.source_offset)? == self.source_boundary
                && sha256(&fs::read(&self.bank_file).map_err(|e| e.to_string())?) == self.bank_file_hash
                && paired_paper_hash_v3(&paired_package_code_identity_v3(&self.workspace_dir)?)
                    == paired_paper_hash_v3(&self.code_identity),
                "PAIRED_PACKAGE_IDENTITY_CHANGED")?;
            let pending = rows(&self.source_file, self.source_offset, Some(stat.len()), true)?;
            if pending.is_empty() { return Ok(0); }
            let available_at = ms_to_iso((self.now)() as f64);
            self.reduce_batch(&pending, &available_at, true)?;
            let mut body = Map::new();
            body.insert("kind".into(), json!("batch"));
            body.insert("availableAt".into(), json!(available_at));
            body.insert("allowDecisions".into(), json!(!self.ended));
            if let V::Object(m) = self.expected() { for (k, v) in m { body.insert(k, v); } }
            self.append(V::Object(body))?;
            committed = true;
            self.source_boundary = boundary(&self.source_file, self.source_offset)?;
            Ok(pending.len() as i64)
        })();
        match result {
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
        let mut st = paired_paper_report_v3(&self.state, &self.bank, &ms_to_iso((self.now)() as f64))?;
        st["cohort"] = json!(self.cohort);
        st["sourceBytes"] = json!(self.source_offset);
        st["sourceSequence"] = self.publication["sequence"].clone();
        st["sourceHash"] = self.publication["hash"].clone();
        st["sourceStateHash"] = json!(digest(&self.publication));
        st["sourceSessionId"] = self.publication["sessionId"].clone();
        st["sourceClosed"] = self.publication["closed"].clone();
        st["evidenceThrough"] = self.publication["lastAt"].clone();
        st["journalSequence"] = json!(self.journal_sequence);
        st["journalHash"] = json!(self.journal_hash);
        st["stateHash"] = json!(digest(&self.state));
        st["pid"] = if self.read_only { V::Null } else { json!(std::process::id()) };
        st["readOnly"] = json!(self.read_only);
        st["networkRequests"] = json!(0);
        if self.read_only { st["runtimeWrites"] = json!(0); }
        Ok(st)
    }
    /// close()
    pub fn close(&mut self) {
        if self.closed { return; }
        let ours = self.owned();
        self.closed = true;
        if self.lock.take().is_some() && ours {
            if let Ok(s) = fs::metadata(&self.lock_file) {
                let ino = { #[cfg(unix)] { use std::os::unix::fs::MetadataExt; s.ino() }
                    #[cfg(not(unix))] { 0 } };
                if ino == self.claim_ino && s.created().ok() == self.claim_birth {
                    let _ = fs::remove_file(&self.lock_file);
                }
            }
        }
    }
}
/// openPairedPackageWatchV3({runtimeDir,cohort,initialize,readOnly,universe,identities,now,workspaceDir,canOperate})
pub fn open_paired_package_watch_v3(runtime_dir: &Path, cohort: &str, initialize: bool, read_only: bool,
    universe: &V, identities: &V, now: Arc<dyn Fn() -> i64 + Send + Sync>, workspace_dir: &Path,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<PairedPackageWatchV3, String> {
    need(runtime_dir.is_absolute() && name_ok(cohort) && (!initialize || !read_only), "PAIRED_PACKAGE_DIRECTORY")?;
    let dir = runtime_dir.join("paired-package-cohorts").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let source_file = runtime_dir.join("public-books.jsonl");
    let lock_file = runtime_dir.join("paired-package.lock");
    let mut lock = None;
    let mut claim_ino = 0u64;
    let mut claim_birth = None;
    let mut opened = || -> Result<PairedPackageWatchV3, String> {
        let stopped = |dir: &Path| -> bool {
            !(can_operate)()
                || runtime_dir.join("STOP").exists()
                || runtime_dir.join("PAIRED_PACKAGE_STOP").exists()
                || workspace_dir.join("STOP").exists()
                || dir.join("STOP").exists()
        };
        need(read_only || !stopped(&dir), "PAIRED_PACKAGE_STOPPED")?;
        if !read_only {
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)); }
        }
        if !read_only {
            let mut o = fs::OpenOptions::new();
            o.write(true).create_new(true);
            #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
            let fd = o.open(&lock_file).map_err(|e| e.to_string())?;
            let claim = fd.metadata().map_err(|e| e.to_string())?;
            { #[cfg(unix)] { use std::os::unix::fs::MetadataExt; claim_ino = claim.ino(); } }
            claim_birth = claim.created().ok();
            let mut fd = fd;
            fd.write_all(std::process::id().to_string().as_bytes()).map_err(|e| e.to_string())?;
            fd.sync_all().map_err(|e| e.to_string())?;
            lock = Some(fd);
        }
        let mut bank = if initialize { V::Null } else { read_json_file(&bank_file)? };
        let universe = if js_truthy(bank.get("universe")) { bank["universe"].clone() } else { universe.clone() };
        need(universe["exchanges"].as_array().is_some_and(|a| !a.is_empty()), "PAIRED_PACKAGE_UNIVERSE")?;
        let code_identity = paired_package_code_identity_v3(workspace_dir)?;
        let source_meta = fs::metadata(&source_file).map_err(|e| e.to_string())?;
        let source_ino = { #[cfg(unix)] { use std::os::unix::fs::MetadataExt; source_meta.ino() }
            #[cfg(not(unix))] { 0 } };
        let source_birth = source_meta.created().ok();
        let mut publication = create_book_publication_state(&universe)?;
        let mut source_offset = 0u64;
        // JS bank?.sourcePrefixBytes ?? sourceIdentity.size — nullish coalescing only.
        let source_prefix_size = match bank.get("sourcePrefixBytes") {
            Some(v) if !v.is_null() && !initialize => v.as_u64().unwrap_or(0),
            _ => source_meta.len(),
        };
        need(source_prefix_size > 0 && source_prefix_size <= 512 * 1024 * 1024, "PAIRED_PACKAGE_PREFIX")?;
        // warm events are built exactly like JS (they can throw on a malformed
        // book record) even though warmPairedPackage ignores them.
        let mut warm = Vec::new();
        for (row, offset) in rows(&source_file, 0, Some(source_prefix_size), initialize)? {
            publication = apply_book_publication(&publication, &row)?;
            source_offset = offset;
            if row["kind"].as_str() == Some("book") {
                warm.push(paired_package_event_v3(&row, 0, row["at"].as_str().unwrap_or(""))?);
            }
        }
        need(!js_truthy(publication.get("closed")) && source_offset > 0, "PAIRED_PACKAGE_SOURCE_CLOSED")?;
        if initialize {
            let markets: Vec<V> = publication["scope"]["exchanges"].as_array().cloned().unwrap_or_default()
                .iter().filter(|x| ["381", "382"].contains(&x["marketId"].as_str().unwrap_or("")))
                .map(|x| {
                    let mut m = identity_entry(identities, x["marketId"].as_str().unwrap_or(""));
                    m.insert("marketId".into(), x["marketId"].clone());
                    m.insert("exchangeId".into(), x["exchangeId"].clone());
                    V::Object(m)
                }).collect();
            bank = json!({"schemaVersion":1,"mode":"sequential_dual_no_paper","cohort":cohort,
                "tournamentId":universe["tournamentId"],"universe":universe,"markets":markets,
                "cutoff":ms_to_iso((now)() as f64),"startEventId":0,
                "sourceScopeHash":publication["scopeHash"],"sourceSessionId":publication["sessionId"],
                "sourcePrefixBytes":source_offset,"sourcePrefixSequence":publication["sequence"],
                "sourcePrefixJournalHash":publication["hash"],
                "sourcePrefixHash":prefix_hash(&source_file, source_offset)?,
                "sourceFile":"public-books.jsonl","policy":paired_paper_policy_v3(),
                "variants":paired_paper_variants_v3(),"confirmationPolicy":paired_paper_confirmation_v3(),
                "codeIdentity":code_identity});
            need(!bank_file.exists() && !journal_file.exists(), "PAIRED_PACKAGE_COHORT_EXISTS")?;
        } else {
            need(json!(source_offset) == bank["sourcePrefixBytes"]
                && prefix_hash(&source_file, source_offset)? == bank["sourcePrefixHash"].as_str().unwrap_or("")
                && publication["sequence"] == bank["sourcePrefixSequence"]
                && publication["hash"] == bank["sourcePrefixJournalHash"]
                && publication["scopeHash"] == bank["sourceScopeHash"]
                && publication["sessionId"] == bank["sourceSessionId"]
                && paired_paper_hash_v3(&code_identity) == paired_paper_hash_v3(&bank["codeIdentity"]),
                "PAIRED_PACKAGE_FROZEN_IDENTITY")?;
        }
        let state = warm_paired_paper_v3(&create_paired_paper_v3(&bank)?);
        let _ = warm;
        let journal_hash = paired_paper_hash_v3(&bank);
        let mut w = PairedPackageWatchV3 {
            dir, bank, workspace_dir: workspace_dir.to_path_buf(),
            runtime_dir: runtime_dir.to_path_buf(), journal_file, source_file, lock_file: lock_file.clone(), bank_file,
            cohort: cohort.to_string(), read_only, publication, state, source_offset, ended: false,
            journal_sequence: 0, journal_hash, journal_bytes: 0,
            source_boundary: String::new(), bank_file_hash: String::new(), code_identity,
            source_ino, source_birth, lock: lock.take(), claim_ino, claim_birth,
            closed: false, failed: false, now, can_operate,
        };
        if !initialize {
            let input_rows = rows(&w.source_file, w.source_offset, None, true)?;
            let mut input_index = 0usize;
            for (saved_row, saved_offset) in rows(&w.journal_file, 0, None, false)? {
                let mut body = saved_row;
                let h = body.as_object_mut().and_then(|m| m.shift_remove("hash"));
                let body_hash = paired_paper_hash_v3(&body);
                need(h.as_ref().and_then(|x| x.as_str()) == Some(body_hash.as_str())
                    && js_int(&body["sequence"]) == Some(w.journal_sequence + 1)
                    && body["previous"].as_str() == Some(w.journal_hash.as_str())
                    && body["bankHash"].as_str() == Some(paired_paper_hash_v3(&w.bank).as_str()),
                    "PAIRED_PACKAGE_JOURNAL_CHAIN")?;
                if body["kind"].as_str() == Some("genesis") {
                    need(w.journal_sequence == 0
                        && body["stateHash"].as_str() == Some(paired_paper_hash_v3(&w.state).as_str()),
                        "PAIRED_PACKAGE_GENESIS")?;
                } else {
                    need(body["kind"].as_str() == Some("batch")
                        && js_int(&body["sourceSequence"]).unwrap_or(i64::MIN)
                            > js_int(&w.publication["sequence"]).unwrap_or(i64::MIN),
                        "PAIRED_PACKAGE_CHECKPOINT")?;
                    let mut pending = Vec::new();
                    while js_int(&w.publication["sequence"]).unwrap_or(i64::MIN) + (pending.len() as i64)
                        < js_int(&body["sourceSequence"]).unwrap_or(i64::MIN) {
                        need(input_index < input_rows.len(), "PAIRED_PACKAGE_SOURCE_TRUNCATED")?;
                        pending.push(input_rows[input_index].clone());
                        input_index += 1;
                    }
                    w.reduce_batch(&pending, body["availableAt"].as_str().unwrap_or(""),
                        js_truthy(body.get("allowDecisions")))?;
                    for (k, v) in w.expected().as_object().cloned().unwrap_or_default() {
                        need(paired_paper_hash_v3(&body.get(&k).cloned().unwrap_or(V::Null))
                            == paired_paper_hash_v3(&v), "PAIRED_PACKAGE_REPLAY_MISMATCH")?;
                    }
                }
                w.journal_sequence += 1;
                w.journal_hash = h.and_then(|x| x.as_str().map(String::from)).unwrap_or_default();
                w.journal_bytes = saved_offset;
            }
            need(w.journal_sequence > 0, "PAIRED_PACKAGE_GENESIS_MISSING")?;
        }
        if initialize {
            w.live()?;
            durable(&w.bank_file, format!("{}\n", serde_json::to_string_pretty(&w.bank).map_err(|e| e.to_string())?).as_bytes(), true)?;
            durable(&w.journal_file, &[], true)?;
            w.append(json!({"kind":"genesis","stateHash":paired_paper_hash_v3(&w.state)}))?;
        }
        w.bank_file_hash = sha256(&fs::read(&w.bank_file).map_err(|e| e.to_string())?);
        w.source_boundary = boundary(&w.source_file, w.source_offset)?;
        Ok(w)
    };
    match opened() {
        Ok(w) => Ok(w),
        Err(e) => {
            // JS: catch(e){try{close();}catch{}throw e}
            let ours = lock.is_some() && fs::read_to_string(&lock_file)
                .map(|s| s == std::process::id().to_string()).unwrap_or(false);
            drop(lock);
            if ours {
                if let Ok(s) = fs::metadata(&lock_file) {
                    let ino = { #[cfg(unix)] { use std::os::unix::fs::MetadataExt; s.ino() }
                        #[cfg(not(unix))] { 0 } };
                    if ino == claim_ino && s.created().ok() == claim_birth {
                        let _ = fs::remove_file(&lock_file);
                    }
                }
            }
            Err(e)
        }
    }
}
/// runPairedPackageWatchV3({runtimeDir,cohort,initialize,universe,identities,once,now,sleep,workspaceDir})
/// Signals map onto `can_operate`; sleep is a synchronous 250ms.
pub fn run_paired_package_watch_v3(runtime_dir: &Path, cohort: &str, initialize: bool, universe: &V,
    identities: &V, once: bool, now: Arc<dyn Fn() -> i64 + Send + Sync>, workspace_dir: &Path,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<(), String> {
    let mut watch = match open_paired_package_watch_v3(runtime_dir, cohort, initialize, false,
        universe, identities, now, workspace_dir, can_operate) {
        Ok(w) => w,
        Err(_) => return Err("PAIRED_PACKAGE_WORKER_FAILED_CLOSED".into()),
    };
    let publish = |watch: &PairedPackageWatchV3, status: &str| -> Result<(), String> {
        let mut report = watch.status()?;
        report["status"] = json!(status);
        if status != "running" {
            if let Some(wallets) = report["wallets"].as_object_mut() {
                for (_, w) in wallets.iter_mut() {
                    let Some(positions) = w["positions"].as_array() else {
                        return Err("Cannot read properties of undefined (reading 'length')".into());
                    };
                    let n = positions.len();
                    if n > 0 {
                        w["totalPnl"] = V::Null;
                        w["liquidationPnl"] = V::Null;
                    }
                    w["marksComplete"] = json!(n == 0);
                    for p in w["positions"].as_array_mut().unwrap() {
                        p["markReady"] = json!(false);
                        p["liquidationValue"] = V::Null;
                    }
                }
            }
        }
        let file = watch.dir.join("stats.json");
        let can_commit = || watch.owned() && (status != "running" || !watch.stopped());
        atomic_paper_json(&file, &report, &can_commit).map_err(|e| e.to_string())?;
        Ok(())
    };
    let outcome = (|| -> Result<(), String> {
        loop {
            if watch.stopped() || !watch.current() { break; }
            watch.consume()?;
            publish(&watch, if watch.current() { "running" } else { "source_session_ended" })?;
            if once { break; }
            std::thread::sleep(Duration::from_millis(250));
        }
        if watch.stopped() || !watch.current() {
            publish(&watch, if watch.stopped() { "stopped" } else { "source_session_ended" })?;
        }
        Ok(())
    })();
    if outcome.is_err() {
        let _ = publish(&watch, if watch.stopped() { "stopped" } else { "failed" });
        watch.close();
        return Err("PAIRED_PACKAGE_WORKER_FAILED_CLOSED".into());
    }
    watch.close();
    Ok(())
}
