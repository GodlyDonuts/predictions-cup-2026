use crate::core::payload_hash;
use crate::transport::OrderTransport;
use baal_service_runtime::fixed::{instant, money, price, TICK};
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::market::validate_price_observation;
use serde_json::{json, Value as V};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const MEDIAN_REFERENCE_POLICY: &str =
    r#"{"dipTicks":1,"excludeMostRecentKnownBulkPoint":true,"historyMs":3600000,"knownBeforeOriginalBookRequest":true,"median":"upper","minimumSamples":20,"minimumSpanMs":1800000}"#;
fn hash_json(x: &V) -> String {
    sha256(js_stringify(x).as_bytes())
}
fn hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
// Exact binary representations of documented 0.005 ticks, including exact
// complement artifacts. No rounding or float money arithmetic.
fn tick_prices() -> BTreeMap<u64, i128> {
    let mut map = BTreeMap::new();
    for i in 1..200i64 {
        let tick = i as i128 * TICK;
        for v in [i as f64 / 200.0, 1.0 - (200 - i) as f64 / 200.0, 1.0 - (1.0 - i as f64 / 200.0)] {
            map.insert(v.to_bits(), tick);
        }
    }
    map
}
fn exact_ask(x: &V) -> Option<i128> {
    if let Some(s) = x.as_str() {
        price(s).ok()
    } else {
        x.as_f64().and_then(|n| tick_prices().get(&n.to_bits()).copied())
    }
}
fn validate_prefix(pin: &V) -> Result<(), String> {
    let bytes = pin.get("bytes").and_then(V::as_i64);
    let seq = pin.get("sequence").and_then(V::as_i64);
    let ok = bytes.is_some_and(|b| b >= 0)
        && seq.is_some_and(|s| s >= 0)
        && pin.get("sha256").and_then(V::as_str).is_some_and(hex)
        && if seq == Some(0) {
            pin.get("hash").and_then(V::as_str) == Some("") && bytes == Some(0)
        } else {
            pin.get("hash").and_then(V::as_str).is_some_and(hex) && bytes.unwrap() > 0
        };
    if ok { Ok(()) } else { Err("MEDIAN_REFERENCE_PREFIX_CONFIG".into()) }
}

