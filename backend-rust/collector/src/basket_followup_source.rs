// live-paper/basket-followup-source.mjs — read-only, bounded incremental
// evidence reader over the source + basket-observer journals.
use crate::basket_observer::create_basket_observer;
use crate::basket_watch::{advance_basket_record, basket_code_identity};
use crate::model::{digest, normalize_book};
use baal_service_runtime::contract::{instant_ns, ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

const MAX_PENDING: usize = 128;
const MAX_PROOFS: usize = 64;
const MAX_AGE_MS: f64 = 600_000.0;
const CHUNK_BYTES: usize = 1024 * 1024;
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn clock(x: &V) -> bool {
    x.as_str().and_then(parse_iso_ms).is_some_and(|ms| ms_to_iso(ms) == x.as_str().unwrap())
}
fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(code.into()); } Ok(()) }
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
fn spread(v: &V) -> Result<Vec<V>, String> {
    if !js_truthy(Some(v)) { return Ok(vec![]); }
    match v {
        V::Array(a) => Ok(a.clone()),
        V::String(s) => Ok(s.chars().map(|c| V::from(c.to_string())).collect()),
        _ => Err("value is not iterable".into()),
    }
}
fn upper_code(e: &str) -> bool {
    e.len() >= 2 && e.as_bytes()[0].is_ascii_uppercase()
        && e[1..].bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
}

#[derive(Clone)]
struct Span { start: u64, length: u64, hash: String }
#[derive(Clone)]
struct Peeked { row: V, span: Span, bytes: Vec<u8> }

#[derive(Clone)]
struct FileStamp {
    dev: u64,
    ino: u64,
    birth_ms: Option<u64>,
    size: u64,
    mtime_ms: u64,
    ctime_ms: u64,
    complete: bool,
}
fn ns_ms_bits(sec: i64, nsec: i64) -> u64 {
    // Node reports stat times as tv_sec*1e3 + tv_nsec/1e6 f64 milliseconds.
    (sec as f64 * 1e3 + nsec as f64 / 1e6).to_bits()
}
fn file_stamp(m: &std::fs::Metadata) -> FileStamp {
    let to_ms = |r: std::io::Result<std::time::SystemTime>| -> Option<u64> {
        r.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| (d.as_secs_f64() * 1e3 + d.subsec_nanos() as f64 / 1e6).to_bits())
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        FileStamp {
            dev: m.dev(), ino: m.ino(), birth_ms: to_ms(m.created()), size: m.len(),
            mtime_ms: ns_ms_bits(m.mtime(), m.mtime_nsec()),
            ctime_ms: ns_ms_bits(m.ctime(), m.ctime_nsec()),
            complete: false,
        }
    }
    #[cfg(not(unix))]
    {
        FileStamp {
            dev: 0, ino: 0, birth_ms: to_ms(m.created()), size: m.len(),
            mtime_ms: to_ms(m.modified()).unwrap_or(0), ctime_ms: 0, complete: false,
        }
    }
}

