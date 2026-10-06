// ml/core.mjs — offline normalized-book dataset contract, causal sample
// builder and train/validation/test splitter. Byte/semantic port.
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};

pub const FORMAT: &str = "sig-offline-books";
pub const FEATURE_NAMES: [&str; 5] = ["spread", "depthImbalance", "micropriceMinusMid", "pastMidChange", "pastImbalanceChange"];
pub const WARNING: &str = "SYNTHETIC-ONLY SOFTWARE CHECK. Not market evidence, calibrated fills, strategy P&L or a trained market edge.";
pub(crate) fn ns() -> BigInt { BigInt::from(1_000_000u32) }

/// ML error: `message` is the JS Error message; `summary` mirrors the JS
/// `error.summary` attachment set by develop() on INSUFFICIENT_DATA.
#[derive(Debug, Clone)]
pub struct Error {
    pub message: String,
    pub summary: Option<V>,
}
impl Error {
    pub fn code(message: impl Into<String>) -> Self { Self { message: message.into(), summary: None } }
    pub fn with_summary(message: impl Into<String>, summary: V) -> Self { Self { message: message.into(), summary: Some(summary) } }
}
impl From<&str> for Error { fn from(m: &str) -> Self { Self::code(m) } }
impl From<String> for Error { fn from(m: String) -> Self { Self::code(m) } }
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.message) }
}
impl std::error::Error for Error {}
pub type R<T> = Result<T, Error>;
fn fail<T>(message: impl Into<String>) -> R<T> { Err(Error::code(message)) }

pub fn hash(text: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(text.as_ref()))
}

// ---------------------------------------------------------------------------
// JS JSON.stringify parity writers. Parsed objects keep insertion order; JS
// enumerates integer-index keys numerically first. Numbers take the shortest
// JS spelling (ryu_js). Non-finite numbers serialize as null.
// ---------------------------------------------------------------------------
fn index_key(k: &str) -> Option<u32> {
    let v = k.parse::<u32>().ok()?;
    if v < u32::MAX && v.to_string() == k { Some(v) } else { None }
}
pub(crate) fn ordered_entries(v: &V) -> Vec<(String, V)> {
    let mut a: Vec<_> = v
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    a.sort_by(|(a, _), (b, _)| match (index_key(a), index_key(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    a
}
fn write_number(v: &V, out: &mut String) {
    match v.as_f64() {
        Some(x) if x.is_finite() => {
            let mut b = ryu_js::Buffer::new();
            out.push_str(b.format_finite(x));
        }
        _ => out.push_str("null"),
    }
}
fn write_js(v: &V, out: &mut String) {
    match v {
        V::Object(_) => {
            out.push('{');
            for (i, (k, v)) in ordered_entries(v).iter().enumerate() {
                if i > 0 { out.push(',') }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push(':');
                write_js(v, out)
            }
            out.push('}')
        }
        V::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 { out.push(',') }
                write_js(v, out)
            }
            out.push(']')
        }
        V::Number(_) => write_number(v, out),
        _ => out.push_str(&serde_json::to_string(v).unwrap()),
    }
}
/// JSON.stringify(value) — compact, insertion order with integer-key promotion.
pub fn js_stringify(v: &V) -> String {
    let mut out = String::new();
    write_js(v, &mut out);
    out
}
fn write_pretty(v: &V, out: &mut String, indent: &str) {
    match v {
        V::Object(m) if m.is_empty() => out.push_str("{}"),
        V::Object(_) => {
            let inner = format!("{indent}  ");
            out.push('{');
            for (i, (k, v)) in ordered_entries(v).iter().enumerate() {
                if i > 0 { out.push(',') }
                out.push('\n');
                out.push_str(&inner);
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push_str(": ");
                write_pretty(v, out, &inner);
            }
            out.push('\n');
            out.push_str(indent);
            out.push('}')
        }
        V::Array(a) if a.is_empty() => out.push_str("[]"),
        V::Array(a) => {
            let inner = format!("{indent}  ");
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 { out.push(',') }
                out.push('\n');
                out.push_str(&inner);
                write_pretty(v, out, &inner);
            }
            out.push('\n');
            out.push_str(indent);
            out.push(']')
        }
        V::Number(_) => write_number(v, out),
        _ => out.push_str(&serde_json::to_string(v).unwrap()),
    }
}
/// JSON.stringify(value, null, 2) — two-space pretty print.
pub fn js_pretty(v: &V) -> String {
    let mut out = String::new();
    write_pretty(v, &mut out, "");
    out
}
// canonical(v): JSON.stringify with a replacer that sorts every object's keys
// (UTF-16 code-unit order, like JS Object.keys().sort()).
fn sorted_entries(v: &V) -> Vec<(String, V)> {
    let mut a: Vec<_> = v
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    a.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    a
}
fn write_canonical(v: &V, out: &mut String) {
    match v {
        V::Object(_) => {
            out.push('{');
            for (i, (k, v)) in sorted_entries(v).iter().enumerate() {
                if i > 0 { out.push(',') }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push(':');
                write_canonical(v, out)
            }
            out.push('}')
        }
        V::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 { out.push(',') }
                write_canonical(v, out)
            }
            out.push(']')
        }
        V::Number(_) => write_number(v, out),
        _ => out.push_str(&serde_json::to_string(v).unwrap()),
    }
}
pub fn canonical(v: &V) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out);
    out
}

