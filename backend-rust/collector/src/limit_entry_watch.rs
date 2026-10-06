// live-paper/limit-entry-watch.mjs — limit-entry cohort host: locked single-writer
// loop over the shared verified capture source with per-cohort journals.
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const CHUNK: usize = 1024 * 1024;
const MAX_RECORD: usize = 16 * 1024 * 1024;
fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.to_string()); }
    Ok(())
}
fn name_ok(x: &str) -> bool {
    let b = x.as_bytes();
    !x.is_empty() && x.len() <= 64 && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && !["con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
            "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9"]
            .contains(&x)
}
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn write_all(fd: &mut std::fs::File, bytes: &[u8]) -> Result<(), String> {
    fd.write_all(bytes).map_err(|_| "DURABLE_WRITE_FAILED".to_string())?;
    fd.sync_all().map_err(|e| e.to_string())
}
fn create_file(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    o.mode(0o600);
    let mut fd = o.open(file).map_err(|e| e.to_string())?;
    write_all(&mut fd, bytes)
}
/// limitEntryCodeIdentity() — sha256 of every file in this host's local import
/// closure, sorted by workspace-relative path.
pub fn limit_entry_code_identity(workspace_dir: &Path) -> Result<V, String> {
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    fn visit(file: &Path, workspace_dir: &Path, found: &mut BTreeMap<String, String>) -> Result<(), String> {
        let rel = file.strip_prefix(workspace_dir)
            .map_err(|_| "CODE_OUTSIDE_WORKSPACE".to_string())?
            .to_string_lossy().replace('\\', "/");
        need(!rel.is_empty() && !rel.starts_with("../") && !Path::new(&rel).is_absolute(),
            "CODE_OUTSIDE_WORKSPACE")?;
        if found.contains_key(&rel) { return Ok(()); }
        let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
        found.insert(rel, sha256(&bytes));
        let source = String::from_utf8_lossy(&bytes);
        // import/export ... from 'specifier' / import 'specifier'
        for line_spec in import_specifiers(&source) {
            if line_spec.starts_with('.') {
                let resolved = file.parent().unwrap_or(workspace_dir).join(&line_spec);
                let resolved = normalize_path(&resolved);
                visit(&resolved, workspace_dir, found)?;
            }
        }
        Ok(())
    }
    visit(&workspace_dir.join("live-paper/limit-entry-watch.mjs"), workspace_dir, &mut found)?;
    let mut m = serde_json::Map::new();
    for (k, v) in found { m.insert(k, json!(v)); }
    Ok(V::Object(m))
}
fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => { out.pop(); }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
fn import_specifiers(source: &str) -> Vec<String> {
    // Mirrors /(?:import|export)\s+(?:[^'";]+?\s+from\s+)?['"]([^'"]+)['"]/g:
    // after the keyword + whitespace, either a quote immediately (side-effect
    // import) or non-quote/semicolon text ending in "<ws>from<ws>" then a quote.
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let mut out = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let matched = ["import", "export"].iter().find(|w| {
            let w: Vec<char> = w.chars().collect();
            i + w.len() <= n && chars[i..i + w.len()] == w[..]
                && (i == 0 || !ident(chars[i - 1]))
                && (i + w.len() >= n || !ident(chars[i + w.len()]))
        });
        let Some(word) = matched else { i += 1; continue; };
        let wlen = word.chars().count();
        let mut j = i + wlen;
        if j >= n || !chars[j].is_whitespace() { i += wlen; continue; }
        while j < n && chars[j].is_whitespace() { j += 1; }
        // first of ' " ; after the whitespace
        let mut r = j;
        while r < n && chars[r] != '\'' && chars[r] != '"' && chars[r] != ';' { r += 1; }
        if r >= n || chars[r] == ';' { i = j.max(i + 1); continue; }
        let prefix: String = chars[j..r].iter().collect();
        let t = prefix.trim_end();
        let via_from = t.ends_with("from")
            && t[..t.len() - 4].len() >= 1
            && t[..t.len() - 4].ends_with(|c: char| c.is_whitespace());
        if !prefix.is_empty() && !via_from { i = j.max(i + 1); continue; }
        let quote = chars[r];
        let mut k = r + 1;
        while k < n && chars[k] != quote && chars[k] != '\'' && chars[k] != '"' { k += 1; }
        // regex needs [^'"]+ (>=1 char) and a closing quote
        if k >= n || k == r + 1 { i = j.max(i + 1); continue; }
        out.push(chars[r + 1..k].iter().collect::<String>());
        i = k + 1;
    }
    out
}
pub struct ScanRowMeta { pub prefix_hash: String, pub complete_bytes: usize }
pub struct ScanResult {
    pub sequence: i64, pub hash: String, pub prefix_hash: String,
    pub complete_bytes: usize, pub last_at: f64, pub partial_bytes: usize,
}
/// scan(file, {size, allowPartial, onRow}) — chunked verified chain reader.
fn scan<F: FnMut(&V, &ScanRowMeta) -> Result<(), String>>(file: &Path, size: Option<u64>,
    allow_partial: bool, mut on_row: F) -> Result<ScanResult, String> {
    let size = match size {
        Some(s) => s,
        None => std::fs::metadata(file).map_err(|e| e.to_string())?.len(),
    };
    let mut fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut prefix = sha2::Sha256::new();
    use sha2::Digest;
    let mut offset = 0u64;
    let mut complete_bytes = 0usize;
    let mut partial: Vec<u8> = Vec::new();
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut last_at = f64::NEG_INFINITY;
    while offset < size {
        let want = CHUNK.min((size - offset) as usize);
        let mut b = vec![0u8; want];
        fd.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let n = fd.read(&mut b).map_err(|e| e.to_string())?;
        need(n > 0, "SOURCE_READ_FAILED")?;
        offset += n as u64;
        let mut all = partial.clone();
        all.extend_from_slice(&b[..n]);
        let mut start = 0usize;
        while let Some(rel) = all[start..].iter().position(|x| *x == b'\n') {
            let end = start + rel;
            let raw = &all[start..=end];
            need(raw.len() > 1 && raw.len() <= MAX_RECORD, "INVALID_RECORD_SIZE")?;
            let row: V = serde_json::from_slice(&raw[..raw.len() - 1]).map_err(|e| e.to_string())?;
            let mut body = row.clone();
            let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
            let at = baal_service_runtime::contract::parse_iso_ms(body["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
            need(hash.as_str() == Some(crate::model::digest(&body).as_str())
                && body["sequence"].as_i64() == Some(sequence + 1)
                && body["previous"].as_str() == Some(previous.as_str())
                && at.is_finite() && at >= last_at, "SOURCE_CHAIN_INVALID")?;
            prefix.update(raw);
            complete_bytes += raw.len();
            sequence += 1;
            previous = hash.as_str().unwrap_or("").to_string();
            last_at = at;
            let meta = ScanRowMeta {
                prefix_hash: format!("{:x}", prefix.clone().finalize()),
                complete_bytes,
            };
            on_row(&row, &meta)?;
            start = end + 1;
        }
        partial = all[start..].to_vec();
        need(partial.len() <= MAX_RECORD, "SOURCE_OVERSIZED_RECORD")?;
    }
    need(allow_partial || partial.is_empty(), "JOURNAL_INCOMPLETE")?;
    Ok(ScanResult {
        sequence, hash: previous, prefix_hash: format!("{:x}", prefix.finalize()),
        complete_bytes, last_at, partial_bytes: partial.len(),
    })
}
fn record_availability(list: &mut Vec<V>, sequence: i64, at: &str) {
    let last_at = list.last().and_then(|l| l["at"].as_str().map(String::from));
    let last_through = list.last().and_then(|l| l["through"].as_i64());
    if last_at.as_deref() == Some(at) && last_through == Some(sequence - 1) {
        if let Some(l) = list.last_mut() { l["through"] = json!(sequence); }
    } else {
        list.push(json!({"from": sequence, "through": sequence, "at": at}));
    }
}
struct SourceStat { ino: u64, birthtime_ms: f64, size: u64 }
fn stat_of(file: &Path) -> Result<SourceStat, String> {
    let m = std::fs::metadata(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    return Ok(SourceStat { ino: m.ino(), birthtime_ms: m.created().map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs_f64() * 1000.0,
        size: m.len() });
    #[cfg(not(unix))]
    return Ok(SourceStat { ino: 0, birthtime_ms: 0.0, size: m.len() });
}
/// openLimitEntry({runtimeDir, cohort, initialize, now, workspaceDir, canOperate})
pub struct LimitEntryHost {
    pub dir: PathBuf,
    pub bank: V,
    state: V,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    cohort: String,
    code_identity: V,
    journal_sequence: i64,
    journal_hash: String,
    processed_bytes: usize,
    offset: u64,
    partial: Vec<u8>,
    last_source_at: f64,
    last_available: f64,
    bank_bytes_hash: String,
    head_length: u64,
    head_hash: String,
    boundary_length: u64,
    boundary_hash: String,
    source_stat: SourceStat,
    lock_file: PathBuf,
    closed: bool,
    failed: bool,
    now: Box<dyn Fn() -> f64>,
    can_operate: Box<dyn Fn() -> bool>,
}
impl LimitEntryHost {
    fn live(&self) -> Result<(), String> {
        need(self.owned() && !self.stopped(), "LIMIT_ENTRY_STOPPED")
    }
    pub fn owned(&self) -> bool {
        std::fs::read_to_string(&self.lock_file)
            .map(|s| s == std::process::id().to_string()).unwrap_or(false)
            && !self.closed
    }
    pub fn stopped(&self) -> bool {
        if !(self.can_operate)() { return true; }
        [
            self.workspace_dir.join("STOP"),
            self.runtime_dir.join("STOP"),
            self.runtime_dir.join("LIMIT_ENTRY_STOP"),
            self.dir.join("STOP"),
        ].iter().any(|p| p.exists())
    }
    fn journal_file(&self) -> PathBuf { self.dir.join("journal.jsonl") }
    fn source_file(&self) -> PathBuf { self.runtime_dir.join("journal.jsonl") }
    fn guard(&self, at: u64, length: u64) -> Result<String, String> {
        let mut fd = std::fs::File::open(self.source_file()).map_err(|e| e.to_string())?;
        let mut bytes = vec![0u8; length as usize];
        fd.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        fd.read_exact(&mut bytes).map_err(|_| "SOURCE_CHANGED".to_string())?;
        Ok(sha256(&bytes))
    }
    fn append(&mut self, next: &V, type_: &str, available: &[V]) -> Result<(), String> {
        self.live()?;
        let at = baal_service_runtime::contract::ms_to_iso((self.now)());
        need(baal_service_runtime::contract::parse_iso_ms(&at).unwrap_or(f64::NAN) >= self.last_available,
            "HOST_CLOCK_ROLLBACK")?;
        let mut body = json!({"sequence":self.journal_sequence+1,"previous":self.journal_hash,"at":at,
            "event":{"type":type_,"sourceSequence":next["sourceSequence"],"sourceHash":next["lastRecordHash"],
                "availability":available},"state":next});
        let hash = crate::model::digest(&body);
        body["hash"] = json!(hash);
        let mut fd = OpenOptions::new().append(true).open(self.journal_file()).map_err(|e| e.to_string())?;
        write_all(&mut fd, format!("{}\n", js_stringify(&body)).as_bytes())?;
        self.journal_sequence += 1;
        self.journal_hash = hash;
        Ok(())
    }
    /// consume() — number of source records consumed this pass.
    pub fn consume(&mut self) -> Result<usize, String> {
        need(!self.failed && !self.closed, "LIMIT_ENTRY_FAILED_CLOSED")?;
        let r = (|h: &mut Self| -> Result<usize, String> {
            h.live()?;
            need(crate::model::digest(&h.code_identity)
                == crate::model::digest(&limit_entry_code_identity(&h.workspace_dir)?), "FROZEN_LIMIT_ENTRY_CODE_CHANGED")?;
            need(sha256(&std::fs::read(h.dir.join("bank.json")).map_err(|e| e.to_string())?) == h.bank_bytes_hash,
                "FROZEN_LIMIT_ENTRY_CODE_CHANGED")?;
            let stat = stat_of(&h.source_file())?;
            need(stat.ino == h.source_stat.ino && stat.birthtime_ms == h.source_stat.birthtime_ms
                && stat.size >= h.offset, "SOURCE_REPLACED_OR_TRUNCATED")?;
            need(h.guard(0, h.head_length)? == h.head_hash
                && h.guard(h.offset - h.boundary_length, h.boundary_length)? == h.boundary_hash,
                "SOURCE_CHANGED")?;
            if stat.size == h.offset { return Ok(0); }
            let mut fd = std::fs::File::open(h.source_file()).map_err(|e| e.to_string())?;
            let want = CHUNK.min((stat.size - h.offset) as usize);
            let mut bytes = vec![0u8; want];
            fd.seek(SeekFrom::Start(h.offset)).map_err(|e| e.to_string())?;
            let n = fd.read(&mut bytes).map_err(|e| e.to_string())?;
            need(n > 0, "SOURCE_READ_FAILED")?;
            let mut combined = h.partial.clone();
            combined.extend_from_slice(&bytes[..n]);
            let mut available: Vec<V> = Vec::new();
            let mut count = 0usize;
            let mut start = 0usize;
            let mut next = h.state.clone();
            let mut next_source_at = h.last_source_at;
            while let Some(rel) = combined[start..].iter().position(|x| *x == b'\n') {
                let end = start + rel;
                need(end + 1 - start > 1 && end + 1 - start <= MAX_RECORD, "INVALID_RECORD_SIZE")?;
                let row: V = serde_json::from_slice(&combined[start..end]).map_err(|e| e.to_string())?;
                let available_at = baal_service_runtime::contract::ms_to_iso((h.now)());
                let source_at = baal_service_runtime::contract::parse_iso_ms(row["at"].as_str().unwrap_or(""))
                    .unwrap_or(f64::NAN);
                need(source_at.is_finite() && source_at >= next_source_at, "SOURCE_CLOCK_ROLLBACK")?;
                next_source_at = source_at;
                need(baal_service_runtime::contract::parse_iso_ms(&available_at).unwrap_or(f64::NAN)
                    >= h.last_available, "HOST_CLOCK_ROLLBACK")?;
                h.last_available = baal_service_runtime::contract::parse_iso_ms(&available_at).unwrap_or(f64::NAN);
                next = crate::limit_entry::advance_limit_entry(&next, &row, &h.bank, Some(&available_at))?;
                record_availability(&mut available, row["sequence"].as_i64().unwrap_or(0), &available_at);
                count += 1;
                start = end + 1;
            }
            let pending = combined[start..].to_vec();
            need(pending.len() <= MAX_RECORD, "SOURCE_OVERSIZED_RECORD")?;
            if count > 0 {
                h.append(&next, "source_advanced", &available)?;
                h.state = next;
                h.processed_bytes += start;
                h.last_source_at = next_source_at;
            }
            h.offset += n as u64;
            h.partial = pending;
            h.boundary_length = 4096u64.min(h.offset);
            h.boundary_hash = h.guard(h.offset - h.boundary_length, h.boundary_length)?;
            Ok(count)
        })(self);
        if r.is_err() { self.failed = true; }
        r
    }
    /// status()
    pub fn status(&self) -> Result<V, String> {
        let report = crate::limit_entry::limit_entry_report(&self.state, &self.bank,
            &baal_service_runtime::contract::ms_to_iso((self.now)()))?;
        let mut out = report.as_object().cloned().unwrap_or_default();
        out.insert("cohort".into(), json!(self.cohort));
        out.insert("codeIdentity".into(), self.code_identity.clone());
        out.insert("sourceBytesConsumed".into(), json!(self.processed_bytes));
        out.insert("sourceBytesRead".into(), json!(self.offset));
        out.insert("incompleteTailBytes".into(), json!(self.partial.len()));
        out.insert("journalSequence".into(), json!(self.journal_sequence));
        out.insert("journalHash".into(), json!(self.journal_hash));
        out.insert("networkRequests".into(), json!(0));
        out.insert("competitionOrders".into(), json!(0));
        Ok(V::Object(out))
    }
    pub fn source_state(&self) -> V { self.state.clone() }
    pub fn close(&mut self) {
        if self.closed { return; }
        let ours = self.owned();
        self.closed = true;
        if ours { let _ = std::fs::remove_file(&self.lock_file); }
    }
}
/// openLimitEntry — initialize creates the cohort's bank+journal; otherwise it
/// replays the saved journal then the whole source prefix.
pub fn open_limit_entry(runtime_dir: &Path, cohort: &str, initialize: bool,
    now: impl Fn() -> f64 + 'static, workspace_dir: &Path,
    can_operate: impl Fn() -> bool + 'static) -> Result<LimitEntryHost, String> {
    need(runtime_dir.is_absolute() && name_ok(cohort), "INVALID_LIMIT_ENTRY_DIRECTORY")?;
    let dir = runtime_dir.join("limit-entry-cohorts").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let source_file = runtime_dir.join("journal.jsonl");
    let stopped_check = |can_operate: &dyn Fn() -> bool| -> bool {
        can_operate() != true
            || [workspace_dir.join("STOP"), runtime_dir.join("STOP"),
                runtime_dir.join("LIMIT_ENTRY_STOP"), dir.join("STOP")].iter().any(|p| p.exists())
    };
    need(!stopped_check(&can_operate), "LIMIT_ENTRY_STOPPED")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)); }
    let lock_file = dir.join("worker.lock");
    {
        let mut o = OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        o.mode(0o600);
        let mut fd = o.open(&lock_file).map_err(|e| format!("{}:{}", "LIMIT_ENTRY_LOCK", e))?;
        write_all(&mut fd, std::process::id().to_string().as_bytes())
            .map_err(|e| { let _ = std::fs::remove_file(&lock_file); e })?;
    }
    let code_identity = limit_entry_code_identity(workspace_dir)?;
    let source_stat = stat_of(&source_file)?;
    let source_size = source_stat.size;
    let mut journal_sequence = 0i64;
    let mut journal_hash = String::new();
    let mut processed_bytes = 0usize;
    let mut checkpoints: Vec<(i64, String)> = Vec::new();
    let mut availability: Vec<V> = Vec::new();
    let mut new_availability: Vec<V> = Vec::new();
    let mut bank: V;
    if initialize {
        need(!bank_file.exists() && !journal_file.exists(), "FROZEN_LIMIT_ENTRY_ALREADY_EXISTS")?;
        let prefix = scan(&source_file, Some(source_size), true, |_, _| Ok(()))?;
        need(prefix.sequence > 0, "SOURCE_PREFIX_EMPTY")?;
        let base = read_json(&runtime_dir.join("dip-responsive-bank.json"))?;
        let cutoff = baal_service_runtime::contract::ms_to_iso(now());
        need(prefix.last_at <= baal_service_runtime::contract::parse_iso_ms(&cutoff).unwrap_or(f64::NAN),
            "FUTURE_SOURCE_PREFIX")?;
        let mut config = base["config"].clone();
        config["marketIds"] = json!(["377", "354", "277", "305"]);
        let created = crate::limit_entry::create_limit_entry(prefix.sequence, &prefix.prefix_hash,
            &cutoff, &config, &base["universe"], &base["identities"], &base["policy"], &code_identity)?;
        bank = created["bank"].clone();
    } else {
        need(bank_file.exists() && journal_file.exists(), "LIMIT_ENTRY_ACCOUNTING_MISSING")?;
        bank = read_json(&bank_file)?;
        let mut prior = 0i64;
        let saved = scan(&journal_file, None, false, |row, _meta| {
            let source_seq = row["state"]["sourceSequence"].as_i64();
            need(source_seq.is_some_and(|x| x > prior)
                && row["event"]["availability"].is_array(), "INVALID_CHECKPOINT_SEQUENCE")?;
            let source_seq = source_seq.unwrap_or(0);
            let mut next = prior + 1;
            for range in row["event"]["availability"].as_array().cloned().unwrap_or_default() {
                let clock_v = baal_service_runtime::contract::parse_iso_ms(range["at"].as_str().unwrap_or(""))
                    .unwrap_or(f64::NAN);
                let previous_clock = availability.last()
                    .and_then(|r| baal_service_runtime::contract::parse_iso_ms(r["at"].as_str().unwrap_or("")))
                    .unwrap_or(clock_v);
                let row_at = baal_service_runtime::contract::parse_iso_ms(row["at"].as_str().unwrap_or(""))
                    .unwrap_or(f64::NAN);
                need(baal_service_runtime::json_text::js_int(&range["from"]) == Some(next)
                    && baal_service_runtime::json_text::js_int(&range["through"])
                        .is_some_and(|t| t >= range["from"].as_i64().unwrap_or(0) && t <= source_seq)
                    && clock_v.is_finite()
                    && baal_service_runtime::contract::ms_to_iso(clock_v) == range["at"].as_str().unwrap_or("")
                    && clock_v >= previous_clock && clock_v <= row_at,
                    "INVALID_HOST_AVAILABILITY")?;
                availability.push(range.clone());
                next = range["through"].as_i64().unwrap_or(0) + 1;
            }
            need(next == source_seq + 1, "HOST_AVAILABILITY_INCOMPLETE")?;
            prior = source_seq;
            checkpoints.push((source_seq, crate::model::digest(&row["state"])));
            Ok(())
        })?;
        need(saved.sequence > 0, "LIMIT_ENTRY_ACCOUNTING_EMPTY")?;
        journal_sequence = saved.sequence;
        journal_hash = saved.hash;
    }
    need(crate::model::digest(&bank["codeIdentity"]) == crate::model::digest(&code_identity),
        "FROZEN_LIMIT_ENTRY_CODE_CHANGED")?;
    let mut state = crate::limit_entry::empty_limit_entry_state(&bank)?;
    let mut compared = 0usize;
    let mut prefix_verified = false;
    let mut availability_index = 0usize;
    let mut last_available = f64::NEG_INFINITY;
    let source_records = bank["sourceRecords"].as_i64().unwrap_or(0);
    let bank_source_hash = bank["sourceHash"].as_str().unwrap_or("").to_string();
    let bank_cutoff = bank["cutoff"].as_str().unwrap_or("").to_string();
    let replay = scan(&source_file, Some(source_size), true, |row, meta| {
        if row["sequence"].as_i64() == Some(source_records) {
            need(meta.prefix_hash == bank_source_hash, "FROZEN_SOURCE_PREFIX_CHANGED")?;
            prefix_verified = true;
        }
        while availability.get(availability_index)
            .is_some_and(|a| a["through"].as_i64().unwrap_or(0) < row["sequence"].as_i64().unwrap_or(0)) {
            availability_index += 1;
        }
        let saved_clock = availability.get(availability_index).cloned();
        let seq = row["sequence"].as_i64().unwrap_or(0);
        let available_at = if let Some(sc) = &saved_clock {
            if sc["from"].as_i64().unwrap_or(0) <= seq {
                sc["at"].as_str().unwrap_or("").to_string()
            } else if initialize && seq <= source_records { bank_cutoff.clone() }
            else { baal_service_runtime::contract::ms_to_iso(now()) }
        } else if initialize && seq <= source_records { bank_cutoff.clone() }
        else { baal_service_runtime::contract::ms_to_iso(now()) };
        need(baal_service_runtime::contract::parse_iso_ms(&available_at).unwrap_or(f64::NAN) >= last_available,
            "HOST_CLOCK_ROLLBACK")?;
        last_available = baal_service_runtime::contract::parse_iso_ms(&available_at).unwrap_or(f64::NAN);
        if saved_clock.is_none() {
            record_availability(&mut new_availability, seq, &available_at);
        }
        state = crate::limit_entry::advance_limit_entry(&state, row, &bank, Some(&available_at))?;
        if checkpoints.get(compared).map(|(s, _)| *s) == row["sequence"].as_i64() {
            need(checkpoints[compared].1 == crate::model::digest(&state), "LIMIT_ENTRY_REPLAY_MISMATCH")?;
            compared += 1;
        }
        Ok(())
    })?;
    need(prefix_verified && compared == checkpoints.len(), "LIMIT_ENTRY_REPLAY_INCOMPLETE")?;
    processed_bytes = replay.complete_bytes;
    let mut host = LimitEntryHost {
        dir, bank, state, runtime_dir: runtime_dir.to_path_buf(),
        workspace_dir: workspace_dir.to_path_buf(), cohort: cohort.to_string(),
        code_identity, journal_sequence, journal_hash, processed_bytes,
        offset: processed_bytes as u64, partial: Vec::new(), last_source_at: replay.last_at,
        last_available, bank_bytes_hash: String::new(), head_length: 0, head_hash: String::new(),
        boundary_length: 0, boundary_hash: String::new(), source_stat,
        lock_file, closed: false, failed: false,
        now: Box::new(now), can_operate: Box::new(can_operate),
    };
    host.live()?;
    need(crate::model::digest(&host.code_identity)
        == crate::model::digest(&limit_entry_code_identity(workspace_dir)?), "FROZEN_LIMIT_ENTRY_CODE_CHANGED")?;
    let current_source_stat = stat_of(&host.source_file())?;
    need(current_source_stat.ino == host.source_stat.ino
        && current_source_stat.birthtime_ms == host.source_stat.birthtime_ms
        && current_source_stat.size >= source_size, "SOURCE_REPLACED_OR_TRUNCATED")?;
    if initialize {
        create_file(&bank_file, format!("{}\n", serde_json::to_string_pretty(&host.bank).map_err(|e| e.to_string())?).as_bytes())?;
        create_file(&journal_file, b"")?;
    }
    if initialize || checkpoints.last().map(|(s, _)| *s) != Some(host.state["sourceSequence"].as_i64().unwrap_or(0)) {
        let st = host.state.clone();
        host.append(&st, if initialize { "cohort_frozen" } else { "source_advanced" }, &new_availability)?;
    }
    host.bank_bytes_hash = sha256(&std::fs::read(&bank_file).map_err(|e| e.to_string())?);
    host.head_length = 4096u64.min(host.offset);
    host.head_hash = host.guard(0, host.head_length)?;
    host.boundary_length = 4096u64.min(host.offset);
    host.boundary_hash = host.guard(host.offset - host.boundary_length, host.boundary_length)?;
    Ok(host)
}
/// runLimitEntry({runtimeDir, cohort, initialize, once, now, workspaceDir, sleep})
/// Signals are left to the embedding process: it can flip `ending` through the
/// returned Arc between iterations (JS uses SIGINT/SIGTERM).
pub fn run_limit_entry(runtime_dir: &Path, cohort: &str, initialize: bool, once: bool,
    now: impl Fn() -> f64 + 'static + Clone, workspace_dir: &Path,
    sleep: impl Fn(u64)) -> Result<V, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let ending = Arc::new(AtomicBool::new(false));
    let mut watch = open_limit_entry(runtime_dir, cohort, initialize, now.clone(), workspace_dir,
        {
            let ending = ending.clone();
            move || !ending.load(Ordering::Relaxed)
        });
    let result = (|watch: &mut Result<LimitEntryHost, String>| -> Result<V, String> {
        let w = watch.as_mut().map_err(|e| e.clone())?;
        loop {
            if w.stopped() { break; }
            w.consume()?;
            let status = w.status()?;
            let mut body = status.as_object().cloned().unwrap_or_default();
            let stat_label = if status["incompleteTailBytes"].as_i64().unwrap_or(0) != 0 {
                "awaiting_capture"
            } else { "running" };
            body.insert("status".into(), json!(stat_label));
            body.insert("pid".into(), json!(std::process::id()));
            body.insert("updatedAt".into(),
                json!(baal_service_runtime::contract::ms_to_iso(now())));
            crate::paper_persistence::atomic_paper_json(&w.dir.join("stats.json"), &V::Object(body),
                &|| w.owned() && (stat_label != "running" || !w.stopped()))
                .map_err(|e| format!("{:?}", e.details))?;
            if once { break; }
            if w.stopped() { break; }
            sleep(1000);
        }
        if w.stopped() {
            let status = w.status()?;
            let mut body = status.as_object().cloned().unwrap_or_default();
            body.insert("status".into(), json!("stopped"));
            body.insert("pid".into(), json!(std::process::id()));
            body.insert("updatedAt".into(),
                json!(baal_service_runtime::contract::ms_to_iso(now())));
            let _ = crate::paper_persistence::atomic_paper_json(&w.dir.join("stats.json"),
                &V::Object(body), &|| w.owned() && !w.stopped());
        }
        w.status()
    })(&mut watch);
    if let Ok(w) = &mut watch { w.close(); }
    match result {
        Ok(v) => Ok(v),
        Err(_) => Err("LIMIT_ENTRY_FAILED_CLOSED".to_string()),
    }
}