// Append-only local files are the trust boundary, not authenticated messages.
// Every row is checked once; exact candidate rows are reread before emission.
// Identity, size, same-size edits, head and prior-tail guards detect replacement
// and ordinary rewrites. Arbitrary edits elsewhere in an old prefix combined
// with an append cannot be disproved without rescanning that entire prefix.
// Restart performs that full verification. No account/source bodies are exposed.
struct JournalReader {
    file: PathBuf,
    identity: Option<FileStamp>,
    prior: Option<FileStamp>,
    head: Option<Span>,
    tail: Option<Span>,
    buffer: Vec<u8>,
    offset: u64,
    start: u64,
    cached: Option<Peeked>,
    streamed_bytes: u64,
}
impl JournalReader {
    fn new(file: PathBuf) -> Self {
        Self { file, identity: None, prior: None, head: None, tail: None,
            buffer: Vec::new(), offset: 0, start: 0, cached: None, streamed_bytes: 0 }
    }
    fn read(&self, at: u64, length: u64) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; length as usize];
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            let fd = std::fs::File::open(&self.file).map_err(|e| e.to_string())?;
            fd.read_exact_at(&mut buf, at).map_err(|_| "JOURNAL_CHANGED".to_string())?;
        }
        #[cfg(not(unix))]
        {
            use std::io::{Read, Seek, SeekFrom};
            let mut fd = std::fs::File::open(&self.file).map_err(|e| e.to_string())?;
            fd.seek(SeekFrom::Start(at)).map_err(|_| "JOURNAL_CHANGED".to_string())?;
            fd.read_exact(&mut buf).map_err(|_| "JOURNAL_CHANGED".to_string())?;
        }
        Ok(buf)
    }
    fn verify(&self, span: &Span) -> Result<(), String> {
        need(sha256(&self.read(span.start, span.length)?) == span.hash, "JOURNAL_CHANGED")
    }
    fn snapshot(&mut self) -> Result<FileStamp, String> {
        let meta = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        need(meta.is_file(), "JOURNAL_NOT_FILE")?;
        let stamp = file_stamp(&meta);
        if self.identity.is_none() { self.identity = Some(stamp.clone()); }
        let id = self.identity.as_ref().unwrap();
        need(stamp.dev == id.dev && stamp.ino == id.ino && stamp.birth_ms == id.birth_ms, "JOURNAL_REPLACED")?;
        if let Some(prior) = &self.prior {
            need(stamp.size >= prior.size, "JOURNAL_TRUNCATED")?;
            need(stamp.size != prior.size
                || (stamp.mtime_ms == prior.mtime_ms && stamp.ctime_ms == prior.ctime_ms), "JOURNAL_CHANGED")?;
            if let Some(h) = self.head.clone() { self.verify(&h)?; }
            if let Some(t) = self.tail.clone() { self.verify(&t)?; }
        }
        let complete = stamp.size == 0 || self.read(stamp.size - 1, 1)?[0] == b'\n';
        Ok(FileStamp { complete, ..stamp })
    }
    fn remember(&mut self, stat: &FileStamp) -> Result<(), String> {
        if self.head.is_none() && stat.size > 0 {
            let length = stat.size.min(4096);
            self.head = Some(Span { start: 0, length, hash: sha256(&self.read(0, length)?) });
        }
        let length = stat.size.min(65536);
        self.tail = if length > 0 {
            Some(Span { start: stat.size - length, length, hash: sha256(&self.read(stat.size - length, length)?) })
        } else { None };
        self.prior = Some(stat.clone());
        Ok(())
    }
    fn peek(&mut self, stat: &FileStamp) -> Result<Option<Peeked>, String> {
        if let Some(c) = &self.cached {
            return Ok(Some(c.clone()));
        }
        loop {
            if let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                need(end > 0 && end + 1 <= MAX_LINE_BYTES, "INVALID_JOURNAL_LINE")?;
                let bytes: Vec<u8> = self.buffer[..end + 1].to_vec();
                // JS decodes lossy then requires the re-encode to round-trip;
                // strict UTF-8 validation is equivalent.
                let text = String::from_utf8(self.buffer[..end].to_vec()).map_err(|_| "INVALID_JOURNAL_UTF8".to_string())?;
                let row: V = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                self.cached = Some(Peeked {
                    row,
                    span: Span { start: self.start, length: (end + 1) as u64, hash: sha256(&bytes) },
                    bytes,
                });
                return Ok(self.cached.clone());
            }
            need(self.buffer.len() <= MAX_LINE_BYTES, "OVERSIZED_JOURNAL_LINE")?;
            if self.offset >= stat.size { return Ok(None); }
            let want = (CHUNK_BYTES as u64).min(stat.size - self.offset);
            let bytes = self.read(self.offset, want)?;
            self.offset += bytes.len() as u64;
            self.streamed_bytes += bytes.len() as u64;
            self.buffer.extend_from_slice(&bytes);
        }
    }
    fn consume(&mut self) -> Result<(), String> {
        let c = self.cached.take().ok_or_else(|| "consume without a peeked row".to_string())?;
        self.start += c.span.length;
        self.buffer.drain(..c.span.length as usize);
        Ok(())
    }
    fn progress(&self) -> (u64, u64) { (self.streamed_bytes, self.start) }
}

