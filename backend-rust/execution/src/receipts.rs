use crate::core::{decimal_micros, format_micros, payload_hash};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::BTreeMap;
use std::path::PathBuf;

// execution/exact-money-receipts.mjs — exact-decimal receipts bound to raw API
// bodies; the fetch wrapper only observes the two money-bearing routes.
pub const BASE: &str = "https://sig.thesuper.market";

// Parse JSON without losing number lexemes. Reject duplicate keys, including
// escaped duplicates, so a receipt cannot bind a different value than the API.
pub fn parse_number_lexemes(raw: &str) -> Result<(V, BTreeMap<String, String>), String> {
    if raw.len() > 8 * 1024 * 1024 {
        return Err("EXACT_JSON_SIZE".into());
    }
    let b = raw.as_bytes();
    let mut numbers: BTreeMap<String, String> = BTreeMap::new();
    fn utf8_len(c: u8) -> usize {
        if c >= 0xF0 { 4 } else if c >= 0xE0 { 3 } else { 2 }
    }
    fn parse(raw: &str, at: &mut usize, path: &[V], depth: usize, numbers: &mut BTreeMap<String, String>) -> Result<V, String> {
        let b = raw.as_bytes();
        let space = |at: &mut usize| {
            while *at < b.len() && matches!(b[*at], b'\t' | b'\n' | b'\r' | b' ') {
                *at += 1;
            }
        };
        let string = |raw: &str, at: &mut usize| -> Result<String, String> {
            let b = raw.as_bytes();
            if b.get(*at) != Some(&b'"') {
                return Err("EXACT_JSON_STRING".into());
            }
            let start = *at;
            *at += 1;
            while *at < raw.len() {
                let c = b[*at];
                *at += 1;
                if c == b'\\' {
                    *at += 1;
                    continue;
                }
                if c == b'"' {
                    return serde_json::from_str::<String>(&raw[start..*at])
                        .map_err(|_| "EXACT_JSON_STRING".to_string());
                }
                if c >= 0x80 {
                    *at += utf8_len(c) - 1;
                }
            }
            Err("EXACT_JSON_STRING".into())
        };
        if depth > 64 {
            return Err("EXACT_JSON_DEPTH".into());
        }
        space(at);
        match b.get(*at) {
            Some(b'{') => {
                *at += 1;
                space(at);
                let mut out = Map::new();
                let mut seen = std::collections::HashSet::new();
                if b.get(*at) == Some(&b'}') {
                    *at += 1;
                    return Ok(V::Object(out));
                }
                loop {
                    space(at);
                    let key = string(raw, at)?;
                    if !seen.insert(key.clone()) {
                        return Err("EXACT_JSON_DUPLICATE_KEY".into());
                    }
                    space(at);
                    if b.get(*at) != Some(&b':') {
                        return Err("EXACT_JSON_COLON".into());
                    }
                    *at += 1;
                    let mut path2 = path.to_vec();
                    path2.push(json!(key));
                    let v = parse(raw, at, &path2, depth + 1, numbers)?;
                    out.insert(key, v);
                    space(at);
                    match b.get(*at) {
                        Some(b'}') => {
                            *at += 1;
                            return Ok(V::Object(out));
                        }
                        Some(b',') => *at += 1,
                        _ => return Err("EXACT_JSON_OBJECT".into()),
                    }
                }
            }
            Some(b'[') => {
                *at += 1;
                space(at);
                let mut out = Vec::new();
                if b.get(*at) == Some(&b']') {
                    *at += 1;
                    return Ok(V::Array(out));
                }
                loop {
                    if out.len() >= 200_000 {
                        return Err("EXACT_JSON_ARRAY_SIZE".into());
                    }
                    let mut path2 = path.to_vec();
                    path2.push(json!(out.len()));
                    out.push(parse(raw, at, &path2, depth + 1, numbers)?);
                    space(at);
                    match b.get(*at) {
                        Some(b']') => {
                            *at += 1;
                            return Ok(V::Array(out));
                        }
                        Some(b',') => *at += 1,
                        _ => return Err("EXACT_JSON_ARRAY".into()),
                    }
                }
            }
            Some(b'"') => Ok(V::String(string(raw, at)?)),
            _ => {
                for (word, v) in [("true", V::Bool(true)), ("false", V::Bool(false)), ("null", V::Null)] {
                    if raw[*at..].starts_with(word) {
                        *at += word.len();
                        return Ok(v);
                    }
                }
                // -?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?
                let start = *at;
                let mut i = *at;
                if b.get(i) == Some(&b'-') {
                    i += 1;
                }
                match b.get(i) {
                    Some(b'0') => i += 1,
                    Some(d) if d.is_ascii_digit() => {
                        while b.get(i).is_some_and(|d| d.is_ascii_digit()) {
                            i += 1;
                        }
                    }
                    _ => return Err("EXACT_JSON_VALUE".into()),
                }
                if b.get(i) == Some(&b'.') {
                    i += 1;
                    if !b.get(i).is_some_and(|d| d.is_ascii_digit()) {
                        return Err("EXACT_JSON_VALUE".into());
                    }
                    while b.get(i).is_some_and(|d| d.is_ascii_digit()) {
                        i += 1;
                    }
                }
                if matches!(b.get(i), Some(b'e') | Some(b'E')) {
                    i += 1;
                    if matches!(b.get(i), Some(b'+') | Some(b'-')) {
                        i += 1;
                    }
                    if !b.get(i).is_some_and(|d| d.is_ascii_digit()) {
                        return Err("EXACT_JSON_VALUE".into());
                    }
                    while b.get(i).is_some_and(|d| d.is_ascii_digit()) {
                        i += 1;
                    }
                }
                let lexeme = &raw[start..i];
                let n: f64 = lexeme.parse().map_err(|_| "EXACT_JSON_NUMBER")?;
                if !n.is_finite() {
                    return Err("EXACT_JSON_NUMBER".into());
                }
                *at = i;
                numbers.insert(js_stringify(&V::Array(path.to_vec())), lexeme.to_string());
                Ok(json!(n))
            }
        }
    }
    let mut at = 0usize;
    let data = parse(raw, &mut at, &[], 0, &mut numbers)?;
    while at < b.len() && matches!(b[at], b'\t' | b'\n' | b'\r' | b' ') {
        at += 1;
    }
    if at != raw.len() {
        return Err("EXACT_JSON_TRAILING".into());
    }
    Ok((data, numbers))
}
fn decimal(numbers: &BTreeMap<String, String>, path: &[V]) -> V {
    let key = js_stringify(&V::Array(path.to_vec()));
    let Some(lexeme) = numbers.get(&key) else {
        return json!({"available": false, "reason": "numeric_source_missing"});
    };
    match decimal_micros(&json!(lexeme), "exact") {
        Ok(m) if m >= 0 => {
            json!({"available": true, "lexeme": lexeme, "decimal": format_micros(m)})
        }
        _ => json!({"available": false, "reason": "non_exact_nonnegative_micro_decimal"}),
    }
}

