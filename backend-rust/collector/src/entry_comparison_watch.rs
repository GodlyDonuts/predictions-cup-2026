// live-paper/entry-comparison-watch.mjs — worker hosting entry-comparison on the
// collector source journal; frozen code identity guards replay parity.
// Append-only local files are the trust boundary: head/boundary guards detect
// replacement and ordinary rewrites; arbitrary edits to an old middle prefix
// during an append require the full verification performed on restart.

use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, sha256};
use crate::entry_comparison::{advance_entry_comparison, create_entry_comparison,
    empty_entry_comparison_state, entry_comparison_report};
use crate::ml_shadow::verify_source;
use crate::model::digest;
use serde_json::{json, Map, Value as V};
use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

/// The repo-relative source files whose sha256 makes up the frozen identity.
const IDENTITY_FILES: [&str; 14] = [
    "live-paper/entry-comparison-watch.mjs", "live-paper/entry-comparison.mjs",
    "live-paper/model.mjs", "live-paper/ml-shadow.mjs", "live-paper/dip-shadow.mjs",
    "live-paper/dip-performance.mjs", "live-paper/history.mjs", "live-paper/breadth.mjs",
    "live-paper/shock-rebound.mjs", "live-paper/runtime.mjs", "live-paper/network-budget.mjs",
    "execution/ledger.mjs", "paper/fixed.mjs", "feed/contract.mjs"];

fn need(ok: bool, message: &str) -> Result<(), String> {
    if !ok { return Err(message.to_string()); }
    Ok(())
}
fn name_ok(x: &str) -> bool {
    !x.is_empty() && x.len() <= 64 && x.starts_with(|c: char| c.is_ascii_lowercase())
        && x.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !["con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5",
            "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5",
            "lpt6", "lpt7", "lpt8", "lpt9"].contains(&x)
}
/// /^[a-z][a-z0-9-]{0,63}$/ — leading lowercase + up to 63 more.
fn cohort_name_ok(x: &str) -> bool {
    name_ok(x)
}
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::Bool(b) => if *b { 1.0 } else { 0.0 },
        V::Null => 0.0,
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    }
}

/// entryComparisonCodeIdentity() — sha256 per source file, keyed by the
/// repo-relative path. `workspace` is the parent directory of live-paper/.
pub fn entry_comparison_code_identity(workspace: &Path) -> Result<V, String> {
    let mut out = Map::new();
    for f in IDENTITY_FILES {
        let bytes = fs::read(workspace.join(f)).map_err(|e| e.to_string())?;
        out.insert(f.to_string(), json!(sha256(&bytes)));
    }
    Ok(V::Object(out))
}