// ---------------------------------------------------------------------------
// JS scalar semantics helpers.
// ---------------------------------------------------------------------------
fn object(x: &V) -> bool { x.is_object() }
fn exact(x: &V, keys: &[&str], name: &str) -> R<()> {
    let mut have: Vec<&str> = x
        .as_object()
        .map(|m| m.keys().map(|k| k.as_str()).collect())
        .unwrap_or_default();
    let mut want: Vec<&str> = keys.to_vec();
    have.sort();
    want.sort();
    if !x.is_object() || have != want { return fail(format!("SCHEMA: {name}")); }
    Ok(())
}
fn id(x: &V, name: &str) -> R<()> {
    let ok = x.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 128
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
    });
    if !ok { return fail(format!("ID: {name}")); }
    Ok(())
}
/// Number.isSafeInteger(x) — integral double or integer within ±(2^53-1).
pub(crate) fn js_int(v: &V) -> Option<i64> {
    match v {
        V::Number(n) => {
            if let Some(x) = n.as_i64() {
                if x.unsigned_abs() <= 9_007_199_254_740_991 { Some(x) } else { None }
            } else if let Some(x) = n.as_u64() {
                if x <= 9_007_199_254_740_991 { i64::try_from(x).ok() } else { None }
            } else {
                let x = n.as_f64()?;
                if x.is_finite() && x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_991.0 {
                    Some(x as i64)
                } else {
                    None
                }
            }
        }
        _ => None,
    }
}
fn integer(x: &V, name: &str, min: i64, max: i64) -> R<()> {
    match js_int(x) {
        Some(v) if v >= min && v <= max => Ok(()),
        _ => fail(format!("INTEGER: {name}")),
    }
}
/// JS ToNumber coercion for the few places non-JSON-typed values can appear.
pub(crate) fn to_f64(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::Bool(b) => {
            if *b { 1.0 } else { 0.0 }
        }
        V::Null => 0.0,
        V::String(s) => js_str_to_num(s),
        _ => f64::NAN,
    }
}
fn js_str_to_num(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() { return 0.0; }
    let lower = t.to_ascii_lowercase();
    if lower == "infinity" || lower == "+infinity" { return f64::INFINITY; }
    if lower == "-infinity" { return f64::NEG_INFINITY; }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2), ("+0x", 16), ("+0o", 8), ("+0b", 2), ("-0x", 16), ("-0o", 8), ("-0b", 2)] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let sign = if prefix.starts_with('-') { -1.0 } else { 1.0 };
            return u128::from_str_radix(rest, radix).map(|v| sign * v as f64).unwrap_or(f64::NAN);
        }
    }
    // JS decimal grammar: [+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?
    let bytes = t.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') { i += 1; }
    let start_digits = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
    let mut saw_digit = i > start_digits;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let fs = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
        saw_digit = saw_digit || i > fs;
    }
    if !saw_digit { return f64::NAN; }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') { i += 1; }
        let es = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
        if i == es { return f64::NAN; }
    }
    if i != bytes.len() { return f64::NAN; }
    t.parse::<f64>().unwrap_or(f64::NAN)
}
/// JS ToString on JSON values (template literals / Array.join operands):
/// null->"null", bools spell out, numbers via JSON.stringify, arrays join
/// with ',' (null/undefined elements empty), objects "[object Object]".
pub(crate) fn js_str(v: &V) -> String {
    match v {
        V::Null => "null".to_string(),
        V::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        V::Number(_) | V::String(_) => match v {
            V::String(s) => s.clone(),
            _ => js_stringify(v),
        },
        V::Array(a) => a
            .iter()
            .map(|e| if e.is_null() { String::new() } else { js_str(e) })
            .collect::<Vec<_>>()
            .join(","),
        V::Object(_) => "[object Object]".to_string(),
    }
}
/// JS property length: array length, string UTF-16 length, else undefined.
pub(crate) fn js_length(v: &V) -> Option<usize> {
    match v {
        V::Array(a) => Some(a.len()),
        V::String(s) => Some(s.encode_utf16().count()),
        _ => None,
    }
}
pub(crate) fn type_error_length(kind: &V) -> Error {
    let what = if kind.is_null() { "null" } else { "undefined" };
    Error::code(format!("Cannot read properties of {what} (reading 'length')"))
}
/// `v.length` — Err with the JS TypeError message for null/undefined,
/// Ok(None) for values whose `.length` is JS undefined, Ok(Some(n)) for
/// arrays/strings.
pub(crate) fn js_read_length(v: &V) -> R<Option<usize>> {
    match v {
        V::Null => Err(type_error_length(v)),
        _ => Ok(js_length(v)),
    }
}
/// `obj.field.length` where the property itself may be absent (undefined).
pub(crate) fn js_read_length_opt(v: Option<&V>) -> R<Option<usize>> {
    match v {
        None => Err(Error::code("Cannot read properties of undefined (reading 'length')")),
        Some(v) => js_read_length(v),
    }
}

