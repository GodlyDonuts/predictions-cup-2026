// live-paper/entry-comparison-watch-v2.mjs — streaming successor host: reads only
// the 16MB source tail on open, replays through journalLines, verifies the bank
// bytes and checkpoint bindings, and gates on canOperate().
// (No CLI entry ported; no sibling .rs CLI pattern exists.)

use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::journal::{journal_lines, Journal, JournalReadOptions};
use baal_service_runtime::json_text::{js_int, sha256};
use crate::entry_comparison::{advance_entry_comparison, empty_entry_comparison_state,
    entry_comparison_report};
use crate::entry_comparison_watch::entry_comparison_code_identity;
use crate::model::digest;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

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
fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() && f.fract() == 0.0 { (f as i64).to_string() } else { n.to_string() }
        }
        V::Array(a) => a.iter().map(|x| match x { V::Null => String::new(), o => js_str(o) })
            .collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}
fn js_truthy(v: &V) -> bool {
    baal_service_runtime::json_text::js_truthy(Some(v))
}
fn ms_iso(ms: f64) -> Result<String, String> {
    if !ms.is_finite() { return Err("RangeError: Invalid time value".into()); }
    Ok(baal_service_runtime::contract::ms_to_iso(ms))
}
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn birthtime_ms(m: &fs::Metadata) -> f64 {
    m.created().map(|t| t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)).unwrap_or(f64::NAN)
}

pub struct EntryComparisonWatchV2 {
    pub dir: PathBuf,
    pub bank: V,
    state: V,
    journal: Journal,
    source_file: PathBuf,
    source_ino: u64,
    source_birthtime_ms: f64,
    bank_file: PathBuf,
    bank_bytes_hash: String,
    offset: usize,
    partial: Vec<u8>,
    processed_bytes: usize,
    failed: bool,
    compared: usize,
    head_len: usize,
    head_hash: String,
    boundary_len: usize,
    boundary_hash: String,
    code_identity: V,
    cohort: String,
    now: Box<dyn Fn() -> f64>,
    can_operate: Box<dyn Fn() -> bool>,
    workspace_dir: PathBuf,
}

impl EntryComparisonWatchV2 {
    fn guard(&self, at: usize, length: usize) -> Result<String, String> {
        let f = fs::File::open(&self.source_file).map_err(|e| e.to_string())?;
        let mut b = vec![0u8; length];
        let n = f.read_at(&mut b, at as u64).map_err(|e| e.to_string())?;
        need(n == length, "SOURCE_CHANGED")?;
        Ok(sha256(&b))
    }

    pub fn consume(&mut self) -> Result<usize, String> {
        need(!self.failed, "COMPARISON_FAILED_CLOSED")?;
        need((self.can_operate)(), "COMPARISON_STOPPED")?;
        match self.consume_inner() {
            Ok(n) => Ok(n),
            Err(e) => { self.failed = true; Err(e) }
        }
    }

    fn consume_inner(&mut self) -> Result<usize, String> {
        need(digest(&self.code_identity)
            == digest(&entry_comparison_code_identity(&self.workspace_dir)?),
            "FROZEN_COMPARISON_CODE_CHANGED")?;
        need(sha256(&fs::read(&self.bank_file).map_err(|e| e.to_string())?)
            == self.bank_bytes_hash, "FROZEN_COMPARISON_BANK_CHANGED")?;
        let stat = fs::metadata(&self.source_file).map_err(|e| e.to_string())?;
        need(stat.ino() == self.source_ino
            && birthtime_ms(&stat) == self.source_birthtime_ms
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
            need((self.can_operate)(), "COMPARISON_STOPPED")?;
            self.journal.commit(&next, &json!({"type": "source_advanced",
                "sourceSequence": next["sourceSequence"], "sourceHash": next["lastRecordHash"]}))?;
            self.state = next;
        }
        self.boundary_len = std::cmp::min(4096, self.offset);
        self.boundary_hash = self.guard(self.offset - self.boundary_len, self.boundary_len)?;
        Ok(count)
    }

