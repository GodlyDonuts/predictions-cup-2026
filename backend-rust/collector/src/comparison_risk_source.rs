// live-paper/comparison-risk-source.mjs — risk source adapter over the frozen
// entry-comparison cohort. Local append-only journals are the trust boundary:
// this validates their hash chain and wallet accounting, not official orders or
// original-source replay.

use baal_service_runtime::json_text::{digest, js_int, sha256};
use crate::entry_comparison::{empty_entry_comparison_state, ENTRY_COMPARISON_MARKETS};
use crate::entry_comparison_watch::entry_comparison_code_identity;
use crate::model::assert_state;
use serde_json::{json, Map, Value as V};
use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

const CHUNK: usize = 1024 * 1024;
const MAX_RECORD: usize = 16 * 1024 * 1024;
const HEARTBEAT_MS: f64 = 60_000.0;
const PROOFS: usize = 16;

/// JS distinguishes RiskError(code) from native throws; only the former surfaces
/// its message as `reason` — everything else becomes COMPARISON_EVIDENCE_UNAVAILABLE.
enum RiskErr {
    Risk(String),
    Other,
}
impl From<std::io::Error> for RiskErr { fn from(_: std::io::Error) -> Self { RiskErr::Other } }
impl From<serde_json::Error> for RiskErr { fn from(_: serde_json::Error) -> Self { RiskErr::Other } }
type RR<T> = Result<T, RiskErr>;

fn need(ok: bool, code: &str) -> RR<()> {
    if !ok { return Err(RiskErr::Risk(code.to_string())); }
    Ok(())
}
fn need_pub(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.to_string()); }
    Ok(())
}
fn object(x: &V) -> bool { x.is_object() }
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| s.len() == 64
        && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| !s.is_empty()
        && (b'1'..=b'9').contains(&s.as_bytes()[0])
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<f64>().map(|n| n.is_finite() && n.fract() == 0.0
            && n.abs() <= 9_007_199_254_740_991.0).unwrap_or(false))
}
fn clock(x: &V) -> bool {
    x.as_str().is_some_and(|s| baal_service_runtime::contract::parse_iso_ms(s)
        .is_some_and(|ms| baal_service_runtime::contract::ms_to_iso(ms) == s))
}
fn cohort_name(x: &V) -> bool {
    x.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !["con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5",
            "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5",
            "lpt6", "lpt7", "lpt8", "lpt9"].contains(&s))
}
fn parse(x: &V) -> f64 {
    x.as_str().and_then(baal_service_runtime::contract::parse_iso_ms).unwrap_or(f64::NAN)
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
fn same_code(a: &V, b: &V) -> bool {
    match (a.as_object(), b.as_object()) {
        (Some(x), Some(y)) => x.len() == y.len()
            && x.iter().all(|(k, v)| hex(v) && y.get(k).is_some_and(|b| strict_eq(b, v))),
        _ => false,
    }
}
fn bounded_json(file: &Path, max: u64) -> RR<(V, String)> {
    let stat = fs::metadata(file)?;              // IO failure -> Other
    need(stat.is_file() && stat.len() > 0 && stat.len() <= max, "INVALID_IDENTITY_FILE")?;
    let bytes = fs::read(file)?;
    need(bytes.len() as u64 <= max, "INVALID_IDENTITY_FILE")?;
    let value: V = serde_json::from_slice(&bytes)?;   // JSON.parse failure -> Other
    Ok((value, sha256(&bytes)))
}

struct Span { start: u64, length: usize, hash: String }
struct StatSnap { size: u64, mtime_ms: f64, ctime_ms: f64 }

pub struct ComparisonRiskSource {
    scope: V,
    markets: Vec<(String, String)>,   // insertion-ordered marketId -> exchangeId
    manifest_file: PathBuf,
    bank_file: PathBuf,
    journal_file: PathBuf,
    stats_file: PathBuf,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    now: Box<dyn Fn() -> f64>,
    configured: bool,
    closed: bool,
    failed: bool,
    reason: String,
    manifest_hash: Option<String>,
    bank: Option<V>,
    bank_hash: Option<String>,
    cohort: Option<String>,
    last_now: f64,
    identity: Option<(u64, u64, f64)>,  // dev, ino, birthtimeMs
    prior: Option<StatSnap>,
    head: Option<Span>,
    boundary: Option<Span>,
    offset: u64,
    consumed: u64,
    bytes_read: u64,
    partial: Vec<u8>,
    journal_sequence: i64,
    journal_hash: String,
    source_sequence: i64,
    source_hash: Option<V>,
    last_journal_at: f64,
    last_source_at: f64,
    held: Vec<String>,
    pending: Vec<String>,
    proofs: VecDeque<(String, V)>,      // insertion-ordered Map<sequence, proof>
}

fn mtime_ms(m: &fs::Metadata) -> f64 {
    m.mtime() as f64 * 1000.0 + m.mtime_nsec() as f64 / 1e6
}
fn ctime_ms(m: &fs::Metadata) -> f64 {
    m.ctime() as f64 * 1000.0 + m.ctime_nsec() as f64 / 1e6
}
fn birthtime_ms(m: &fs::Metadata) -> f64 {
    m.created().map(|t| t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)).unwrap_or(f64::NAN)
}