fn checked_bank(input: &V, universe: &V, live_paper_dir: &Path) -> Result<V, String> {
    let bank = input.clone();
    need(js_int(&bank["schemaVersion"]) == Some(1)
        && bank["mode"].as_str() == Some("dual_no_basket_measurement")
        && clock(&bank["cutoff"])
        && js_int(&bank["sourceRecords"]).is_some_and(|x| x > 0)
        && hex(&bank["sourceHash"])
        && bank["pairs"].as_array().is_some_and(|p| !p.is_empty() && p.len() <= 1000)
        && bank["definitions"].as_array().map(|d| d.len()) == bank["pairs"].as_array().map(|p| p.len()),
        "INVALID_BASKET_BANK")?;
    need(digest(&bank["codeIdentity"]) == digest(&basket_code_identity(live_paper_dir)?), "FROZEN_BASKET_CODE_CHANGED")?;
    need(universe["tournamentId"].is_string() && universe["exchanges"].is_array(), "INVALID_UNIVERSE")?;
    let mut markets: HashMap<String, String> = HashMap::new();
    let mut exchanges: HashSet<String> = HashSet::new();
    for x in universe["exchanges"].as_array().unwrap() {
        let (m, e) = (x["marketId"].as_str(), x["exchangeId"].as_str());
        need(m.is_some() && e.is_some() && !markets.contains_key(m.unwrap()) && !exchanges.contains(e.unwrap()), "INVALID_UNIVERSE")?;
        markets.insert(m.unwrap().into(), e.unwrap().into());
        exchanges.insert(e.unwrap().into());
    }
    let mut seen: HashSet<String> = HashSet::new();
    for (i, p) in bank["pairs"].as_array().unwrap().iter().enumerate() {
        let definition = &bank["definitions"][i];
        need(hex(&p["id"])
            && p["id"].as_str().is_some_and(|s| !seen.contains(s))
            && strict_eq(&definition["id"], &p["id"])
            && strict_eq(&p["bank"]["tournamentId"], &universe["tournamentId"])
            && strict_eq(&definition["tournamentId"], &universe["tournamentId"])
            && digest(&definition["legs"]) == digest(&p["bank"]["legs"])
            && strict_eq(&p["bank"]["sourceHash"], &bank["sourceHash"])
            && strict_eq(&p["bank"]["cutoff"], &bank["cutoff"]),
            "FOREIGN_BASKET_SCOPE")?;
        seen.insert(p["id"].as_str().unwrap().into());
        for l in p["bank"]["legs"].as_array().ok_or_else(|| "p.bank.legs is not iterable".to_string())? {
            let ok = match l["marketId"].as_str() {
                Some(mid) => markets.get(mid).map(String::as_str) == l["exchangeId"].as_str(),
                None => l.get("exchangeId").is_none(),
            };
            need(ok, "FOREIGN_BASKET_SCOPE")?;
        }
        let expected = create_basket_observer(
            p["bank"]["tournamentId"].as_str().unwrap_or(""), &p["bank"]["legs"],
            bank["sourceHash"].as_str().unwrap_or(""), bank["cutoff"].as_str().unwrap_or(""),
            &p["bank"]["settlementProof"], &p["bank"]["policy"])?;
        need(digest(&expected["bank"]) == digest(&p["bank"])
            && digest(&expected["state"]) == digest(&p["initialState"]),
            "INVALID_BASKET_INITIAL_STATE")?;
    }
    Ok(bank)
}

