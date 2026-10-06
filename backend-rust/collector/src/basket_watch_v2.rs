// live-paper/basket-watch-v2.mjs — identical to basket-watch.mjs except the
// source journal is consumed through the incremental verified reader
// (identity/head/tail guards plus source-read metrics), not verifySource.
use crate::basket_observer::basket_report;
use crate::ml_shadow::VerifiedSource;
use crate::model::digest;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::json_text::{digest as sha256_hex, js_int, js_truthy};
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

// ---------------------------------------------------------------------------
// incremental-journal.mjs — createVerifiedSourceReader / readIncrementalSource.
// Vendored here rather than reusing crate::incremental_journal: that port folds
// the file length into the identity check (JS same() compares dev/ino/birthtime
// only) so every legitimate append reads as SOURCE_REPLACED_OR_TRUNCATED, and
// it never updates metrics.bytesRead — both observably wrong here.
// ---------------------------------------------------------------------------

/// journalLines(file, {chunkBytes, maxRecordBytes, startByte, endByte, onRead})
/// — lazy: a chunk is readSync'd only when the buffered data holds no complete
/// line, mirroring the JS generator so `bytesRead` stays byte-identical.
struct LazyJournalLines {
    fd: std::fs::File,
    chunk_bytes: usize,
    max_record_bytes: usize,
    size: u64,
    pos: u64,
    data: Vec<u8>,
    begin: usize,
    exhausted: bool,
    bytes_read: u64,
}
impl LazyJournalLines {
    fn new(file: &Path, chunk_bytes: usize, max_record_bytes: usize, start_byte: u64, end_byte: u64) -> Result<Self, String> {
        if chunk_bytes == 0 || chunk_bytes > 1_048_576 || max_record_bytes < chunk_bytes {
            return Err("INVALID_JOURNAL_READ_LIMIT".into());
        }
        let fd = std::fs::File::open(file).map_err(|e| e.to_string())?;
        let actual = fd.metadata().map_err(|e| e.to_string())?.len();
        // endByte is always finite here; size<startByte or size>actual is a range error.
        if start_byte > end_byte || end_byte > actual {
            return Err("INVALID_JOURNAL_READ_RANGE".into());
        }
        Ok(Self { fd, chunk_bytes, max_record_bytes, size: end_byte, pos: start_byte, data: Vec::new(), begin: 0, exhausted: false, bytes_read: 0 })
    }
}
impl Iterator for LazyJournalLines {
    type Item = Result<String, String>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted { return None; }
        loop {
            if let Some(rel) = self.data[self.begin..].iter().position(|b| *b == b'\n') {
                let end = self.begin + rel;
                if end - self.begin > self.max_record_bytes {
                    self.exhausted = true;
                    return Some(Err("JOURNAL_RECORD_TOO_LARGE".into()));
                }
                let line = String::from_utf8_lossy(&self.data[self.begin..end]).into_owned();
                self.begin = end + 1;
                return Some(Ok(line));
            }
            if self.pos >= self.size {
                self.exhausted = true;
                if self.begin < self.data.len() {
                    return Some(Err("JOURNAL_INCOMPLETE_TAIL: Incomplete paper journal: manual recovery required".into()));
                }
                return None;
            }
            if self.data.len() - self.begin > self.max_record_bytes {
                self.exhausted = true;
                return Some(Err("JOURNAL_RECORD_TOO_LARGE".into()));
            }
            let want = (self.chunk_bytes as u64).min(self.size - self.pos) as usize;
            let mut buf = vec![0u8; want];
            #[cfg(unix)]
            let n = {
                use std::os::unix::fs::FileExt;
                match self.fd.read_at(&mut buf, self.pos) {
                    Ok(n) => n,
                    Err(e) => { self.exhausted = true; return Some(Err(e.to_string())); }
                }
            };
            #[cfg(not(unix))]
            let n = {
                use std::io::{Read, Seek, SeekFrom};
                match self.fd.seek(SeekFrom::Start(self.pos)).and_then(|_| self.fd.read(&mut buf)) {
                    Ok(n) => n,
                    Err(e) => { self.exhausted = true; return Some(Err(e.to_string())); }
                }
            };
            if n < 1 {
                self.exhausted = true;
                return Some(Err("JOURNAL_READ_TRUNCATED".into()));
            }
            self.pos += n as u64;
            self.bytes_read += n as u64;
            // data = tail ++ chunk
            let mut data = self.data.split_off(self.begin);
            data.extend_from_slice(&buf[..n]);
            self.data = data;
            self.begin = 0;
        }
    }
}