impl ComparisonRiskSource {
    fn time(&mut self) -> RR<f64> {
        let at = (self.now)();
        need(at.is_finite() && at.fract() == 0.0 && at.abs() <= 9_007_199_254_740_991.0
            && at >= 0.0 && at >= self.last_now, "INVALID_RISK_CLOCK")?;
        self.last_now = at;
        Ok(at)
    }

    fn result(&self, available: bool) -> V {
        json!({"schemaVersion": 1, "available": available, "configured": self.configured,
            "reason": self.reason,
            "cohort": self.cohort.clone().map_or(V::Null, |c| json!(c)),
            "bankHash": self.bank_hash.clone().map_or(V::Null, |h| json!(h)),
            "codeIdentity": self.bank.as_ref().map(|b| b["codeIdentity"].clone()).unwrap_or(V::Null),
            "sourceSequence": self.source_sequence,
            "sourceHash": self.source_hash.clone().unwrap_or(V::Null),
            "journalSequence": self.journal_sequence,
            "journalHash": if self.journal_hash.is_empty() { V::Null } else { json!(self.journal_hash) },
            "heldMarketIds": self.held, "pendingMarketIds": self.pending,
            "journalBytesRead": self.bytes_read, "journalBytesConsumed": self.consumed,
            "incompleteTailBytes": self.partial.len(),
            "retainedCheckpoints": self.proofs.len(),
            "eligibleForExecution": false, "networkRequests": 0, "runtimeWrites": 0})
    }

    fn bind(&mut self, manifest: (V, String)) -> RR<()> {
        let m = manifest.0;
        need(object(&m) && m.as_object().map(|o| o.len() == 3
            && o.keys().all(|k| ["schemaVersion", "cohort", "bankHash"].contains(&k.as_str())))
            .unwrap_or(false)
            && strict_eq(&m["schemaVersion"], &json!(1)) && cohort_name(&m["cohort"])
            && hex(&m["bankHash"]), "INVALID_COMPARISON_MANIFEST")?;
        self.cohort = m["cohort"].as_str().map(String::from);
        self.manifest_hash = Some(manifest.1);
        self.bank_hash = m["bankHash"].as_str().map(String::from);
        let dir = self.runtime_dir.join("entry-comparisons").join(m["cohort"].as_str().unwrap_or(""));
        self.bank_file = dir.join("bank.json");
        self.journal_file = dir.join("journal.jsonl");
        self.stats_file = dir.join("stats.json");
        let bank = bounded_json(&self.bank_file, 4 * 1024 * 1024)?.0;
        need(digest(&bank) == self.bank_hash.clone().unwrap_or_default(), "COMPARISON_BANK_CHANGED")?;
        need(same_code(&bank["codeIdentity"],
            &entry_comparison_code_identity(&self.workspace_dir).map_err(|_| RiskErr::Other)?),
            "COMPARISON_CODE_CHANGED")?;
        // Validate the frozen execution contract — its errors are plain Errors in JS,
        // i.e. non-RiskError -> COMPARISON_EVIDENCE_UNAVAILABLE.
        empty_entry_comparison_state(&bank).map_err(|_| RiskErr::Other)?;
        let t = self.time()?;
        need(clock(&bank["cutoff"])
            && parse(&bank["cutoff"]) <= t
            && strict_eq(&bank["config"]["tournamentId"], &self.scope["tournamentId"])
            && strict_eq(&bank["universe"]["tournamentId"], &self.scope["tournamentId"]),
            "FOREIGN_COMPARISON_BANK")?;
        let bex = bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default();
        need(bex.len() == self.scope["exchanges"].as_array().map(|a| a.len()).unwrap_or(0)
            && bex.iter().all(|x| x["option"].as_str() == Some("YES")
                && self.markets.iter().any(|(mk, ex)| {
                    *mk == js_str(&x["marketId"]) && strict_eq(&json!(ex), &x["exchangeId"])
                })), "FOREIGN_COMPARISON_UNIVERSE")?;
        need(ENTRY_COMPARISON_MARKETS.iter().all(|x| self.markets.iter().any(|(mk, _)| mk == x)),
            "FOREIGN_COMPARISON_UNIVERSE")?;
        self.bank = Some(bank);
        Ok(())
    }

