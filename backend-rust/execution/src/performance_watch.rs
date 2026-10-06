// execution/performance-watch.mjs — projection only. It cannot read
// credentials or dispatch an account request. The module's CLI `main` is not
// ported (no CLI sibling pattern exists in this crate).
use crate::candidate_markouts_audit::book_publication::{
    apply_book_publication, create_book_publication_state,
};
use baal_service_runtime::contract::{instant_ns, parse_iso_ms};
use baal_service_runtime::json_text::{digest, js_int, js_truthy};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn instant_ns_of(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("\u{0}")).map_err(|e| e.0)
}
fn parse_ms(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
// JS BigInt(v) for the decimal-string (or integral number) sequences the
// normalizer emits; any other value raises like BigInt()'s TypeError/RangeError.
fn js_bigint(v: &V) -> Result<i128, String> {
    match v {
        V::Number(_) => js_int(v).map(|n| n as i128).ok_or_else(String::new),
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Ok(0)
            } else {
                t.parse::<i128>().map_err(|_| String::new())
            }
        }
        _ => Err(String::new()),
    }
}
// /^\/markets\/([1-9]\d*)\/orderbook\?/.exec(endpoint)?.[1]
fn market_from_endpoint(endpoint: &str) -> Option<String> {
    let rest = endpoint.strip_prefix("/markets/")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.starts_with('0') {
        return None;
    }
    rest[digits.len()..].starts_with("/orderbook?").then_some(digits)
}
// [...(e.sourceSnapshots||[]), ...(e.supplementalDepth||[])] — JS spreads a
// string char-wise and throws TypeError on other truthy non-iterables.
fn spread_rows(v: Option<&V>) -> Result<Vec<V>, String> {
    let v = match v {
        Some(v) if js_truthy(Some(v)) => v,
        _ => return Ok(Vec::new()),
    };
    match v {
        V::Array(a) => Ok(a.clone()),
        V::String(s) => Ok(s.chars().map(|c| json!(c.to_string())).collect()),
        _ => Err("e.sourceSnapshots is not iterable".into()),
    }
}

/// createPerformanceCapture(universe)
pub struct PerformanceCapture {
    universe_tournament_id: String,
    scope: Vec<(V, V)>,                  // marketId -> exchangeId (JS Map)
    books: Map<String, V>,               // exchangeId -> book, insertion order
    seen: HashMap<String, V>,            // exchangeId -> newest accepted book
    invalidated: HashMap<String, f64>,   // exchangeId -> ms barrier
    sequence: i64,
    previous: String,
    source_at: Option<String>,
}