#[derive(Clone)]
struct Point {
    known_at: i64,
    bid: i128,
    sequence: i64,
    hash: String,
    #[allow(dead_code)]
    request_started_at: String,
    #[allow(dead_code)]
    received_at: String,
}
pub struct MedianReferenceScreen {
    scope: V,
    pin: V,
    cutoff_ms: i64,
    members: BTreeMap<String, V>,
    sequence: i64,
    previous: String,
    known: i64,
    received: i64,
    bytes: i64,
    failed: bool,
    sealed: bool,
    history: BTreeMap<String, Vec<Point>>,
}
impl MedianReferenceScreen {
    pub fn new(universe: &V, source_prefix: &V, cutoff: &str) -> Result<Self, String> {
        validate_prefix(source_prefix)?;
        let cutoff_ms = instant(cutoff)?;
        let scope = universe.clone();
        let exchanges = scope.get("exchanges").and_then(V::as_array).cloned().unwrap_or_default();
        let mut members = BTreeMap::new();
        for x in &exchanges {
            members.insert(x.get("marketId").and_then(V::as_str).unwrap_or("").to_string(), x.clone());
        }
        let ok = scope.get("hash").is_some_and(V::is_string)
            && scope.get("tournamentId").is_some_and(V::is_string)
            && !exchanges.is_empty()
            && members.len() == exchanges.len()
            && {
                let ids: HashSet<&str> = exchanges.iter().filter_map(|x| x.get("exchangeId").and_then(V::as_str)).collect();
                ids.len() == exchanges.len()
            }
            && exchanges.iter().all(|x| x.get("option").and_then(V::as_str) == Some("YES"));
        if !ok {
            return Err("MEDIAN_REFERENCE_UNIVERSE".into());
        }
        let sealed = source_prefix.get("sequence").and_then(V::as_i64) == Some(0);
        Ok(MedianReferenceScreen {
            scope,
            pin: source_prefix.clone(),
            cutoff_ms,
            members,
            sequence: 0,
            previous: String::new(),
            known: i64::MIN,
            received: i64::MIN,
            bytes: 0,
            failed: false,
            sealed,
            history: BTreeMap::new(),
        })
    }
    #[allow(dead_code)]
    fn reject<T>(&mut self, code: &str) -> Result<T, String> {
        self.failed = true;
        Err(format!("MEDIAN_REFERENCE_{code}"))
    }
    pub fn accept(&mut self, row: &V, end_bytes: i64) -> Result<V, String> {
        let result = (|| -> Result<(), String> {
            if self.failed {
                return Err("FAILED".into());
            }
            let keys_ok = row.as_object().is_some_and(|m| {
                let mut k: Vec<&str> = m.keys().map(|s| s.as_str()).collect();
                k.sort();
                k == ["at", "event", "hash", "previous", "sequence", "state"]
            });
            if !keys_ok {
                return Err("ROW_SHAPE".into());
            }
            let mut body = row.clone();
            let got = body.as_object_mut().unwrap().remove("hash").unwrap_or(V::Null);
            if !(row.get("sequence").and_then(V::as_i64) == Some(self.sequence + 1)
                && row.get("previous").and_then(V::as_str) == Some(self.previous.as_str())
                && got.as_str() == Some(hash_json(&body).as_str())
                && row.get("state") == Some(&V::Null)
                && row.pointer("/event/type").and_then(V::as_str) == Some("market_price_observation"))
            {
                return Err("SOURCE_CHAIN".into());
            }
            let at = instant(row.get("at").and_then(V::as_str).unwrap_or(""))?;
            let snapshot = validate_price_observation(
                row.pointer("/event/snapshot").unwrap_or(&V::Null),
                &self.scope,
            )?;
            let receipt = instant(snapshot.get("receivedAt").and_then(V::as_str).unwrap_or(""))?;
            if !(at >= self.known && receipt >= self.received && receipt <= at) {
                return Err("SOURCE_CLOCK".into());
            }
            if !(end_bytes > self.bytes) {
                return Err("SOURCE_BYTES".into());
            }
            let seq = row.get("sequence").and_then(V::as_i64).unwrap();
            let pin_seq = self.pin.get("sequence").and_then(V::as_i64).unwrap();
            let pin_bytes = self.pin.get("bytes").and_then(V::as_i64).unwrap();
            if seq <= pin_seq && !(at <= self.cutoff_ms && end_bytes <= pin_bytes) {
                return Err("PRIME_FUTURE".into());
            }
            if seq == pin_seq {
                if !(end_bytes == pin_bytes
                    && got.as_str() == self.pin.get("hash").and_then(V::as_str))
                {
                    return Err("SEALED_PREFIX".into());
                }
                self.sealed = true;
            }
            if seq > pin_seq && !self.sealed {
                return Err("UNSEALED_PREFIX".into());
            }
            let policy: V = serde_json::from_str(MEDIAN_REFERENCE_POLICY).unwrap();
            let history_ms = policy.get("historyMs").and_then(V::as_i64).unwrap();
            for q in snapshot.get("quotes").and_then(V::as_array).cloned().unwrap_or_default() {
                if q.get("option").and_then(V::as_str) != Some("YES")
                    || q.get("crossed") == Some(&V::Bool(true))
                {
                    continue;
                }
                let Some(bid) = q.get("bestBid").and_then(|v| v.as_f64()).and_then(|n| tick_prices().get(&n.to_bits()).copied()) else {
                    continue;
                };
                let market_id = q.get("marketId").and_then(V::as_str).unwrap_or("").to_string();
                let points = self.history.entry(market_id).or_default();
                while points.first().is_some_and(|p| at - p.known_at > history_ms) {
                    points.remove(0);
                }
                if points.len() >= 10000 {
                    return Err("HISTORY_BOUND".into());
                }
                points.push(Point {
                    known_at: at,
                    bid,
                    sequence: seq,
                    hash: got.as_str().unwrap_or("").to_string(),
                    request_started_at: snapshot.get("requestStartedAt").and_then(V::as_str).unwrap_or("").to_string(),
                    received_at: snapshot.get("receivedAt").and_then(V::as_str).unwrap_or("").to_string(),
                });
            }
            self.sequence = seq;
            self.previous = got.as_str().unwrap_or("").to_string();
            self.known = at;
            self.received = receipt;
            self.bytes = end_bytes;
            Ok(())
        })();
        match result {
            Ok(()) => Ok(self.status()),
            Err(e) => {
                self.failed = true;
                Err(format!("MEDIAN_REFERENCE_{e}"))
            }
        }
    }
    pub fn status(&self) -> V {
        json!({
            "sequence": self.sequence,
            "hash": self.previous,
            "bytes": self.bytes,
            "knownAt": if self.known == i64::MIN { V::Null } else {
                json!(chrono::DateTime::from_timestamp_millis(self.known).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
            },
            "sealed": self.sealed,
            "failed": self.failed,
            "universeHash": self.scope.get("hash").unwrap().clone(),
            "executionAuthority": false,
        })
    }
    pub fn assert_prefix(&self) -> Result<V, String> {
        if self.failed || !self.sealed || self.sequence < self.pin.get("sequence").and_then(V::as_i64).unwrap() {
            return Err("MEDIAN_REFERENCE_UNSEALED_PREFIX".into());
        }
        Ok(self.status())
    }
    pub fn reference(&self, input: &V) -> Result<V, String> {
        let policy: V = serde_json::from_str(MEDIAN_REFERENCE_POLICY).unwrap();
        if self.failed {
            return Err("MEDIAN_REFERENCE_FAILED".into());
        }
        if !self.sealed {
            return Err("MEDIAN_REFERENCE_UNSEALED_PREFIX".into());
        }
        let market_id = input.get("marketId").and_then(V::as_str).unwrap_or("");
        let member = self.members.get(market_id);
        if member.map(|m| m.get("exchangeId").and_then(V::as_str)) != input.get("exchangeId").and_then(V::as_str).map(Some) || member.is_none() {
            return Err("MEDIAN_REFERENCE_BOOK_SCOPE".into());
        }
        let request = instant(input.get("bookRequestStartedAt").and_then(V::as_str).unwrap_or(""))?;
        let available = instant(input.get("availableAt").and_then(V::as_str).unwrap_or(""))?;
        if !(request <= available && self.known <= available) {
            return Err("MEDIAN_REFERENCE_BOOK_CLOCK".into());
        }
        let ask = exact_ask(input.get("bestAsk").unwrap_or(&V::Null))
            .ok_or("MEDIAN_REFERENCE_ASK_OFF_TICK")?;
        let history_ms = policy.get("historyMs").and_then(V::as_i64).unwrap();
        let all = self.history.get(market_id).cloned().unwrap_or_default();
        let mut prior: Vec<Point> = all
            .into_iter()
            .filter(|x| available - x.known_at <= history_ms && x.known_at < request)
            .collect();
        let dropped = prior.pop();
        let first = prior.first();
        let last = prior.last();
        let retained: Vec<V> = prior
            .iter()
            .map(|x| {
                json!({
                    "sequence": x.sequence,
                    "hash": x.hash,
                    "knownAt": x.known_at,
                    "bid": money(x.bid),
                })
            })
            .collect();
        let iso = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let proof = json!({
            "sourcePrefix": self.pin.clone(),
            "frontier": self.status(),
            "marketId": market_id,
            "exchangeId": input.get("exchangeId").unwrap().clone(),
            "bookRequestStartedAt": input.get("bookRequestStartedAt").unwrap().clone(),
            "availableAt": input.get("availableAt").unwrap().clone(),
            "eligibleSamplesBeforeDrop": prior.len() as i64 + if dropped.is_some() { 1 } else { 0 },
            "retainedSamples": prior.len() as i64,
            "firstKnownAt": first.map(|p| json!(iso(p.known_at))).unwrap_or(V::Null),
            "lastKnownAt": last.map(|p| json!(iso(p.known_at))).unwrap_or(V::Null),
            "excludedNewest": dropped.as_ref().map(|d| json!({
                "sequence": d.sequence, "hash": d.hash, "knownAt": iso(d.known_at),
            })).unwrap_or(V::Null),
            "retainedPointsHash": hash_json(&V::Array(retained)),
            "windowLowerBound": iso(available - history_ms),
            "clockModel": "journal_known_before_original_book_request",
            "depthAvailable": false,
            "executable": false,
        });
        let no_signal = |reason: &str| -> V {
            json!({
                "outcome": "outcomeNoSignal",
                "outcomeNoSignal": true,
                "reason": reason,
                "reference": null,
                "sourceProof": proof,
                "executionAuthority": false,
            })
        };
        if (prior.len() as i64) < policy.get("minimumSamples").and_then(V::as_i64).unwrap() {
            return Ok(no_signal("insufficient_lagged_samples"));
        }
        if last.unwrap().known_at - first.unwrap().known_at
            < policy.get("minimumSpanMs").and_then(V::as_i64).unwrap()
        {
            return Ok(no_signal("insufficient_lagged_span"));
        }
        let mut values: Vec<i128> = prior.iter().map(|x| x.bid).collect();
        values.sort();
        let median = values[values.len() / 2];
        let gap = median - ask;
        if gap < TICK {
            let mut r = no_signal("insufficient_one_tick_gap");
            let m = r.as_object_mut().unwrap();
            m.insert("reference".into(), json!(money(median)));
            m.insert("ask".into(), json!(money(ask)));
            m.insert("gap".into(), json!(money(gap)));
            return Ok(r);
        }
        Ok(json!({
            "outcome": "reference",
            "outcomeNoSignal": false,
            "reference": money(median),
            "ask": money(ask),
            "gap": money(gap),
            "gapTicks": (gap / TICK) as i64,
            "sourceProof": proof,
            "executionAuthority": false,
        }))
    }
}

fn prefix_hash(file: &Path, bytes: u64) -> Result<String, String> {
    if bytes == 0 {
        return Ok(sha256(b""));
    }
    let mut f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; bytes as usize];
    f.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(sha256(&buf))
}
// Bootstrap once; refresh explicitly once per collector batch, not per book.
// Rehash consumed bytes before refresh to fail closed on in-place source edits.
pub struct MedianReferenceReader {
    file: PathBuf,
    screen: MedianReferenceScreen,
    bytes: u64,
    sha256: String,
    dev: u64,
    ino: u64,
    failed: bool,
}
impl MedianReferenceReader {
    pub fn open(file: &Path, universe: &V, source_prefix: &V, cutoff: &str) -> Result<Self, String> {
        let screen = MedianReferenceScreen::new(universe, source_prefix, cutoff)?;
        let meta = std::fs::metadata(file).map_err(|e| e.to_string())?;
        use std::os::unix::fs::MetadataExt;
        let pin_bytes = source_prefix.get("bytes").and_then(V::as_i64).unwrap_or(0) as u64;
        if meta.size() < pin_bytes {
            return Err("MEDIAN_REFERENCE_PREFIX_TRUNCATED".into());
        }
        let mut r = MedianReferenceReader {
            file: file.to_path_buf(),
            screen,
            bytes: 0,
            sha256: sha256(b""),
            dev: meta.dev(),
            ino: meta.ino(),
            failed: false,
        };
        if prefix_hash(file, pin_bytes)? != source_prefix.get("sha256").and_then(V::as_str).unwrap_or("") {
            return Err("MEDIAN_REFERENCE_PREFIX_HASH".into());
        }
        r.consume(pin_bytes)?;
        r.screen.assert_prefix()?;
        if r.sha256 != source_prefix.get("sha256").and_then(V::as_str).unwrap_or("") {
            return Err("MEDIAN_REFERENCE_PREFIX_CHANGED".into());
        }
        Ok(r)
    }
    fn consume(&mut self, end: u64) -> Result<(), String> {
        if end == self.bytes {
            return Ok(());
        }
        let mut f = std::fs::File::open(&self.file).map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(self.bytes)).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; (end - self.bytes) as usize];
        f.read_exact(&mut buf).map_err(|e| e.to_string())?;
        let mut pending = self.bytes;
        for chunk in buf.split(|&c| c == b'\n') {
            if chunk.is_empty() {
                pending += 1;
                continue;
            }
            if chunk.len() > 2 * 1024 * 1024 {
                return Err("MEDIAN_REFERENCE_ROW_BOUND".into());
            }
            pending += chunk.len() as u64 + 1;
            if pending > end {
                return Err("MEDIAN_REFERENCE_PARTIAL_RECORD".into());
            }
            let line = std::str::from_utf8(chunk).map_err(|_| "MEDIAN_REFERENCE_ROW_BOUND")?;
            let row: V = serde_json::from_str(line).map_err(|_| "MEDIAN_REFERENCE_ROW_BOUND")?;
            self.screen.accept(&row, pending as i64)?;
        }
        if pending != end {
            return Err("MEDIAN_REFERENCE_PREFIX_BOUNDARY".into());
        }
        self.bytes = pending;
        self.sha256 = prefix_hash(&self.file, self.bytes)?;
        Ok(())
    }
    pub fn reference(&self, input: &V) -> Result<V, String> {
        if self.failed {
            return Err("MEDIAN_REFERENCE_FAILED".into());
        }
        self.screen.reference(input)
    }
    pub fn status(&self) -> V {
        let mut s = self.screen.status();
        let m = s.as_object_mut().unwrap();
        m.insert("sha256".into(), json!(self.sha256));
        m.insert("fileIdentity".into(), json!({"dev": self.dev.to_string(), "ino": self.ino.to_string()}));
        s
    }
    pub fn refresh(&mut self) -> Result<V, String> {
        use std::os::unix::fs::MetadataExt;
        let result = (|| -> Result<V, String> {
            if self.failed {
                return Err("FAILED".into());
            }
            let current = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
            if current.dev() != self.dev || current.ino() != self.ino || current.size() < self.bytes {
                return Err("SOURCE_IDENTITY_OR_TRUNCATION".into());
            }
            if prefix_hash(&self.file, self.bytes)? != self.sha256 {
                return Err("CONSUMED_PREFIX_CHANGED".into());
            }
            let mut end = current.size();
            if end > self.bytes {
                let tail_size = std::cmp::min(end - self.bytes, 2 * 1024 * 1024 + 1);
                let mut tail = vec![0u8; tail_size as usize];
                let mut f = std::fs::File::open(&self.file).map_err(|e| e.to_string())?;
                f.seek(SeekFrom::Start(end - tail_size)).map_err(|e| e.to_string())?;
                f.read_exact(&mut tail).map_err(|e| e.to_string())?;
                match tail.iter().rposition(|&c| c == b'\n') {
                    None => {
                        if end - self.bytes > 2 * 1024 * 1024 {
                            return Err("ROW_BOUND".into());
                        }
                        end = self.bytes;
                    }
                    Some(last) => end = end - tail_size + last as u64 + 1,
                }
                self.consume(end)?;
            }
            Ok(self.screen.status())
        })();
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                self.failed = true;
                Err(format!("MEDIAN_REFERENCE_{e}"))
            }
        }
    }
}