    fn read_span(&self, start: u64, length: usize) -> RR<Vec<u8>> {
        let f = fs::File::open(&self.journal_file).map_err(|_| RiskErr::Other)?;
        let mut bytes = vec![0u8; length];
        let mut n = 0usize;
        while n < length {
            let got = f.read_at(&mut bytes[n..], start + n as u64).map_err(|_| RiskErr::Other)?;
            need(got > 0, "COMPARISON_JOURNAL_CHANGED")?;
            n += got;
        }
        Ok(bytes)
    }

    fn remember(&mut self, stat: &fs::Metadata) -> RR<()> {
        if self.head.is_none() && stat.len() > 0 {
            let length = std::cmp::min(4096usize, stat.len() as usize);
            self.head = Some(Span { start: 0, length, hash: sha256(&self.read_span(0, length)?) });
        }
        let length = std::cmp::min(4096u64, self.offset);
        self.boundary = if length > 0 {
            Some(Span { start: self.offset - length, length: length as usize,
                hash: sha256(&self.read_span(self.offset - length, length as usize)?) })
        } else { None };
        self.prior = Some(StatSnap { size: stat.len(), mtime_ms: mtime_ms(stat),
            ctime_ms: ctime_ms(stat) });
        Ok(())
    }

    fn checked_state(&mut self, state: &V, row: &V) -> RR<()> {
        let bank_hash = self.bank_hash.clone().unwrap_or_default();
        need(object(state) && strict_eq(&state["schemaVersion"], &json!(1))
            && strict_eq(&state["bankHash"], &json!(bank_hash))
            && js_int(&state["sourceSequence"]).is_some()
            && js_num(&state["sourceSequence"]) >= js_num(&self.bank.as_ref().unwrap()["sourceRecords"])
            && js_num(&state["sourceSequence"]) > self.source_sequence as f64
            && hex(&state["lastRecordHash"]), "INVALID_COMPARISON_CHECKPOINT")?;
        need(clock(&state["lastSourceAt"]) && parse(&state["lastSourceAt"]) >= self.last_source_at
            && parse(&state["lastSourceAt"]) <= parse(&row["at"]), "INVALID_COMPARISON_SOURCE_CLOCK")?;
        need(strict_eq(&row["event"]["sourceSequence"], &state["sourceSequence"])
            && strict_eq(&row["event"]["sourceHash"], &state["lastRecordHash"]),
            "COMPARISON_EVENT_MISMATCH")?;
        need(object(&state["strategies"])
            && state["strategies"].as_object().map(|o| o.len() == 2).unwrap_or(false)
            && ["historical_dip", "shock_rebound"].iter()
                .all(|k| object(&state["strategies"][*k])), "INVALID_COMPARISON_STRATEGIES")?;
        let mut next_held: Vec<String> = Vec::new();
        let mut next_pending: Vec<String> = Vec::new();
        for name in ["historical_dip", "shock_rebound"] {
            let wallet = &state["strategies"][name]["wallet"];
            let t = self.time()?;
            need(object(wallet) && object(&wallet["books"]) && clock(&wallet["lastAt"])
                && parse(&wallet["lastAt"]) <= t, "INVALID_COMPARISON_WALLET")?;
            assert_state(wallet, &self.bank.as_ref().unwrap()["config"]).map_err(|_| RiskErr::Other)?;
            for (instrument, b) in wallet["books"].as_object().map(|o| o.iter().collect::<Vec<_>>())
                .unwrap_or_default() {
                let t = self.time()?;
                need(object(b)
                    && ENTRY_COMPARISON_MARKETS.iter().any(|m| strict_eq(&b["marketId"], &json!(m)))
                    && self.markets.iter().any(|(mk, ex)| *mk == js_str(&b["marketId"])
                        && strict_eq(&json!(ex), &b["exchangeId"]))
                    && instrument.as_str() == format!("{}:YES", js_str(&b["exchangeId"]))
                    && strict_eq(&b["instrument"], &json!(instrument))
                    && clock(&b["receivedAt"]) && clock(&b["requestStartedAt"])
                    && parse(&b["requestStartedAt"]) <= parse(&b["receivedAt"])
                    && parse(&b["receivedAt"]) <= t, "FOREIGN_COMPARISON_BOOK")?;
            }
            let strategy = &wallet["strategies"]["mean_reversion"];
            for (rows, pending_flag) in [("positions", false), ("pending", true)] {
                let rows_obj = &strategy[rows];
                need(object(rows_obj), "INVALID_COMPARISON_RISK")?;
                let entries: Vec<(String, V)> = rows_obj.as_object()
                    .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                for (instrument, p) in entries {
                    let b = &wallet["books"][&instrument];
                    need(object(&p) && js_int(&p["quantity"]).is_some_and(|q| q > 0)
                        && object(b), "INVALID_COMPARISON_RISK")?;
                    let t = self.time()?;
                    if pending_flag {
                        need(["buy", "sell"].contains(&p["action"].as_str().unwrap_or(""))
                            && clock(&p["createdAt"]) && parse(&p["createdAt"]) <= t,
                            "INVALID_COMPARISON_PENDING")?;
                    } else {
                        need(clock(&p["enteredAt"]) && parse(&p["enteredAt"]) <= t,
                            "INVALID_COMPARISON_POSITION")?;
                    }
                    let target = if pending_flag { &mut next_pending } else { &mut next_held };
                    if !target.contains(&js_str(&b["marketId"])) {
                        target.push(js_str(&b["marketId"]));
                    }
                }
            }
        }
        next_held.sort();
        next_pending.sort();
        self.held = next_held;
        self.pending = next_pending;
        Ok(())
    }