// ---------------------------------------------------------------------------
// Calendar: proleptic Gregorian days<->civil (Howard Hinnant), matching
// Date.parse/toISOString semantics for the fixed-width UTC format.
// ---------------------------------------------------------------------------
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * if m > 2 { m - 3 } else { m + 9 } + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (y + if m <= 2 { 1 } else { 0 }, m as u32, d as u32)
}
/// new Date(ms).toISOString() for the millisecond-precision form.
pub(crate) fn ms_to_iso(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (y, mo, d) = civil_from_days(days);
    let h = rem / 3_600_000;
    let mi = rem % 3_600_000 / 60_000;
    let s = rem % 60_000 / 1_000;
    let milli = rem % 1_000;
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{milli:03}Z")
}

/// instant(x) — strict canonical UTC timestamp -> exact nanosecond BigInt.
pub fn instant(x: &V) -> R<BigInt> {
    let s = match x.as_str() {
        Some(s) => s,
        None => return fail("TIMESTAMP"),
    };
    // ^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d{1,9}))?Z$
    let b = s.as_bytes();
    let dgt = |i: usize| i < b.len() && b[i].is_ascii_digit();
    let shape = (0..4).all(|i| dgt(i))
        && b.len() >= 20
        && b[4] == b'-' && (5..7).all(|i| dgt(i))
        && b[7] == b'-' && (8..10).all(|i| dgt(i))
        && b[10] == b'T' && (11..13).all(|i| dgt(i))
        && b[13] == b':' && (14..16).all(|i| dgt(i))
        && b[16] == b':' && (17..19).all(|i| dgt(i));
    if !shape { return fail("TIMESTAMP"); }
    let frac_digits: &str = if b.len() == 20 && b[19] == b'Z' {
        ""
    } else if b.len() >= 22 && b.len() <= 30 && b[19] == b'.' && b[b.len() - 1] == b'Z'
        && b[20..b.len() - 1].iter().all(|c| c.is_ascii_digit())
    {
        std::str::from_utf8(&b[20..b.len() - 1]).unwrap()
    } else {
        return fail("TIMESTAMP");
    };
    let y: i64 = s[0..4].parse().unwrap();
    let mo: i64 = s[5..7].parse().unwrap();
    let d: i64 = s[8..10].parse().unwrap();
    let h: i64 = s[11..13].parse().unwrap();
    let mi: i64 = s[14..16].parse().unwrap();
    let sec: i64 = s[17..19].parse().unwrap();
    let ms = days_from_civil(y, mo, d) * 86_400_000 + h * 3_600_000 + mi * 60_000 + sec * 1_000;
    // Date.parse normalizes impossible fields; the toISOString round-trip rejects them.
    if ms_to_iso(ms) != format!("{}.000Z", &s[..19]) { return fail("TIMESTAMP"); }
    let frac_ns: u64 = format!("{frac_digits:0<9}").parse().unwrap();
    Ok(BigInt::from(ms) * ns() + BigInt::from(frac_ns))
}