    pub fn status(&self) -> Result<V, String> {
        let mut out = entry_comparison_report(&self.state, &self.bank, &ms_iso((self.now)())?)?;
        out["cohort"] = json!(self.cohort);
        out["codeIdentity"] = self.code_identity.clone();
        out["hostVersion"] = json!("entry-comparison-streaming-v2");
        out["verifiedOriginalCheckpoints"] = json!(self.compared);
        let max_mark_age = js_num(&self.bank["config"]["maxDataAgeMs"])
            + js_num(&self.bank["config"]["cycleIntervalMs"]) * 4.0;
        out["maxMarkAgeMs"] = if max_mark_age.is_finite() && max_mark_age.fract() == 0.0
            && max_mark_age.abs() < 9.007_199_254_740_992e15 { json!(max_mark_age as i64) }
            else { json!(max_mark_age) };
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

/// openEntryComparison({runtimeDir, cohort, initialize, now, codeIdentity, canOperate}).
pub fn open_entry_comparison_v2(runtime_dir: &Path, cohort: &str, initialize: bool,
    now: impl Fn() -> f64 + 'static, code_identity: V,
    can_operate: impl Fn() -> bool + 'static, workspace_dir: &Path)
    -> Result<EntryComparisonWatchV2, String> {
    need(runtime_dir.is_absolute() && name_ok(cohort), "INVALID_COMPARISON_DIRECTORY")?;
    need(!initialize, "STREAMING_SUCCESSOR_REQUIRES_EXISTING_FROZEN_BANK")?;
    need(can_operate(), "COMPARISON_STOPPED")?;
    let dir = runtime_dir.join("entry-comparisons").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let source_file = runtime_dir.join("journal.jsonl");
    let source_stat = fs::metadata(&source_file).map_err(|e| e.to_string())?;
    // Read the last 16MB tail only.
    let tail_length = std::cmp::min(source_stat.len() as usize, 16 * 1024 * 1024);
    let f = fs::File::open(&source_file).map_err(|e| e.to_string())?;
    let mut tail = vec![0u8; tail_length];
    let n = f.read_at(&mut tail, (source_stat.len() as usize - tail_length) as u64)
        .map_err(|e| e.to_string())?;
    need(n == tail_length, "SOURCE_READ_FAILED")?;
    let tail_end = tail.iter().rposition(|b| *b == b'\n')
        .ok_or("SOURCE_PREFIX_EMPTY_OR_OVERSIZED_TAIL")?;
    let end = source_stat.len() as usize - tail_length + tail_end;
    need(bank_file.exists() && journal_file.exists(), "COMPARISON_ACCOUNTING_MISSING")?;
    let bank = read_json(&bank_file)?;
    need(digest(&bank["codeIdentity"]) == digest(&code_identity), "FROZEN_COMPARISON_CODE_CHANGED")?;
    let mut state = empty_entry_comparison_state(&bank)?;
    let mut compared = 0usize;
    // Streaming saved-journal verification (journalLines iterator).
    let saved = journal_lines(&journal_file, &JournalReadOptions::default())?;
    let mut saved_iter = saved.iter();
    let mut saved_sequence = 0i64;
    let mut saved_hash = String::new();
    let mut last_saved_source = 0i64;
    let mut saved_clock = f64::NEG_INFINITY;
    let mut next_saved = move || -> Result<Option<V>, String> {
        let Some(line) = saved_iter.next() else { return Ok(None); };
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let h = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.shift_remove("hash"); }
        saved_sequence += 1;
        need(strict_eq(&row["sequence"], &json!(saved_sequence))
            && strict_eq(&row["previous"], &json!(saved_hash))
            && digest(&body) == h, "COMPARISON_JOURNAL_INVALID")?;
        let at_v = if js_truthy(&row["event"]["clockAt"]) { row["event"]["clockAt"].clone() }
            else { row["at"].clone() };
        let at = baal_service_runtime::contract::parse_iso_ms(at_v.as_str().unwrap_or(""))
            .unwrap_or(f64::NAN);
        need(at.is_finite() && at >= saved_clock, "COMPARISON_JOURNAL_CLOCK_REGRESSION")?;
        saved_clock = at;
        let seq = js_int(&row["state"]["sourceSequence"]);
        need(seq.is_some() && seq.unwrap_or(0) > last_saved_source, "COMPARISON_SOURCE_SEQUENCE_INVALID")?;
        need(strict_eq(&row["event"]["sourceSequence"], &row["state"]["sourceSequence"])
            && strict_eq(&row["event"]["sourceHash"], &row["state"]["lastRecordHash"]),
            "COMPARISON_CHECKPOINT_BINDING_INVALID")?;
        saved_hash = h;
        last_saved_source = seq.unwrap_or(0);
        Ok(Some(row))
    };
    let mut prefix = Sha256::new();
    let mut checkpoint = next_saved()?;
    need(checkpoint.is_some(), "COMPARISON_JOURNAL_EMPTY")?;
    let mut prefix_verified = false;
    for line in journal_lines(&source_file, &JournalReadOptions {
        end_byte: Some(end as u64 + 1), ..Default::default() })? {
        let record: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        prefix.update(format!("{line}\n").as_bytes());
        state = advance_entry_comparison(&state, &record, &bank)?;
        if strict_eq(&record["sequence"], &bank["sourceRecords"]) {
            need(format!("{:x}", prefix.clone().finalize()) == bank["sourceHash"].as_str().unwrap_or(""),
                "FROZEN_SOURCE_PREFIX_CHANGED")?;
            prefix_verified = true;
        }
        if checkpoint.as_ref().is_some_and(|c| strict_eq(&c["state"]["sourceSequence"], &record["sequence"])) {
            need(digest(&checkpoint.as_ref().unwrap()["state"]) == digest(&state),
                "COMPARISON_REPLAY_MISMATCH")?;
            compared += 1;
            checkpoint = next_saved()?;
        }
    }
    need(prefix_verified && checkpoint.is_none() && compared as i64 == saved_sequence,
        "COMPARISON_REPLAY_INCOMPLETE")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut journal = Journal::open(&journal_file)?;
    need(can_operate(), "COMPARISON_STOPPED")?;
    let stale = journal.last.as_ref().is_none_or(|l| !strict_eq(
        &l["state"]["sourceSequence"], &state["sourceSequence"]));
    if stale {
        journal.commit(&state, &json!({"type": "source_advanced",
            "sourceSequence": state["sourceSequence"], "sourceHash": state["lastRecordHash"],
            "cutoff": bank["cutoff"]}))?;
    }
    let offset = end + 1;
    let partial: Vec<u8> = Vec::new();
    let processed_bytes = offset;
    let guard_read = |at: usize, length: usize| -> Result<String, String> {
        let f = fs::File::open(&source_file).map_err(|e| e.to_string())?;
        let mut b = vec![0u8; length];
        let n = f.read_at(&mut b, at as u64).map_err(|e| e.to_string())?;
        need(n == length, "SOURCE_CHANGED")?;
        Ok(sha256(&b))
    };
    let head_len = std::cmp::min(4096, offset);
    let head_hash = guard_read(0, head_len)?;
    let boundary_len = std::cmp::min(4096, offset);
    let boundary_hash = guard_read(offset - boundary_len, boundary_len)?;
    let bank_bytes_hash = sha256(&fs::read(&bank_file).map_err(|e| e.to_string())?);
    Ok(EntryComparisonWatchV2 {
        dir, bank, state, journal, source_file,
        source_ino: source_stat.ino(), source_birthtime_ms: birthtime_ms(&source_stat),
        bank_file, bank_bytes_hash, offset, partial, processed_bytes, failed: false,
        compared, head_len, head_hash, boundary_len, boundary_hash,
        code_identity, cohort: cohort.to_string(), now: Box::new(now),
        can_operate: Box::new(can_operate), workspace_dir: workspace_dir.to_path_buf(),
    })
}

/// runEntryComparison({runtimeDir, cohort, initialize, once, now, workspaceDir}).
/// Returns Ok(V::Null) on the operator-stop-during-replay catch path.
pub fn run_entry_comparison_v2(runtime_dir: &Path, cohort: &str, initialize: bool, once: bool,
    now: impl Fn() -> f64 + 'static, workspace_dir: &Path) -> Result<V, String> {
    use std::rc::Rc;
    let now: Rc<dyn Fn() -> f64> = Rc::new(now);
    need(runtime_dir.is_absolute() && name_ok(cohort), "INVALID_COMPARISON_DIRECTORY")?;
    let dir = runtime_dir.join("entry-comparisons").join(cohort);
    let stop_files: Vec<PathBuf> = vec![workspace_dir.join("STOP"), runtime_dir.join("STOP"),
        runtime_dir.join("ENTRY_COMPARISON_STOP"), dir.join("STOP")];
    let stopped = || stop_files.iter().any(|p| p.exists());
    need(!stopped(), "STOP_EXISTS")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _release = crate::paper_runtime::lock(&dir.join("worker.lock"))?;
    let code_identity = entry_comparison_code_identity(workspace_dir)?;
    let stats_file = dir.join("stats.json");
    let publish = |watch: Option<&EntryComparisonWatchV2>, status: &str| -> Result<(), String> {
        let mut body = watch.map(|w| w.status()).transpose()?.unwrap_or_else(|| json!({}));
        body["status"] = json!(status);
        body["pid"] = json!(std::process::id());
        body["updatedAt"] = json!(ms_iso(now())?);
        atomic_json_canonical(&stats_file, &body)
    };
    let mut watch: Option<EntryComparisonWatchV2> = None;
    let result: Result<V, String> = (|| {
        watch = Some(open_entry_comparison_v2(runtime_dir, cohort, initialize,
            { let n = now.clone(); move || n() }, code_identity.clone(),
            { let files = stop_files.clone(); move || !files.iter().any(|p| p.exists()) },
            workspace_dir)?);
        let w = watch.as_mut().unwrap();
        loop {
            if stopped() { break; }
            w.consume()?;
            let st = w.status()?;
            let status = if js_truthy(&st["incompleteTailBytes"]) { "awaiting_capture" } else { "running" };
            let mut body = st;
            body["status"] = json!(status);
            body["pid"] = json!(std::process::id());
            body["updatedAt"] = json!(ms_iso(now())?);
            atomic_json_canonical(&stats_file, &body)?;
            if once { break; }
            std::thread::sleep(std::time::Duration::from_millis(2000));
            if stopped() { break; }
        }
        if stopped() { publish(Some(&*w), "stopped")?; }
        w.status()
    })();
    match result {
        Ok(v) => Ok(v),
        Err(_) => {
            // catch: operator stop while replaying/opening resolves quietly.
            if stopped() {
                if let Some(w) = &watch { publish(Some(w), "stopped")?; }
                else {
                    atomic_json_canonical(&stats_file, &json!({
                        "status": "stopped", "mode": "entry_comparison_paper",
                        "updatedAt": ms_iso(now())?, "reason": "OPERATOR_STOP_DURING_REPLAY",
                        "networkRequests": 0, "competitionOrders": 0}))?;
                }
                return Ok(V::Null);
            }
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