    fn consume_row(&mut self, line: &[u8]) -> RR<()> {
        // JS toString('utf8') lossy-decodes then re-encodes to compare bytes; strict
        // UTF-8 validation covers the same check.
        let text = std::str::from_utf8(line).map_err(|_| RiskErr::Risk("INVALID_COMPARISON_UTF8".into()))?;
        let row: V = serde_json::from_str(text)?;
        let h = row["hash"].clone();
        let mut body = row.clone();
        if let Some(o) = body.as_object_mut() { o.shift_remove("hash"); }
        need(js_int(&row["sequence"]) == Some(self.journal_sequence + 1)
            && strict_eq(&row["previous"], &json!(self.journal_hash))
            && hex(&h) && digest(&body) == js_str(&h), "COMPARISON_CHAIN_MISMATCH")?;
        need(clock(&row["at"]) && parse(&row["at"]) >= self.last_journal_at
            && parse(&row["at"]) >= parse(&self.bank.as_ref().unwrap()["cutoff"])
            && parse(&row["at"]) <= self.time()?, "INVALID_COMPARISON_JOURNAL_CLOCK")?;
        self.checked_state(&row["state"], &row)?;
        self.journal_sequence = js_int(&row["sequence"]).unwrap_or(0);
        self.journal_hash = js_str(&h);
        self.source_sequence = js_int(&row["state"]["sourceSequence"]).unwrap_or(0);
        self.source_hash = Some(row["state"]["lastRecordHash"].clone());
        self.last_journal_at = parse(&row["at"]);
        self.last_source_at = parse(&row["state"]["lastSourceAt"]);
        self.proofs.push_back((js_str(&row["sequence"]), json!({
            "journalHash": h, "sourceSequence": self.source_sequence,
            "sourceHash": self.source_hash })));
        while self.proofs.len() > PROOFS { self.proofs.pop_front(); }
        Ok(())
    }