/// validateConfig(c) — strict key/integer/alpha/window contract, returns a
/// deep clone of the accepted config.
pub fn validate_config(c: &V) -> R<V> {
    exact(c, &["schemaVersion", "horizonMs", "toleranceMs", "maxSourceAgeMs", "maxGapMs", "trainEnd", "validationEnd", "embargoMs", "alphas", "seed", "minSamples", "minSessions", "bootstrapReplicates"], "config")?;
    if c["schemaVersion"].as_f64() != Some(1.0) { return fail("CONFIG_VERSION"); }
    for k in ["horizonMs", "maxSourceAgeMs", "maxGapMs"] { integer(&c[k], k, 1, 86_400_000)?; }
    let horizon = js_int(&c["horizonMs"]).unwrap();
    integer(&c["toleranceMs"], "toleranceMs", 0, horizon)?;
    let tolerance = js_int(&c["toleranceMs"]).unwrap();
    integer(&c["embargoMs"], "embargoMs", horizon + tolerance, 86_400_000)?;
    let embargo = js_int(&c["embargoMs"]).unwrap();
    integer(&c["seed"], "seed", 0, 4_294_967_295)?;
    integer(&c["bootstrapReplicates"], "bootstrapReplicates", 100, 10_000)?;
    exact(&c["minSamples"], &["train", "validation", "test"], "minSamples")?;
    exact(&c["minSessions"], &["train", "validation", "test"], "minSessions")?;
    for p in ["train", "validation", "test"] {
        integer(&c["minSamples"][p], &format!("minSamples.{p}"), 2, 1_000_000_000)?;
        integer(&c["minSessions"][p], &format!("minSessions.{p}"), 1, 1_000_000_000)?;
    }
    let alphas = c["alphas"].as_array().cloned().unwrap_or_default();
    if alphas.is_empty() || alphas.len() > 20 { return fail("ALPHAS"); }
    let mut prev = 0.0f64;
    for (i, a) in alphas.iter().enumerate() {
        let a = match a.as_f64() { Some(x) if x.is_finite() => x, _ => return fail("ALPHAS") };
        if a <= 0.0 || a > 1_000_000.0 || (i > 0 && a <= prev) { return fail("ALPHAS"); }
        prev = a;
    }
    let t = instant(&c["trainEnd"])?;
    let v = instant(&c["validationEnd"])?;
    if v - t <= BigInt::from(embargo * 2 + horizon + tolerance) * ns() { return fail("SPLIT_WINDOW"); }
    Ok(c.clone())
}

fn levels(xs: &V, side: &str) -> R<()> {
    let arr = match xs.as_array() {
        Some(a) if !a.is_empty() && a.len() <= 200 => a,
        _ => return fail("LEVELS"),
    };
    let mut prior = if side == "bids" { f64::INFINITY } else { f64::NEG_INFINITY };
    for x in arr {
        exact(x, &["price", "quantity"], "level")?;
        let price = x["price"].as_f64();
        let qty = x["quantity"].as_f64();
        let ok = price.is_some_and(|p| p.is_finite() && (0.0..=1.0).contains(&p))
            && qty.is_some_and(|q| q.is_finite() && q > 0.0 && q <= 1e12);
        if !ok { return fail("LEVEL"); }
        let price = price.unwrap();
        if if side == "bids" { price >= prior } else { price <= prior } { return fail("LEVEL_ORDER"); }
        prior = price;
    }
    Ok(())
}
fn scope(r: &V) -> String {
    format!("{}/{}/{}", r["tournamentId"].as_str().unwrap_or(""), r["marketId"].as_str().unwrap_or(""), r["exchangeId"].as_str().unwrap_or(""))
}
fn local_scope(r: &V) -> String {
    format!("{}/{}", r["sessionId"].as_str().unwrap_or(""), scope(r))
}
const QUALITY_KEYS: [&str; 4] = ["continuous", "recovering", "stale", "expiryKnown"];
const ROW_KEYS: [&str; 9] = ["kind", "rowId", "sessionId", "tournamentId", "marketId", "exchangeId", "segmentId", "receivedAt", "availableAt"];

#[derive(Debug, Clone)]
pub struct Row {
    /// Parsed row JSON, all original keys preserved.
    pub v: V,
    pub index: usize,
    pub available_ns: BigInt,
    pub received_ns: BigInt,
    /// Books only (`None` on barrier rows).
    pub source_ns: Option<BigInt>,
    pub expiry_ns: Option<BigInt>,
}
#[derive(Debug)]
pub struct Dataset {
    pub header: V,
    pub rows: Vec<Row>,
    pub source_hash: String,
}

