use crate::{money::price, types::*};
use serde::Deserialize;
use serde_json::{Value, value::RawValue};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
    time::Instant,
};
const MAX_RECORD: u64 = 8 * 1024 * 1024;
#[derive(Deserialize)]
struct RawRecord<'a> {
    sequence: usize,
    #[serde(borrow)]
    previous: &'a str,
    #[serde(borrow)]
    at: &'a str,
    #[serde(borrow)]
    event: &'a RawValue,
    #[serde(borrow)]
    state: &'a RawValue,
    #[serde(borrow)]
    hash: &'a str,
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| "MISSING_SOURCE_FIELD".into())
}
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        _ => true,
    }
}
fn id(v: &Value) -> Result<String, String> {
    let s = if let Some(s) = v.as_str() {
        s.to_owned()
    } else if let Some(n) = v.as_u64() {
        n.to_string()
    } else {
        return Err("INVALID_ID".into());
    };
    if s.is_empty() || !s.bytes().all(|x| x.is_ascii_digit()) {
        return Err("INVALID_ID".into());
    }
    Ok(s)
}
fn levels(v: &Value, descending: bool) -> Result<Vec<Level>, String> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for row in v.as_array().ok_or("MISSING_DEPTH")? {
        let p = if let Some(s) = row["price"].as_str() {
            price(s)?
        } else {
            price(&row["price"].to_string())?
        };
        let q = row["quantity"].as_i64().ok_or("INVALID_DEPTH_QUANTITY")?;
        if q < 1 || q > 9_007_199_254_740_991 || !seen.insert(p) {
            return Err("INVALID_DEPTH".into());
        }
        rows.push(Level {
            price: p,
            quantity: q,
        });
    }
    rows.sort_by_key(|r| if descending { -r.price } else { r.price });
    if rows.is_empty() {
        return Err("MISSING_DEPTH".into());
    }
    Ok(rows)
}
fn normalize(
    s: &Value,
    market: &str,
    c: &Config,
    excluded: &HashSet<&str>,
) -> Result<Book, String> {
    let received = instant(text(s, "receivedAt")?)?;
    let started = instant(text(s, "requestStartedAt")?)?;
    if received < started || received - started > c.max_data_age_ms {
        return Err("SLOW_SNAPSHOT".into());
    }
    let contexts = s["data"]["contexts"].as_array().ok_or("MISSING_CONTEXT")?;
    let found: Vec<_> = contexts
        .iter()
        .filter(|x| x["tournament"]["id"].as_str() == Some(&c.tournament_id))
        .collect();
    if found.is_empty()
        || found
            .iter()
            .any(|x| x["type"].as_str() != Some("tournament"))
    {
        return Err("MISSING_CONTEXT".into());
    }
    let canonical = serde_json::to_string(&found[0]["orderbook"]).map_err(|_| "INVALID_CONTEXT")?;
    if found
        .iter()
        .skip(1)
        .any(|x| serde_json::to_string(&x["orderbook"]).ok().as_ref() != Some(&canonical))
    {
        return Err("CONFLICTING_CONTEXT".into());
    }
    let es = found[0]["orderbook"]["exchanges"]
        .as_array()
        .ok_or("MISSING_EXCHANGE")?;
    if es.len() != 1 || es[0]["option"].as_str() != Some("YES") {
        return Err("UNSUPPORTED_EXCHANGE".into());
    }
    let e = &es[0];
    let exchange = id(&e["exchangeId"])?;
    if excluded.contains(exchange.as_str()) {
        return Err("ACCOUNT_EXCLUSION".into());
    }
    let source = nanos(text(&e["asOf"], "at")?)?;
    let sequence = e["asOf"]["sequence"].as_u64().ok_or("INVALID_VERSION")?;
    if sequence > 9_007_199_254_740_991
        || source / 1_000_000 > received + 2000
        || received - source / 1_000_000 > c.max_data_age_ms
    {
        return Err("STALE_VERSION".into());
    }
    let bids = levels(&e["bids"], true)?;
    let asks = levels(&e["asks"], false)?;
    if bids[0].price >= asks[0].price {
        return Err("CROSSED_BOOK".into());
    }
    Ok(Book {
        market_id: market.to_owned(),
        instrument: format!("{exchange}:YES"),
        exchange_id: exchange,
        received_at: received,
        request_started_at: started,
        source_ns: source,
        sequence,
        bids,
        asks,
    })
}
fn market(s: &Value) -> Option<String> {
    if truthy(s.get("skipped")) {
        return s.get("marketId").and_then(Value::as_str).map(str::to_owned);
    }
    let path = s.get("endpoint")?.as_str()?.strip_prefix("/markets/")?;
    let (m, rest) = path.split_once("/orderbook?")?;
    if rest.is_empty() || m.is_empty() || !m.bytes().all(|x| x.is_ascii_digit()) {
        return None;
    }
    Some(m.to_owned())
}
pub fn read_capture(
    path: &Path,
    end: u64,
    c: &Config,
    expected: Option<&str>,
    receipt_order: bool,
) -> Result<Capture, String> {
    read_capture_seeded(path, end, c, expected, receipt_order, None)
}
// A pinned cache supplies only observations from an already verified prefix.
// Rehash original prefix bytes before accepting it, then verify every append
// with the exact same canonical chain, clocks and normalization as a full read.
pub fn read_capture_seeded(
    path: &Path, end: u64, c: &Config, expected: Option<&str>,
    receipt_order: bool, seed: Option<Capture>,
) -> Result<Capture, String> {
    c.validate()?;
    let start = Instant::now();
    let file = File::open(path).map_err(|_| "SOURCE_OPEN_FAILED")?;
    if end == 0 || file.metadata().map_err(|_| "SOURCE_METADATA_FAILED")?.len() < end {
        return Err("INVALID_SOURCE_PREFIX".into());
    }
    let mut input = file.take(end);
    let mut prefix = Sha256::new();
    let mut bytes = 0;
    let mut previous = String::new();
    let mut records = 0;
    let mut last = i64::MIN;
    let mut first = 0;
    let scope: HashSet<&str> = c.market_ids.iter().map(String::as_str).collect();
    let mut frames = Vec::new();
    let mut prefix_hashes = Vec::new();
    let mut cycles = 0;
    let mut prefix_bytes = Vec::new();
    let mut accepted = 0;
    let mut rejected = 0;
    if let Some(seed) = seed {
        if seed.meta.source_bytes > end { return Err("CACHE_SOURCE_REGRESSION".into()); }
        let mut buffer = vec![0; 4 * 1024 * 1024];
        while bytes < seed.meta.source_bytes {
            let limit = ((seed.meta.source_bytes - bytes) as usize).min(buffer.len());
            let n = input.read(&mut buffer[..limit]).map_err(|_| "SOURCE_READ_FAILED")?;
            if n == 0 { return Err("CACHE_SOURCE_TRUNCATED".into()); }
            prefix.update(&buffer[..n]); bytes += n as u64;
        }
        if format!("{:x}", prefix.clone().finalize()) != seed.meta.source_hash {
            return Err("CACHE_ORIGINAL_PREFIX_CHANGED".into());
        }
        previous=seed.tail_hash; records=seed.meta.source_records;
        last=seed.meta.through; first=seed.meta.first_at;
        cycles=seed.meta.cycles; accepted=seed.meta.accepted_books;
        rejected=seed.meta.invalid_books; frames=seed.frames;
        prefix_hashes=seed.prefix_hashes; prefix_bytes=seed.prefix_bytes;
    }
    let mut reader = BufReader::with_capacity(4 * 1024 * 1024, input);
    loop {
        let mut line = Vec::new();
        let n = reader
            .by_ref()
            .take(MAX_RECORD + 1)
            .read_until(b'\n', &mut line)
            .map_err(|_| "SOURCE_READ_FAILED")?;
        if n == 0 {
            break;
        }
        if n as u64 > MAX_RECORD {
            return Err("SOURCE_RECORD_TOO_LARGE".into());
        }
        if line.last() != Some(&b'\n') {
            return Err("INCOMPLETE_SOURCE_RECORD".into());
        }
        bytes += n as u64;
        prefix.update(&line);
        let raw = std::str::from_utf8(&line[..line.len() - 1])
            .map_err(|_| "SOURCE_UTF8_INVALID")?
            .trim();
        let r: RawRecord = serde_json::from_str(raw).map_err(|_| "SOURCE_JSON_INVALID")?;
        let _state = r.state;
        if r.sequence != records + 1 || r.previous != previous {
            return Err("SOURCE_CHAIN_INVALID".into());
        }
        let suffix = format!(",\"hash\":\"{}\"}}", r.hash);
        let body = raw
            .strip_suffix(&suffix)
            .ok_or("SOURCE_CANONICAL_HASH_REQUIRED")?;
        let mut h = Sha256::new();
        h.update(body.as_bytes());
        h.update(b"}");
        if format!("{:x}", h.finalize()) != r.hash {
            return Err("SOURCE_HASH_INVALID".into());
        }
        let event: Value =
            serde_json::from_str(r.event.get()).map_err(|_| "SOURCE_EVENT_INVALID")?;
        let at = instant(event.get("clockAt").and_then(Value::as_str).unwrap_or(r.at))?;
        if at < last {
            return Err("SOURCE_CLOCK_ROLLBACK".into());
        }
        if records == 0 {
            first = at;
        }
        last = at;
        records += 1;
        previous = r.hash.to_owned();
        prefix_hashes.push(format!("{:x}", prefix.clone().finalize()));
        prefix_bytes.push(bytes);
        let kind = text(&event, "type")?;
        if kind == "account_read_outage" {
            frames.push(Frame {
                sequence: records,
                at,
                observations: Vec::new(),
                account_outage: true,
                is_cycle: false,
            });
            continue;
        }
        if kind != "cycle" {
            frames.push(Frame {
                sequence: records,
                at,
                observations: Vec::new(),
                account_outage: false,
                is_cycle: false,
            });
            continue;
        }
        cycles += 1;
        let exclusions = event["accountExclusions"]
            .as_array()
            .ok_or("MISSING_ACCOUNT_EXCLUSIONS")?;
        let excluded: HashSet<&str> = exclusions.iter().filter_map(Value::as_str).collect();
        let mut scoped = Vec::new();
        for field in ["sourceSnapshots", "supplementalDepth"] {
            if let Some(value) = event.get(field) {
                for s in value.as_array().ok_or("INVALID_CAPTURE_ARRAY")? {
                    if let Some(m) = market(s) {
                        if scope.contains(m.as_str()) {
                            let skipped = truthy(s.get("skipped"));
                            let receipt = if skipped {
                                s.get("at")
                                    .and_then(Value::as_str)
                                    .filter(|v| !v.is_empty())
                                    .map(instant)
                                    .transpose()?
                                    .unwrap_or(at)
                            } else {
                                instant(text(s, "receivedAt")?)?
                            };
                            if receipt > at {
                                return Err("OBSERVATION_EXCEEDS_EVENT_CLOCK".into());
                            }
                            if !skipped && instant(text(s, "requestStartedAt")?)? > receipt {
                                return Err("INVALID_REQUEST_CHRONOLOGY".into());
                            }
                            scoped.push((receipt, m, s));
                        }
                    }
                }
            }
        }
        if receipt_order {
            scoped.sort_by_key(|x| x.0);
        }
        let mut observations = Vec::new();
        for (receipt, m, s) in scoped {
            if truthy(s.get("skipped")) {
                observations.push(Observation::Invalid {
                    market_id: m,
                    at: receipt,
                });
                continue;
            }
            match normalize(s, &m, c, &excluded) {
                Ok(book) => {
                    accepted += 1;
                    observations.push(Observation::Book(book));
                }
                Err(_) => {
                    rejected += 1;
                    observations.push(Observation::Invalid {
                        market_id: m,
                        at: receipt,
                    });
                }
            }
        }
        frames.push(Frame {
            sequence: records,
            at,
            observations,
            account_outage: false,
            is_cycle: true,
        });
    }
    if bytes != end || records == 0 {
        return Err("INCOMPLETE_SOURCE_PREFIX".into());
    }
    let source_hash = format!("{:x}", prefix.finalize());
    if expected.is_some_and(|h| h != source_hash) {
        return Err("SOURCE_PREFIX_MISMATCH".into());
    }
    Ok(Capture {
        meta: CaptureMeta {
            engine_hash: env!("BAAL_ENGINE_HASH"),
            source_format: "verified_original_canonical_journal_v1",
            source_bytes: bytes,
            source_records: records,
            source_hash,
            first_at: first,
            through: last,
            cycles,
            accepted_books: accepted,
            invalid_books: rejected,
            verify_ms: start.elapsed().as_millis(),
        },
        frames,
        prefix_hashes,
        prefix_bytes,
        tail_hash: previous,
    })
}