    fn stream(&mut self) -> RR<()> {
        let stat = fs::metadata(&self.journal_file).map_err(|_| RiskErr::Other)?;
        need(stat.is_file(), "INVALID_COMPARISON_JOURNAL")?;
        if self.identity.is_none() {
            self.identity = Some((stat.dev(), stat.ino(), birthtime_ms(&stat)));
        }
        let (d, i, bt) = self.identity.unwrap();
        need(stat.dev() == d && stat.ino() == i && birthtime_ms(&stat) == bt,
            "COMPARISON_JOURNAL_REPLACED")?;
        if let Some(prior) = &self.prior {
            need(stat.len() >= prior.size && stat.len() >= self.offset,
                "COMPARISON_JOURNAL_TRUNCATED")?;
            need(stat.len() != prior.size
                || (mtime_ms(&stat) == prior.mtime_ms && ctime_ms(&stat) == prior.ctime_ms),
                "COMPARISON_JOURNAL_CHANGED")?;
            for span in [&self.head, &self.boundary].into_iter().flatten() {
                need(sha256(&self.read_span(span.start, span.length)?) == span.hash,
                    "COMPARISON_JOURNAL_CHANGED")?;
            }
        }
        while self.offset < stat.len() {
            let take = std::cmp::min(CHUNK as u64, stat.len() - self.offset) as usize;
            let bytes = self.read_span(self.offset, take)?;
            self.offset += bytes.len() as u64;
            self.bytes_read += bytes.len() as u64;
            self.partial.extend_from_slice(&bytes);
            while let Some(end) = self.partial.iter().position(|b| *b == b'\n') {
                need(end > 0 && end + 1 <= MAX_RECORD, "INVALID_COMPARISON_RECORD_SIZE")?;
                let line = self.partial[..end].to_vec();
                self.consume_row(&line)?;
                self.consumed += end as u64 + 1;
                self.partial = self.partial[end + 1..].to_vec();
            }
            need(self.partial.len() < MAX_RECORD, "INVALID_COMPARISON_RECORD_SIZE")?;
        }
        self.remember(&stat)
    }

    fn heartbeat(&mut self, stats: Option<V>) -> RR<String> {
        let Some(stats) = stats else { return Ok("COMPARISON_WORKER_UNAVAILABLE".into()) };
        if !strict_eq(&stats["cohort"], &self.cohort.clone().map_or(V::Null, |c| json!(c)))
            || !strict_eq(&stats["bankHash"], &self.bank_hash.clone().map_or(V::Null, |h| json!(h)))
            || !same_code(&stats["codeIdentity"],
                &self.bank.as_ref().map(|b| b["codeIdentity"].clone()).unwrap_or(V::Null)) {
            return Ok("COMPARISON_WORKER_IDENTITY_MISMATCH".into());
        }
        let jsq = js_str(&stats["journalSequence"]);
        let proof = self.proofs.iter().find(|(k, _)| k.as_str() == jsq);
        let ok_proof = proof.is_some_and(|(_, p)| strict_eq(&stats["journalHash"], &p["journalHash"])
            && strict_eq(&stats["sourceSequence"], &p["sourceSequence"])
            && strict_eq(&stats["sourceHash"], &p["sourceHash"]));
        if !ok_proof { return Ok("COMPARISON_WORKER_CHECKPOINT_UNVERIFIED".into()); }
        let t = self.time()?;
        if !clock(&stats["updatedAt"]) || parse(&stats["updatedAt"]) > t
            || t - parse(&stats["updatedAt"]) > HEARTBEAT_MS {
            return Ok("COMPARISON_WORKER_STALE".into());
        }
        if stats["status"].as_str() != Some("running") {
            return Ok("COMPARISON_WORKER_NOT_RUNNING".into());
        }
        Ok("VERIFIED".into())
    }

    /// poll() — one bounded update; never throws (returns result(false) on failure).
    pub fn poll(&mut self) -> V {
        if self.closed { self.reason = "CLOSED".into(); return self.result(false); }
        if self.failed { return self.result(false); }
        match self.poll_inner() {
            Ok(v) => v,
            Err(RiskErr::Risk(code)) => {
                self.failed = true; self.reason = code; self.result(false)
            }
            Err(RiskErr::Other) => {
                self.failed = true; self.reason = "COMPARISON_EVIDENCE_UNAVAILABLE".into();
                self.result(false)
            }
        }
    }