/// parseDataset(text) — strict JSONL contract, global availability monotonicity,
/// returns header/rows/sourceHash.
pub fn parse_dataset(text: &str) -> R<Dataset> {
    if !text.ends_with('\n') { return fail("DATASET: complete newline-terminated JSONL required"); }
    let lines: Vec<&str> = text[..text.len() - 1].split('\n').collect();
    if lines.len() > 1_000_001 || lines.iter().any(|l| l.is_empty() || l.encode_utf16().count() > 1_000_000) {
        return fail("DATASET_LIMIT");
    }
    let mut values: Vec<V> = Vec::with_capacity(lines.len());
    for l in &lines {
        match serde_json::from_str::<V>(l) {
            Ok(v) => values.push(v),
            Err(_) => return fail("DATASET_JSON"),
        }
    }
    let header = values.remove(0);
    exact(&header, &["format", "schemaVersion", "datasetId", "dataKind", "normalization", "provenance"], "header")?;
    let fmt_ok = header["format"].as_str() == Some(FORMAT)
        && header["schemaVersion"].as_f64() == Some(1.0)
        && matches!(header["dataKind"].as_str(), Some("synthetic") | Some("real"))
        && header["normalization"].as_str() == Some("quality-segmented-books-v1");
    if !fmt_ok { return fail("DATASET_FORMAT"); }
    id(&header["datasetId"], "datasetId")?;
    exact(&header["provenance"], &["sourceSha256", "normalizerVersion"], "provenance")?;
    let sha_ok = header["provenance"]["sourceSha256"].as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    if !sha_ok { return fail("SOURCE_HASH"); }
    id(&header["provenance"]["normalizerVersion"], "normalizerVersion")?;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut prior: Option<BigInt> = None;
    let mut rows: Vec<Row> = Vec::with_capacity(values.len());
    for (index, r) in values.into_iter().enumerate() {
        let kind = r["kind"].as_str().unwrap_or("");
        if !object(&r) || !matches!(kind, "book" | "barrier") { return fail("ROW_KIND"); }
        let mut keys: Vec<&str> = ROW_KEYS.to_vec();
        if kind == "book" {
            keys.extend(["sourceAt", "sequence", "quality", "nextExpiryAt", "bids", "asks"]);
        } else {
            keys.push("reason");
        }
        exact(&r, &keys, "row")?;
        for k in ["rowId", "sessionId", "tournamentId", "marketId", "exchangeId", "segmentId"] { id(&r[k], k)?; }
        let row_id = r["rowId"].as_str().unwrap().to_string();
        if !seen.insert(row_id) { return fail("DUPLICATE_ROW_ID"); }
        let available_ns = instant(&r["availableAt"])?;
        let received_ns = instant(&r["receivedAt"])?;
        if received_ns > available_ns || prior.as_ref().is_some_and(|p| available_ns < *p) {
            return fail("AVAILABILITY_ORDER");
        }
        prior = Some(available_ns.clone());
        if kind == "barrier" {
            if !matches!(r["reason"].as_str(), Some("gap") | Some("recovery") | Some("disconnect") | Some("stale") | Some("expiry") | Some("settlement")) {
                return fail("BARRIER_REASON");
            }
            rows.push(Row { v: r, index, available_ns, received_ns, source_ns: None, expiry_ns: None });
            continue;
        }
        let source_ns = instant(&r["sourceAt"])?;
        integer(&r["sequence"], "sequence", 0, 9_007_199_254_740_991)?;
        exact(&r["quality"], &QUALITY_KEYS, "quality")?;
        if QUALITY_KEYS.iter().any(|k| !r["quality"][k].is_boolean()) { return fail("QUALITY"); }
        levels(&r["bids"], "bids")?;
        levels(&r["asks"], "asks")?;
        if to_f64(&r["bids"][0]["price"]) > to_f64(&r["asks"][0]["price"]) { return fail("CROSSED_BOOK"); }
        let expiry_ns = if r["nextExpiryAt"].is_null() { None } else { Some(instant(&r["nextExpiryAt"])?) };
        rows.push(Row { v: r, index, available_ns, received_ns, source_ns: Some(source_ns), expiry_ns });
    }
    Ok(Dataset { header, rows, source_hash: hash(text.as_bytes()) })
}

fn basic(r: &V) -> (f64, f64, f64, f64) {
    let bid = &r["bids"][0];
    let ask = &r["asks"][0];
    let bid_depth: f64 = r["bids"].as_array().unwrap().iter().map(|x| to_f64(&x["quantity"])).sum();
    let ask_depth: f64 = r["asks"].as_array().unwrap().iter().map(|x| to_f64(&x["quantity"])).sum();
    let mid = (to_f64(&bid["price"]) + to_f64(&ask["price"])) / 2.0;
    let spread = to_f64(&ask["price"]) - to_f64(&bid["price"]);
    let imbalance = (bid_depth - ask_depth) / (bid_depth + ask_depth);
    let micro = (to_f64(&ask["price"]) * to_f64(&bid["quantity"]) + to_f64(&bid["price"]) * to_f64(&ask["quantity"]))
        / (to_f64(&bid["quantity"]) + to_f64(&ask["quantity"])) - mid;
    (mid, spread, imbalance, micro)
}

/// An accepted book row: `{...r, ...basic(r), feature, featureStart}`.
#[derive(Debug, Clone)]
pub(crate) struct BuiltBook {
    pub v: V,
    pub sequence: i64,
    pub available_ns: BigInt,
    pub received_ns: BigInt,
    pub source_ns: BigInt,
    pub expiry_ns: Option<BigInt>,
    pub mid: f64,
    pub spread: f64,
    pub imbalance: f64,
    pub micro: f64,
    pub feature: Option<Vec<f64>>,
    pub feature_start: Option<BigInt>,
}