fn initial_followups(record: &V, output: &V, bank: &V, bank_hash: &str) -> Result<Vec<V>, String> {
    if record["event"].is_null() { return Err("Cannot read properties of undefined (reading 'basketMeasurement')".into()); }
    let e = &record["event"];
    let origin = &e["basketMeasurement"];
    let origin_ms = origin["receivedAt"].as_str().and_then(parse_iso_ms);
    let cycle_i = js_int(&origin["cycle"]);
    let every_i = js_int(&origin["every"]);
    if e["type"].as_str() != Some("cycle")
        || origin["reason"].as_str() != Some("paired_depth_measurement")
        || origin.get("followup").is_some()
        || !clock(&origin["receivedAt"])
        || cycle_i.map_or(true, |c| c < 0)
        || every_i.map_or(true, |v| v < 1)
        || cycle_i.unwrap() % every_i.unwrap() != 0
    { return Ok(vec![]); }
    let same_legs = |p: &V| -> bool {
        let (mids, xids) = (origin["marketIds"].as_array(), origin["exchangeIds"].as_array());
        mids.as_ref().is_some_and(|m| m.len() == 2 && !strict_eq(&m[0], &m[1]))
            && xids.as_ref().is_some_and(|x| x.len() == 2)
            && p["bank"]["legs"].as_array().is_some_and(|ls| ls.iter().all(|l| {
                mids.as_ref().unwrap().iter().enumerate()
                    .any(|(i, id)| strict_eq(id, &l["marketId"]) && strict_eq(&xids.as_ref().unwrap()[i], &l["exchangeId"]))
            }))
    };
    let snapshots: Vec<V> = spread(&e["sourceSnapshots"])?.into_iter().chain(spread(&e["supplementalDepth"])?.into_iter()).collect();
    let excluded_opt: Option<BTreeSet<String>> = match &e["accountExclusions"] {
        v if !js_truthy(Some(v)) => Some(BTreeSet::new()),
        V::Array(a) => Some(a.iter().filter_map(|x| x.as_str().map(String::from)).collect()),
        V::String(s) => Some(s.chars().map(|c| c.to_string()).collect()),
        _ => None, // new Set(non-iterable) throws inside the per-row try -> false
    };
    let mut found = Vec::new();
    for o in output["observations"].as_array().cloned().unwrap_or_default() {
        let Some(p) = bank["pairs"].as_array().unwrap().iter().find(|x| strict_eq(&x["id"], &o["pairId"])) else { continue };
        let o_at = o["at"].as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN);
        if !same_legs(p)
            || o["status"].as_str() != Some("apparent_positive_basket")
            || o["apparentPositive"] != V::Bool(true)
            || o["newPairEvidence"] != V::Bool(true)
            || o["independentPair"] != V::Bool(false)
            || js_int(&o["positivePersistenceCount"]) != Some(1)
            || origin_ms.unwrap() > o_at
        { continue; }
        // Both legs must have been fetched in this exact initiating cycle. A cached
        // leg, incidental pair, or an observation invalidated later in the cycle is
        // insufficient, even if it appears positive in the genuine observer output.
        let legs = p["bank"]["legs"].as_array().cloned().unwrap_or_default();
        let pid = js_str(&p["id"]);
        let mut exact = true;
        for (i, l) in legs.iter().enumerate() {
            let leg_ok = (|| -> Result<bool, String> {
                let current = &output["state"]["pairs"][pid.as_str()]["books"][js_str(&l["exchangeId"]).as_str()];
                if !js_truthy(Some(current)) { return Ok(false); }
                // o.pairReceipts[i] / o.pairVersions[i] throw in JS when the field is missing.
                if o["pairReceipts"].is_null() { return Err("Cannot read properties of undefined".into()); }
                if !strict_eq(&current["receivedAt"], &o["pairReceipts"][i]) { return Ok(false); }
                if o["pairVersions"].is_null() { return Err("Cannot read properties of undefined".into()); }
                if !strict_eq(&current["versionKey"], &o["pairVersions"][i]) { return Ok(false); }
                let expected_ep = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
                    js_str(&l["marketId"]), js_str(&bank["pairs"][0]["bank"]["tournamentId"]));
                Ok(snapshots.iter().any(|r| {
                    if js_truthy(r.get("skipped"))
                        || !strict_eq(&r["receivedAt"], &o["pairReceipts"][i])
                        || !strict_eq(&r["endpoint"], &V::from(expected_ep.as_str()))
                    { return false; }
                    let Some(excluded) = &excluded_opt else { return false; };
                    let Ok(b) = normalize_book(&r["data"], l["marketId"].as_str().unwrap_or(""),
                        p["bank"]["tournamentId"].as_str().unwrap_or(""),
                        r["receivedAt"].as_str().unwrap_or(""), r["requestStartedAt"].as_str().unwrap_or(""),
                        20000, excluded, "authoritative_receipt_v1")
                    else { return false; };
                    strict_eq(&b["versionKey"], &o["pairVersions"][i])
                        && b["requestStartedAt"].as_str().and_then(parse_iso_ms)
                            .zip(origin_ms)
                            .is_some_and(|(r_ms, o_ms)| r_ms >= o_ms)
                }))
            })()?;
            if !leg_ok { exact = false; break; }
        }
        if exact {
            found.push(json!({
                "tournamentId": p["bank"]["tournamentId"], "observerBankHash": bank_hash, "pairId": p["id"],
                "sourceSequence": record["sequence"], "sourceHash": record["hash"], "cutoff": bank["cutoff"],
                "marketIds": legs.iter().map(|l| l["marketId"].clone()).collect::<Vec<_>>(),
                "exchangeIds": legs.iter().map(|l| l["exchangeId"].clone()).collect::<Vec<_>>(),
                "observation": o, "sourceMeasurement": origin,
            }));
        }
    }
    Ok(found)
}