// Adapter around the existing fetch, not a second client or admission system.
pub struct MoneyReceiptOptions<'a> {
    pub tournament_id: &'a str,
    pub tournament_slug: &'a str,
    pub on_receipt: &'a dyn Fn(&V) -> Result<(), String>,
    pub now: &'a dyn Fn() -> i64,
}
pub fn exact_money_fetch<'a>(
    fetch: &'a dyn Fn(&str, &V) -> Result<V, String>,
    options: &'a MoneyReceiptOptions<'a>,
) -> impl Fn(&str, &V) -> Result<V, String> + 'a {
    move |url: &str, opts: &V| -> Result<V, String> {
        if options.tournament_id.is_empty()
            || !options
                .tournament_slug
                .split('-')
                .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
        {
            return Err("EXACT_MONEY_DEPENDENCIES".into());
        }
        let response = fetch(url, opts)?;
        let method = opts.get("method").and_then(V::as_str).unwrap_or("GET").to_string();
        let path = url.strip_prefix(BASE).unwrap_or("");
        let cash = method == "GET" && path == format!("/api/v1/tournaments/{}", options.tournament_slug);
        let order = method == "POST" && path == "/api/v1/orders";
        if !url.starts_with(BASE) || path.contains('?') || response.get("ok") != Some(&V::Bool(true)) || (!cash && !order) {
            return Ok(response);
        }
        let Some(raw) = response.get("text").and_then(V::as_str).map(str::to_string) else {
            return Ok(response);
        };
        let (data, numbers) = parse_number_lexemes(&raw)?;
        let source_body_hash = sha256(raw.as_bytes());
        let received_at = chrono::DateTime::from_timestamp_millis((options.now)())
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let receipt = if cash {
            if data.get("id").and_then(V::as_str) != Some(options.tournament_id) {
                return Err("EXACT_CASH_SCOPE_MISMATCH".into());
            }
            json!({
                "kind": "tournament_cash",
                "tournamentId": options.tournament_id,
                "joined": data.get("isPendingEnrolment") == Some(&V::Bool(false)),
                "fields": {"myBalance": decimal(&numbers, &[json!("myBalance")])},
            })
        } else {
            let request: V = opts
                .get("body")
                .and_then(V::as_str)
                .and_then(|b| serde_json::from_str(b).ok())
                .ok_or("EXACT_ORDER_REQUEST")?;
            if request.get("tournamentId").and_then(V::as_str) != Some(options.tournament_id)
                || request.get("exchangeId").and_then(V::as_str).is_none()
            {
                return Err("EXACT_ORDER_SCOPE_MISMATCH".into());
            }
            let order_id = data.get("orderId").and_then(baal_service_runtime::json_text::js_int);
            let matched = data.get("exchangeId").and_then(V::as_str)
                == request.get("exchangeId").and_then(V::as_str)
                && order_id.is_some_and(|n| n > 0);
            json!({
                "kind": "order_ack_principal",
                "tournamentId": options.tournament_id,
                "matchedIdentity": matched,
                "exchangeId": if matched { data.get("exchangeId").cloned().unwrap_or(V::Null) } else { V::Null },
                "orderId": if matched { json!(order_id.unwrap().to_string()) } else { V::Null },
                "canonicalSide": data.get("side").cloned().unwrap_or(V::Null),
                "canonicalAction": data.get("action").cloned().unwrap_or(V::Null),
                "requestHash": sha256(opts.get("body").and_then(V::as_str).unwrap_or("").as_bytes()),
                "fields": {
                    "totalCost": if matched {
                        decimal(&numbers, &[json!("totalCost")])
                    } else {
                        json!({"available": false, "reason": "ack_identity_unbound"})
                    },
                },
            })
        };
        let mut full = receipt.as_object().unwrap().clone();
        full.insert("schemaVersion".into(), json!(1));
        full.insert("method".into(), json!(method));
        full.insert("endpoint".into(), json!(path.trim_start_matches("/api/v1")));
        full.insert("receivedAt".into(), json!(received_at));
        full.insert("sourceBodyHash".into(), json!(source_body_hash));
        full.insert("executionAuthority".into(), json!(false));
        (options.on_receipt)(&V::Object(full))?;
        let mut out = response.as_object().unwrap().clone();
        out.insert("data".into(), data);
        Ok(V::Object(out))
    }
}