    fn poll_inner(&mut self) -> RR<V> {
        self.time()?;
        if !self.manifest_file.exists() {
            need(!self.configured, "COMPARISON_MANIFEST_MISSING")?;
            self.reason = "NOT_CONFIGURED".into();
            return Ok(self.result(true));
        }
        self.configured = true;
        let manifest = bounded_json(&self.manifest_file, 4096)?;
        if self.manifest_hash.is_none() { self.bind(manifest)?; }
        else {
            need(Some(&manifest.1) == self.manifest_hash.as_ref(), "COMPARISON_MANIFEST_CHANGED")?;
        }
        need(digest(&bounded_json(&self.bank_file, 4 * 1024 * 1024)?.0)
            == self.bank_hash.clone().unwrap_or_default(), "COMPARISON_BANK_CHANGED")?;
        need(same_code(&self.bank.as_ref().unwrap()["codeIdentity"],
            &entry_comparison_code_identity(&self.workspace_dir).map_err(|_| RiskErr::Other)?),
            "COMPARISON_CODE_CHANGED")?;
        let stats = bounded_json(&self.stats_file, 4 * 1024 * 1024).ok().map(|x| x.0);
        self.stream()?;
        self.reason = if !self.partial.is_empty() { "COMPARISON_INCOMPLETE_TAIL".into() }
            else if self.journal_sequence == 0 { "COMPARISON_EMPTY_JOURNAL".into() }
            else { self.heartbeat(stats)? };
        let available = self.reason == "VERIFIED";
        Ok(self.result(available))
    }

    /// close() — releases the tail; idempotent.
    pub fn close(&mut self) {
        self.closed = true;
        self.partial = Vec::new();
    }
}

/// createComparisonRiskSource({runtimeDir, universe, now}) — the frozen object.
/// `workspace_dir` locates the repo root for entryComparisonCodeIdentity.
pub fn create_comparison_risk_source(runtime_dir: &str, universe: &V,
    now: impl Fn() -> f64 + 'static, workspace_dir: &Path)
    -> Result<ComparisonRiskSource, String> {
    need_pub(Path::new(runtime_dir).is_absolute(), "INVALID_RISK_SOURCE_OPTIONS")?;
    need_pub(object(universe) && universe["tournamentId"].is_string()
        && universe["exchanges"].is_array(), "INVALID_RISK_UNIVERSE")?;
    let scope = universe.clone();
    let mut markets: Vec<(String, String)> = Vec::new();
    let mut exchanges: Vec<String> = Vec::new();
    for row in scope["exchanges"].as_array().cloned().unwrap_or_default() {
        let ok = id(&row["marketId"]) && id(&row["exchangeId"])
            && row["option"].as_str() == Some("YES")
            && !markets.iter().any(|(m, _)| *m == js_str(&row["marketId"]))
            && !exchanges.iter().any(|e| *e == js_str(&row["exchangeId"]));
        need_pub(ok, "INVALID_RISK_UNIVERSE")?;
        markets.push((js_str(&row["marketId"]), js_str(&row["exchangeId"])));
        exchanges.push(js_str(&row["exchangeId"]));
    }
    Ok(ComparisonRiskSource {
        scope,
        markets,
        manifest_file: Path::new(runtime_dir).join("entry-comparison-active.json"),
        bank_file: PathBuf::new(),
        journal_file: PathBuf::new(),
        stats_file: PathBuf::new(),
        runtime_dir: Path::new(runtime_dir).to_path_buf(),
        workspace_dir: workspace_dir.to_path_buf(),
        now: Box::new(now),
        configured: false, closed: false, failed: false,
        reason: "NOT_POLLED".into(),
        manifest_hash: None, bank: None, bank_hash: None, cohort: None,
        last_now: f64::NEG_INFINITY, identity: None, prior: None, head: None,
        boundary: None, offset: 0, consumed: 0, bytes_read: 0, partial: Vec::new(),
        journal_sequence: 0, journal_hash: String::new(), source_sequence: 0,
        source_hash: None, last_journal_at: f64::NEG_INFINITY,
        last_source_at: f64::NEG_INFINITY, held: Vec::new(), pending: Vec::new(),
        proofs: VecDeque::new(),
    })
}