fn create_bank_file(file: &Path, bank: &V) -> Result<(), String> {
    let mut fd = fs::OpenOptions::new().write(true).create_new(true).open(file)
        .map_err(|e| e.to_string())?;
    let bytes = format!("{}\n", serde_json::to_string_pretty(bank).map_err(|e| e.to_string())?)
        .into_bytes();
    let mut offset = 0;
    while offset < bytes.len() {
        let n = fd.write(&bytes[offset..]).map_err(|e| e.to_string())?;
        need(n > 0, "BANK_WRITE_FAILED")?;
        offset += n;
    }
    fd.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub struct EntryComparisonWatch {
    pub dir: PathBuf,
    pub bank: V,
    state: V,
    journal: Journal,
    source_file: PathBuf,
    source_ino: u64,
    source_birthtime_ms: f64,
    offset: usize,
    partial: Vec<u8>,
    processed_bytes: usize,
    failed: bool,
    head_hash: String,
    head_len: usize,
    boundary_len: usize,
    boundary_hash: String,
    code_identity: V,
    cohort: String,
    now: Box<dyn Fn() -> f64>,
    workspace_dir: PathBuf,
}

impl EntryComparisonWatch {
    fn guard(&self, at: usize, length: usize) -> Result<String, String> {
        let f = fs::File::open(&self.source_file).map_err(|e| e.to_string())?;
        let mut b = vec![0u8; length];
        let n = f.read_at(&mut b, at as u64).map_err(|e| e.to_string())?;
        need(n == length, "SOURCE_CHANGED")?;
        Ok(sha256(&b))
    }

    pub fn consume(&mut self) -> Result<usize, String> {
        need(!self.failed, "COMPARISON_FAILED_CLOSED")?;
        match self.consume_inner() {
            Ok(n) => Ok(n),
            Err(e) => { self.failed = true; Err(e) }
        }
    }

    fn consume_inner(&mut self) -> Result<usize, String> {
        need(digest(&self.code_identity)
            == digest(&entry_comparison_code_identity(&self.workspace_dir)?),
            "FROZEN_COMPARISON_CODE_CHANGED")?;
        let stat = fs::metadata(&self.source_file).map_err(|e| e.to_string())?;
        need(stat.ino() == self.source_ino
            && stat.created().map(|t| t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)).unwrap_or(f64::NAN)
                == self.source_birthtime_ms
            && stat.len() as usize >= self.offset, "SOURCE_REPLACED_OR_TRUNCATED")?;
        need(self.guard(0, self.head_len)? == self.head_hash
            && self.guard(self.offset - self.boundary_len, self.boundary_len)? == self.boundary_hash,
            "SOURCE_CHANGED")?;
        if stat.len() as usize == self.offset { return Ok(0); }
        let take = std::cmp::min(stat.len() as usize - self.offset, 4 * 1024 * 1024);
        let f = fs::File::open(&self.source_file).map_err(|e| e.to_string())?;
        let mut bytes = vec![0u8; take];
        let n = f.read_at(&mut bytes, self.offset as u64).map_err(|e| e.to_string())?;
        need(n > 0, "SOURCE_READ_FAILED")?;
        self.offset += n;
        let mut combined = self.partial.clone();
        combined.extend_from_slice(&bytes[..n]);
        let last = combined.iter().rposition(|b| *b == b'\n');
        let mut count = 0usize;
        let mut next = self.state.clone();
        if let Some(last) = last {
            for line in String::from_utf8_lossy(&combined[..last]).split('\n') {
                need(!line.is_empty(), "SOURCE_EMPTY_RECORD")?;
                let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
                next = advance_entry_comparison(&next, &row, &self.bank)?;
                count += 1;
            }
            self.processed_bytes += last + 1;
            self.partial = combined[last + 1..].to_vec();
        } else {
            self.partial = combined;
        }
        need(self.partial.len() <= 8 * 1024 * 1024, "SOURCE_OVERSIZED_RECORD")?;
        if count > 0 {
            self.journal.commit(&next, &json!({"type": "source_advanced",
                "sourceSequence": next["sourceSequence"], "sourceHash": next["lastRecordHash"]}))?;
            self.state = next;
        }
        self.boundary_len = std::cmp::min(4096, self.offset);
        self.boundary_hash = self.guard(self.offset - self.boundary_len, self.boundary_len)?;
        Ok(count)
    }

    pub fn status(&self) -> Result<V, String> {
        let at = ms_iso((self.now)())?;
        let mut out = entry_comparison_report(&self.state, &self.bank, &at)?;
        out["cohort"] = json!(self.cohort);
        out["codeIdentity"] = self.code_identity.clone();
        out["maxMarkAgeMs"] = json!(js_num(&self.bank["config"]["maxDataAgeMs"])
            + js_num(&self.bank["config"]["cycleIntervalMs"]) * 4.0);
        out["sourceBytesConsumed"] = json!(self.processed_bytes);
        out["sourceBytesRead"] = json!(self.offset);
        out["incompleteTailBytes"] = json!(self.partial.len());
        out["journalSequence"] = json!(self.journal.sequence);
        out["journalHash"] = json!(self.journal.previous);
        out["networkRequests"] = json!(0);
        out["competitionOrders"] = json!(0);
        Ok(out)
    }

    pub fn source_state(&self) -> V { self.state.clone() }
}

fn ms_iso(ms: f64) -> Result<String, String> {
    if !ms.is_finite() { return Err("RangeError: Invalid time value".into()); }
    Ok(baal_service_runtime::contract::ms_to_iso(ms))
}