/// createPerformanceCapture(universe)
pub fn create_performance_capture(universe: &V) -> PerformanceCapture {
    let scope = universe
        .get("exchanges")
        .and_then(V::as_array)
        .map(|a| {
            a.iter()
                .map(|x| {
                    (
                        x.get("marketId").cloned().unwrap_or(V::Null),
                        x.get("exchangeId").cloned().unwrap_or(V::Null),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    PerformanceCapture {
        universe_tournament_id: universe.get("tournamentId").and_then(V::as_str).unwrap_or("").to_string(),
        scope,
        books: Map::new(),
        seen: HashMap::new(),
        invalidated: HashMap::new(),
        sequence: 0,
        previous: String::new(),
        source_at: None,
    }
}

impl PerformanceCapture {
    fn invalidate(&mut self, id: &str, at_ms: f64) {
        self.books.shift_remove(id);
        let cur = self.invalidated.get(id).copied().unwrap_or(0.0);
        self.invalidated.insert(id.to_string(), cur.max(at_ms));
    }
    /// consume(record) — verify one journal record, then fold a cycle's book
    /// rows into the per-exchange projection.
    pub fn consume(&mut self, record: &V) -> Result<(), String> {
        let mut body = if record.is_object() { record.clone() } else { json!({}) };
        let hash = body
            .as_object_mut()
            .and_then(|m| m.shift_remove("hash"))
            .unwrap_or(V::Null);
        if body.get("sequence").and_then(V::as_f64) != Some((self.sequence + 1) as f64)
            || body.get("previous").and_then(V::as_str) != Some(self.previous.as_str())
            || hash.as_str() != Some(digest(&body).as_str())
        {
            return Err("SOURCE_INTEGRITY_FAILURE".into());
        }
        let e = body.get("event").cloned().unwrap_or(V::Null);
        let at = if js_truthy(e.get("clockAt")) {
            e.get("clockAt").unwrap().clone()
        } else {
            body.get("at").cloned().unwrap_or(V::Null)
        };
        let at_ms = at.as_str().and_then(parse_iso_ms);
        if at_ms.is_none()
            || (self.source_at.is_some()
                && at_ms.unwrap() < self.source_at.as_ref().and_then(|s| parse_iso_ms(s)).unwrap_or(f64::NAN))
        {
            return Err("SOURCE_CLOCK_INVALID".into());
        }
        let at_ms = at_ms.unwrap();
        if e.get("type").and_then(V::as_str) == Some("account_read_outage") {
            let ids: Vec<String> =
                self.scope.iter().map(|(_, v)| crate::js_key(v)).collect();
            for id in ids {
                self.invalidate(&id, at_ms);
            }
        }
        if e.get("type").and_then(V::as_str) == Some("cycle") {
            let mut deferred: HashSet<String> = HashSet::new();
            let mut accepted_this_cycle: HashSet<String> = HashSet::new();
            let mut rows: Vec<(usize, V, V)> = Vec::new();
            for (index, r) in spread_rows(e.get("sourceSnapshots"))?
                .into_iter()
                .chain(spread_rows(e.get("supplementalDepth"))?)
                .enumerate()
            {
                let t = if js_truthy(r.get("receivedAt")) {
                    r.get("receivedAt").unwrap().clone()
                } else {
                    r.get("at").cloned().unwrap_or(V::Null)
                };
                rows.push((index, t, r));
            }
            rows.sort_by(|a, b| {
                let ka = parse_ms(&a.1);
                let kb = parse_ms(&b.1);
                let ka = if ka.is_nan() { f64::INFINITY } else { ka };
                let kb = if kb.is_nan() { f64::INFINITY } else { kb };
                ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0))
            });
            for (_index, t, r) in rows {
                let market_id = if js_truthy(r.get("marketId")) {
                    r.get("marketId").unwrap().clone()
                } else {
                    market_from_endpoint(r.get("endpoint").and_then(V::as_str).unwrap_or(""))
                        .map(V::String)
                        .unwrap_or(V::Null)
                };
                let exchange_id = self
                    .scope
                    .iter()
                    .find(|(k, _)| *k == market_id)
                    .map(|(_, v)| v.clone());
                let Some(exchange_id) = exchange_id else { continue };
                let key = crate::js_key(&exchange_id);
                let t_ms = t.as_str().and_then(parse_iso_ms);
                if t_ms.is_none() || t_ms.unwrap() > at_ms {
                    deferred.insert(key);
                    continue;
                }
                let t_ms = t_ms.unwrap();
                if js_truthy(r.get("skipped")) {
                    // The collector records raw depth before the paper engine
                    // excludes real account inventory. That exact paper-only
                    // marker does not erase a book which this cycle already
                    // validated for attributed official valuation.
                    let keep = r.get("skipped").and_then(V::as_str)
                        == Some("Exchange has real account holdings or open orders")
                        && e.get("accountExclusions").and_then(V::as_array).is_some_and(|a| {
                            a.iter().any(|x| *x == exchange_id)
                        })
                        && accepted_this_cycle.contains(&key)
                        && self.books.contains_key(&key)
                        && t_ms
                            >= self
                                .books
                                .get(&key)
                                .and_then(|b| b.get("receivedAt"))
                                .map(parse_ms)
                                .unwrap_or(f64::NAN);
                    if !keep {
                        self.invalidate(&key, t_ms);
                    }
                    continue;
                }
                let evaluated = (|| -> Result<V, String> {
                    let endpoint = format!(
                        "/markets/{}/orderbook?tournamentId={}&depth=20",
                        crate::js_key(&market_id),
                        self.universe_tournament_id
                    );
                    if r.get("endpoint").and_then(V::as_str) != Some(endpoint.as_str()) {
                        return Err(String::new());
                    }
                    let mut book = normalize_book(
                        r.get("data").unwrap_or(&V::Null),
                        &crate::js_key(&market_id),
                        &self.universe_tournament_id,
                        r.get("receivedAt").and_then(V::as_str).unwrap_or("\u{0}"),
                        r.get("requestStartedAt").and_then(V::as_str).unwrap_or("\u{0}"),
                        20000.0,
                        &BTreeSet::new(),
                        "authoritative_receipt_v1",
                    )?;
                    if let Some(m) = book.as_object_mut() {
                        m.insert("tournamentId".into(), json!(self.universe_tournament_id));
                    } else {
                        book = json!({"tournamentId": self.universe_tournament_id});
                    }
                    if book.get("exchangeId") != Some(&exchange_id)
                        || parse_ms(book.get("requestStartedAt").unwrap_or(&V::Null))
                            < self.invalidated.get(&key).copied().unwrap_or(0.0)
                    {
                        return Err(String::new());
                    }
                    if let Some(old) = self.seen.get(&key) {
                        let bad = parse_ms(book.get("requestStartedAt").unwrap_or(&V::Null))
                            < parse_ms(old.get("requestStartedAt").unwrap_or(&V::Null))
                            || parse_ms(book.get("receivedAt").unwrap_or(&V::Null))
                                <= parse_ms(old.get("receivedAt").unwrap_or(&V::Null))
                            || js_bigint(book.get("sequence").unwrap_or(&V::Null))?
                                < js_bigint(old.get("sequence").unwrap_or(&V::Null))?
                            || instant_ns_of(book.get("sourceAt").unwrap_or(&V::Null))?
                                < instant_ns_of(old.get("sourceAt").unwrap_or(&V::Null))?
                            || (book.get("sequence") == old.get("sequence")
                                && (book.get("versionKey") != old.get("versionKey")
                                    || digest(&json!([
                                        book.get("bids").cloned().unwrap_or(V::Null),
                                        book.get("asks").cloned().unwrap_or(V::Null)
                                    ])) != digest(&json!([
                                        old.get("bids").cloned().unwrap_or(V::Null),
                                        old.get("asks").cloned().unwrap_or(V::Null)
                                    ]))));
                        if bad {
                            return Err(String::new());
                        }
                    }
                    Ok(book)
                })();
                match evaluated {
                    Ok(book) => {
                        self.seen.insert(key.clone(), book.clone());
                        self.books.insert(key.clone(), book);
                        accepted_this_cycle.insert(key);
                    }
                    Err(_) => self.invalidate(&key, t_ms),
                }
            }
            for id in deferred {
                self.invalidate(&id, at_ms);
            }
        }
        self.sequence = body.get("sequence").and_then(V::as_f64).unwrap_or(self.sequence as f64) as i64;
        self.previous = hash.as_str().unwrap_or("").to_string();
        self.source_at = at.as_str().map(String::from);
        Ok(())
    }
    /// status()
    pub fn status(&self) -> V {
        json!({
            "sourceSequence": self.sequence,
            "sourceHash": self.previous,
            "sourceAt": self.source_at,
            "books": self.books.values().cloned().collect::<Vec<V>>(),
        })
    }
}

fn file_ino(md: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::ino(md)
    }
    #[cfg(not(unix))]
    {
        let _ = md;
        0
    }
}
// JS stat.birthtimeMs — 0 when the filesystem does not report one.
fn birthtime_ms(md: &fs::Metadata) -> f64 {
    md.created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// openPerformanceTail(file, capture) — incremental newline reader; `capture`
/// arrives as the `consume` callback argument on each poll.
pub struct PerformanceTail {
    file: PathBuf,
    offset: u64,
    partial: Vec<u8>,
    identity_ino: u64,
    identity_birthtime_ms: f64,
}
pub fn open_performance_tail(file: &Path) -> Result<PerformanceTail, String> {
    let identity = fs::metadata(file).map_err(|e| e.to_string())?;
    Ok(PerformanceTail {
        file: file.to_path_buf(),
        offset: 0,
        partial: Vec::new(),
        identity_ino: file_ino(&identity),
        identity_birthtime_ms: birthtime_ms(&identity),
    })
}
impl PerformanceTail {
    /// One poll of the JS returned closure; returns whether unread bytes remain.
    pub fn poll(&mut self, consume: &mut dyn FnMut(&V) -> Result<(), String>) -> Result<bool, String> {
        let stat = fs::metadata(&self.file).map_err(|e| e.to_string())?;
        if stat.len() < self.offset
            || file_ino(&stat) != self.identity_ino
            || birthtime_ms(&stat) != self.identity_birthtime_ms
        {
            return Err("SOURCE_REPLACED_OR_TRUNCATED".into());
        }
        let length = (stat.len() - self.offset).min(4 * 1024 * 1024) as usize;
        if length == 0 {
            return Ok(false);
        }
        let mut chunk = vec![0u8; length];
        let n = {
            // fs.readSync positional read; fd closes on drop like JS finally{closeSync}.
            let mut fd = fs::File::open(&self.file).map_err(|e| e.to_string())?;
            fd.seek(SeekFrom::Start(self.offset)).and_then(|_| fd.read(&mut chunk))
        };
        let n = n.map_err(|e| e.to_string())?;
        self.offset += n as u64;
        let mut bytes = std::mem::take(&mut self.partial);
        bytes.extend_from_slice(&chunk[..n]);
        let end = bytes.iter().rposition(|b| *b == b'\n');
        if let Some(end) = end {
            for line in bytes[..end].split(|b| *b == b'\n') {
                if line.is_empty() {
                    return Err("SOURCE_EMPTY_RECORD".into());
                }
                consume(&serde_json::from_slice(line).map_err(|e| e.to_string())?)?;
            }
            self.partial = bytes[end + 1..].to_vec();
        } else {
            self.partial = bytes;
        }
        if self.partial.len() > 8 * 1024 * 1024 {
            return Err("SOURCE_OVERSIZED_RECORD".into());
        }
        Ok(self.offset < stat.len())
    }
}

/// createPerformanceBookSource(universe, legacy) — once the immediate stream
/// is present it owns the entire book projection. Missing, invalidated, or
/// closed-session books must never fall back to older cycle snapshots. The
/// `legacy` capture arrives as an argument to `status` instead of being
/// captured at construction (Rust ownership; identical observable behavior).
pub struct PerformanceBookSource {
    universe: V,
    state: Option<V>,
}
pub fn create_performance_book_source(universe: &V) -> PerformanceBookSource {
    PerformanceBookSource { universe: universe.clone(), state: None }
}
impl PerformanceBookSource {
    pub fn use_publication(&mut self) -> Result<(), String> {
        if self.state.is_none() {
            self.state = Some(create_book_publication_state(&self.universe)?);
        }
        Ok(())
    }
    pub fn consume(&mut self, record: &V) -> Result<(), String> {
        match self.state.take() {
            Some(state) => {
                self.state = Some(apply_book_publication(&state, record)?);
                Ok(())
            }
            None => Err("PUBLICATION_NOT_ATTACHED".into()),
        }
    }
    pub fn status(&self, legacy: &PerformanceCapture) -> V {
        match &self.state {
            Some(state) => json!({
                "books": state.get("books").and_then(V::as_object).map(|m| m.values().cloned().collect::<Vec<V>>()).unwrap_or_default(),
                "bookSource": "immediate_publication",
                "publicationSequence": state.get("sequence").cloned().unwrap_or(V::Null),
                "publicationHash": state.get("hash").cloned().unwrap_or(V::Null),
                "publicationAt": state.get("lastAt").cloned().unwrap_or(V::Null),
                "publicationSessionId": state.get("sessionId").cloned().unwrap_or(V::Null),
                "publicationClosed": state.get("closed").cloned().unwrap_or(V::Null),
            }),
            None => json!({
                "books": legacy.status().get("books").cloned().unwrap_or(V::Null),
                "bookSource": "cycle_journal",
                "publicationSequence": V::Null,
                "publicationHash": V::Null,
                "publicationAt": V::Null,
                "publicationSessionId": V::Null,
                "publicationClosed": V::Null,
            }),
        }
    }
}