// createMoneyReceiptWriter: chained append journal {sequence, previous, receipt}
// where hash = payloadHash (canonical sha256) of the row without hash.
pub struct MoneyReceiptWriter<'a> {
    file: PathBuf,
    sequence: u64,
    previous: V,
    bytes: u64,
    ino: u64,
    birthtime_ms: f64,
    can_commit: &'a dyn Fn() -> bool,
}
#[cfg(unix)]
fn file_identity(file: &std::path::Path) -> Result<(u64, f64, u64), String> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(file).map_err(|e| e.to_string())?;
    Ok((m.ino(), m.created().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0), m.size()))
}
impl<'a> MoneyReceiptWriter<'a> {
    pub fn open(file: PathBuf, can_commit: &'a dyn Fn() -> bool) -> Result<Self, String> {
        let mut w = MoneyReceiptWriter {
            file: file.clone(),
            sequence: 0,
            previous: V::Null,
            bytes: 0,
            ino: 0,
            birthtime_ms: 0.0,
            can_commit,
        };
        if file.exists() {
            let raw = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
            if !raw.is_empty() && !raw.ends_with('\n') {
                return Err("EXACT_RECEIPT_PARTIAL".into());
            }
            for line in raw.trim_end().split('\n').filter(|l| !l.is_empty()) {
                let row: V = serde_json::from_str(line).map_err(|_| "EXACT_RECEIPT_CHAIN")?;
                let hash = row.get("hash").cloned().unwrap_or(V::Null);
                let mut body = row.clone();
                body.as_object_mut().map(|m| m.remove("hash"));
                if body.get("sequence").and_then(V::as_u64) != Some(w.sequence + 1)
                    || body.get("previous") != Some(&w.previous)
                    || payload_hash(&body).ok().as_deref() != Some(hash.as_str().unwrap_or(""))
                {
                    return Err("EXACT_RECEIPT_CHAIN".into());
                }
                w.sequence += 1;
                w.previous = hash;
            }
            w.bytes = raw.len() as u64;
        } else {
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        #[cfg(unix)]
        {
            let (ino, birth, _size) = file_identity(&file)?;
            w.ino = ino;
            w.birthtime_ms = birth;
        }
        Ok(w)
    }
    pub fn commit(&mut self, receipt: &V) -> Result<V, String> {
        if !(self.can_commit)() {
            return Err("EXACT_RECEIPT_STOPPED".into());
        }
        #[cfg(unix)]
        {
            let (ino, birth, size) = file_identity(&self.file)?;
            if ino != self.ino || birth != self.birthtime_ms || size != self.bytes {
                return Err("EXACT_RECEIPT_CHANGED".into());
            }
        }
        let body = json!({
            "sequence": self.sequence + 1,
            "previous": self.previous.clone(),
            "receipt": receipt.clone(),
        });
        let hash = payload_hash(&body);
        let mut record = body.clone();
        record.as_object_mut().unwrap().insert("hash".into(), json!(hash.clone()));
        let data = format!("{}\n", js_stringify(&record));
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
            .map_err(|e| e.to_string())?;
        f.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        self.bytes += data.len() as u64;
        self.sequence += 1;
        self.previous = json!(hash.clone());
        Ok(json!({"sequence": self.sequence, "hash": hash}))
    }
}