/// openEntryComparison({runtimeDir, cohort, initialize, now, codeIdentity}).
/// `code_identity` = entry_comparison_code_identity(workspace_dir).
pub fn open_entry_comparison(runtime_dir: &Path, cohort: &str, initialize: bool,
    now: impl Fn() -> f64 + 'static, code_identity: V, workspace_dir: &Path)
    -> Result<EntryComparisonWatch, String> {
    need(runtime_dir.is_absolute() && cohort_name_ok(cohort), "INVALID_COMPARISON_DIRECTORY")?;
    let dir = runtime_dir.join("entry-comparisons").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let source_file = runtime_dir.join("journal.jsonl");
    let source_stat = fs::metadata(&source_file).map_err(|e| e.to_string())?;
    let source_bytes = fs::read(&source_file).map_err(|e| e.to_string())?;
    let end = source_bytes.iter().rposition(|b| *b == b'\n');
    let end = end.ok_or("SOURCE_PREFIX_EMPTY")?;
    let complete = &source_bytes[..end + 1];
    let records = verify_source(&String::from_utf8_lossy(complete).into_owned())?;
    let records = records.filter(|r| !r.records.is_empty());
    need(records.is_some(), "SOURCE_PREFIX_INVALID")?;
    let records = records.unwrap();
    let bank: V;
    if initialize {
        need(!bank_file.exists() && !journal_file.exists(), "FROZEN_COMPARISON_ALREADY_EXISTS")?;
        let base = read_json(&runtime_dir.join("dip-responsive-bank.json"))?;
        let cutoff = ms_iso(now())?;
        let last = &records.records[records.records.len() - 1];
        let last_at = if js_truthy_compat(&last["event"]["clockAt"]) { last["event"]["clockAt"].clone() }
            else { last["at"].clone() };
        need(baal_service_runtime::contract::parse_iso_ms(last_at.as_str().unwrap_or(""))
            .unwrap_or(f64::NAN) <= now(), "FUTURE_SOURCE_PREFIX")?;
        let mut config = base["config"].clone();
        config["marketIds"] = json!(["377", "354", "277", "305"]);
        bank = create_entry_comparison(&json!({
            "sourceRecords": records.records.len(), "sourceHash": records.prefix_hashes
                .last().map(|h| json!(h)).unwrap_or(V::Null), "cutoff": cutoff,
            "config": config, "universe": base["universe"], "identities": base["identities"],
            "policy": base["policy"], "codeIdentity": code_identity}))?["bank"].clone();
    } else {
        need(bank_file.exists() && journal_file.exists(), "COMPARISON_ACCOUNTING_MISSING")?;
        bank = read_json(&bank_file)?;
    }
    need(digest(&bank["codeIdentity"]) == digest(&code_identity), "FROZEN_COMPARISON_CODE_CHANGED")?;
    let source_records = js_int(&bank["sourceRecords"]).unwrap_or(0);
    need(source_records >= 1
        && (source_records as usize) <= records.prefix_hashes.len()
        && records.prefix_hashes[source_records as usize - 1] == bank["sourceHash"].as_str().unwrap_or(""),
        "FROZEN_SOURCE_PREFIX_CHANGED")?;
    let mut state = empty_entry_comparison_state(&bank)?;
    let mut saved: Vec<V> = Vec::new();
    if !initialize {
        let raw = fs::read_to_string(&journal_file).map_err(|e| e.to_string())?;
        need(!raw.is_empty() && raw.ends_with('\n'), "COMPARISON_JOURNAL_INCOMPLETE")?;
        saved = verify_source(&raw)?.map(|v| v.records).unwrap_or_default();
        need(!saved.is_empty(), "COMPARISON_JOURNAL_INVALID")?;
        let mut prior = 0i64;
        for row in &saved {
            let seq = js_int(&row["state"]["sourceSequence"]);
            need(seq.is_some() && seq.unwrap_or(0) > prior
                && seq.unwrap_or(0) as usize <= records.records.len(), "COMPARISON_SOURCE_SEQUENCE_INVALID")?;
            prior = seq.unwrap_or(0);
        }
    }
    let mut compared = 0usize;
    for record in &records.records {
        state = advance_entry_comparison(&state, record, &bank)?;
        if strict_eq(&saved.get(compared).map(|r| &r["state"]["sourceSequence"]).unwrap_or(&V::Null),
            &record["sequence"]) {
            need(digest(&saved[compared]["state"]) == digest(&state), "COMPARISON_REPLAY_MISMATCH")?;
            compared += 1;
        }
    }
    need(compared == saved.len(), "COMPARISON_REPLAY_INCOMPLETE")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if initialize { create_bank_file(&bank_file, &bank)?; }
    let mut journal = Journal::open(&journal_file)?;
    let stale = journal.last.as_ref().is_none_or(|l| !strict_eq(
        &l["state"]["sourceSequence"], &state["sourceSequence"]));
    if stale {
        journal.commit(&state, &json!({"type": if initialize { "cohort_frozen" } else { "source_advanced" },
            "sourceSequence": state["sourceSequence"], "sourceHash": state["lastRecordHash"],
            "cutoff": bank["cutoff"]}))?;
    }
    let offset = end + 1;
    let head_len = std::cmp::min(4096, complete.len());
    let head_hash = sha256(&complete[..head_len]);
    let boundary_len = std::cmp::min(4096, offset);
    let boundary_hash = sha256(&complete[offset - boundary_len..]);
    let source_birthtime = source_stat.created().map(|t| t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)).unwrap_or(f64::NAN);
    Ok(EntryComparisonWatch {
        dir, bank, state, journal, source_file,
        source_ino: source_stat.ino(), source_birthtime_ms: source_birthtime,
        offset, partial: Vec::new(), processed_bytes: offset, failed: false,
        head_hash, head_len, boundary_len, boundary_hash,
        code_identity, cohort: cohort.to_string(), now: Box::new(now),
        workspace_dir: workspace_dir.to_path_buf(),
    })
}