// execution/median-preflight.mjs — read-only reconciliation adapter. The
// injected dispatch is the collector's sole shared queue; this module has no
// credential, fetch, timer or retry loop.
pub const MEDIAN_TID: &str = crate::TOURNAMENT_ID;
fn byte_hash(b: &[u8]) -> String {
    sha256(b)
}
fn request_of(raw: &[u8], now: i64) -> Result<V, String> {
    let x: V = serde_json::from_slice(raw).map_err(|_| "MEDIAN_PREFLIGHT_REQUEST_SCHEMA")?;
    let mut keys: Vec<&str> = x.as_object().map(|m| m.keys().map(|s| s.as_str()).collect()).unwrap_or_default();
    keys.sort();
    let ok = x.is_object()
        && keys == ["activationAuthorityHash", "requestedAt", "runId", "schemaVersion"]
        && x.get("schemaVersion").and_then(V::as_i64) == Some(1)
        && x.get("runId").and_then(V::as_str).is_some_and(|s| {
            s.len() >= 2 && s.len() <= 101 && s.starts_with(|c: char| c.is_ascii_lowercase())
                && s[1..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && x.get("activationAuthorityHash").and_then(V::as_str).is_some_and(hex);
    if !ok {
        return Err("MEDIAN_PREFLIGHT_REQUEST_SCHEMA".into());
    }
    let requested = x.get("requestedAt").and_then(V::as_str).unwrap_or("");
    let at = chrono::DateTime::parse_from_rfc3339(requested)
        .ok()
        .filter(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == requested)
        .map(|d| d.timestamp_millis());
    if !(at.is_some_and(|a| a <= now && now - a <= 90000)) {
        return Err("MEDIAN_PREFLIGHT_REQUEST_CLOCK".into());
    }
    Ok(x)
}
fn safe_code(e: &str) -> String {
    if e.starts_with("MEDIAN_PREFLIGHT_") && e.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
        e.to_string()
    } else {
        "MEDIAN_PREFLIGHT_RECONCILIATION_BLOCKED".into()
    }
}
pub struct MedianPreflight<'a> {
    runtime_dir: PathBuf,
    request_file: PathBuf,
    latest_file: PathBuf,
    transport: OrderTransport,
    universe: V,
    can_operate: &'a dyn Fn() -> bool,
    now: std::rc::Rc<dyn Fn() -> i64>,
}
struct Active {
    request: V,
    request_hash: String,
    byte_hash: String,
    status_file: PathBuf,
}
impl<'a> MedianPreflight<'a> {
    pub fn new(
        runtime_dir: &Path,
        universe: &V,
        dispatch: Box<crate::transport::Dispatch>,
        can_operate: &'a dyn Fn() -> bool,
        now: std::rc::Rc<dyn Fn() -> i64>,
    ) -> Result<Self, String> {
        if !(runtime_dir.is_absolute() && runtime_dir.is_dir()
            && universe.get("tournamentId").and_then(V::as_str) == Some(MEDIAN_TID))
        {
            return Err("MEDIAN_PREFLIGHT_DEPENDENCIES".into());
        }
        let now_t = now.clone();
        let transport = OrderTransport::new(crate::transport::OrderTransportOptions {
            dispatch,
            scope: crate::transport::scope_of(&json!({
                "tournamentId": MEDIAN_TID,
                "tournamentSlug": "midterm-elections",
                "exchanges": universe.get("exchanges").unwrap().clone(),
            }))
            .map_err(|e| e.code)?,
            can_write: Box::new(|| false),
            owns_order: Box::new(|_| false),
            validate_permit: Box::new(|_, _| Ok(false)),
            validate_lease: Box::new(|_, _| Ok(false)),
            validate_network_permit: Box::new(|_, _| Ok(false)),
            now: Box::new(move || (*now_t)()),
            max_pages: 20,
        })
        .map_err(|e: crate::transport::OrderTransportError| e.code)?;
        Ok(MedianPreflight {
            runtime_dir: runtime_dir.to_path_buf(),
            request_file: runtime_dir.join("median-production-preflight-request.json"),
            latest_file: runtime_dir.join("median-production-preflight-latest.json"),
            transport,
            universe: universe.clone(),
            can_operate,
            now,
        })
    }
    fn load(&self) -> Result<Option<Active>, String> {
        if !self.request_file.exists() {
            return Ok(None);
        }
        let raw = std::fs::read(&self.request_file).map_err(|e| e.to_string())?;
        if raw.len() > 8192 {
            return Err("MEDIAN_PREFLIGHT_REQUEST_BOUND".into());
        }
        let request = request_of(&raw, (self.now)())?;
        let request_hash = payload_hash(&request)?;
        let status_file = self.runtime_dir.join(format!(
            "median-production-preflight-{}.status.json",
            request.get("runId").and_then(V::as_str).unwrap()
        ));
        Ok(Some(Active {
            request,
            request_hash,
            byte_hash: byte_hash(&raw),
            status_file,
        }))
    }
    #[allow(dead_code)]
    fn fence(&self, active: Option<&Active>) -> Result<(), String> {
        let active = active.ok_or("MEDIAN_PREFLIGHT_NO_ACTIVE_REQUEST")?;
        if !((self.can_operate)()
            && !["STOP", "COLLECTOR_STOP", "EXECUTION_STOP", "MEDIAN_PRODUCTION_STOP", "MEDIAN_PREFLIGHT_STOP"]
                .iter()
                .any(|x| self.runtime_dir.join(x).exists()))
        {
            return Err("MEDIAN_PREFLIGHT_STOPPED".into());
        }
        let requested = chrono::DateTime::parse_from_rfc3339(
            active.request.get("requestedAt").and_then(V::as_str).unwrap_or(""),
        )
        .map(|d| d.timestamp_millis())
        .unwrap_or(i64::MIN);
        if !((self.now)() >= requested && (self.now)() - requested <= 90000) {
            return Err("MEDIAN_PREFLIGHT_REQUEST_EXPIRED".into());
        }
        let raw = std::fs::read(&self.request_file).map_err(|_| "MEDIAN_PREFLIGHT_REQUEST_CHANGED".to_string())?;
        if byte_hash(&raw) != active.byte_hash {
            return Err("MEDIAN_PREFLIGHT_REQUEST_CHANGED".into());
        }
        Ok(())
    }
    pub fn allowed_route(&self, spec: &V) -> bool {
        if spec.get("method").and_then(V::as_str) != Some("GET")
            || spec.get("body").is_some()
            || spec.get("bodyJson").is_some()
        {
            return false;
        }
        let e = spec.get("endpoint").and_then(V::as_str).unwrap_or("");
        if crate::production::fills_read_authorized(spec) {
            return true;
        }
        if e == "/tournaments/midterm-elections" || e == "/tournaments/midterm-elections/portfolio/positions" {
            return true;
        }
        let orders = regex_lite_orders(e);
        orders && self.transport.authorize_request(spec).is_ok()
    }
    pub fn pending(&self) -> bool {
        match self.load() {
            Ok(None) => false,
            Ok(Some(x)) => {
                if !x.status_file.exists() {
                    return true;
                }
                let st = std::fs::read_to_string(&x.status_file)
                    .ok()
                    .and_then(|s| serde_json::from_str::<V>(&s).ok());
                match st {
                    Some(st) => st.get("requestHash").and_then(V::as_str) != Some(x.request_hash.as_str()),
                    None => false,
                }
            }
            Err(_) => false,
        }
    }
    pub fn consume(
        &self,
        dispatch: &dyn Fn(&V) -> Result<V, String>,
    ) -> Result<V, String> {
        let Some(x) = self.load()? else {
            return Ok(json!({"status": "no_request", "executionAuthority": false}));
        };
        if x.status_file.exists() {
            let st: V = serde_json::from_str(
                &std::fs::read_to_string(&x.status_file).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if st.get("requestHash").and_then(V::as_str) != Some(x.request_hash.as_str()) {
                return Err("MEDIAN_PREFLIGHT_RUN_ID_REUSED".into());
            }
            let mut out = json!({
                "status": st.get("status").unwrap().clone(),
                "runId": x.request.get("runId").unwrap().clone(),
                "requestHash": x.request_hash,
                "deduplicated": true,
                "executionAuthority": false,
            });
            if let Some(h) = st.get("receiptHash") {
                out.as_object_mut().unwrap().insert("receiptHash".into(), h.clone());
            }
            return Ok(out);
        }
        let requested_ms = chrono::DateTime::parse_from_rfc3339(
            x.request.get("requestedAt").and_then(V::as_str).unwrap_or(""),
        )
        .map(|d| d.timestamp_millis())
        .unwrap_or(i64::MIN);
        let now = self.now.clone();
        let can = self.can_operate;
        let dir = self.runtime_dir.clone();
        let byte = x.byte_hash.clone();
        let request_file = self.request_file.clone();
        let active_ref = &x;
        let fence = move || -> Result<(), String> {
            let active = active_ref;
            if !(*can)()
                || ["STOP", "COLLECTOR_STOP", "EXECUTION_STOP", "MEDIAN_PRODUCTION_STOP", "MEDIAN_PREFLIGHT_STOP"]
                    .iter()
                    .any(|f| dir.join(f).exists())
            {
                return Err("MEDIAN_PREFLIGHT_STOPPED".into());
            }
            if !((*now)() >= requested_ms && (*now)() - requested_ms <= 90000) {
                return Err("MEDIAN_PREFLIGHT_REQUEST_EXPIRED".into());
            }
            let raw = std::fs::read(&request_file).map_err(|_| "MEDIAN_PREFLIGHT_REQUEST_CHANGED".to_string())?;
            if byte_hash(&raw) != byte {
                return Err("MEDIAN_PREFLIGHT_REQUEST_CHANGED".into());
            }
            let _ = active;
            Ok(())
        };
        let status_base = json!({
            "schemaVersion": 1,
            "runId": x.request.get("runId").unwrap().clone(),
            "requestHash": x.request_hash,
            "activationAuthorityHash": x.request.get("activationAuthorityHash").unwrap().clone(),
        });
        let at = || chrono::DateTime::from_timestamp_millis((self.now)()).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let result = (|| -> Result<V, String> {
            fence()?;
            let mut started = status_base.clone();
            let m = started.as_object_mut().unwrap();
            m.insert("status".into(), json!("started"));
            m.insert("at".into(), json!(at()));
            m.insert("executionAuthority".into(), json!(false));
            baal_service_runtime::atomic::atomic_json(&x.status_file, &started)?;
            let guarded = |spec: &V| -> Result<V, String> {
                fence()?;
                if !self.allowed_route(spec) {
                    return Err("MEDIAN_PREFLIGHT_READ_SCOPE".into());
                }
                let receipt = dispatch(spec)?;
                fence()?;
                let ok = receipt.get("endpoint").and_then(V::as_str) == spec.get("endpoint").and_then(V::as_str)
                    && chrono::DateTime::parse_from_rfc3339(receipt.get("requestStartedAt").and_then(V::as_str).unwrap_or("")).is_ok()
                    && chrono::DateTime::parse_from_rfc3339(receipt.get("receivedAt").and_then(V::as_str).unwrap_or("")).is_ok()
                    && chrono::DateTime::parse_from_rfc3339(receipt.get("requestStartedAt").and_then(V::as_str).unwrap()).unwrap().timestamp_millis() >= requested_ms
                    && chrono::DateTime::parse_from_rfc3339(receipt.get("requestStartedAt").and_then(V::as_str).unwrap()).unwrap()
                        <= chrono::DateTime::parse_from_rfc3339(receipt.get("receivedAt").and_then(V::as_str).unwrap()).unwrap()
                    && chrono::DateTime::parse_from_rfc3339(receipt.get("receivedAt").and_then(V::as_str).unwrap()).unwrap().timestamp_millis() <= (self.now)();
                if !ok {
                    return Err("MEDIAN_PREFLIGHT_RESPONSE_CLOCK_OR_SCOPE".into());
                }
                Ok(receipt)
            };
            let result = crate::production::reconcile_production_account(
                &self.transport,
                &guarded,
                &self.universe,
                &*self.now,
                20,
            )?;
            fence()?;
            let file = format!("production-human-reconciliation-{}.json", x.request.get("runId").and_then(V::as_str).unwrap());
            let receipt_file = self.runtime_dir.join(&file);
            if receipt_file.exists() {
                return Err("MEDIAN_PREFLIGHT_RECEIPT_ALREADY_EXISTS".into());
            }
            baal_service_runtime::atomic::atomic_json(&receipt_file, &json!({
                "schemaVersion": 1,
                "runId": x.request.get("runId").unwrap().clone(),
                "requestHash": x.request_hash,
                "activationAuthorityHash": x.request.get("activationAuthorityHash").unwrap().clone(),
                "receiptHash": result.get("receiptHash").unwrap().clone(),
                "receipt": result.get("receipt").unwrap().clone(),
            }))?;
            fence()?;
            let pointer = json!({
                "schemaVersion": 1,
                "runId": x.request.get("runId").unwrap().clone(),
                "activationAuthorityHash": x.request.get("activationAuthorityHash").unwrap().clone(),
                "requestHash": x.request_hash,
                "receiptHash": result.get("receiptHash").unwrap().clone(),
                "at": result.pointer("/receipt/at").unwrap().clone(),
                "file": file,
                "executionAuthority": false,
            });
            let mut done = status_base.clone();
            let m = done.as_object_mut().unwrap();
            m.insert("status".into(), json!("complete"));
            m.insert("receiptHash".into(), result.get("receiptHash").unwrap().clone());
            m.insert("at".into(), result.pointer("/receipt/at").unwrap().clone());
            m.insert("executionAuthority".into(), json!(false));
            baal_service_runtime::atomic::atomic_json(&x.status_file, &done)?;
            baal_service_runtime::atomic::atomic_json(&self.latest_file, &pointer)?;
            let mut out = pointer.clone();
            out.as_object_mut().unwrap().insert("status".into(), json!("complete"));
            Ok(out)
        })();
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                let mut blocked = status_base.clone();
                let m = blocked.as_object_mut().unwrap();
                m.insert("status".into(), json!("blocked"));
                m.insert("reason".into(), json!(safe_code(&e)));
                m.insert("at".into(), json!(at()));
                m.insert("executionAuthority".into(), json!(false));
                if baal_service_runtime::atomic::atomic_json(&x.status_file, &blocked).is_err() {
                    return Err("MEDIAN_PREFLIGHT_DURABILITY_FAILURE".into());
                }
                Err(safe_code(&e))
            }
        }
    }
}
fn regex_lite_orders(e: &str) -> bool {
    // ^/orders\?status=(?:all|open)&limit=200&tournamentId={TID}(?:&cursor=[A-Za-z0-9%._~-]+)?$
    let prefix = format!("/orders?status=");
    let Some(rest) = e.strip_prefix(&prefix) else { return false };
    let suffix = format!("&limit=200&tournamentId={}", MEDIAN_TID);
    for status in ["all", "open"] {
        if let Some(tail) = rest.strip_prefix(&format!("{status}{suffix}")) {
            if tail.is_empty() {
                return true;
            }
            if let Some(c) = tail.strip_prefix("&cursor=") {
                return !c.is_empty()
                    && c.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'%' | b'.' | b'_' | b'~' | b'-')
                    });
            }
            return false;
        }
    }
    false
}