/// A built sample: JSON metadata + x/y plus the BigInt info-interval fields.
#[derive(Debug, Clone)]
pub struct Sample {
    /// Every sample field except BigInt `start`/`featureStart`/`end`
    /// (rowId..labelQuality, then `x`, `y` in JS key order).
    pub v: V,
    pub start: BigInt,
    pub feature_start: BigInt,
    pub end: BigInt,
}
#[derive(Debug)]
pub struct Partitions {
    pub train: Vec<Sample>,
    pub validation: Vec<Sample>,
    pub test: Vec<Sample>,
}
impl Partitions {
    pub fn named(&self, p: &str) -> &Vec<Sample> {
        match p {
            "train" => &self.train,
            "validation" => &self.validation,
            _ => &self.test,
        }
    }
}
#[derive(Debug)]
pub struct Split {
    pub partitions: Partitions,
    pub excluded: V,
}
#[derive(Debug)]
pub struct Built {
    pub samples: Vec<Sample>,
    pub coverage: V,
    pub excluded: V,
    pub missing_labels: V,
}

struct Instrument {
    session_id: Option<String>,
    closed_sessions: std::collections::HashSet<String>,
    sequence: i64,
    source_ns: Option<BigInt>,
    available_ns: Option<BigInt>,
}
struct SegmentState {
    segment_id: Option<String>,
    closed: std::collections::HashSet<String>,
    blocked: bool,
    latest: Option<BuiltBook>,
    rows_index: Option<usize>,
}