fn js_truthy_compat(v: &V) -> bool {
    baal_service_runtime::json_text::js_truthy(Some(v))
}

/// runEntryComparison({runtimeDir, cohort, initialize, once, now, workspaceDir}).
/// Synchronous; SIGINT/SIGTERM are the caller's concern (sibling convention).
pub fn run_entry_comparison(runtime_dir: &Path, cohort: &str, initialize: bool, once: bool,
    now: impl Fn() -> f64 + 'static, workspace_dir: &Path) -> Result<V, String> {
    use std::rc::Rc;
    let now: Rc<dyn Fn() -> f64> = Rc::new(now);
    need(runtime_dir.is_absolute() && cohort_name_ok(cohort), "INVALID_COMPARISON_DIRECTORY")?;
    let dir = runtime_dir.join("entry-comparisons").join(cohort);
    let stopped = |dir: &Path| [workspace_dir.join("STOP"), runtime_dir.join("STOP"),
        runtime_dir.join("ENTRY_COMPARISON_STOP"), dir.join("STOP")].iter().any(|p| p.exists());
    need(!stopped(&dir), "STOP_EXISTS")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _release = crate::paper_runtime::lock(&dir.join("worker.lock"))?;
    let code_identity = entry_comparison_code_identity(workspace_dir)?;
    let stats_file = dir.join("stats.json");
    let publish = |watch: Option<&EntryComparisonWatch>, status: &str| -> Result<(), String> {
        // JS: atomicJson(stats.json, {...watch.status(), status, pid, updatedAt})
        let mut body = watch.map(|w| w.status()).transpose()?.unwrap_or_else(|| json!({}));
        body["status"] = json!(status);
        body["pid"] = json!(std::process::id());
        body["updatedAt"] = json!(ms_iso(now())?);
        atomic_json_canonical(&stats_file, &body)
    };
    // JS `let watch` outside try: on open failure the catch publishes minimal stats.
    let mut watch: Option<EntryComparisonWatch> = None;
    let result: Result<V, String> = (|| {
        // JS binds watch as soon as open succeeds; failures after that still
        // publish('failed') through it in the catch.
        watch = Some(open_entry_comparison(runtime_dir, cohort, initialize,
            { let n = now.clone(); move || n() }, code_identity.clone(), workspace_dir)?);
        let w = watch.as_mut().unwrap();
        loop {
            if stopped(&dir) { break; }
            w.consume()?;
            let st = w.status()?;
            let status = if baal_service_runtime::json_text::js_truthy(Some(&st["incompleteTailBytes"])) {
                "awaiting_capture" } else { "running" };
            // publish(status) — status() spread plus status/pid/updatedAt.
            let mut body = st;
            body["status"] = json!(status);
            body["pid"] = json!(std::process::id());
            body["updatedAt"] = json!(ms_iso(now())?);
            atomic_json_canonical(&stats_file, &body)?;
            if once { break; }
            std::thread::sleep(std::time::Duration::from_millis(2000));
            if stopped(&dir) { break; }
        }
        if stopped(&dir) { publish(Some(w), "stopped")?; }
        w.status()
    })();
    match result {
        Ok(v) => Ok(v),
        Err(_) => {
            // JS catch: publish('failed') via the live watch when present, else the
            // minimal stats document; then rethrow the fixed message.
            if let Some(w) = &watch { publish(Some(w), "failed")?; }
            else {
                atomic_json_canonical(&stats_file, &json!({
                    "status": "failed", "mode": "entry_comparison_paper",
                    "updatedAt": ms_iso(now())?, "reason": "COMPARISON_FAILED_CLOSED",
                    "networkRequests": 0, "competitionOrders": 0}))?;
            }
            Err("Entry comparison failed; inspect its local evidence".into())
        }
    }
}