#[cfg(unix)]
fn file_identity(file: &Path) -> Result<(u64, u64, f64, u64), String> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(file).map_err(|e| e.to_string())?;
    // JS same(): dev === dev && ino === ino && birthtimeMs === birthtimeMs.
    let btime = m
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(f64::NAN);
    Ok((m.dev(), m.ino(), btime, m.len()))
}
#[cfg(not(unix))]
fn file_identity(file: &Path) -> Result<(u64, u64, f64, u64), String> {
    let m = std::fs::metadata(file).map_err(|e| e.to_string())?;
    Ok((0, 0, f64::NAN, m.len()))
}
/// JS same(a, b) — dev/ino/birthtime; NaN birthtimes (unsupported fs) compare equal.
fn same_stat(a: &(u64, u64, f64, u64), b: &(u64, u64, f64, u64)) -> bool {
    a.0 == b.0 && a.1 == b.1 && (a.2 == b.2 || (a.2.is_nan() && b.2.is_nan()))
}

/// createVerifiedSourceReader(file, {chunkBytes, maxRecordBytes}) — the module's
/// `readers` map is represented by the watch owning exactly one reader.
pub(crate) struct IncrementalSourceReader {
    file: PathBuf,
    chunk_bytes: usize,
    max_record_bytes: usize,
    pub rows: Vec<V>,
    pub prefix_hashes: Vec<String>,
    identity: Option<(u64, u64, f64, u64)>,
    offset: u64,
    previous: String,
    clock: f64,
    prefix: Sha256,
    head: Option<String>,
    boundary: Option<String>,
    metrics: V,
}
impl IncrementalSourceReader {
    pub(crate) fn new(file: &Path, chunk_bytes: usize, max_record_bytes: usize) -> Self {
        Self {
            file: file.to_path_buf(), chunk_bytes, max_record_bytes,
            rows: vec![], prefix_hashes: vec![], identity: None, offset: 0,
            previous: String::new(), clock: f64::NEG_INFINITY, prefix: Sha256::new(),
            head: None, boundary: None,
            metrics: json!({"polls":0,"fullVerifications":0,"appendVerifications":0,"bytesRead":0,
                "guardBytesRead":0,"partialTails":0,"verifiedRecords":0,"verifiedBytes":0}),
        }
    }
    fn guard(&mut self, start: u64, n: usize) -> Result<String, String> {
        let mut buf = vec![0u8; n];
        {
            let f = std::fs::File::open(&self.file).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileExt;
                f.read_exact_at(&mut buf, start).map_err(|_| "SOURCE_GUARD_READ".to_string())?;
            }
            #[cfg(not(unix))]
            {
                use std::io::{Read, Seek, SeekFrom};
                let mut f = f;
                f.seek(SeekFrom::Start(start)).and_then(|_| f.read_exact(&mut buf))
                    .map_err(|_| "SOURCE_GUARD_READ".to_string())?;
            }
        }
        self.metrics["guardBytesRead"] = json!(self.metrics["guardBytesRead"].as_u64().unwrap_or(0) + n as u64);
        let mut h = Sha256::new();
        h.update(&buf);
        Ok(format!("{:x}", h.finalize()))
    }
    /// read() — Ok(true) when verified rows are available (the JS return is
    /// `rows.length ? rows : null`; rows/prefix_hashes are fields here).
    pub(crate) fn read(&mut self) -> Result<bool, String> {
        self.metrics["polls"] = json!(self.metrics["polls"].as_u64().unwrap_or(0) + 1);
        let stat = file_identity(&self.file)?;
        if let Some(id) = &self.identity {
            if !same_stat(id, &stat) || stat.3 < self.offset {
                return Err("SOURCE_REPLACED_OR_TRUNCATED".into());
            }
            if self.offset > 0 {
                if self.guard(0, self.offset.min(4096) as usize)? != self.head.clone().unwrap_or_default()
                    || self.guard(self.offset.saturating_sub(32768), self.offset.min(32768) as usize)?
                        != self.boundary.clone().unwrap_or_default()
                {
                    return Err("SOURCE_VERIFIED_BOUNDARY_CHANGED".into());
                }
            }
        }
        if stat.3 == self.offset { return Ok(!self.rows.is_empty()); }
        // Verify into temporary state; a partial tail or bad append commits nothing.
        let mut appended: Vec<V> = Vec::new();
        let mut hashes: Vec<String> = Vec::new();
        let mut next_prefix = self.prefix.clone();
        let mut next_previous = self.previous.clone();
        let mut next_clock = self.clock;
        let mut consumed = 0u64;
        let mut iter_err: Option<String> = None;
        let mut lines = LazyJournalLines::new(&self.file, self.chunk_bytes, self.max_record_bytes, self.offset, stat.3)?;
        for item in &mut lines {
            let line = match item {
                Ok(l) => l,
                Err(e) => { iter_err = Some(e); break; }
            };
            let r: V = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => { iter_err = Some(e.to_string()); break; } // JSON.parse throws raw
            };
            let mut body = r.clone();
            if let Some(m) = body.as_object_mut() { m.shift_remove("hash"); }
            let expected = self.rows.len() as f64 + appended.len() as f64 + 1.0;
            let body_hash = sha256_hex(&body);
            if r["sequence"].as_f64() != Some(expected)
                || r["previous"].as_str() != Some(next_previous.as_str())
                || r["hash"].as_str() != Some(body_hash.as_str())
            {
                iter_err = Some("Source integrity failure".into());
                break;
            }
            // Date.parse(r.event?.clockAt || r.at): a truthy non-string clockAt
            // parses as NaN, matching JS argument coercion.
            let clock_src = if js_truthy(r["event"].get("clockAt")) { r["event"].get("clockAt") } else { r.get("at") };
            let at = match clock_src {
                Some(V::String(s)) => baal_service_runtime::contract::parse_iso_ms(s),
                _ => None,
            };
            if !at.is_some_and(|a| a.is_finite() && a >= next_clock) {
                iter_err = Some("Source clock rollback".into());
                break;
            }
            next_clock = at.unwrap();
            next_previous = r["hash"].as_str().unwrap_or_default().to_string();
            next_prefix.update(line.as_bytes());
            next_prefix.update(b"\n");
            hashes.push(format!("{:x}", next_prefix.clone().finalize()));
            consumed += line.len() as u64 + 1;
            let mut input = r;
            if let Some(m) = input.as_object_mut() { m.shift_remove("state"); }
            appended.push(input);
        }
        let read_bytes = lines.bytes_read;
        self.metrics["bytesRead"] = json!(self.metrics["bytesRead"].as_u64().unwrap_or(0) + read_bytes);
        if let Some(e) = iter_err {
            if e.starts_with("JOURNAL_INCOMPLETE_TAIL") {
                self.metrics["partialTails"] = json!(self.metrics["partialTails"].as_u64().unwrap_or(0) + 1);
                return Ok(false);
            }
            return Err(e);
        }
        if self.offset + consumed != stat.3 { return Err("SOURCE_BYTE_ACCOUNTING".into()); }
        let after = file_identity(&self.file)?;
        if !same_stat(&stat, &after) || after.3 < stat.3 {
            return Err("SOURCE_REPLACED_DURING_READ".into());
        }
        let next_head = self.guard(0, stat.3.min(4096) as usize)?;
        let next_boundary = self.guard(stat.3.saturating_sub(32768), stat.3.min(32768) as usize)?;
        self.rows.extend(appended);
        self.prefix_hashes.extend(hashes);
        if self.identity.is_some() {
            self.metrics["appendVerifications"] = json!(self.metrics["appendVerifications"].as_u64().unwrap_or(0) + 1);
        } else {
            self.metrics["fullVerifications"] = json!(self.metrics["fullVerifications"].as_u64().unwrap_or(0) + 1);
        }
        self.identity = Some(stat);
        self.offset = stat.3;
        self.previous = next_previous;
        self.clock = next_clock;
        self.prefix = next_prefix;
        self.head = Some(next_head);
        self.boundary = Some(next_boundary);
        self.metrics["verifiedRecords"] = json!(self.rows.len());
        self.metrics["verifiedBytes"] = json!(self.offset);
        Ok(!self.rows.is_empty())
    }
    /// metrics() — {...metrics, sourceSequence, sourceHash, bytePrefixHash, trustBoundary}
    pub(crate) fn metrics(&self) -> V {
        let mut m = self.metrics.clone();
        m["sourceSequence"] = json!(self.rows.len());
        m["sourceHash"] = json!(self.previous);
        m["bytePrefixHash"] = json!(format!("{:x}", self.prefix.clone().finalize()));
        m["trustBoundary"] = json!("full_chain_on_restart_then_append_identity_head_and_last32KiB_guards; arbitrary_old_middle_rewrites_require_restart_verification");
        m
    }
}
fn source_of(reader: &IncrementalSourceReader) -> VerifiedSource {
    VerifiedSource { records: reader.rows.clone(), prefix_hashes: reader.prefix_hashes.clone() }
}

fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true".into() } else { "false".into() },
        V::Null => "null".into(),
        _ => "[object Object]".into(),
    }
}
/// JS `===` for JSON values: scalars by value, objects/arrays never equal.
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
/// a === b + 1 under JS number semantics.
fn plus1_eq(a: &V, b: &V) -> bool {
    a.is_number() && b.is_number() && a.as_f64() == b.as_f64().map(|x| x + 1.0)
}
/// records[i]?.hash !== expected — distinguishes a missing row/hash (undefined)
/// from a stored null.
fn row_hash_eq(records: &VerifiedSource, index: Option<usize>, expected: &V, expected_key_present: bool) -> bool {
    match index.and_then(|i| records.records.get(i)) {
        Some(r) => strict_eq(&r["hash"], expected),
        None => !expected_key_present,
    }
}
fn json_index_usize(v: &V) -> Option<usize> {
    // arr[i] hits only for canonical integer indices.
    v.as_f64().filter(|x| x.fract() == 0.0 && *x >= 0.0 && *x <= usize::MAX as f64).map(|x| x as usize)
}

/// basketCodeIdentity() — digests of the frozen JS sources, read relative to
/// the live-paper directory supplied by the caller (JS resolves its own dir).
/// Identical file list to basket-watch.mjs.
pub fn basket_code_identity(live_paper_dir: &Path) -> Result<V, String> {
    crate::basket_watch::basket_code_identity(live_paper_dir)
}