/// buildSamples(dataset, c) — causal row validation and segmentation. Never
/// sorts, interpolates or renews a duplicate.
pub fn build_samples(dataset: &Dataset, c: &V) -> R<Built> {
    validate_config(c)?;
    let mut states: std::collections::HashMap<String, SegmentState> = std::collections::HashMap::new();
    let mut instruments: std::collections::HashMap<String, Instrument> = std::collections::HashMap::new();
    let mut version_seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut segments: Vec<Vec<BuiltBook>> = Vec::new();
    let mut excluded: Vec<V> = Vec::new();
    let max_age = BigInt::from(js_int(&c["maxSourceAgeMs"]).unwrap()) * ns();
    let max_gap = BigInt::from(js_int(&c["maxGapMs"]).unwrap()) * ns();
    for r in &dataset.rows {
        let rv = &r.v;
        let instrument_key = scope(rv);
        let instrument = instruments.entry(instrument_key.clone()).or_insert_with(|| Instrument {
            session_id: None, closed_sessions: std::collections::HashSet::new(), sequence: -1,
            source_ns: None, available_ns: None,
        });
        if instrument.available_ns.as_ref().is_some_and(|p| r.available_ns <= *p) {
            return fail("AMBIGUOUS_INSTRUMENT_TIME");
        }
        instrument.available_ns = Some(r.available_ns.clone());
        let session_id = rv["sessionId"].as_str().unwrap().to_string();
        if instrument.session_id.as_deref() != Some(session_id.as_str()) {
            if instrument.closed_sessions.contains(&session_id) { return fail("SESSION_RESUMED"); }
            if let Some(old) = instrument.session_id.take() { instrument.closed_sessions.insert(old); }
            instrument.session_id = Some(session_id.clone());
        }
        let key = local_scope(rv);
        let st = states.entry(key).or_insert_with(|| SegmentState {
            segment_id: None, closed: std::collections::HashSet::new(), blocked: false,
            latest: None, rows_index: None,
        });
        let segment_id = rv["segmentId"].as_str().unwrap().to_string();
        if st.segment_id.as_deref() != Some(segment_id.as_str()) {
            if st.closed.contains(&segment_id) { return fail("SEGMENT_REUSED"); }
            if let Some(old) = st.segment_id.take() { st.closed.insert(old); }
            st.segment_id = Some(segment_id.clone());
            st.blocked = false;
            st.latest = None;
            st.rows_index = Some(segments.len());
            segments.push(Vec::new());
        }
        if rv["kind"].as_str() == Some("barrier") {
            st.blocked = true;
            excluded.push(json!({"rowId": rv["rowId"].clone(), "reason": format!("barrier:{}", rv["reason"].as_str().unwrap())}));
            continue;
        }
        let q = &rv["quality"];
        let seq = js_int(&rv["sequence"]).unwrap();
        let source_ns = r.source_ns.clone().unwrap();
        let version = format!("{instrument_key}/{seq}/{source_ns}");
        let content = canonical(&json!({
            "bids": rv["bids"].clone(), "asks": rv["asks"].clone(),
            "nextExpiryAt": rv["nextExpiryAt"].clone(), "expiryKnown": q["expiryKnown"].clone(),
        }));
        let duplicate = version_seen.contains_key(&version);
        if duplicate && version_seen[&version] != content { return fail("CONFLICTING_BOOK_VERSION"); }
        if !duplicate {
            if seq < instrument.sequence
                || (instrument.source_ns.is_some() && source_ns < *instrument.source_ns.as_ref().unwrap())
            {
                return fail("VERSION_ROLLBACK");
            }
            version_seen.insert(version, content);
            instrument.sequence = seq;
            instrument.source_ns = Some(source_ns.clone());
        }
        let mut reason: Option<&str> = if !q["continuous"].as_bool().unwrap() || q["recovering"].as_bool().unwrap()
            || q["stale"].as_bool().unwrap() || !q["expiryKnown"].as_bool().unwrap()
        {
            Some("quality")
        } else {
            None
        };
        if source_ns > r.received_ns {
            reason = Some("source-clock-future");
        } else if r.available_ns.clone() - &source_ns > max_age {
            reason = Some("source-stale");
        } else if r.expiry_ns.as_ref().is_some_and(|e| *e <= r.available_ns) {
            reason = Some("expired");
        }
        if let Some(reason) = reason {
            st.blocked = true;
            excluded.push(json!({"rowId": rv["rowId"].clone(), "reason": reason}));
            continue;
        }
        if st.blocked {
            excluded.push(json!({"rowId": rv["rowId"].clone(), "reason": "segment-blocked"}));
            continue;
        }
        if duplicate {
            excluded.push(json!({"rowId": rv["rowId"].clone(), "reason": "duplicate-version"}));
            continue;
        }
        let prior = st.latest.clone();
        if let Some(prior) = &prior {
            if seq < prior.sequence || source_ns < prior.source_ns { return fail("VERSION_ROLLBACK"); }
            if r.available_ns <= prior.available_ns { return fail("AMBIGUOUS_INSTRUMENT_TIME"); }
            if r.available_ns.clone() - &prior.available_ns > max_gap
                || prior.expiry_ns.as_ref().is_some_and(|e| *e <= r.available_ns)
                || r.available_ns.clone() - &prior.source_ns > max_age
            {
                st.blocked = true;
                excluded.push(json!({"rowId": rv["rowId"].clone(), "reason": "unmarked-regime-gap"}));
                continue;
            }
        }
        let (mid, spread, imbalance, micro) = basic(rv);
        let mut b = BuiltBook {
            v: rv.clone(), sequence: seq, available_ns: r.available_ns.clone(),
            received_ns: r.received_ns.clone(), source_ns, expiry_ns: r.expiry_ns.clone(),
            mid, spread, imbalance, micro, feature: None, feature_start: None,
        };
        if let Some(prior) = &prior {
            b.feature = Some(vec![b.spread, b.imbalance, b.micro, b.mid - prior.mid, b.imbalance - prior.imbalance]);
            b.feature_start = Some(prior.available_ns.clone());
        }
        segments[st.rows_index.unwrap()].push(b.clone());
        st.latest = Some(b);
    }

    let mut samples: Vec<Sample> = Vec::new();
    let mut missing_labels: Vec<String> = Vec::new();
    let horizon = BigInt::from(js_int(&c["horizonMs"]).unwrap()) * ns();
    let tolerance = BigInt::from(js_int(&c["toleranceMs"]).unwrap()) * ns();
    let mut accepted_books = 0usize;
    let mut warmup = 0usize;
    for rows in &segments {
        accepted_books += rows.len();
        let mut end = 0usize;
        for i in 0..rows.len() {
            let r = &rows[i];
            if r.feature.is_none() { warmup += 1; continue; }
            let target = r.available_ns.clone() + &horizon;
            end = end.max(i + 1);
            while end < rows.len() && rows[end].available_ns < target { end += 1; }
            let label = match rows.get(end) {
                Some(l) if l.available_ns <= target.clone() + &tolerance => l,
                _ => {
                    missing_labels.push(r.v["rowId"].as_str().unwrap().to_string());
                    continue;
                }
            };
            let past = &rows[i - 1];
            let v = json!({
                "rowId": r.v["rowId"].clone(), "labelRowId": label.v["rowId"].clone(),
                "sessionId": r.v["sessionId"].clone(), "tournamentId": r.v["tournamentId"].clone(),
                "marketId": r.v["marketId"].clone(), "exchangeId": r.v["exchangeId"].clone(),
                "segmentId": r.v["segmentId"].clone(), "availableAt": r.v["availableAt"].clone(),
                "sourceAt": r.v["sourceAt"].clone(), "receivedAt": r.v["receivedAt"].clone(),
                "sequence": r.v["sequence"].clone(), "quality": r.v["quality"].clone(),
                "pastRowId": past.v["rowId"].clone(), "pastAvailableAt": past.v["availableAt"].clone(),
                "pastReceivedAt": past.v["receivedAt"].clone(), "pastSourceAt": past.v["sourceAt"].clone(),
                "pastSequence": past.v["sequence"].clone(), "pastQuality": past.v["quality"].clone(),
                "labelAvailableAt": label.v["availableAt"].clone(), "labelSourceAt": label.v["sourceAt"].clone(),
                "labelReceivedAt": label.v["receivedAt"].clone(), "labelSequence": label.v["sequence"].clone(),
                "labelQuality": label.v["quality"].clone(),
                "x": r.feature.clone().unwrap(), "y": label.mid - r.mid,
            });
            samples.push(Sample {
                v,
                start: r.available_ns.clone(),
                feature_start: r.feature_start.clone().unwrap(),
                end: label.available_ns.clone(),
            });
        }
    }
    samples.sort_by(|a, b| {
        a.start.cmp(&b.start).then_with(|| {
            let (a, b) = (a.v["rowId"].as_str().unwrap_or(""), b.v["rowId"].as_str().unwrap_or(""));
            a.cmp(b)
        })
    });
    let source_books = dataset.rows.iter().filter(|r| r.v["kind"].as_str() == Some("book")).count();
    let reasons: Vec<String> = excluded.iter().map(|e| e["reason"].as_str().unwrap_or("").to_string()).collect();
    let coverage = json!({
        "sourceRows": dataset.rows.len(),
        "sourceBooks": source_books,
        "acceptedBooks": accepted_books,
        "warmup": warmup,
        "labeled": samples.len(),
        "missingLabels": missing_labels.len(),
        "excluded": excluded.len(),
        "labelCoverageOfSourceBooks": samples.len() as f64 / source_books.max(1) as f64,
        "reasons": count_by(&reasons),
    });
    Ok(Built {
        samples,
        coverage,
        excluded: V::Array(excluded),
        missing_labels: V::Array(missing_labels.into_iter().map(V::from).collect()),
    })
}
/// countBy(values) — first-seen ordered reason counts.
pub fn count_by(values: &[String]) -> V {
    let mut counts = serde_json::Map::new();
    for v in values {
        match counts.get_mut(v) {
            Some(c) => *c = V::from(c.as_u64().unwrap_or(0) + 1),
            None => {
                counts.insert(v.clone(), V::from(1u64));
            }
        }
    }
    V::Object(counts)
}