struct Pending { row: V, followups: Vec<V>, source_span: Span }
struct Proof { followup: V, source_span: Span, observer_span: Span }

/// createBasketFollowupSource({runtimeDir, bank, universe, now})
/// `live_paper_dir` supplies the JS sources that basketCodeIdentity() digests;
/// `now` must be a safe-integer millisecond clock.
pub struct BasketFollowupSource<F: Fn() -> i64> {
    live_paper_dir: PathBuf,
    bank: V,
    bank_hash: String,
    source: JournalReader,
    observer: JournalReader,
    prefix: Sha256,
    pending: Vec<Pending>,
    proofs: Vec<Proof>,
    source_sequence: f64,
    source_hash: String,
    source_at: f64,
    state: Option<V>,
    derived_sequence: i64,
    derived_hash: String,
    journal_sequence: V,
    journal_hash: String,
    code: String,
    failed: bool,
    closed: bool,
    last_now: i64,
    rejected_origins: i64,
    now: F,
}
pub fn create_basket_followup_source<F: Fn() -> i64>(
    runtime_dir: &Path, live_paper_dir: &Path, input: &V, universe: &V, now: F,
) -> Result<BasketFollowupSource<F>, String> {
    // INVALID_FOLLOWUP_SOURCE_OPTIONS is subsumed by the typed signature.
    let bank = checked_bank(input, universe, live_paper_dir)?;
    let bank_hash = digest(&bank);
    Ok(BasketFollowupSource {
        live_paper_dir: live_paper_dir.to_path_buf(),
        bank,
        bank_hash: bank_hash.clone(),
        source: JournalReader::new(runtime_dir.join("journal.jsonl")),
        observer: JournalReader::new(runtime_dir.join("basket-journal.jsonl")),
        prefix: Sha256::new(),
        pending: Vec::new(),
        proofs: Vec::new(),
        source_sequence: 0.0,
        source_hash: String::new(),
        source_at: f64::NEG_INFINITY,
        state: None,
        derived_sequence: 0,
        derived_hash: bank_hash.clone(),
        journal_sequence: json!(0),
        journal_hash: bank_hash,
        code: "not_polled".into(),
        failed: false,
        closed: false,
        last_now: i64::MIN,
        rejected_origins: 0,
        now,
    })
}
impl<F: Fn() -> i64> BasketFollowupSource<F> {
    /// poll() — planner followups; empty on failure, backlog or staleness.
    pub fn poll(&mut self) -> Vec<V> {
        if self.closed || self.failed { return vec![]; }
        match self.poll_inner() {
            Ok(r) => r,
            Err(error) => {
                self.failed = true;
                self.proofs.clear();
                self.pending.clear();
                // Never publish parser errors, paths or raw journal contents.
                self.code = if upper_code(&error) { error } else { "FOLLOWUP_SOURCE_INVALID".into() };
                vec![]
            }
        }
    }
    fn poll_inner(&mut self) -> Result<Vec<V>, String> {
        let at = (self.now)();
        need(at >= 0 && at >= self.last_now, "INVALID_FOLLOWUP_CLOCK")?;
        self.last_now = at;
        need(digest(&self.bank["codeIdentity"]) == digest(&basket_code_identity(&self.live_paper_dir)?),
            "FROZEN_BASKET_CODE_CHANGED")?;
        let ss = self.source.snapshot()?;
        let os = self.observer.snapshot()?;
        if !ss.complete || !os.complete { self.code = "awaiting_complete_append".into(); return Ok(vec![]); }
        let mut source_done = false;
        loop {
            if !self.pending.is_empty() {
                if let Some(item) = self.observer.peek(&os)? {
                    let wanted = self.pending.remove(0);
                    let hash = item.row["hash"].as_str().unwrap_or("").to_string();
                    let mut body = item.row.clone();
                    if let Some(m) = body.as_object_mut() { m.shift_remove("hash"); }
                    need(!hash.is_empty() && hash == digest(&body)
                        && digest(&item.row) == digest(&wanted.row),
                        "BASKET_REPLAY_MISMATCH")?;
                    self.journal_sequence = item.row["sequence"].clone();
                    self.journal_hash = hash;
                    for followup in &wanted.followups {
                        self.proofs.push(Proof {
                            followup: followup.clone(),
                            source_span: wanted.source_span.clone(),
                            observer_span: item.span.clone(),
                        });
                    }
                    while self.proofs.len() > MAX_PROOFS { self.proofs.remove(0); }
                    self.observer.consume()?;
                    continue;
                }
            }
            if self.pending.len() >= MAX_PENDING { self.code = "awaiting_observer_backlog".into(); break; }
            let Some(item) = self.source.peek(&ss)? else { source_done = true; break; };
            let r = &item.row;
            let hash = r["hash"].as_str().unwrap_or("").to_string();
            let mut body = r.clone();
            if let Some(m) = body.as_object_mut() { m.shift_remove("hash"); }
            let stamp = if js_truthy(r["event"].get("clockAt")) { &r["event"]["clockAt"] } else { &r["at"] };
            need(body["sequence"].is_number()
                && body["sequence"].as_f64().unwrap() == self.source_sequence + 1.0
                && body["previous"].as_str() == Some(self.source_hash.as_str())
                && !hash.is_empty() && hash == digest(&body),
                "SOURCE_INTEGRITY_FAILURE")?;
            let stamp_ms = stamp.as_str().and_then(parse_iso_ms);
            need(clock(stamp) && stamp_ms.unwrap() >= self.source_at, "SOURCE_CLOCK_ROLLBACK")?;
            self.source_sequence = r["sequence"].as_f64().unwrap_or(f64::NAN);
            self.source_hash = hash;
            self.source_at = stamp_ms.unwrap();
            self.prefix.update(&item.bytes);
            let bank_source_records = js_int(&self.bank["sourceRecords"]).unwrap() as f64;
            if self.source_sequence == bank_source_records {
                need(format!("{:x}", self.prefix.clone().finalize()) == self.bank["sourceHash"].as_str().unwrap_or("")
                    && self.source_at <= parse_iso_ms(self.bank["cutoff"].as_str().unwrap_or("")).unwrap_or(f64::NAN),
                    "BASKET_SOURCE_PREFIX_CHANGED")?;
                let mut pairs = Map::new();
                for p in self.bank["pairs"].as_array().cloned().unwrap_or_default() {
                    pairs.insert(js_str(&p["id"]), p["initialState"].clone());
                }
                self.state = Some(json!({
                    "bankHash": self.bank_hash, "sourceSequence": self.source_sequence,
                    "lastRecordHash": self.source_hash, "pairs": V::Object(pairs),
                }));
            } else if self.source_sequence > bank_source_records {
                let bank = self.bank.clone();
                let state = self.state.as_ref()
                    .ok_or_else(|| "Cannot read properties of undefined (reading 'bankHash')".to_string())?;
                let output = advance_basket_record(state, r, &bank)?;
                self.derived_sequence += 1;
                let next = json!({
                    "sequence": self.derived_sequence, "previous": self.derived_hash,
                    "sourceSequence": self.source_sequence, "sourceHash": self.source_hash,
                    "stateHash": digest(&output["state"]), "observationsHash": digest(&output["observations"]),
                    "observations": output["observations"],
                });
                self.derived_hash = digest(&next);
                let followups = initial_followups(r, &output, &bank, &self.bank_hash)?;
                let positives = output["observations"].as_array().map(|o| {
                    o.iter().filter(|x| x["status"].as_str() == Some("apparent_positive_basket")).count() as i64
                }).unwrap_or(0);
                self.rejected_origins += positives - followups.len() as i64;
                let mut row = next.clone();
                row.as_object_mut().unwrap().insert("hash".into(), json!(self.derived_hash));
                self.pending.push(Pending { row, followups, source_span: item.span.clone() });
                self.state = Some(output["state"].clone());
            }
            self.source.consume()?;
        }
        // Freeze exactly the byte boundaries processed this poll. Concurrent appends
        // are picked up next time; edits of already verified anchors are fatal.
        let se = self.source.snapshot()?;
        let oe = self.observer.snapshot()?;
        need(se.size >= ss.size && oe.size >= os.size, "JOURNAL_TRUNCATED")?;
        self.source.remember(&ss)?;
        self.observer.remember(&os)?;
        while self.proofs.first().is_some_and(|p| {
            parse_iso_ms(p.followup["observation"]["at"].as_str().unwrap_or(""))
                .map(|o| at as f64 - o > MAX_AGE_MS).unwrap_or(false)
        }) { self.proofs.remove(0); }
        if !source_done { return Ok(vec![]); }
        if self.state.is_none() { self.code = "awaiting_source_prefix".into(); return Ok(vec![]); }
        if self.source_at > at as f64 { self.code = "awaiting_source_clock".into(); return Ok(vec![]); }
        if !self.pending.is_empty() || self.observer.peek(&os)?.is_some() {
            self.code = if !self.pending.is_empty() { "awaiting_observer".into() } else { "awaiting_source".into() };
            return Ok(vec![]);
        }
        if se.size != ss.size || oe.size != os.size || !se.complete || !oe.complete {
            self.code = "awaiting_complete_append".into();
            return Ok(vec![]);
        }
        let mut results = Vec::new();
        for proof in &self.proofs {
            let f = &proof.followup;
            let observed_at = parse_iso_ms(f["observation"]["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
            let mut skip = at as f64 <= observed_at + 1000.0 || at as f64 - observed_at > MAX_AGE_MS;
            if !skip {
                for v in f["observation"]["pairVersions"].as_array().ok_or_else(|| "f.observation.pairVersions is not iterable".to_string())? {
                    let s = v.as_str().ok_or_else(|| "v.slice is not a function".to_string())?;
                    let idx = s.find(':').map(|i| i + 1).unwrap_or(0);
                    if instant_ns(&s[idx..]).map_err(|e| e.0)? > at as i128 * 1_000_000 { skip = true; break; }
                }
            }
            if skip { continue; }
            self.source.verify(&proof.source_span)?;
            self.observer.verify(&proof.observer_span)?;
            results.push(f.clone());
        }
        self.code = "verified".into();
        Ok(results)
    }
    /// status() — compact identities and counters only.
    pub fn status(&self) -> V {
        json!({
            "schemaVersion": 1, "status": self.code, "bankHash": self.bank_hash, "cutoff": self.bank["cutoff"],
            "sourceSequence": self.source_sequence, "sourceHash": self.source_hash,
            "journalSequence": self.journal_sequence, "journalHash": self.journal_hash,
            "pendingRows": self.pending.len(), "retainedProofs": self.proofs.len(),
            "sourceBytesRead": self.source.progress().0, "observerBytesRead": self.observer.progress().0,
            "rejectedOrigins": self.rejected_origins, "maxPendingRows": MAX_PENDING, "maxRetainedProofs": MAX_PROOFS,
            "eligibleForExecution": false, "networkRequests": 0,
        })
    }
    /// close() — drop retained proofs/pending/state.
    pub fn close(&mut self) {
        self.closed = true;
        self.proofs.clear();
        self.pending.clear();
        self.state = None;
        self.code = "closed".into();
    }
}