/// createBasketCapture(records, definitions, cutoff, codeIdentity)
/// `records` is the verified source stream (incremental reader output).
pub fn create_basket_capture(records: &VerifiedSource, definitions: &V, cutoff: &str, code_identity: &V) -> Result<V, String> {
    crate::basket_watch::create_basket_capture(records, definitions, cutoff, code_identity)
}
fn bound(bank: &V, records: &VerifiedSource) -> Result<(), String> {
    let n = js_int(&bank["sourceRecords"]);
    if js_int(&bank["schemaVersion"]) != Some(1)
        || bank["mode"].as_str() != Some("dual_no_basket_measurement")
        || n.map_or(true, |x| x < 1)
        || n.and_then(|x| records.prefix_hashes.get(x as usize - 1)).map(String::as_str)
            != bank["sourceHash"].as_str()
    {
        return Err("Basket source prefix changed".into());
    }
    Ok(())
}
fn initial_capture(bank: &V, records: &VerifiedSource) -> Result<V, String> {
    bound(bank, records)?;
    let n = js_int(&bank["sourceRecords"]).unwrap() as usize;
    let last_hash = records.records.get(n - 1).map(|r| r["hash"].clone())
        .ok_or_else(|| "Cannot read properties of undefined (reading 'hash')".to_string())?;
    let mut pairs = Map::new();
    for p in bank["pairs"].as_array().cloned().unwrap_or_default() {
        pairs.insert(js_str(&p["id"]), p["initialState"].clone());
    }
    Ok(json!({
        "bankHash": digest(bank),
        "sourceSequence": bank["sourceRecords"],
        "lastRecordHash": last_hash,
        "pairs": V::Object(pairs),
    }))
}

/// advanceBasketRecord(input, record, bank) — {state, observations}
/// Identical body to basket-watch.mjs.
pub fn advance_basket_record(input: &V, record: &V, bank: &V) -> Result<V, String> {
    crate::basket_watch::advance_basket_record(input, record, bank)
}