/// splitSamples(samples, c) — strict information intervals: past feature start
/// through actual future label time.
pub fn split_samples(samples: &[Sample], c: &V) -> R<Split> {
    validate_config(c)?;
    let train_cut = instant(&c["trainEnd"])?;
    let val_cut = instant(&c["validationEnd"])?;
    let e = BigInt::from(js_int(&c["embargoMs"]).unwrap()) * ns();
    let mut partitions = Partitions { train: Vec::new(), validation: Vec::new(), test: Vec::new() };
    let mut excluded: Vec<V> = Vec::new();
    for s in samples {
        let p = if s.start < train_cut { "train" } else if s.start < val_cut { "validation" } else { "test" };
        let embargoed = [&train_cut, &val_cut]
            .iter()
            .any(|cut| s.start >= (**cut).clone() - &e && s.start < (**cut).clone() + &e);
        let reason = if embargoed {
            Some("embargo")
        } else if (p == "train" && s.end >= train_cut.clone() - &e)
            || (p == "validation" && s.end >= val_cut.clone() - &e)
        {
            Some("label-purge")
        } else if (p == "validation" && s.feature_start < train_cut.clone() + &e)
            || (p == "test" && s.feature_start < val_cut.clone() + &e)
        {
            Some("feature-purge")
        } else {
            None
        };
        match reason {
            Some(reason) => excluded.push(json!({"rowId": s.v["rowId"].clone(), "nominal": p, "reason": reason})),
            None => match p {
                "train" => partitions.train.push(s.clone()),
                "validation" => partitions.validation.push(s.clone()),
                _ => partitions.test.push(s.clone()),
            },
        }
    }
    Ok(Split { partitions, excluded: V::Array(excluded) })
}

/// publicSample(s, includeLabel) — holdout-safe sample projection.
pub fn public_sample(s: &Sample, include_label: bool) -> V {
    if include_label {
        // metadata = s minus {start, end, featureStart, x, y}, then
        // {features: x, labelMidChange: y} appended.
        let mut meta: serde_json::Map<String, V> = s
            .v
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.as_str() != "x" && k.as_str() != "y")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        meta.insert("features".to_string(), s.v["x"].clone());
        meta.insert("labelMidChange".to_string(), s.v["y"].clone());
        V::Object(meta)
    } else {
        json!({
            "rowId": s.v["rowId"].clone(),
            "sessionId": s.v["sessionId"].clone(),
            "availableAt": s.v["availableAt"].clone(),
        })
    }
}