fn append_journal(file: &Path, row: &V) -> Result<(), String> {
    // fs.openSync(file, 'a', 0o600) — the mode applies only at creation.
    let mut oo = std::fs::OpenOptions::new();
    oo.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        oo.mode(0o600);
    }
    let mut fd = oo.open(file).map_err(|e| e.to_string())?;
    let bytes = format!("{}\n", baal_service_runtime::json_text::js_stringify(row));
    fd.write_all(bytes.as_bytes()).map_err(|e| {
        // 'Basket journal short write' only for a zero-byte write; io errors propagate.
        if e.kind() == std::io::ErrorKind::WriteZero { "Basket journal short write".to_string() } else { e.to_string() }
    })?;
    fd.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

/// openBasketWatch(runtime, {initialize, cutoff, definitions, codeIdentity})
/// `cutoff` must already be the caller's instant (JS defaults to now).
/// The incremental reader is held by the watch, like the module-level `readers`
/// map in incremental-journal.mjs.
pub struct BasketWatchV2 {
    runtime: PathBuf,
    journal_file: PathBuf,
    reader: IncrementalSourceReader,
    bank: V,
    state: V,
    sequence: i64,
    previous: String,
}
impl BasketWatchV2 {
    pub fn bank(&self) -> &V { &self.bank }
    pub fn state(&self) -> &V { &self.state }
    /// consume() — None while the source journal has an incomplete tail.
    pub fn consume(&mut self) -> Result<Option<i64>, String> {
        if !self.reader.read()? { return Ok(None); }
        let records = source_of(&self.reader);
        bound(&self.bank, &records)?;
        let src_seq = self.state["sourceSequence"].as_f64().unwrap_or(f64::NAN);
        let prior_ok = row_hash_eq(&records,
            json_index_usize(&self.state["sourceSequence"]).map(|i| i.wrapping_sub(1)).filter(|_| src_seq.is_finite()),
            &self.state["lastRecordHash"],
            self.state.as_object().map(|m| m.contains_key("lastRecordHash")).unwrap_or(false));
        if (records.records.len() as f64) < src_seq || !prior_ok {
            return Err("Basket source changed".into());
        }
        let start = if src_seq.is_finite() && src_seq >= 0.0 { (src_seq as usize).min(records.records.len()) } else { 0 };
        let mut consumed = 0i64;
        for record in &records.records[start..] {
            let output = advance_basket_record(&self.state, record, &self.bank)?;
            let mut body = json!({
                "sequence": self.sequence + 1, "previous": self.previous,
                "sourceSequence": record["sequence"], "sourceHash": record["hash"],
                "stateHash": digest(&output["state"]), "observationsHash": digest(&output["observations"]),
                "observations": output["observations"],
            });
            let hash = digest(&body);
            body.as_object_mut().unwrap().insert("hash".into(), json!(hash));
            append_journal(&self.journal_file, &body)?;
            self.state = output["state"].clone();
            self.sequence += 1;
            self.previous = hash;
            consumed += 1;
        }
        atomic_json(&self.runtime.join("basket-state.json"), &self.state)?;
        Ok(Some(consumed))
    }
    pub fn status(&self) -> Result<V, String> {
        let mut pairs = Vec::new();
        for p in self.bank["pairs"].as_array().cloned().unwrap_or_default() {
            let pid = js_str(&p["id"]);
            let report = basket_report(&self.state["pairs"][&pid], &p["bank"])?;
            let mut o = Map::new();
            o.insert("pairId".into(), p["id"].clone());
            if let Some(obj) = report.as_object() {
                for (k, v) in obj { o.insert(k.clone(), v.clone()); }
            }
            pairs.push(V::Object(o));
        }
        Ok(json!({
            "schemaVersion": 1, "sourceReadMetrics": self.reader.metrics(),
            "mode": "dual_no_basket_measurement", "bankHash": digest(&self.bank),
            "cutoff": self.bank["cutoff"], "sourceSequence": self.state["sourceSequence"],
            "sourceHash": self.state["lastRecordHash"], "journalSequence": self.sequence,
            "journalHash": self.previous, "pairs": pairs,
            "networkRequests": 0, "ordersSubmitted": 0, "simulatedFills": null, "pnl": null,
            "eligibleForExecution": false,
        }))
    }
}
/// openBasketWatch(runtime, {initialize=false, cutoff=now, definitions, codeIdentity=auto})
/// `code_identity: None` computes basketCodeIdentity() over `live_paper_dir`;
/// `cutoff: None` defaults to the current instant, as in JS.
pub fn open_basket_watch_v2(runtime: &Path, live_paper_dir: &Path, initialize: bool, cutoff: Option<&str>, definitions: &V, code_identity: Option<&V>) -> Result<BasketWatchV2, String> {
    // JS parameter defaults resolve at call time, before the body: cutoff=now,
    // then codeIdentity=basketCodeIdentity() (file reads may throw first).
    let now_iso;
    let cutoff = match cutoff {
        Some(c) => c,
        None => {
            let ms = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
            now_iso = baal_service_runtime::contract::ms_to_iso(ms);
            &now_iso
        }
    };
    let auto_identity;
    let code_identity = match code_identity {
        Some(c) => c,
        None => { auto_identity = basket_code_identity(live_paper_dir)?; &auto_identity }
    };
    let bank_file = runtime.join("basket-bank.json");
    let journal_file = runtime.join("basket-journal.jsonl");
    // readIncrementalSource(runtime/journal.jsonl) — one persistent reader.
    let mut reader = IncrementalSourceReader::new(&runtime.join("journal.jsonl"), 1_048_576, 16_777_216);
    if !reader.read()? { return Err("Basket source temporarily incomplete".into()); }
    let records = source_of(&reader);
    let bank: V;
    if initialize {
        if bank_file.exists() || journal_file.exists() { return Err("Refusing to replace existing basket cohort".into()); }
        bank = create_basket_capture(&records, definitions, cutoff, code_identity)?["bank"].clone();
        let pretty = serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?;
        let mut fd = std::fs::OpenOptions::new();
        fd.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            fd.mode(0o600);
        }
        fd.open(&bank_file).and_then(|mut f| f.write_all(pretty.as_bytes())).map_err(|e| e.to_string())?;
        let mut jf = std::fs::OpenOptions::new();
        jf.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            jf.mode(0o600);
        }
        jf.open(&journal_file).map_err(|e| e.to_string())?;
    } else {
        bank = serde_json::from_str(&std::fs::read_to_string(&bank_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    if digest(&bank["codeIdentity"]) != digest(code_identity) {
        return Err("Frozen basket code changed; new cohort required".into());
    }
    let mut state = initial_capture(&bank, &records)?;
    let mut sequence = 0i64;
    let mut previous = digest(&bank);
    let raw = std::fs::read_to_string(&journal_file).map_err(|e| e.to_string())?;
    if !raw.is_empty() && !raw.ends_with('\n') { return Err("Incomplete basket journal".into()); }
    for line in raw.split('\n').filter(|l| !l.is_empty()) {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(m) = body.as_object_mut() { m.shift_remove("hash"); }
        sequence += 1;
        let src_idx = json_index_usize(&body["sourceSequence"]).map(|i| i.wrapping_sub(1));
        let expected_src = plus1_eq(&body["sourceSequence"], &state["sourceSequence"]);
        if hash != digest(&body)
            || js_int(&body["sequence"]) != Some(sequence)
            || body["previous"].as_str() != Some(previous.as_str())
            || !expected_src
            || !row_hash_eq(&records, src_idx, &body["sourceHash"], body.as_object().map(|m| m.contains_key("sourceHash")).unwrap_or(false))
        {
            return Err("Basket journal integrity failure".into());
        }
        let rec = src_idx.and_then(|i| records.records.get(i))
            .ok_or_else(|| "Cannot read properties of undefined (reading 'sequence')".to_string())?;
        let output = advance_basket_record(&state, rec, &bank)?;
        if digest(&output["state"]) != body["stateHash"].as_str().unwrap_or("")
            || digest(&output["observations"]) != body["observationsHash"].as_str().unwrap_or("")
            || digest(&body["observations"]) != body["observationsHash"].as_str().unwrap_or("")
        {
            return Err("Basket replay mismatch".into());
        }
        state = output["state"].clone();
        previous = hash;
    }
    Ok(BasketWatchV2 { runtime: runtime.to_path_buf(), journal_file, reader, bank, state, sequence, previous })
}

